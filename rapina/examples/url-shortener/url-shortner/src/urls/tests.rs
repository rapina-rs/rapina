#[cfg(test)]
mod tests {
    use http::StatusCode;
    use rapina::cache::CacheConfig;
    use rapina::database::DatabaseConfig;
    use rapina::prelude::*;
    use rapina::testing::TestClient;

    use crate::migrations;

    async fn db() -> Rapina {
        Rapina::new()
            .with_database(DatabaseConfig::new("sqlite::memory:"))
            .await
            .unwrap()
            .run_migrations::<migrations::Migrator>()
            .await
            .unwrap()
            .discover()
    }

    async fn setup() -> TestClient {
        TestClient::new(db().await).await
    }

    async fn create_url(client: &TestClient, long_url: &str) -> String {
        let res = client
            .post("/api/v1/shorten")
            .json(&serde_json::json!({ "long_url": long_url }))
            .send()
            .await;
        let body: serde_json::Value = res.json();
        body["short_code"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn test_create_url_returns_short_code() {
        let client = setup().await;

        let res = client
            .post("/api/v1/shorten")
            .json(&serde_json::json!({ "long_url": "https://www.example.com" }))
            .send()
            .await;

        assert_eq!(res.status(), StatusCode::OK);
        let body: serde_json::Value = res.json();
        assert!(body["short_code"].as_str().is_some());
        assert_eq!(body["long_url"], "https://www.example.com");
    }

    #[tokio::test]
    async fn test_invalid_url_returns_422() {
        let client = setup().await;

        let res = client
            .post("/api/v1/shorten")
            .json(&serde_json::json!({ "long_url": "not-a-url" }))
            .send()
            .await;

        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_list_urls() {
        let client = setup().await;
        create_url(&client, "https://www.example.com").await;

        let res = client.get("/api/v1/shorten").send().await;

        assert_eq!(res.status(), StatusCode::OK);
        let body: serde_json::Value = res.json();
        assert!(!body.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_redirect_returns_302() {
        let client = setup().await;
        let code = create_url(&client, "https://www.example.com").await;

        let res = client
            .get(&format!("/api/v1/shorten/{}", code))
            .send()
            .await;

        assert_eq!(res.status(), StatusCode::FOUND);
        assert_eq!(
            res.headers().get("location").unwrap(),
            "https://www.example.com"
        );
    }

    #[tokio::test]
    async fn test_redirect_increments_click_count() {
        let client = setup().await;
        let code = create_url(&client, "https://www.example.com").await;

        client
            .get(&format!("/api/v1/shorten/{}", code))
            .send()
            .await;
        client
            .get(&format!("/api/v1/shorten/{}", code))
            .send()
            .await;

        let res = client.get("/api/v1/shorten").send().await;
        let body: serde_json::Value = res.json();
        let item = body.as_array().unwrap().first().unwrap();
        assert_eq!(item["click_count"], 2);
    }

    #[tokio::test]
    async fn test_redirect_not_found() {
        let client = setup().await;

        let res = client
            .get("/api/v1/shorten/nonexistent")
            .send()
            .await;

        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_expired_url_returns_410() {
        let client = setup().await;

        let res = client
            .post("/api/v1/shorten")
            .json(&serde_json::json!({
                "long_url": "https://www.example.com",
                "expires_at": "2000-01-01T00:00:00Z"
            }))
            .send()
            .await;
        let body: serde_json::Value = res.json();
        let code = body["short_code"].as_str().unwrap();

        let res = client
            .get(&format!("/api/v1/shorten/{}", code))
            .send()
            .await;

        assert_eq!(res.status(), StatusCode::GONE);
    }

    #[tokio::test]
    async fn test_delete_url() {
        let client = setup().await;
        let code = create_url(&client, "https://www.example.com").await;

        let res = client
            .delete(&format!("/api/v1/shorten/{}", code))
            .send()
            .await;
        assert_eq!(res.status(), StatusCode::OK);

        let res = client
            .get(&format!("/api/v1/shorten/{}", code))
            .send()
            .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_nonexistent_returns_404() {
        let client = setup().await;

        let res = client
            .delete("/api/v1/shorten/nonexistent")
            .send()
            .await;

        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_rate_limit() {
        let app = Rapina::new()
            .with_rate_limit(RateLimitConfig::per_minute(5))
            .with_database(DatabaseConfig::new("sqlite::memory:"))
            .await
            .unwrap()
            .run_migrations::<migrations::Migrator>()
            .await
            .unwrap()
            .discover();
        let client = TestClient::new(app).await;

        for _ in 0..5 {
            let res = client.get("/api/v1/shorten").send().await;
            assert_ne!(res.status(), StatusCode::TOO_MANY_REQUESTS);
        }

        let res = client.get("/api/v1/shorten").send().await;
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    async fn setup_with_cache() -> TestClient {
        let app = Rapina::new()
            .with_cache(CacheConfig::in_memory(100))
            .await
            .unwrap()
            .with_database(DatabaseConfig::new("sqlite::memory:"))
            .await
            .unwrap()
            .run_migrations::<migrations::Migrator>()
            .await
            .unwrap()
            .discover();
        TestClient::new(app).await
    }

    #[tokio::test]
    async fn test_cache_miss_then_hit() {
        let client = setup_with_cache().await;
        let code = create_url(&client, "https://www.example.com").await;

        let first = client
            .get(&format!("/api/v1/shorten/{}", code))
            .send()
            .await;
        assert_eq!(first.headers().get("x-cache").unwrap(), "MISS");

        let second = client
            .get(&format!("/api/v1/shorten/{}", code))
            .send()
            .await;
        assert_eq!(second.headers().get("x-cache").unwrap(), "HIT");
    }

    #[tokio::test]
    async fn test_cache_invalidated_on_delete() {
        let client = setup_with_cache().await;
        let code = create_url(&client, "https://www.example.com").await;

        client
            .get(&format!("/api/v1/shorten/{}", code))
            .send()
            .await;

        client
            .delete(&format!("/api/v1/shorten/{}", code))
            .send()
            .await;

        let res = client
            .get(&format!("/api/v1/shorten/{}", code))
            .send()
            .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}
