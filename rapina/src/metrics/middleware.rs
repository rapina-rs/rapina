use std::time::Instant;

use hyper::body::Incoming;
use hyper::{Request, Response};

use crate::context::RequestContext;
use crate::middleware::{BoxFuture, Middleware, Next};
use crate::response::BoxBody;

use super::prometheus::MetricsRegistry;

pub struct MetricsMiddleware {
    registry: MetricsRegistry,
}

impl MetricsMiddleware {
    pub fn new(registry: MetricsRegistry) -> Self {
        Self { registry }
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
            registry.http_requests_in_flight.inc();
            let start = Instant::now();
            let response = next.run(req).await;
            let duration = start.elapsed().as_secs_f64();
            registry.http_requests_in_flight.dec();

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
}
