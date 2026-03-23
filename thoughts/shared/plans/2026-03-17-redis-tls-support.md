# Redis TLS Support Implementation Plan

## Overview

Add TLS support (custom CA certs and mutual TLS) to the Redis cache backend. Currently `RedisCache::connect` uses `redis::Client::open(url)` which only supports system root certificates. This adds `redis::Client::build_with_tls` support for environments requiring internal PKI or client certificate authentication.

## Current State Analysis

- `RedisCache::connect(url)` at `cache_redis.rs:55` calls `redis::Client::open(url)` — no TLS configuration
- Redis crate dependency at `Cargo.toml:86` enables only `tokio-comp` feature
- `CacheConfig::Redis { url: String }` is the sole Redis config variant (`cache.rs:183`)
- The redis crate (v1.0.5) has a `tokio-rustls-comp` feature that combines `tokio-comp` + `tls-rustls` + `tokio-rustls`
- `Client::build_with_tls(conn_info, TlsCertificates)` is the TLS connection API (`redis/src/client.rs:161`)
- `TlsCertificates` has `client_tls: Option<ClientTlsConfig>` and `root_cert: Option<Vec<u8>>` — all PEM bytes
- The URL must use `rediss://` scheme for TLS (`redis/src/tls.rs:54`)

### Key Discoveries:
- `redis::TlsCertificates` takes raw PEM bytes, not file paths — our wrapper reads files (`cache_redis.rs`)
- `tokio-rustls-comp` is the correct compound feature — enables async TLS in one flag (`redis Cargo.toml:146`)
- Project already uses `rustls` via sea-orm (`Cargo.toml:76`) — consistent choice
- `DatabaseConfig` at `database.rs:59` provides the `from_env()` pattern to follow

## Desired End State

Users can connect to Redis over TLS with custom CA certs and/or client certificates:

```rust
// Custom CA only
let tls = RedisTlsConfig::new()
    .ca_cert_path("certs/ca.pem");

// Mutual TLS
let tls = RedisTlsConfig::new()
    .ca_cert_path("certs/ca.pem")
    .client_cert_path("certs/client.pem")
    .client_key_path("certs/client-key.pem");

// From environment variables
let tls = RedisTlsConfig::from_env()?;

Rapina::new()
    .with_cache(CacheConfig::redis_tls("rediss://redis.internal:6380", tls)).await?
```

### Verification:
- Unit tests for `RedisTlsConfig` builder and `from_env()`
- Unit test that `CacheConfig::redis_tls` produces the correct variant
- `cargo test -p rapina --features cache-redis-tls` passes
- `cargo clippy -p rapina --features cache-redis-tls` passes
- `cargo fmt --check -p rapina` passes

## What We're NOT Doing

- Changing the existing `CacheConfig::redis(url)` API — it continues to work as-is
- Adding TLS support to the relay backend (future work, separate ticket)
- Supporting `tls-native-tls` — using `rustls` consistently with the rest of the project
- Adding integration tests that require a TLS-enabled Redis instance (would need cert fixtures and a configured server)
- `rediss://` URL validation at config time — the redis crate validates this at connect time with a clear error

## Implementation Approach

TDD: write tests first for the config structs and builder, then implement the code.

## Phase 1: Feature Flag and `RedisTlsConfig` Struct

### Overview
Add the `cache-redis-tls` feature flag and `RedisTlsConfig` builder struct with `from_env()`.

### Changes Required:

#### 1. Add feature flag
**File**: `rapina/Cargo.toml`
**Changes**: Update redis dependency features and add new feature flag

Change line 86:
```toml
redis = { version = "1.0", optional = true, features = ["tokio-comp"] }
```
to:
```toml
redis = { version = "1.0", optional = true, features = ["tokio-comp"] }
```
(dependency stays the same — the TLS feature is additive via the feature flag)

Add after line 120 (`cache-redis = ["redis"]`):
```toml
cache-redis-tls = ["cache-redis", "redis/tokio-rustls-comp"]
```

#### 2. Add `RedisTlsConfig` struct and tests
**File**: `rapina/src/cache_redis.rs`
**Changes**: Add struct with builder pattern and `from_env()`, gated behind `cache-redis-tls`

```rust
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
    fn into_tls_certificates(self) -> Result<redis::TlsCertificates, std::io::Error> {
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
                Some(redis::TlsCertificates::ClientTlsConfig {
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
```

Note: The `into_tls_certificates` method constructs `redis::tls::ClientTlsConfig` (not `redis::TlsCertificates::ClientTlsConfig`). The exact type path is `redis::ClientTlsConfig { client_cert, client_key }`. Verify during implementation.

### Tests to add (in `cache_redis.rs` `mod tests`):

