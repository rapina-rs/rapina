#![cfg(feature = "websocket")]

use std::sync::Arc;
use std::time::Duration;

use rapina::extract::FromRequestParts;
use rapina::futures_util::{SinkExt, StreamExt};
use rapina::prelude::*;
use rapina::relay::protocol::ServerMessage;
use rapina::relay::{Relay, RelayConfig, RelayEvent};
use rapina::response::IntoResponse;
use rapina::testing::TestClient;
use rapina::tokio_tungstenite::tungstenite;
use tokio::sync::Mutex;

// Type aliases so the helper signatures aren't a wall of generics.
type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type WsTx = futures_util::stream::SplitSink<WsStream, tungstenite::Message>;
type WsRx = futures_util::stream::SplitStream<WsStream>;

async fn ws_connect(addr: std::net::SocketAddr) -> (WsTx, WsRx) {
    let (ws, _) = rapina::tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
        .await
        .unwrap();
    futures_util::StreamExt::split(ws)
}

async fn ws_connect_with_auth(addr: std::net::SocketAddr, token: &str) -> (WsTx, WsRx) {
    use tungstenite::client::IntoClientRequest;
    let mut request = format!("ws://{addr}/ws").into_client_request().unwrap();
    request
        .headers_mut()
        .insert("Authorization", format!("Bearer {token}").parse().unwrap());
    let (ws, _) = rapina::tokio_tungstenite::connect_async(request)
        .await
        .unwrap();
    futures_util::StreamExt::split(ws)
}

async fn send_json(tx: &mut WsTx, json: &str) {
    tx.send(tungstenite::Message::Text(json.into()))
        .await
        .unwrap();
}

async fn recv_server_msg(rx: &mut WsRx) -> ServerMessage {
    let msg = tokio::time::timeout(Duration::from_secs(5), rx.next())
        .await
        .expect("timed out waiting for message")
        .unwrap()
        .unwrap();
    let text = msg.into_text().unwrap();
    serde_json::from_str(&text).unwrap()
}

// ---------------------------------------------------------------------------
// Shared state for recording events
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct TestLog(Arc<Mutex<Vec<String>>>);

impl TestLog {
    async fn push(&self, msg: String) {
        self.0.lock().await.push(msg);
    }
}

// ---------------------------------------------------------------------------
// Channel handlers
// ---------------------------------------------------------------------------

/// Handles "test:*" topics — logs events, tracks presence, echoes messages.
#[rapina::relay("test:*")]
async fn test_channel(event: RelayEvent, relay: Relay, log: State<TestLog>) -> Result<()> {
    match &event {
        RelayEvent::Join { topic, conn_id } => {
            log.push(format!("join:{topic}:{conn_id}")).await;
            relay.track(topic, *conn_id, serde_json::json!({}));
        }
        RelayEvent::Message {
            topic,
            event: ev,
            payload,
            conn_id,
        } => {
            log.push(format!("msg:{topic}:{ev}:{conn_id}")).await;
            relay.push(topic, ev, payload).await?;
        }
        RelayEvent::Leave { topic, conn_id } => {
            log.push(format!("leave:{topic}:{conn_id}")).await;
        }
    }
    Ok(())
}

/// Handles "reject:*" topics — always rejects joins.
#[rapina::relay("reject:*")]
async fn reject_channel(event: RelayEvent) -> Result<()> {
    match event {
        RelayEvent::Join { .. } => Err(Error::bad_request("not allowed")),
        _ => Ok(()),
    }
}

/// Handles the exact topic "exact:topic" — logs join only.
#[rapina::relay("exact:topic")]
async fn exact_channel(event: RelayEvent, log: State<TestLog>) -> Result<()> {
    if let RelayEvent::Join { topic, .. } = &event {
        log.push(format!("exact-join:{topic}")).await;
    }
    Ok(())
}

/// Handles "exact:*" topics — prefix handler that should lose to exact_channel
/// for the literal "exact:topic" but win for anything else like "exact:other".
#[rapina::relay("exact:*")]
async fn exact_prefix_channel(event: RelayEvent, log: State<TestLog>) -> Result<()> {
    if let RelayEvent::Join { topic, .. } = &event {
        log.push(format!("exact-prefix-join:{topic}")).await;
    }
    Ok(())
}

