use std::time::Instant;

use crate::context::RequestContext;
use crate::middleware::{BoxFuture, Middleware, Next};
use crate::response::BoxBody;
use hyper::body::Incoming;
use hyper::{Request, Response};
use prometheus::IntGauge;

use super::prometheus::MetricsRegistry;

pub struct MetricsMiddleware {
    registry: MetricsRegistry,
}

impl MetricsMiddleware {
    pub fn new(registry: MetricsRegistry) -> Self {
        Self { registry }
    }
}

/// RAII guard that safely manages the `http_requests_in_flight` metric.
///
/// In Hyper/Tokio, if a client closes the TCP connection mid-request,
/// the executing Future is abruptly dropped. If we manually called `.inc()`
/// and `.dec()` in the middleware, the `.dec()` would never execute upon
/// cancellation, causing a permanent metric leak.
///
/// This guard ensures that the gauge is strictly incremented on creation
/// and guaranteed to be decremented when dropped, regardless of whether
/// the request succeeds or is cancelled by the runtime.
struct InFlightGuard {
    gauge: IntGauge,
}

impl InFlightGuard {
    /// Constructs a new Inflight Guard, which automatically increments the Prometheus gauge
    fn new(gauge: IntGauge) -> Self {
        gauge.inc();
        Self { gauge }
    }
}

impl Drop for InFlightGuard {
    /// Drops the Inflight Guard, which automatically decrements the Prometheus gauge
    fn drop(&mut self) {
        self.gauge.dec();
    }
}

/// Replaces pure-numeric path segments with `:id` to avoid label cardinality explosion.
/// e.g `/users/123/posts` -> `/users/:id/posts`
fn normalize_path(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            if !seg.is_empty() && seg.chars().all(|c| c.is_ascii_digit()) {
                ":id"
            } else {
                seg
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

impl Middleware for MetricsMiddleware {
    fn handle<'a>(
        &'a self,
        req: Request<Incoming>,
        _ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        let method = req.method().to_string();
        let path = normalize_path(req.uri().path());
        let registry = self.registry.clone();

        Box::pin(async move {
            let _in_flight_guard = InFlightGuard::new(registry.http_requests_in_flight.clone());

            let start = Instant::now();
            let response = next.run(req).await;
            let duration = start.elapsed().as_secs_f64();

            let status = response.status().as_u16().to_string();
            registry
                .http_requests_total
                .with_label_values(&[&method, &path, &status])
                .inc();
            registry
                .http_request_duration_seconds
                .with_label_values(&[&method, &path])
                .observe(duration);

            response
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path_root() {
        assert_eq!(normalize_path("/"), "/");
    }

    #[test]
    fn test_normalize_path_no_numbers() {
        assert_eq!(normalize_path("/users/posts"), "/users/posts");
    }

    #[test]
    fn test_normalize_path_numeric_segment() {
        assert_eq!(normalize_path("/users/123"), "/users/:id");
    }

    #[test]
    fn test_normalize_path_nested_numeric() {
        assert_eq!(
            normalize_path("/users/123/posts/456"),
            "/users/:id/posts/:id"
        );
    }

    #[test]
    fn test_normalize_path_alphanumeric_preserved() {
        // "abc123" is not purely numeric, so it should be kept as-is
        assert_eq!(normalize_path("/users/abc123"), "/users/abc123");
    }

    #[test]
    fn test_normalize_path_mixed() {
        assert_eq!(
            normalize_path("/orgs/99/repos/name"),
            "/orgs/:id/repos/name"
        );
    }

    #[test]
    fn test_metrics_middleware_new() {
        let registry = MetricsRegistry::new();
        let _middleware = MetricsMiddleware::new(registry);
    }

    #[test]
    fn test_in_flight_guard_increments_and_decrements_cleanly() {
        let registry = MetricsRegistry::new();
        let gauge = registry.http_requests_in_flight.clone();

        assert_eq!(gauge.get(), 0, "In-Flight Gauge should start at 0");

        {
            let _guard = InFlightGuard::new(gauge.clone());
            assert_eq!(
                gauge.get(),
                1,
                "In-Flight Gauge should be 1 while guard is in scope"
            );
        }

        assert_eq!(
            gauge.get(),
            0,
            "In-Flight Gauge should return to 0 after guard drops"
        );
    }
}
