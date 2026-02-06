use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use hyper::body::Incoming;
use hyper::{Request, Response};

use crate::context::RequestContext;
use crate::error::Error;
use crate::response::{BoxBody, IntoResponse};

use super::{BoxFuture, Middleware, Next};

/// Type alias for custom key extractor functions
type KeyExtractorFn = Arc<dyn Fn(&Request<Incoming>) -> String + Send + Sync>;

/// How often to run cleanup (every N requests)
const CLEANUP_INTERVAL: u64 = 1000;

/// Remove buckets not seen in this duration
const STALE_AFTER: Duration = Duration::from_secs(600); // 10 minutes

/// Internal state for each rate-limited key
#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
}

/// How to identify clients for rate limiting
#[derive(Clone)]
pub enum KeyExtractor {
    /// Extract from X-Forwarded-For, X-Real-IP, or fallback to "unknown"
    Ip,
    /// Custom extraction function
    Custom(KeyExtractorFn),
}

impl std::fmt::Debug for KeyExtractor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyExtractor::Ip => write!(f, "KeyExtractor::Ip"),
            KeyExtractor::Custom(_) => write!(f, "KeyExtractor::Custom(...)"),
        }
    }
}

impl KeyExtractor {
    /// Extract the rate limit key from a request
    fn extract(&self, req: &Request<Incoming>) -> String {
        match self {
            KeyExtractor::Ip => Self::extract_ip(req),
            KeyExtractor::Custom(f) => f(req),
        }
    }

    fn extract_ip(req: &Request<Incoming>) -> String {
        // X-Forwarded-For can have multiple IPs: "client, proxy1, proxy2"
        // We want the leftmost (original client)
        if let Some(ip) = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
        {
            return ip.trim().to_string();
        }

        // Fallback to X-Real-IP (common with nginx)
        if let Some(ip) = req.headers().get("x-real-ip").and_then(|v| v.to_str().ok()) {
            return ip.trim().to_string();
        }

        // No proxy headers found
        "unknown".to_string()
    }
}

/// Configuration for rate limiting
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests allowed per window
    pub requests_per_second: f64,
    /// Burst capacity (max tokens that can accumulate)
    pub burst: u32,
    /// How to identify clients
    pub key_extractor: KeyExtractor,
}

impl RateLimitConfig {
    /// Create config with requests per second and burst capacity
    pub fn new(requests_per_second: f64, burst: u32) -> Self {
        Self {
            requests_per_second,
            burst,
            key_extractor: KeyExtractor::Ip,
        }
    }

    /// Convenience: configure as requests per minute
    pub fn per_minute(requests: u32) -> Self {
        Self::new(requests as f64 / 60.0, requests)
    }

    /// Set a custom key extractor
    pub fn with_key_extractor(mut self, extractor: KeyExtractor) -> Self {
        self.key_extractor = extractor;
        self
    }
}

/// Rate limiting middleware using token bucket algorithm
#[derive(Debug)]
pub struct RateLimitMiddleware {
    config: RateLimitConfig,
    buckets: Arc<DashMap<String, TokenBucket>>,
    request_count: Arc<AtomicU64>,
}

impl Clone for RateLimitMiddleware {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            buckets: Arc::clone(&self.buckets),
            request_count: Arc::clone(&self.request_count),
        }
    }
}

impl RateLimitMiddleware {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            buckets: Arc::new(DashMap::new()),
            request_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Remove buckets that haven't been accessed recently
    fn cleanup_stale_buckets(&self) {
        let now = Instant::now();
        self.buckets
            .retain(|_, bucket| now.duration_since(bucket.last_refill) < STALE_AFTER);
    }

    /// Check if request is allowed, returns Some(retry_after_secs) if rate limited
    fn check_rate_limit(&self, key: &str) -> Option<u64> {
        // Periodic cleanup: every CLEANUP_INTERVAL requests, prune stale buckets
        let count = self.request_count.fetch_add(1, Ordering::Relaxed);
        if count > 0 && count.is_multiple_of(CLEANUP_INTERVAL) {
            self.cleanup_stale_buckets();
        }

        let now = Instant::now();
        let mut bucket = self
            .buckets
            .entry(key.to_string())
            .or_insert_with(|| TokenBucket {
                tokens: self.config.burst as f64,
                last_refill: now,
            });

        // Refill tokens based on elapsed time
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        let refill = elapsed * self.config.requests_per_second;
        bucket.tokens = (bucket.tokens + refill).min(self.config.burst as f64);
        bucket.last_refill = now;

        // Try to consume one token
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            None // Request allowed
        } else {
            // Calculate when bucket will have 1 token
            let tokens_needed = 1.0 - bucket.tokens;
            let seconds_until_ready = tokens_needed / self.config.requests_per_second;
            Some(seconds_until_ready.ceil() as u64)
        }
    }
}

