//! The relay hub: manages topics, subscriptions, channel dispatch, and
//! the per-connection event loop.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::mpsc;

use crate::auth::CurrentUser;
use crate::error::Error;
use crate::extract::{FromRequest, FromRequestParts, PathParams};
use crate::relay::backend::RelayBackend;
use crate::relay::channel::{ChannelDescriptor, PresenceEntry, PresenceMap, RelayEvent};
use crate::relay::protocol::ClientMessage;
use crate::state::AppState;
use crate::websocket::{Message, WebSocket, WebSocketUpgrade};

use super::RelayConfig;

/// Serializes a push directly to JSON in one pass, avoiding the intermediate
/// `serde_json::Value` allocation that `ServerMessage::Push` would require.
#[derive(serde::Serialize)]
struct PushEnvelope<'a, T: serde::Serialize> {
    r#type: &'static str,
    topic: &'a str,
    event: &'a str,
    payload: &'a T,
}

/// Shared relay state stored in [`AppState`].
///
/// Delegates pub/sub to a pluggable [`RelayBackend`]. The default
/// [`InMemoryBackend`](super::backend::InMemoryBackend) uses per-topic
/// broadcast channels. Handlers use the [`Relay`] extractor to push messages;
/// WebSocket clients subscribe via the wire protocol.
///
/// Channel handlers registered via `#[relay("pattern")]` receive
/// [`RelayEvent`] callbacks for subscribe, message, and disconnect events.
pub struct RelayHub {
    backend: Box<dyn RelayBackend>,
    config: RelayConfig,
    channels: Vec<&'static ChannelDescriptor>,
    presence: PresenceMap,
    next_conn_id: AtomicU64,
}

impl RelayHub {
    pub(crate) fn new(
        config: RelayConfig,
        backend: Box<dyn RelayBackend>,
        channels: Vec<&'static ChannelDescriptor>,
    ) -> Self {
        Self {
            backend,
            config,
            channels,
            presence: PresenceMap::new(),
            next_conn_id: AtomicU64::new(1),
        }
    }

    /// Push a JSON-serializable payload to all subscribers of `topic`.
    ///
    /// If nobody is subscribed to the topic, the message is silently dropped.
    pub async fn push<T: serde::Serialize>(
        &self,
        topic: &str,
        event: &str,
        payload: &T,
    ) -> Result<(), Error> {
        let envelope = PushEnvelope {
            r#type: "push",
            topic,
            event,
            payload,
        };
        let json = serde_json::to_string(&envelope)
            .map_err(|e| Error::internal(format!("relay serialization error: {e}")))?;

        self.backend.push(topic, Arc::new(json)).await
    }

    /// Find the first channel handler matching `topic`.
    ///
    /// Channels are pre-sorted by specificity in `prepare()`: exact matches
    /// first, then prefix matches by length descending. The first match wins.
    fn find_channel(&self, topic: &str) -> Option<&'static ChannelDescriptor> {
        self.channels.iter().find(|ch| ch.matches(topic)).copied()
    }

    /// The handler function for the built-in relay WebSocket endpoint.
    ///
    /// Registered as a normal route through the router so it goes through
    /// the full middleware stack (auth, rate limiting, etc).
    pub(crate) async fn ws_handler(
        req: hyper::Request<hyper::body::Incoming>,
        params: PathParams,
        state: Arc<AppState>,
    ) -> Result<hyper::Response<crate::response::BoxBody>, Error> {
        // Extract CurrentUser BEFORE upgrade consumes the request.
        let current_user = req.extensions().get::<CurrentUser>().cloned();

        let upgrade = WebSocketUpgrade::from_request(req, &params, &state).await?;
        let hub = state.get::<Arc<RelayHub>>().ok_or_else(|| {
            Error::internal("RelayHub not found in state. Did you forget to call .with_relay()?")
        })?;

        let hub = Arc::clone(hub);
        let state = Arc::clone(&state);

        Ok(upgrade.on_upgrade(move |socket| connection_loop(socket, hub, state, current_user)))
    }
}

