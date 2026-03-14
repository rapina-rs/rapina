//! WebSocket chat — open http://127.0.0.1:3000 in multiple tabs.

use bytes::Bytes;
use http_body_util::Full;
use rapina::prelude::*;
use rapina::websocket::{Message, WebSocket, WebSocketUpgrade};
use std::sync::Arc;
use tokio::sync::broadcast;

struct ChatRoom {
    tx: broadcast::Sender<String>,
}

#[get("/ws")]
#[public]
async fn chat(upgrade: WebSocketUpgrade, room: State<ChatRoom>) -> rapina::http::Response<Full<Bytes>> {
    upgrade.on_upgrade(|socket| handle_connection(socket, room.into_inner()))
}

async fn handle_connection(socket: WebSocket, room: Arc<ChatRoom>) {
    let (mut writer, mut reader) = socket.split();
    let mut broadcast_rx = room.tx.subscribe();

    // Forward broadcast messages to this client
    let mut forward = tokio::spawn(async move {
        while let Ok(msg) = broadcast_rx.recv().await {
            if writer.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    // Read from this client, broadcast to all others
    let broadcast_tx = room.tx.clone();
    let mut ingest = tokio::spawn(async move {
        while let Some(Ok(msg)) = reader.recv().await {
            if let Some(text) = msg.as_text() {
                let _ = broadcast_tx.send(text.to_string());
            }
        }
    });

    tokio::select! {
        _ = &mut forward => ingest.abort(),
        _ = &mut ingest => forward.abort(),
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt::init();

    let (tx, _) = broadcast::channel(100);

    Rapina::new()
        .state(ChatRoom { tx })
        .router(Router::new().route(Method::GET, "/", |_, _, _| async {
            rapina::http::Response::builder()
                .header("content-type", "text/html")
                .body(Full::new(Bytes::from(include_str!("index.html"))))
                .unwrap()
        }))
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
