//! Relay: real-time push from any handler to WebSocket clients.
//!
//! Enable with `Rapina::with_relay()`. Handlers use the [`Relay`] extractor to
//! push messages; clients subscribe to topics over a WebSocket connection at
//! the configured endpoint (default `/ws`).
//!
//! ```rust,ignore
//! use rapina::prelude::*;
//! use rapina::relay::{Relay, RelayConfig};
//!
//! #[post("/orders")]
//! async fn create_order(relay: Relay, body: Json<NewOrder>) -> Result<Json<Order>> {
//!     let order = save_order(&body).await?;
//!     relay.push("orders:new", "created", &order).await?;
//!     Ok(Json(order))
//! }
//!
//! #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     Rapina::new()
//!         .with_relay(RelayConfig::default())
//!         .discover()
//!         .listen("127.0.0.1:3000")
//!         .await
//! }
//! ```

pub mod backend;
pub mod channel;
mod hub;
pub mod protocol;

pub use backend::{InMemoryBackend, RelayBackend, TopicReceiver};
pub use channel::{ChannelDescriptor, PresenceEntry, RelayEvent};
pub use hub::{Relay, RelayHub};

/// Configuration for the relay system.
///
/// # Per-connection overhead
///
/// Each subscription spawns a dedicated forwarding task that reads from the
/// topic's broadcast channel and writes into a shared mpsc funnel. The
/// `max_subscriptions_per_connection` limit caps the number of concurrent
/// tasks per WebSocket connection to prevent resource exhaustion.
#[derive(Debug, Clone)]
pub struct RelayConfig {
    /// Broadcast channel capacity per topic. When the buffer is full, the
    /// slowest receiver lags and loses messages (broadcast semantics).
    /// Default: 128.
    pub topic_capacity: usize,

    /// Maximum number of topics a single WebSocket connection may subscribe
    /// to simultaneously. Each subscription spawns one forwarding task.
    /// Returns an error [`ServerMessage`](protocol::ServerMessage) if exceeded.
    /// Default: 50.
    pub max_subscriptions_per_connection: usize,

    /// The path where the relay WebSocket endpoint is registered.
    /// Default: `"/ws"`.
    pub path: String,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            topic_capacity: 128,
            max_subscriptions_per_connection: 50,
            path: "/ws".to_owned(),
        }
    }
}

impl RelayConfig {
    /// Create a config with a custom WebSocket endpoint path.
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    /// Set the broadcast channel capacity per topic.
    pub fn with_topic_capacity(mut self, capacity: usize) -> Self {
        self.topic_capacity = capacity;
        self
    }

    /// Set the maximum subscriptions per WebSocket connection.
    pub fn with_max_subscriptions(mut self, max: usize) -> Self {
        self.max_subscriptions_per_connection = max;
        self
    }
}
