//! Wire protocol for Relay WebSocket connections.
//!
//! All messages are JSON-encoded. Clients send [`ClientMessage`] and receive
//! [`ServerMessage`] on the WebSocket connection established at the relay
//! endpoint.

use serde::{Deserialize, Serialize};

/// A message sent by the client over the WebSocket connection.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Subscribe to a topic. The server replies with [`ServerMessage::Subscribed`].
    Subscribe { topic: String },
    /// Unsubscribe from a topic.
    Unsubscribe { topic: String },
    /// Publish a message to a topic. Delivered to all other subscribers.
    Message {
        topic: String,
        event: String,
        payload: serde_json::Value,
    },
    /// Heartbeat ping. The server replies with [`ServerMessage::Pong`].
    Ping,
}

/// A message sent by the server to a connected client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Confirms a successful subscription.
    Subscribed { topic: String },
    /// Confirms a successful unsubscription.
    Unsubscribed { topic: String },
    /// A push delivered from another handler or client.
    Push {
        topic: String,
        event: String,
        payload: serde_json::Value,
    },
    /// Heartbeat response.
    Pong,
    /// An error occurred processing a client message.
    Error { message: String },
}