/// Handles "auth:*" topics — requires CurrentUser, records user id.
#[rapina::relay("auth:*")]
async fn auth_channel(
    event: RelayEvent,
    relay: Relay,
    log: State<TestLog>,
    user: CurrentUser,
) -> Result<()> {
    if let RelayEvent::Join { topic, conn_id } = &event {
        log.push(format!("auth-join:{topic}:{}:{conn_id}", user.id))
            .await;
        relay.track(topic, *conn_id, serde_json::json!({"user_id": user.id}));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// App builder helpers
// ---------------------------------------------------------------------------

fn channel_app() -> (Rapina, Arc<Mutex<Vec<String>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let app = Rapina::new()
        .with_introspection(false)
        .with_relay(RelayConfig::default())
        .state(TestLog(log.clone()))
        .router(Router::new().route(
            http::Method::GET,
            "/presence",
            |req, params, state| async move {
                let (parts, _) = req.into_parts();
                let relay = match Relay::from_request_parts(&parts, &params, &state).await {
                    Ok(r) => r,
                    Err(e) => return e.into_response(),
                };
                let query_str = parts.uri.query().unwrap_or("");
                let topic: String = serde_urlencoded::from_str::<Vec<(String, String)>>(query_str)
                    .unwrap_or_default()
                    .into_iter()
                    .find(|(k, _)| k == "topic")
                    .map(|(_, v)| v)
                    .unwrap_or_default();
                let entries = relay.presence(&topic);
                let count = relay.presence_count(&topic);
                Json(serde_json::json!({"count": count, "entries": entries})).into_response()
            },
        ));
    (app, log)
}

fn auth_app() -> (Rapina, Arc<Mutex<Vec<String>>>, AuthConfig) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let auth_config = AuthConfig::new("test-secret", 3600);
    let app = Rapina::new()
        .with_introspection(false)
        .with_relay(RelayConfig::default())
        .with_auth(auth_config.clone())
        .state(TestLog(log.clone()));
    (app, log, auth_config)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_join_handler_runs_on_subscribe() {
    let (app, log) = channel_app();
    let client = TestClient::new(app).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"test:room1"}"#).await;
    let msg = recv_server_msg(&mut ws_rx).await;
    assert!(matches!(msg, ServerMessage::Subscribed { topic } if topic == "test:room1"));

    let entries = log.lock().await;
    assert_eq!(entries.len(), 1);
    assert!(entries[0].starts_with("join:test:room1:"));

    ws_tx.close().await.ok();
}

#[tokio::test]
async fn test_join_handler_rejection() {
    let (app, _log) = channel_app();
    let client = TestClient::new(app).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"reject:room"}"#).await;
    let msg = recv_server_msg(&mut ws_rx).await;
    match msg {
        ServerMessage::Error { message } => {
            assert!(message.contains("join rejected"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    ws_tx.close().await.ok();
}

#[tokio::test]
async fn test_message_handler_runs_and_echoes() {
    let (app, log) = channel_app();
    let client = TestClient::new(app).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    // Subscribe
    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"test:chat"}"#).await;
    let _ = recv_server_msg(&mut ws_rx).await;

    // Send a message — handler should echo it back via relay.push
    send_json(
        &mut ws_tx,
        r#"{"type":"message","topic":"test:chat","event":"say","payload":{"text":"hello"}}"#,
    )
    .await;

    let msg = recv_server_msg(&mut ws_rx).await;
    match msg {
        ServerMessage::Push {
            topic,
            event,
            payload,
        } => {
            assert_eq!(topic, "test:chat");
            assert_eq!(event, "say");
            assert_eq!(payload, serde_json::json!({"text": "hello"}));
        }
        other => panic!("expected Push, got {other:?}"),
    }

    let entries = log.lock().await;
    assert!(entries.iter().any(|e| e.starts_with("msg:test:chat:say:")));

    ws_tx.close().await.ok();
}

#[tokio::test]
async fn test_leave_on_unsubscribe() {
    let (app, log) = channel_app();
    let client = TestClient::new(app).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"test:room2"}"#).await;
    let _ = recv_server_msg(&mut ws_rx).await;

    send_json(&mut ws_tx, r#"{"type":"unsubscribe","topic":"test:room2"}"#).await;
    let msg = recv_server_msg(&mut ws_rx).await;
    assert!(matches!(msg, ServerMessage::Unsubscribed { topic } if topic == "test:room2"));

    // Leave handler is fire-and-forget (spawned), give it a moment
    tokio::time::sleep(Duration::from_millis(50)).await;

    let entries = log.lock().await;
    assert!(entries.iter().any(|e| e.starts_with("leave:test:room2:")));

    ws_tx.close().await.ok();
}

#[tokio::test]
async fn test_leave_on_disconnect() {
    let (app, log) = channel_app();
    let client = TestClient::new(app).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"test:room3"}"#).await;
    let _ = recv_server_msg(&mut ws_rx).await;

    // Drop the connection
    ws_tx.close().await.ok();
    drop(ws_rx);

    // Leave handlers are fire-and-forget during disconnect
    tokio::time::sleep(Duration::from_millis(100)).await;

    let entries = log.lock().await;
    assert!(entries.iter().any(|e| e.starts_with("leave:test:room3:")));
}

#[tokio::test]
async fn test_no_channel_default_broadcast() {
    let (app, _log) = channel_app();
    let client = TestClient::new(app).await;
    let addr = client.addr();

    // Two clients subscribe to a topic with no channel handler
    let (mut tx1, mut rx1) = ws_connect(addr).await;
    let (mut tx2, mut rx2) = ws_connect(addr).await;

    send_json(&mut tx1, r#"{"type":"subscribe","topic":"nohandler:chat"}"#).await;
    let _ = recv_server_msg(&mut rx1).await;

    send_json(&mut tx2, r#"{"type":"subscribe","topic":"nohandler:chat"}"#).await;
    let _ = recv_server_msg(&mut rx2).await;

    // Client 1 sends a message — should be broadcast (no channel handler intercepts)
    send_json(
        &mut tx1,
        r#"{"type":"message","topic":"nohandler:chat","event":"say","payload":{"text":"hi"}}"#,
    )
    .await;

    // Both clients should receive the broadcast
    let msg1 = recv_server_msg(&mut rx1).await;
    let msg2 = recv_server_msg(&mut rx2).await;

    assert!(matches!(msg1, ServerMessage::Push { .. }));
    assert!(matches!(msg2, ServerMessage::Push { .. }));

    tx1.close().await.ok();
    tx2.close().await.ok();
}

#[tokio::test]
async fn test_presence_tracking() {
    let (app, _log) = channel_app();
    let client = TestClient::new(app).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    // Subscribe triggers Join handler which calls relay.track()
    send_json(
        &mut ws_tx,
        r#"{"type":"subscribe","topic":"test:presence"}"#,
    )
    .await;
    let _ = recv_server_msg(&mut ws_rx).await;

    // Query presence via HTTP endpoint
    let resp = client.get("/presence?topic=test:presence").send().await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
    assert_eq!(body["count"], 1);
    assert_eq!(body["entries"].as_array().unwrap().len(), 1);

    ws_tx.close().await.ok();
}

#[tokio::test]
async fn test_presence_auto_cleanup() {
    let (app, _log) = channel_app();
    let client = TestClient::new(app).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"test:cleanup"}"#).await;
    let _ = recv_server_msg(&mut ws_rx).await;

    // Verify presence exists
    let resp = client.get("/presence?topic=test:cleanup").send().await;
    let body: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
    assert_eq!(body["count"], 1);

    // Disconnect
    ws_tx.close().await.ok();
    drop(ws_rx);

    // Wait for cleanup
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Presence should be cleaned up
    let resp = client.get("/presence?topic=test:cleanup").send().await;
    let body: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
    assert_eq!(body["count"], 0);
}

#[tokio::test]
async fn test_current_user_available_in_channel_handler() {
    let (app, log, auth_config) = auth_app();
    let client = TestClient::new(app).await;
    let addr = client.addr();

    let token = auth_config.create_token("user42").unwrap();
    let (mut ws_tx, mut ws_rx) = ws_connect_with_auth(addr, &token).await;

    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"auth:room"}"#).await;
    let msg = recv_server_msg(&mut ws_rx).await;
    assert!(matches!(msg, ServerMessage::Subscribed { topic } if topic == "auth:room"));

    let entries = log.lock().await;
    assert!(
        entries
            .iter()
            .any(|e| e.starts_with("auth-join:auth:room:user42:"))
    );

    ws_tx.close().await.ok();
}

#[tokio::test]
async fn test_exact_pattern_match() {
    let (app, log) = channel_app();
    let client = TestClient::new(app).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    // "exact:topic" matches the exact handler
    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"exact:topic"}"#).await;
    let msg = recv_server_msg(&mut ws_rx).await;
    assert!(matches!(msg, ServerMessage::Subscribed { topic } if topic == "exact:topic"));

    let entries = log.lock().await;
    assert!(entries.iter().any(|e| e == "exact-join:exact:topic"));

    ws_tx.close().await.ok();
}

#[tokio::test]
async fn test_prefix_pattern_match() {
    let (app, log) = channel_app();
    let client = TestClient::new(app).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    // "test:anything" matches the "test:*" prefix handler
    send_json(
        &mut ws_tx,
        r#"{"type":"subscribe","topic":"test:anything"}"#,
    )
    .await;
    let msg = recv_server_msg(&mut ws_rx).await;
    assert!(matches!(msg, ServerMessage::Subscribed { topic } if topic == "test:anything"));

    let entries = log.lock().await;
    assert!(entries.iter().any(|e| e.starts_with("join:test:anything:")));

    ws_tx.close().await.ok();
}

#[tokio::test]
async fn test_exact_pattern_beats_prefix() {
    let (app, log) = channel_app();
    let client = TestClient::new(app).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    // "exact:topic" should match the exact handler, not "exact:*"
    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"exact:topic"}"#).await;
    let msg = recv_server_msg(&mut ws_rx).await;
    assert!(matches!(msg, ServerMessage::Subscribed { topic } if topic == "exact:topic"));

    let entries = log.lock().await;
    assert!(entries.iter().any(|e| e == "exact-join:exact:topic"));
    assert!(!entries.iter().any(|e| e.starts_with("exact-prefix-join:")));
    drop(entries);

    // "exact:other" should match the prefix handler instead
    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"exact:other"}"#).await;
    let _ = recv_server_msg(&mut ws_rx).await;

    let entries = log.lock().await;
    assert!(entries.iter().any(|e| e == "exact-prefix-join:exact:other"));

    ws_tx.close().await.ok();
}