/// Per-connection event loop. Manages subscriptions, dispatches channel
/// handlers, and forwards pushes.
async fn connection_loop(
    socket: WebSocket,
    hub: Arc<RelayHub>,
    state: Arc<AppState>,
    current_user: Option<CurrentUser>,
) {
    let conn_id = hub.next_conn_id.fetch_add(1, Ordering::Relaxed);
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Aggregates messages from all subscribed topic receivers.
    let (funnel_tx, mut funnel_rx) = mpsc::channel::<Arc<String>>(256);

    // Map topic -> forwarding task handle, so unsubscribe can abort the right one.
    // Aborting a task drops the TopicReceiver, which triggers cleanup via Drop.
    let mut subscriptions: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();

    loop {
        tokio::select! {
            msg = ws_rx.recv() => {
                let msg = match msg {
                    Some(Ok(m)) => m,
                    _ => break,
                };

                let text = match msg.as_text() {
                    Some(t) => t,
                    None => continue,
                };

                let client_msg: ClientMessage = match serde_json::from_str(text) {
                    Ok(m) => m,
                    Err(e) => {
                        let json = error_json(&format!("invalid message: {e}"));
                        ws_tx.send(Message::Text(json)).await.ok();
                        continue;
                    }
                };

                match client_msg {
                    ClientMessage::Subscribe { topic } => {
                        if subscriptions.contains_key(&topic) {
                            let json = subscribed_json(&topic);
                            ws_tx.send(Message::Text(json)).await.ok();
                            continue;
                        }

                        if subscriptions.len() >= hub.config.max_subscriptions_per_connection {
                            let json = error_json(&format!(
                                "subscription limit reached (max {})",
                                hub.config.max_subscriptions_per_connection,
                            ));
                            ws_tx.send(Message::Text(json)).await.ok();
                            continue;
                        }

                        // Subscribe first so the Join handler can push
                        // messages that the joining client will receive.
                        let mut receiver = hub.backend.subscribe(&topic).await;
                        let funnel = funnel_tx.clone();

                        let handle = tokio::spawn(async move {
                            while let Some(msg) = receiver.recv().await {
                                if funnel.send(msg).await.is_err() {
                                    break;
                                }
                            }
                        });

                        subscriptions.insert(topic.clone(), handle);

                        // If a channel handler matches, call Join handler.
                        // Rejection (Err) undoes the subscription.
                        if let Some(channel) = hub.find_channel(&topic) {
                            let event = RelayEvent::Join {
                                topic: topic.clone(),
                                conn_id,
                            };
                            if let Err(e) = (channel.handle)(
                                event,
                                Arc::clone(&state),
                                current_user.clone(),
                            )
                            .await
                            {
                                // Undo subscription
                                if let Some(handle) = subscriptions.remove(&topic) {
                                    handle.abort();
                                }
                                let json = error_json(&format!("join rejected: {e}"));
                                ws_tx.send(Message::Text(json)).await.ok();
                                continue;
                            }
                        }

                        let json = subscribed_json(&topic);
                        ws_tx.send(Message::Text(json)).await.ok();
                    }

                    ClientMessage::Unsubscribe { topic } => {
                        if let Some(handle) = subscriptions.remove(&topic) {
                            handle.abort();
                        }

                        // Fire-and-forget Leave handler
                        if let Some(channel) = hub.find_channel(&topic) {
                            let event = RelayEvent::Leave {
                                topic: topic.clone(),
                                conn_id,
                            };
                            let handler = channel.handle;
                            let s = Arc::clone(&state);
                            let u = current_user.clone();
                            tokio::spawn(async move {
                                let _ = handler(event, s, u).await;
                            });
                        }

                        hub.presence.untrack(&topic, conn_id);

                        let json = unsubscribed_json(&topic);
                        ws_tx.send(Message::Text(json)).await.ok();
                    }

                    ClientMessage::Message { topic, event, payload } => {
                        if !subscriptions.contains_key(&topic) {
                            let json = error_json(&format!("not subscribed to {topic}"));
                            ws_tx.send(Message::Text(json)).await.ok();
                            continue;
                        }

                        if let Some(channel) = hub.find_channel(&topic) {
                            // Channel handler takes ownership: it pushes if it wants to.
                            let relay_event = RelayEvent::Message {
                                topic: topic.clone(),
                                event,
                                payload,
                                conn_id,
                            };
                            if let Err(e) = (channel.handle)(
                                relay_event,
                                Arc::clone(&state),
                                current_user.clone(),
                            )
                            .await
                            {
                                let json = error_json(&format!("message handler error: {e}"));
                                ws_tx.send(Message::Text(json)).await.ok();
                            }
                        } else {
                            // No channel handler — default broadcast
                            let envelope = PushEnvelope {
                                r#type: "push",
                                topic: &topic,
                                event: &event,
                                payload: &payload,
                            };
                            let json = Arc::new(serde_json::to_string(&envelope).unwrap());

                            let _ = hub.backend.push(&topic, json).await;
                        }
                    }

                    ClientMessage::Ping => {
                        ws_tx.send(Message::Text(PONG_JSON.to_owned())).await.ok();
                    }
                }
            }

            Some(json) = funnel_rx.recv() => {
                if ws_tx.send(Message::Text((*json).clone())).await.is_err() {
                    break;
                }
            }
        }
    }

    // Disconnect cleanup: abort forwarding tasks, fire Leave handlers,
    // and clean up presence entries.
    for (topic, handle) in subscriptions {
        handle.abort();

        if let Some(channel) = hub.find_channel(&topic) {
            let event = RelayEvent::Leave {
                topic: topic.clone(),
                conn_id,
            };
            let handler = channel.handle;
            let s = Arc::clone(&state);
            let u = current_user.clone();
            tokio::spawn(async move {
                let _ = handler(event, s, u).await;
            });
        }

        hub.presence.untrack(&topic, conn_id);
    }
}

