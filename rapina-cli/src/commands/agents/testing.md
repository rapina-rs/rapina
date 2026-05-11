## Testing

Use the Rapina test harness, not raw axum test helpers.

```rust
use rapina::testing::TestClient;

#[tokio::test]
async fn test_create_todo() {
    let app = Rapina::new().router(router);
    let client = TestClient::new(app).await;

    let res = client.post("/todos").json(&payload).send().await;
    assert_eq!(res.status(), StatusCode::CREATED);
}
```

Assert on the error `code` field, not the human message.

`TestClient` supports `.get()`, `.post()`, `.put()`, `.delete()`, `.patch()`. Request builder has `.json()`, `.header()`, `.body()`. Response has `.status()`, `.json::<T>()`, `.text()`.
