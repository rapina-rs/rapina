//! Pluggable backend for the relay pub/sub system.
//!
//! The default [`InMemoryBackend`] uses per-topic `tokio::sync::broadcast`
//! channels. A Redis-backed implementation (behind the `relay-redis` feature)
//! can replace it for cross-node message routing.
//!
//! Uses `BoxFuture` returns instead of `async_trait` to avoid the macro
//! overhead on the hot push path, following the same pattern as
//! [`CacheBackend`](crate::cache::CacheBackend).
//!
// TODO: relay-redis feature — add RedisBackend behind a feature flag,
// following the same pattern as cache_redis.rs.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::broadcast;

use crate::error::Error;

/// A boxed future for trait object compatibility.
pub type RelayFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Backend trait for the relay pub/sub system.
///
/// Implementations must be `Send + Sync + 'static` so they can be stored
/// behind `Box<dyn RelayBackend>` in the [`RelayHub`](super::RelayHub).
///
/// In-memory implementations wrap synchronous operations in
/// `Box::pin(std::future::ready(...))`. Redis implementations return genuine
/// async futures.
pub trait RelayBackend: Send + Sync + 'static {
    /// Send a pre-serialized JSON message to all subscribers of `topic`.
    ///
    /// If nobody is subscribed, the message is silently dropped.
    fn push(&self, topic: &str, json: Arc<String>) -> RelayFuture<'_, Result<(), Error>>;

    /// Subscribe to a topic, returning a receiver that yields messages.
    ///
    /// The returned [`TopicReceiver`] should handle cleanup in its `Drop`
    /// impl (e.g. removing empty broadcast channels from the topic map).
    fn subscribe(&self, topic: &str) -> RelayFuture<'_, Box<dyn TopicReceiver>>;
}

/// Receives messages for a single topic subscription.
///
/// Dropped when the forwarding task is aborted (on unsubscribe or disconnect).
/// Implementations should clean up backend resources in their `Drop` impl.
pub trait TopicReceiver: Send + 'static {
    /// Wait for the next message. Returns `None` when the subscription is
    /// closed (e.g. the sender side was dropped).
    fn recv(&mut self) -> RelayFuture<'_, Option<Arc<String>>>;
}

/// In-memory relay backend using `tokio::sync::broadcast` per topic.
///
/// This is the default backend created by
/// [`RelayConfig::default()`](super::RelayConfig).
pub struct InMemoryBackend {
    topics: Arc<DashMap<String, broadcast::Sender<Arc<String>>>>,
    topic_capacity: usize,
}

impl InMemoryBackend {
    /// Create a new in-memory backend with the given broadcast channel capacity
    /// per topic.
    pub fn new(topic_capacity: usize) -> Self {
        Self {
            topics: Arc::new(DashMap::new()),
            topic_capacity,
        }
    }
}

impl RelayBackend for InMemoryBackend {
    fn push(&self, topic: &str, json: Arc<String>) -> RelayFuture<'_, Result<(), Error>> {
        if let Some(tx) = self.topics.get(topic) {
            let _ = tx.send(json);
        }
        Box::pin(std::future::ready(Ok(())))
    }

    fn subscribe(&self, topic: &str) -> RelayFuture<'_, Box<dyn TopicReceiver>> {
        let tx = self
            .topics
            .entry(topic.to_owned())
            .or_insert_with(|| broadcast::channel(self.topic_capacity).0)
            .clone();
        let rx = tx.subscribe();
        let receiver = BroadcastReceiver {
            rx,
            topic: topic.to_owned(),
            topics: Arc::clone(&self.topics),
        };
        Box::pin(std::future::ready(
            Box::new(receiver) as Box<dyn TopicReceiver>
        ))
    }
}

/// Wraps a `broadcast::Receiver` with automatic topic cleanup on drop.
struct BroadcastReceiver {
    rx: broadcast::Receiver<Arc<String>>,
    topic: String,
    topics: Arc<DashMap<String, broadcast::Sender<Arc<String>>>>,
}

impl TopicReceiver for BroadcastReceiver {
    fn recv(&mut self) -> RelayFuture<'_, Option<Arc<String>>> {
        Box::pin(async {
            loop {
                match self.rx.recv().await {
                    Ok(msg) => return Some(msg),
                    // Slow receiver fell behind — skip the gap, keep going.
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        })
    }
}

impl Drop for BroadcastReceiver {
    fn drop(&mut self) {
        // Atomic check-and-remove: holds the shard lock across the
        // receiver_count check, preventing a race where another connection
        // subscribes between the check and the removal.
        self.topics
            .remove_if(&self.topic, |_, tx| tx.receiver_count() == 0);
    }
}