// Pre-built JSON for static responses to avoid repeated serde round-trips.
const PONG_JSON: &str = r#"{"type":"pong"}"#;

fn subscribed_json(topic: &str) -> String {
    format!(r#"{{"type":"subscribed","topic":{}}}"#, json_string(topic))
}

fn unsubscribed_json(topic: &str) -> String {
    format!(
        r#"{{"type":"unsubscribed","topic":{}}}"#,
        json_string(topic)
    )
}

fn error_json(message: &str) -> String {
    format!(r#"{{"type":"error","message":{}}}"#, json_string(message))
}

/// JSON-encode a string value (with proper escaping).
fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

/// Handler-side extractor for pushing messages and querying presence.
///
/// Obtained via `FromRequestParts`, which pulls the shared [`RelayHub`] from
/// [`AppState`]. Cloning is cheap (Arc bump).
///
/// ```rust,ignore
/// use rapina::prelude::*;
/// use rapina::relay::Relay;
///
/// #[post("/orders")]
/// async fn create_order(relay: Relay, body: Json<NewOrder>) -> Result<Json<Order>> {
///     let order = save_order(&body).await?;
///     relay.push("orders:new", "created", &order).await?;
///     Ok(Json(order))
/// }
/// ```
#[derive(Clone)]
pub struct Relay {
    hub: Arc<RelayHub>,
}

impl Relay {
    /// Push a JSON-serializable payload to all subscribers of `topic`.
    pub async fn push<T: serde::Serialize>(
        &self,
        topic: &str,
        event: &str,
        payload: &T,
    ) -> Result<(), Error> {
        self.hub.push(topic, event, payload).await
    }

    /// Track a client's presence in a topic.
    ///
    /// Typically called inside a `RelayEvent::Join` handler. The hub
    /// automatically calls [`untrack`](Self::untrack) on unsubscribe and
    /// disconnect, so manual untrack is only needed for early removal.
    pub fn track(&self, topic: &str, conn_id: u64, meta: serde_json::Value) {
        self.hub.presence.track(topic, conn_id, meta);
    }

    /// Remove a client's presence from a topic.
    pub fn untrack(&self, topic: &str, conn_id: u64) {
        self.hub.presence.untrack(topic, conn_id);
    }

    /// List all presence entries for a topic.
    pub fn presence(&self, topic: &str) -> Vec<PresenceEntry> {
        self.hub.presence.list(topic)
    }

    /// Count the number of connected clients in a topic.
    pub fn presence_count(&self, topic: &str) -> usize {
        self.hub.presence.count(topic)
    }
}

impl FromRequestParts for Relay {
    async fn from_request_parts(
        _parts: &http::request::Parts,
        _params: &PathParams,
        state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        let hub = state.get::<Arc<RelayHub>>().ok_or_else(|| {
            Error::internal("RelayHub not registered. Did you forget to call .with_relay()?")
        })?;
        Ok(Relay {
            hub: Arc::clone(hub),
        })
    }
}
