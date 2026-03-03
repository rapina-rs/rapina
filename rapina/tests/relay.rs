#![cfg(feature = "websocket")]

use std::time::Duration;

use rapina::extract::FromRequestParts;
use rapina::futures_util::{SinkExt, StreamExt};
use rapina::prelude::*;
use rapina::relay::protocol::ServerMessage;
use rapina::relay::{Relay, RelayConfig};
use rapina::response::IntoResponse;
use rapina::testing::TestClient;
use rapina::tokio_tungstenite::tungstenite;

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

/// App that uses the `Relay` extractor in a proc-macro handler, proving
/// the full path: proc macro -> is_parts_only_extractor -> FromRequestParts.
fn relay_app() -> Rapina {
    Rapina::new()
        .with_introspection(false)
        .with_relay(RelayConfig::default())
        .router(Router::new().route(
            http::Method::POST,
            "/push",
            |req, params, state| async move {
                let (parts, _body) = req.into_parts();
                let relay = match Relay::from_request_parts(&parts, &params, &state).await {
                    Ok(r) => r,
                    Err(e) => return e.into_response(),
                };
                relay
                    .push("orders:new", "created", &serde_json::json!({"id": 1}))
                    .unwrap();
                StatusCode::OK.into_response()
            },
        ))
}

#[tokio::test]
async fn test_subscribe_and_receive_push() {
    let client = TestClient::new(relay_app()).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"orders:new"}"#).await;
    let msg = recv_server_msg(&mut ws_rx).await;
    assert!(matches!(msg, ServerMessage::Subscribed { topic } if topic == "orders:new"));

    let resp = client.post("/push").send().await;
    assert_eq!(resp.status(), StatusCode::OK);

    let msg = recv_server_msg(&mut ws_rx).await;
    match msg {
        ServerMessage::Push {
            topic,
            event,
            payload,
        } => {
            assert_eq!(topic, "orders:new");
            assert_eq!(event, "created");
            assert_eq!(payload, serde_json::json!({"id": 1}));
        }
        other => panic!("expected Push, got {other:?}"),
    }

    ws_tx.close().await.ok();
}

#[tokio::test]
async fn test_unsubscribe() {
    let client = TestClient::new(relay_app()).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"t1"}"#).await;
    let _ = recv_server_msg(&mut ws_rx).await;

    send_json(&mut ws_tx, r#"{"type":"unsubscribe","topic":"t1"}"#).await;
    let msg = recv_server_msg(&mut ws_rx).await;
    assert!(matches!(msg, ServerMessage::Unsubscribed { topic } if topic == "t1"));

    ws_tx.close().await.ok();
}