impl Middleware for RateLimitMiddleware {
    fn handle<'a>(
        &'a self,
        req: Request<Incoming>,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        Box::pin(async move {
            let key = self.config.key_extractor.extract(&req);

            if let Some(retry_after) = self.check_rate_limit(&key) {
                let mut response = Error::rate_limited("too many requests")
                    .with_trace_id(&ctx.trace_id)
                    .into_response();

                response
                    .headers_mut()
                    .insert("retry-after", retry_after.to_string().parse().unwrap());

                return response;
            }

            next.run(req).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_config_per_minute() {
        let config = RateLimitConfig::per_minute(60);
        assert!((config.requests_per_second - 1.0).abs() < f64::EPSILON);
        assert_eq!(config.burst, 60);
    }

    #[test]
    fn test_config_per_minute_100() {
        let config = RateLimitConfig::per_minute(100);
        assert!((config.requests_per_second - (100.0 / 60.0)).abs() < f64::EPSILON);
        assert_eq!(config.burst, 100);
    }

    #[test]
    fn test_config_new() {
        let config = RateLimitConfig::new(10.0, 50);
        assert!((config.requests_per_second - 10.0).abs() < f64::EPSILON);
        assert_eq!(config.burst, 50);
    }

    #[test]
    fn test_default_key_extractor_is_ip() {
        let config = RateLimitConfig::per_minute(100);
        assert!(matches!(config.key_extractor, KeyExtractor::Ip));
    }

    #[test]
    fn test_middleware_allows_burst() {
        let config = RateLimitConfig::new(1.0, 5); // 1 req/sec, burst of 5
        let middleware = RateLimitMiddleware::new(config);

        // Should allow 5 requests (burst capacity)
        for _ in 0..5 {
            assert!(middleware.check_rate_limit("test-key").is_none());
        }

        // 6th request should be rate limited
        assert!(middleware.check_rate_limit("test-key").is_some());
    }

    #[test]
    fn test_middleware_returns_retry_after() {
        let config = RateLimitConfig::new(1.0, 1); // 1 req/sec, burst of 1
        let middleware = RateLimitMiddleware::new(config);

        // First request allowed
        assert!(middleware.check_rate_limit("test-key").is_none());

        // Second request blocked with retry_after
        let retry_after = middleware.check_rate_limit("test-key");
        assert!(retry_after.is_some());
        assert_eq!(retry_after.unwrap(), 1); // Should wait ~1 second
    }

    #[test]
    fn test_middleware_separate_keys() {
        let config = RateLimitConfig::new(1.0, 1);
        let middleware = RateLimitMiddleware::new(config);

        // Each key gets its own bucket
        assert!(middleware.check_rate_limit("user-1").is_none());
        assert!(middleware.check_rate_limit("user-2").is_none());
        assert!(middleware.check_rate_limit("user-3").is_none());

        // But same key is limited
        assert!(middleware.check_rate_limit("user-1").is_some());
    }

    #[test]
    fn test_middleware_clone_shares_state() {
        let config = RateLimitConfig::new(1.0, 2);
        let middleware1 = RateLimitMiddleware::new(config);
        let middleware2 = middleware1.clone();

        // Use one token via middleware1
        assert!(middleware1.check_rate_limit("shared-key").is_none());

        // Use second token via middleware2 (same shared bucket)
        assert!(middleware2.check_rate_limit("shared-key").is_none());

        // Both should now see the bucket as empty
        assert!(middleware1.check_rate_limit("shared-key").is_some());
        assert!(middleware2.check_rate_limit("shared-key").is_some());
    }

    #[test]
    fn test_cleanup_removes_stale_buckets() {
        let config = RateLimitConfig::new(1.0, 5);
        let middleware = RateLimitMiddleware::new(config);

        // Create some buckets
        middleware.check_rate_limit("key-1");
        middleware.check_rate_limit("key-2");
        middleware.check_rate_limit("key-3");

        assert_eq!(middleware.buckets.len(), 3);

        // Manually age one bucket by setting last_refill to the past
        if let Some(mut bucket) = middleware.buckets.get_mut("key-1") {
            bucket.last_refill = Instant::now() - Duration::from_secs(700); // older than STALE_AFTER
        }

        // Run cleanup
        middleware.cleanup_stale_buckets();

        // key-1 should be removed, key-2 and key-3 should remain
        assert_eq!(middleware.buckets.len(), 2);
        assert!(middleware.buckets.get("key-1").is_none());
        assert!(middleware.buckets.get("key-2").is_some());
        assert!(middleware.buckets.get("key-3").is_some());
    }

    #[test]
    fn test_cleanup_triggered_periodically() {
        let config = RateLimitConfig::new(1000.0, 1000); // high burst to not get limited
        let middleware = RateLimitMiddleware::new(config);

        // Make requests and manually age a bucket
        middleware.check_rate_limit("stale-key");
        if let Some(mut bucket) = middleware.buckets.get_mut("stale-key") {
            bucket.last_refill = Instant::now() - Duration::from_secs(700);
        }

        // Make CLEANUP_INTERVAL requests to trigger cleanup
        for i in 0..super::CLEANUP_INTERVAL {
            middleware.check_rate_limit(&format!("key-{}", i));
        }

        // The stale bucket should have been cleaned up
        assert!(middleware.buckets.get("stale-key").is_none());
    }
}
