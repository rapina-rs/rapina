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

    /// Connects to Redis with TLS at the given URL.
    ///
    /// The URL must use the `rediss://` scheme.
    #[cfg(feature = "cache-redis-tls")]
    pub async fn connect_tls(
        url: &str,
        tls_config: RedisTlsConfig,
    ) -> Result<Self, std::io::Error> {
        let tls_certs = tls_config.into_tls_certificates()?;
        let client = redis::Client::build_with_tls(url, tls_certs)
            .map_err(|e| std::io::Error::other(format!("Redis TLS client error: {}", e)))?;
        let conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| std::io::Error::other(format!("Redis TLS connection failed: {}", e)))?;
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

/// TLS configuration for Redis connections.
///
/// Requires the `cache-redis-tls` feature flag.
///
/// ```toml
/// [dependencies]
/// rapina = { version = "0.10", features = ["cache-redis-tls"] }
/// ```
#[cfg(feature = "cache-redis-tls")]
#[derive(Clone, Debug, Default)]
pub struct RedisTlsConfig {
    ca_cert_path: Option<String>,
    client_cert_path: Option<String>,
    client_key_path: Option<String>,
}

#[cfg(feature = "cache-redis-tls")]
impl RedisTlsConfig {
    /// Creates an empty TLS configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the path to a PEM-encoded CA certificate file.
    pub fn ca_cert_path(mut self, path: &str) -> Self {
        self.ca_cert_path = Some(path.to_string());
        self
    }

    /// Sets the path to a PEM-encoded client certificate file (for mTLS).
    pub fn client_cert_path(mut self, path: &str) -> Self {
        self.client_cert_path = Some(path.to_string());
        self
    }

    /// Sets the path to a PEM-encoded client private key file (for mTLS).
    pub fn client_key_path(mut self, path: &str) -> Self {
        self.client_key_path = Some(path.to_string());
        self
    }

    /// Creates a TLS configuration from environment variables:
    ///
    /// - `REDIS_CA_CERT` — path to CA certificate PEM file
    /// - `REDIS_CLIENT_CERT` — path to client certificate PEM file
    /// - `REDIS_CLIENT_KEY` — path to client private key PEM file
    pub fn from_env() -> Self {
        Self {
            ca_cert_path: std::env::var("REDIS_CA_CERT").ok(),
            client_cert_path: std::env::var("REDIS_CLIENT_CERT").ok(),
            client_key_path: std::env::var("REDIS_CLIENT_KEY").ok(),
        }
    }

    /// Reads certificate files and builds `redis::TlsCertificates`.
    pub(crate) fn into_tls_certificates(self) -> Result<redis::TlsCertificates, std::io::Error> {
        let root_cert = match self.ca_cert_path {
            Some(path) => Some(std::fs::read(&path).map_err(|e| {
                std::io::Error::other(format!("Failed to read CA cert '{}': {}", path, e))
            })?),
            None => None,
        };

        let client_tls = match (self.client_cert_path, self.client_key_path) {
            (Some(cert_path), Some(key_path)) => {
                let client_cert = std::fs::read(&cert_path).map_err(|e| {
                    std::io::Error::other(format!(
                        "Failed to read client cert '{}': {}",
                        cert_path, e
                    ))
                })?;
                let client_key = std::fs::read(&key_path).map_err(|e| {
                    std::io::Error::other(format!(
                        "Failed to read client key '{}': {}",
                        key_path, e
                    ))
                })?;
                Some(redis::ClientTlsConfig {
                    client_cert,
                    client_key,
                })
            }
            (None, None) => None,
            _ => {
                return Err(std::io::Error::other(
                    "Both client_cert_path and client_key_path must be set for mTLS",
                ));
            }
        };

        Ok(redis::TlsCertificates {
            client_tls,
            root_cert,
        })
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

    #[cfg(feature = "cache-redis-tls")]
    #[test]
    fn test_redis_tls_config_builder() {
        let config = RedisTlsConfig::new()
            .ca_cert_path("/path/to/ca.pem")
            .client_cert_path("/path/to/cert.pem")
            .client_key_path("/path/to/key.pem");

        assert_eq!(config.ca_cert_path.as_deref(), Some("/path/to/ca.pem"));
        assert_eq!(
            config.client_cert_path.as_deref(),
            Some("/path/to/cert.pem")
        );
        assert_eq!(config.client_key_path.as_deref(), Some("/path/to/key.pem"));
    }

    #[cfg(feature = "cache-redis-tls")]
    #[test]
    fn test_redis_tls_config_partial_client_tls_fails() {
        let config = RedisTlsConfig::new().client_cert_path("/path/to/cert.pem");
        let result = config.into_tls_certificates();
        assert!(result.is_err());
    }

    #[cfg(feature = "cache-redis-tls")]
    #[test]
    fn test_redis_tls_config_empty_is_valid() {
        let config = RedisTlsConfig::new();
        let result = config.into_tls_certificates();
        assert!(result.is_ok());
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