```rust
#[cfg(feature = "cache-redis-tls")]
#[test]
fn test_redis_tls_config_builder() {
    let config = RedisTlsConfig::new()
        .ca_cert_path("/path/to/ca.pem")
        .client_cert_path("/path/to/cert.pem")
        .client_key_path("/path/to/key.pem");

    assert_eq!(config.ca_cert_path.as_deref(), Some("/path/to/ca.pem"));
    assert_eq!(config.client_cert_path.as_deref(), Some("/path/to/cert.pem"));
    assert_eq!(config.client_key_path.as_deref(), Some("/path/to/key.pem"));
}

#[cfg(feature = "cache-redis-tls")]
#[test]
fn test_redis_tls_config_from_env() {
    std::env::set_var("REDIS_CA_CERT", "/env/ca.pem");
    std::env::set_var("REDIS_CLIENT_CERT", "/env/cert.pem");
    std::env::set_var("REDIS_CLIENT_KEY", "/env/key.pem");

    let config = RedisTlsConfig::from_env();

    assert_eq!(config.ca_cert_path.as_deref(), Some("/env/ca.pem"));
    assert_eq!(config.client_cert_path.as_deref(), Some("/env/cert.pem"));
    assert_eq!(config.client_key_path.as_deref(), Some("/env/key.pem"));

    std::env::remove_var("REDIS_CA_CERT");
    std::env::remove_var("REDIS_CLIENT_CERT");
    std::env::remove_var("REDIS_CLIENT_KEY");
}

#[cfg(feature = "cache-redis-tls")]
#[test]
fn test_redis_tls_config_partial_client_tls_fails() {
    let config = RedisTlsConfig::new()
        .client_cert_path("/path/to/cert.pem");
    // Only cert without key should error
    let result = config.into_tls_certificates();
    assert!(result.is_err());
}

#[cfg(feature = "cache-redis-tls")]
#[test]
fn test_redis_tls_config_empty_is_valid() {
    let config = RedisTlsConfig::new();
    // Empty config should produce valid TlsCertificates (no custom CA, no mTLS)
    let result = config.into_tls_certificates();
    assert!(result.is_ok());
}
```

### Success Criteria:

#### Automated Verification:
- [x] Tests compile: `cargo test -p rapina --features cache-redis-tls --no-run`
- [x] Tests pass: `cargo test -p rapina --features cache-redis-tls -- redis_tls`
- [x] `cargo fmt --check -p rapina` passes
- [x] `cargo clippy -p rapina --features cache-redis-tls` passes

---

## Phase 2: Wire TLS into `RedisCache::connect_tls` and `CacheConfig`

### Overview
Add the `connect_tls` method on `RedisCache` and the `CacheConfig::RedisTls` variant + constructor.

### Changes Required:

#### 1. Add `RedisCache::connect_tls`
**File**: `rapina/src/cache_redis.rs`
**Changes**: Add method after `connect()`

```rust
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
```

#### 2. Add `CacheConfig::RedisTls` variant
**File**: `rapina/src/cache.rs`
**Changes**: Add variant to enum, constructor, and build() match arm

Add to `CacheConfig` enum:
```rust
/// Redis-backed cache with TLS (requires `cache-redis-tls` feature).
#[cfg(feature = "cache-redis-tls")]
RedisTls {
    url: String,
    tls: crate::cache_redis::RedisTlsConfig,
},
```

Add constructor:
```rust
/// Creates a Redis cache configuration with TLS.
///
/// The URL must use the `rediss://` scheme.
#[cfg(feature = "cache-redis-tls")]
pub fn redis_tls(url: &str, tls: crate::cache_redis::RedisTlsConfig) -> Self {
    CacheConfig::RedisTls {
        url: url.to_string(),
        tls,
    }
}
```

Add match arm in `build()`:
```rust
#[cfg(feature = "cache-redis-tls")]
CacheConfig::RedisTls { url, tls } => {
    let backend = crate::cache_redis::RedisCache::connect_tls(&url, tls)
        .await
        .map_err(|e| {
            std::io::Error::other(format!("Redis TLS connection failed: {}", e))
        })?;
    Ok(Arc::new(backend))
}
```

#### 3. Re-export `RedisTlsConfig` in prelude
**File**: `rapina/src/lib.rs`
**Changes**: Add conditional re-export near the existing `cache_redis` module declaration

```rust
#[cfg(feature = "cache-redis-tls")]
pub use cache_redis::RedisTlsConfig;
```

### Tests to add:

In `cache.rs` `mod tests`:
```rust
#[cfg(feature = "cache-redis-tls")]
#[test]
fn test_cache_config_redis_tls() {
    let tls = crate::cache_redis::RedisTlsConfig::new()
        .ca_cert_path("/path/to/ca.pem");
    let config = CacheConfig::redis_tls("rediss://redis.internal:6380", tls);
    assert!(matches!(config, CacheConfig::RedisTls { .. }));
}
```

### Success Criteria:

#### Automated Verification:
- [x] All tests pass: `cargo test -p rapina --features cache-redis-tls` (331 passed)
- [x] Existing tests still pass: `cargo test -p rapina`
- [x] `cargo fmt --check -p rapina` passes
- [x] `cargo clippy -p rapina --features cache-redis-tls` passes

#### Manual Verification:
- [ ] Confirm `rediss://` URL with custom CA cert connects to a TLS-enabled Redis instance
- [ ] Confirm mTLS works with client cert + key
- [ ] Confirm error message is clear when cert file doesn't exist
- [ ] Confirm error message is clear when `redis://` (not `rediss://`) is used with TLS config
