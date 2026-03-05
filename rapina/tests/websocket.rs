#![cfg(feature = "websocket")]

use rapina::extract::FromRequest;
use rapina::futures_util::{SinkExt, StreamExt};
use rapina::prelude::*;
use rapina::response::IntoResponse;
use rapina::testing::TestClient;
use rapina::tokio_tungstenite::tungstenite;
use rapina::websocket::WebSocketUpgrade;

fn echo_app() -> Rapina {
    Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/ws", |req, params, state| async move {
                let ws = match WebSocketUpgrade::from_request(req, &params, &state).await {
                    Ok(ws) => ws,
                    Err(e) => return e.into_response(),
                };
                ws.on_upgrade(|mut socket| async move {
                    while let Some(Ok(msg)) = socket.recv().await {
                        if msg.is_text() || msg.is_binary() {
                            socket.send(msg).await.ok();
                        }
                    }
                })
            }),
        )
}

#[tokio::test]
async fn test_echo_text() {
    let client = TestClient::new(echo_app()).await;
    let (mut ws, _) =
        rapina::tokio_tungstenite::connect_async(format!("ws://{}/ws", client.addr()))
            .await
            .unwrap();

    ws.send(tungstenite::Message::Text("hello".into()))
        .await
        .unwrap();

    let msg = ws.next().await.unwrap().unwrap();
    assert_eq!(msg, tungstenite::Message::Text("hello".into()));

    ws.close(None).await.ok();
}

#[tokio::test]
async fn test_echo_binary() {
    let client = TestClient::new(echo_app()).await;
    let (mut ws, _) =
        rapina::tokio_tungstenite::connect_async(format!("ws://{}/ws", client.addr()))
            .await
            .unwrap();

    let payload = vec![0xDE, 0xAD, 0xBE, 0xEF];
    ws.send(tungstenite::Message::Binary(payload.clone().into()))
        .await
        .unwrap();

    let msg = ws.next().await.unwrap().unwrap();
    assert_eq!(msg, tungstenite::Message::Binary(payload.into()));

    ws.close(None).await.ok();
}

#[tokio::test]
async fn test_non_upgrade_returns_400() {
    let client = TestClient::new(echo_app()).await;
    let resp = client.get("/ws").send().await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_concurrent_connections() {
    let client = TestClient::new(echo_app()).await;
    let addr = client.addr();

    let mut handles = vec![];
    for i in 0..5 {
        handles.push(tokio::spawn(async move {
            let (mut ws, _) = rapina::tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
                .await
                .unwrap();

            let msg = format!("conn-{i}");
            ws.send(tungstenite::Message::Text(msg.clone().into()))
                .await
                .unwrap();

            let reply = ws.next().await.unwrap().unwrap();
            assert_eq!(reply, tungstenite::Message::Text(msg.into()));

            ws.close(None).await.ok();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}
