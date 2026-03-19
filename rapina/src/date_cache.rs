use std::time::{Duration, SystemTime};

use http::HeaderValue;
use tokio::sync::watch;

/// Caches the HTTP `Date` header value and updates it once per second.
///
/// Avoids per-request `SystemTime::now()` + formatting overhead by running a
/// background tokio task that refreshes the value every second. Readers call
/// [`header_value()`](Self::header_value) which clones a `HeaderValue` (cheap —
/// backed by reference-counted `Bytes`).
#[derive(Clone)]
pub(crate) struct DateHeaderCache {
    rx: watch::Receiver<HeaderValue>,
}

fn format_date_header() -> HeaderValue {
    let now = SystemTime::now();
    let formatted = httpdate::HttpDate::from(now).to_string();
    // httpdate always produces valid ASCII, so this never panics
    HeaderValue::from_str(&formatted).unwrap()
}

impl DateHeaderCache {
    /// Spawns the background update task and returns a cache handle.
    pub fn start() -> Self {
        let (tx, rx) = watch::channel(format_date_header());

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                let _ = tx.send(format_date_header());
            }
        });

        Self { rx }
    }

    /// Returns the current cached `Date` header value.
    pub fn header_value(&self) -> HeaderValue {
        self.rx.borrow().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_date_header_is_valid() {
        let cache = DateHeaderCache::start();
        let value = cache.header_value();
        let s = value.to_str().unwrap();
        // HTTP date format: "Mon, 17 Mar 2026 12:00:00 GMT"
        assert!(s.contains("GMT"), "Date header should contain GMT: {}", s);
    }

    #[tokio::test]
    async fn test_date_header_updates() {
        let cache = DateHeaderCache::start();
        let v1 = cache.header_value();
        // The value should be stable within the same second
        let v2 = cache.header_value();
        assert_eq!(v1, v2);
    }
}