#[tokio::test]
async fn test_ping_pong() {
    let client = TestClient::new(relay_app()).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    send_json(&mut ws_tx, r#"{"type":"ping"}"#).await;
    let msg = recv_server_msg(&mut ws_rx).await;
    assert!(matches!(msg, ServerMessage::Pong));

    ws_tx.close().await.ok();
}

#[tokio::test]
async fn test_invalid_message_returns_error() {
    let client = TestClient::new(relay_app()).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    send_json(&mut ws_tx, r#"{"not":"valid"}"#).await;
    let msg = recv_server_msg(&mut ws_rx).await;
    assert!(matches!(msg, ServerMessage::Error { .. }));

    ws_tx.close().await.ok();
}

#[tokio::test]
async fn test_message_to_unsubscribed_topic_returns_error() {
    let client = TestClient::new(relay_app()).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    send_json(
        &mut ws_tx,
        r#"{"type":"message","topic":"t1","event":"e","payload":{}}"#,
    )
    .await;
    let msg = recv_server_msg(&mut ws_rx).await;
    match msg {
        ServerMessage::Error { message } => {
            assert!(message.contains("not subscribed"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    ws_tx.close().await.ok();
}

#[tokio::test]
async fn test_multiple_subscribers_same_topic() {
    let client = TestClient::new(relay_app()).await;
    let addr = client.addr();

    let (mut tx1, mut rx1) = ws_connect(addr).await;
    let (mut tx2, mut rx2) = ws_connect(addr).await;

    send_json(&mut tx1, r#"{"type":"subscribe","topic":"orders:new"}"#).await;
    let _ = recv_server_msg(&mut rx1).await;
    send_json(&mut tx2, r#"{"type":"subscribe","topic":"orders:new"}"#).await;
    let _ = recv_server_msg(&mut rx2).await;

    let resp = client.post("/push").send().await;
    assert_eq!(resp.status(), StatusCode::OK);

    let msg1 = recv_server_msg(&mut rx1).await;
    let msg2 = recv_server_msg(&mut rx2).await;

    assert!(matches!(msg1, ServerMessage::Push { .. }));
    assert!(matches!(msg2, ServerMessage::Push { .. }));

    tx1.close().await.ok();
    tx2.close().await.ok();
}

#[tokio::test]
async fn test_push_to_empty_topic_succeeds() {
    let client = TestClient::new(relay_app()).await;

    let resp = client.post("/push").send().await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_subscription_limit() {
    let app = Rapina::new()
        .with_introspection(false)
        .with_relay(RelayConfig::default().with_max_subscriptions(2));

    let client = TestClient::new(app).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"t1"}"#).await;
    let _ = recv_server_msg(&mut ws_rx).await;
    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"t2"}"#).await;
    let _ = recv_server_msg(&mut ws_rx).await;

    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"t3"}"#).await;
    let msg = recv_server_msg(&mut ws_rx).await;
    match msg {
        ServerMessage::Error { message } => {
            assert!(message.contains("subscription limit"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    ws_tx.close().await.ok();
}

#[tokio::test]
async fn test_non_upgrade_request_returns_400() {
    let client = TestClient::new(relay_app()).await;
    let resp = client.get("/ws").send().await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_duplicate_subscribe_is_idempotent() {
    let client = TestClient::new(relay_app()).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"t1"}"#).await;
    let msg = recv_server_msg(&mut ws_rx).await;
    assert!(matches!(msg, ServerMessage::Subscribed { .. }));

    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"t1"}"#).await;
    let msg = recv_server_msg(&mut ws_rx).await;
    assert!(matches!(msg, ServerMessage::Subscribed { .. }));

    ws_tx.close().await.ok();
}

/// Verifies the Relay extractor works through a proc-macro handler.
#[tokio::test]
async fn test_relay_extractor_via_proc_macro() {
    // This handler is defined with the proc macro, proving that "Relay"
    // is recognized by is_parts_only_extractor and extracted correctly.
    #[rapina::post("/notify")]
    #[rapina::public]
    async fn notify(relay: Relay) -> StatusCode {
        relay
            .push("events", "ping", &serde_json::json!({"ok": true}))
            .unwrap();
        StatusCode::OK
    }

    let app = Rapina::new()
        .with_introspection(false)
        .with_relay(RelayConfig::default())
        .discover();

    let client = TestClient::new(app).await;
    let addr = client.addr();

    // Subscribe a WS client.
    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;
    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"events"}"#).await;
    let _ = recv_server_msg(&mut ws_rx).await;

    // Hit the proc-macro handler.
    let resp = client.post("/notify").send().await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify the push arrived.
    let msg = recv_server_msg(&mut ws_rx).await;
    match msg {
        ServerMessage::Push {
            topic,
            event,
            payload,
        } => {
            assert_eq!(topic, "events");
            assert_eq!(event, "ping");
            assert_eq!(payload, serde_json::json!({"ok": true}));
        }
        other => panic!("expected Push, got {other:?}"),
    }

    ws_tx.close().await.ok();
}

/// After unsubscribe, pushes to that topic should not be received.
#[tokio::test]
async fn test_unsubscribe_stops_delivery() {
    let client = TestClient::new(relay_app()).await;
    let addr = client.addr();

    let (mut ws_tx, mut ws_rx) = ws_connect(addr).await;

    // Subscribe, then immediately unsubscribe.
    send_json(&mut ws_tx, r#"{"type":"subscribe","topic":"orders:new"}"#).await;
    let _ = recv_server_msg(&mut ws_rx).await;
    send_json(&mut ws_tx, r#"{"type":"unsubscribe","topic":"orders:new"}"#).await;
    let _ = recv_server_msg(&mut ws_rx).await;

    // Push — should not reach this client.
    client.post("/push").send().await;

    // Give a short window for any erroneous delivery.
    let result = tokio::time::timeout(Duration::from_millis(100), ws_rx.next()).await;
    assert!(
        result.is_err(),
        "should not receive a message after unsubscribe"
    );

    ws_tx.close().await.ok();
}
