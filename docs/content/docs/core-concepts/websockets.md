+++
title = "WebSockets & Relay"
description = "Real-time bidirectional communication with raw WebSockets and the Relay pub/sub system"
weight = 11
date = 2026-03-05
+++

Rapina supports real-time bidirectional communication through two layers: **raw WebSocket** connections for full control, and the **Relay** pub/sub system for structured topic-based messaging with channel handlers and presence tracking. Both are gated behind the `websocket` feature flag.

## What Are WebSockets?

HTTP follows a request/response pattern: the client sends a request, the server sends a response, and the connection is done. If the client wants updates, it has to keep asking — that's polling, and it's wasteful for anything real-time.

WebSocket is a different protocol. It starts as a normal HTTP request (the "upgrade handshake"), then switches to a persistent, bidirectional connection over the same TCP socket. After the handshake, either side can send a message at any time without the other asking first. No polling, no overhead per message — just an open pipe.

Common use cases: chat, live dashboards, notifications, collaborative editing, multiplayer games — anything where the server needs to push data to the client without being asked.

For deeper reading, see [RFC 6455](https://datatracker.ietf.org/doc/html/rfc6455) (the WebSocket protocol spec) and the [MDN WebSocket docs](https://developer.mozilla.org/en-US/docs/Web/API/WebSockets_API).

Rapina gives you two ways to work with WebSockets. **Raw WebSocket** is the low-level approach — you get a socket and handle messages yourself. **Relay** is the high-level approach — a pub/sub system where clients subscribe to topics, handlers control message flow, and presence tracking comes built in. Most applications should start with Relay and only drop down to raw WebSocket when they need full control over the connection.

## Raw WebSocket

The `WebSocketUpgrade` extractor completes the HTTP upgrade handshake and gives you a `WebSocket` connection. The callback runs on a dedicated tokio task.

```rust
use rapina::prelude::*;
use rapina::websocket::{WebSocketUpgrade, Message};

#[get("/ws")]
#[public]
async fn echo(upgrade: WebSocketUpgrade) -> impl IntoResponse {
    upgrade.on_upgrade(|mut socket| async move {
        while let Some(Ok(msg)) = socket.recv().await {
            if msg.is_text() || msg.is_binary() {
                socket.send(msg).await.ok();
            }
        }
    })
}
```

`WebSocketUpgrade` implements `FromRequest` — it reads the upgrade headers from the incoming request and prepares the 101 Switching Protocols response. If the request isn't a valid upgrade, it returns a 400 Bad Request automatically.

The `WebSocket` type provides:

- `recv()` — receive the next message, or `None` when the peer closes
- `send(msg)` — send a message
- `close()` — send a close frame and flush
- `split()` — split into independent `WsSender` and `WsReceiver` halves for concurrent read/write

The `Message` enum has five variants: `Text(String)`, `Binary(Vec<u8>)`, `Ping(Vec<u8>)`, `Pong(Vec<u8>)`, and `Close(Option<CloseFrame>)`. You can convert from `String`, `&str`, or `Vec<u8>` directly:

```rust
socket.send("hello".into()).await.ok();
socket.send(vec![0xDE, 0xAD].into()).await.ok();
```

Utility methods `is_text()`, `is_binary()`, `is_close()`, `is_ping()`, `is_pong()`, `as_text()`, and `as_bytes()` help with pattern matching.

Since WebSocket routes go through Rapina's middleware stack, they require authentication by default. Mark the route `#[public]` if you want unauthenticated access.

## Relay

Relay is a pub/sub system built on top of the raw WebSocket layer. Clients connect over a single WebSocket endpoint and subscribe to topics. Server-side handlers push messages to topics, and every subscribed client receives them.

### Setup

Enable Relay with `with_relay()` and call `.discover()` so channel handlers are registered:

```rust
use rapina::prelude::*;
use rapina::relay::RelayConfig;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .with_relay(RelayConfig::default())
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```

This registers a WebSocket endpoint at `/ws` (configurable) that handles the subscription protocol automatically.

### Pushing from Handlers

The `Relay` extractor lets you push messages to any topic from any HTTP handler:

```rust
use rapina::prelude::*;
use rapina::relay::Relay;

#[post("/orders")]
async fn create_order(relay: Relay, body: Json<NewOrder>) -> Result<Json<Order>> {
    let order = save_order(&body).await?;
    relay.push("orders:new", "created", &order).await?;
    Ok(Json(order))
}
```

`relay.push(topic, event, payload)` serializes the payload to JSON and delivers it to all subscribers of that topic. If nobody is subscribed, the message is silently dropped.

### Wire Protocol

All messages between client and server are JSON-encoded over the WebSocket connection.

**Client sends:**

```json
{"type": "subscribe", "topic": "orders:new"}
{"type": "unsubscribe", "topic": "orders:new"}
{"type": "message", "topic": "chat:lobby", "event": "say", "payload": {"text": "hello"}}
{"type": "ping"}
```

**Server sends:**

```json
{"type": "subscribed", "topic": "orders:new"}
{"type": "unsubscribed", "topic": "orders:new"}
{"type": "push", "topic": "orders:new", "event": "created", "payload": {"id": 1, "total": 42.50}}
{"type": "pong"}
{"type": "error", "message": "not subscribed to chat:lobby"}
```

### Connection Lifecycle

Each WebSocket connection gets a unique `conn_id`. When a client subscribes to a topic, a dedicated forwarding task is spawned that reads from the topic's broadcast channel and writes into the connection's message funnel. On unsubscribe or disconnect, the task is aborted and presence entries are cleaned up automatically.

## Channel Handlers

Channel handlers let you intercept subscription, message, and disconnect events for specific topics. They're registered with the `#[relay("pattern")]` macro and discovered automatically via `.discover()`.

```rust
use rapina::prelude::*;
use rapina::relay::{Relay, RelayEvent};

#[relay("room:*")]
async fn room_handler(event: RelayEvent, relay: Relay) -> Result<()> {
    match &event {
        RelayEvent::Join { topic, conn_id } => {
            relay.track(topic, *conn_id, serde_json::json!({}));
        }
        RelayEvent::Message { topic, event: ev, payload, .. } => {
            relay.push(topic, ev, payload).await?;
        }
        RelayEvent::Leave { topic, conn_id } => {
            // cleanup logic here
        }
    }
    Ok(())
}
```

### Pattern Matching

Patterns can be exact or prefix:

- `"chat:lobby"` — matches only the literal topic `chat:lobby`
- `"room:*"` — matches any topic starting with `room:` (e.g. `room:123`, `room:vip`)

When both an exact pattern and a prefix pattern match the same topic, the exact match wins. Among prefix patterns, longer prefixes take priority.

### RelayEvent

The `RelayEvent` enum has three variants:

- **`Join { topic, conn_id }`** — a client subscribed. This runs _before_ the subscription is confirmed, so returning `Err(...)` rejects the subscription and the client receives an error message instead of a `subscribed` confirmation.
- **`Message { topic, event, payload, conn_id }`** — a client sent a message to the topic. The handler decides whether to broadcast — if you want to relay the message, call `relay.push()` explicitly.
- **`Leave { topic, conn_id }`** — a client unsubscribed or disconnected. This is fire-and-forget — it doesn't block the disconnect.

Handlers can extract anything available through `FromRequestParts`: `Relay`, `State<T>`, `CurrentUser`, etc. The `CurrentUser` is captured at connection time, so authenticated handlers have access to the user's identity.

```rust
#[relay("private:*")]
async fn private_channel(
    event: RelayEvent,
    relay: Relay,
    user: CurrentUser,
) -> Result<()> {
    if let RelayEvent::Join { topic, conn_id } = &event {
        relay.track(topic, *conn_id, serde_json::json!({"user_id": user.id}));
    }
    Ok(())
}
```

## Presence

Presence tracking lets you know who's connected to a topic. Channel handlers register presence in the `Join` event, and it's automatically cleaned up on unsubscribe or disconnect.

```rust
// In a channel handler — track on join
RelayEvent::Join { topic, conn_id } => {
    relay.track(topic, *conn_id, serde_json::json!({
        "user_id": user.id,
        "name": user.name
    }));
}
```

The `Relay` extractor provides these presence methods:

- `relay.track(topic, conn_id, meta)` — register a client with arbitrary JSON metadata
- `relay.untrack(topic, conn_id)` — manual removal (usually not needed — cleanup is automatic)
- `relay.presence(topic)` — returns `Vec<PresenceEntry>` with each entry's `conn_id` and `meta`
- `relay.presence_count(topic)` — returns the number of connected clients

You can query presence from any HTTP handler, not just channel handlers:

```rust
#[get("/rooms/:room/members")]
#[public]
async fn room_members(relay: Relay, room: Path<String>) -> Json<Vec<PresenceEntry>> {
    let topic = format!("room:{room}");
    Json(relay.presence(&topic))
}
```

## Configuration

`RelayConfig` controls the relay system's behavior:

| Field | Default | Description |
|---|---|---|
| `path` | `"/ws"` | WebSocket endpoint path |
| `topic_capacity` | `128` | Broadcast channel buffer per topic. When full, the slowest receiver lags and loses messages. |
| `max_subscriptions_per_connection` | `50` | Maximum concurrent topic subscriptions per WebSocket connection. |

Use builder methods to customize:

```rust
RelayConfig::default()
    .with_path("/realtime")
    .with_topic_capacity(256)
    .with_max_subscriptions(100)
```

The relay endpoint goes through the normal middleware stack, including authentication. If you need unauthenticated WebSocket access, mark it as a public route:

```rust
Rapina::new()
    .with_relay(RelayConfig::default())
    .public_route("GET", "/ws")
```

## Full Example

A chat room application with presence tracking and system messages:

```rust
use rapina::prelude::*;
use rapina::relay::{Relay, RelayConfig, RelayEvent};

#[relay("room:*")]
async fn chat_room(event: RelayEvent, relay: Relay, user: CurrentUser) -> Result<()> {
    match &event {
        RelayEvent::Join { topic, conn_id } => {
            relay.track(topic, *conn_id, serde_json::json!({
                "user_id": user.id,
            }));
            relay.push(topic, "user_joined", &serde_json::json!({
                "user_id": user.id,
                "online": relay.presence_count(topic),
            })).await?;
        }
        RelayEvent::Message { topic, event: ev, payload, .. } => {
            relay.push(topic, ev, payload).await?;
        }
        RelayEvent::Leave { topic, .. } => {
            relay.push(topic, "user_left", &serde_json::json!({
                "user_id": user.id,
                "online": relay.presence_count(topic),
            })).await?;
        }
    }
    Ok(())
}

#[post("/rooms/:room/announce")]
async fn announce(relay: Relay, room: Path<String>, body: Json<Announcement>) -> Result<StatusCode> {
    let topic = format!("room:{room}");
    relay.push(&topic, "announcement", &*body).await?;
    Ok(StatusCode::OK)
}

#[get("/rooms/:room/members")]
async fn members(relay: Relay, room: Path<String>) -> Json<serde_json::Value> {
    let topic = format!("room:{room}");
    Json(serde_json::json!({
        "count": relay.presence_count(&topic),
        "members": relay.presence(&topic),
    }))
}

#[derive(Deserialize, Serialize)]
struct Announcement {
    message: String,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .with_auth(AuthConfig::new("your-secret", 3600))
        .with_relay(RelayConfig::default())
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```

Clients connect to `ws://localhost:3000/ws` with a Bearer token, subscribe to `room:general` (or any `room:*` topic), and start sending messages. The channel handler tracks presence on join, broadcasts messages, and announces departures. The HTTP endpoints let you push system announcements and query who's online.
