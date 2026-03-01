//! Redis-backed cache backend.
//!
//! Requires the `cache-redis` feature flag.
//!
//! ```toml
//! [dependencies]
//! rapina = { version = "0.7", features = ["cache-redis"] }
//! ```

use std::time::Duration;

use bytes::Bytes;
use redis::AsyncCommands;

use crate::cache::{CacheBackend, CachedResponse};

type CacheFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Serializable form of CachedResponse for Redis storage.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl From<&CachedResponse> for StoredResponse {
    fn from(r: &CachedResponse) -> Self {
        Self {
            status: r.status,
            headers: r.headers.clone(),
            body: r.body.to_vec(),
        }
    }
}

impl From<StoredResponse> for CachedResponse {
    fn from(s: StoredResponse) -> Self {
        Self {
            status: s.status,
            headers: s.headers,
            body: Bytes::from(s.body),
        }
    }
}

/// Redis cache backend using multiplexed async connections.
pub struct RedisCache {
    conn: redis::aio::MultiplexedConnection,
    prefix: String,
}

impl RedisCache {
    /// Connects to Redis at the given URL.
    pub async fn connect(url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(url)?;
        let conn = client.get_multiplexed_async_connection().await?;
        Ok(Self {
            conn,
            prefix: "rapina:".to_string(),
        })
    }

    /// Sets a custom key prefix (default: "rapina:").
    pub fn with_prefix(mut self, prefix: &str) -> Self {
        self.prefix = prefix.to_string();
        self
    }

    fn prefixed_key(&self, key: &str) -> String {
        format!("{}{}", self.prefix, key)
    }
}

impl CacheBackend for RedisCache {
    fn get(&self, key: &str) -> CacheFuture<'_, Option<CachedResponse>> {
        let full_key = self.prefixed_key(key);
        let mut conn = self.conn.clone();

        Box::pin(async move {
            let data: Option<String> = conn.get(&full_key).await.ok()?;
            let stored: StoredResponse = serde_json::from_str(&data?).ok()?;
            Some(stored.into())
        })
    }

    fn set(&self, key: &str, response: CachedResponse, ttl: Duration) -> CacheFuture<'_, ()> {
        let full_key = self.prefixed_key(key);
        let mut conn = self.conn.clone();
        let stored = StoredResponse::from(&response);

        Box::pin(async move {
            let json = match serde_json::to_string(&stored) {
                Ok(j) => j,
                Err(_) => return,
            };

            let _: Result<(), _> = conn.set_ex(&full_key, &json, ttl.as_secs()).await;
        })
    }

    fn invalidate_prefix(&self, prefix: &str) -> CacheFuture<'_, ()> {
        let pattern = format!("{}{}*", self.prefix, prefix);
        let mut conn = self.conn.clone();

        Box::pin(async move {
            let keys: Vec<String> = match redis::cmd("SCAN")
                .arg(0)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(100)
                .query_async::<Vec<redis::Value>>(&mut conn)
                .await
            {
                Ok(result) => {
                    if result.len() >= 2 {
                        if let Some(redis::Value::Array(keys)) = result.into_iter().nth(1) {
                            keys.into_iter()
                                .filter_map(|v| {
                                    if let redis::Value::BulkString(s) = v {
                                        String::from_utf8(s).ok()
                                    } else {
                                        None
                                    }
                                })
                                .collect()
                        } else {
                            return;
                        }
                    } else {
                        return;
                    }
                }
                Err(_) => return,
            };

            if !keys.is_empty() {
                let _: Result<(), _> = redis::cmd("DEL").arg(&keys).query_async(&mut conn).await;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stored_response_roundtrip() {
        let cached = CachedResponse {
            status: 200,
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body: Bytes::from(r#"{"ok":true}"#),
        };

        let stored = StoredResponse::from(&cached);
        let json = serde_json::to_string(&stored).unwrap();
        let restored: StoredResponse = serde_json::from_str(&json).unwrap();
        let result: CachedResponse = restored.into();

        assert_eq!(result.status, 200);
        assert_eq!(result.headers.len(), 1);
        assert_eq!(result.body, Bytes::from(r#"{"ok":true}"#));
    }

    // Integration tests require a running Redis instance.
    // Run with: cargo test --features cache-redis -- --ignored
    #[ignore]
    #[tokio::test]
    async fn test_redis_cache_set_and_get() {
        let cache = RedisCache::connect("redis://127.0.0.1:6379")
            .await
            .expect("Redis connection failed");

        let response = CachedResponse {
            status: 200,
            headers: vec![],
            body: Bytes::from("test data"),
        };

        cache
            .set("test:key1", response, Duration::from_secs(10))
            .await;

        let result = cache.get("test:key1").await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().body, Bytes::from("test data"));
    }
}
