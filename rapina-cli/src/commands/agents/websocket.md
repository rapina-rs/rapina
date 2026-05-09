## WebSockets

WebSocket handlers use the `#[ws]` macro:

```rust
#[ws("/ws")]
async fn handle_ws(mut socket: WebSocket) {
    while let Some(msg) = socket.recv().await {
        if let Ok(msg) = msg {
            socket.send(msg).await.ok();
        }
    }
}
```

Register the handler in the router the same way as HTTP handlers. The upgrade handshake is handled automatically by Rapina.
