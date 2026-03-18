//! W3C Trace Context propagation middleware.

use hyper::body::Incoming;
use hyper::header::HeaderValue;
use hyper::{Request, Response};

use crate::context::RequestContext;
use crate::response::BoxBody;

use super::{BoxFuture, Middleware, Next};

const TRACEPARENT_HEADER: &str = "traceparent";

/// Middleware that propagates W3C `traceparent` headers for distributed tracing.
///
/// When an incoming request carries a `traceparent` header, this middleware
/// creates a child span linked to the upstream trace. The `traceparent`
/// header is echoed back in the response so downstream services can
/// continue the trace chain.
///
/// This middleware is independent of [`TraceIdMiddleware`](super::TraceIdMiddleware)
/// and both can be used simultaneously.
///
/// Requires the `telemetry` feature.
///
/// # Example
///
/// ```rust,ignore
/// use rapina::prelude::*;
/// use rapina::middleware::TraceparentMiddleware;
///
/// Rapina::new()
///     .with_telemetry(TelemetryConfig::new("http://jaeger:4317", "my-api"))
///     .middleware(TraceparentMiddleware::new())
///     .router(router)
///     .listen("127.0.0.1:3000")
///     .await
/// ```
#[derive(Debug, Clone, Copy)]
pub struct TraceparentMiddleware;

impl TraceparentMiddleware {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TraceparentMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

/// Parsed W3C traceparent header fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Traceparent {
    pub version: u8,
    pub trace_id: String,
    pub parent_id: String,
    pub trace_flags: u8,
}

impl Traceparent {
    /// Parses a `traceparent` header value.
    ///
    /// Format: `{version}-{trace_id}-{parent_id}-{flags}`
    /// Example: `00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01`
    pub(crate) fn parse(value: &str) -> Option<Self> {
        let parts: Vec<&str> = value.split('-').collect();
        if parts.len() != 4 {
            return None;
        }

        let version = u8::from_str_radix(parts[0], 16).ok()?;
        let trace_id = parts[1];
        let parent_id = parts[2];
        let trace_flags = u8::from_str_radix(parts[3], 16).ok()?;

        // Validate lengths per W3C spec
        if trace_id.len() != 32 || parent_id.len() != 16 {
            return None;
        }

        // Validate hex characters
        if !trace_id.chars().all(|c| c.is_ascii_hexdigit())
            || !parent_id.chars().all(|c| c.is_ascii_hexdigit())
        {
            return None;
        }

        // All-zero trace_id or parent_id is invalid
        if trace_id.chars().all(|c| c == '0') || parent_id.chars().all(|c| c == '0') {
            return None;
        }

        Some(Self {
            version,
            trace_id: trace_id.to_string(),
            parent_id: parent_id.to_string(),
            trace_flags,
        })
    }

    /// Formats the traceparent header value.
    pub(crate) fn to_header_value(&self) -> String {
        format!(
            "{:02x}-{}-{}-{:02x}",
            self.version, self.trace_id, self.parent_id, self.trace_flags
        )
    }
}

impl Middleware for TraceparentMiddleware {
    fn handle<'a>(
        &'a self,
        req: Request<Incoming>,
        _ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        Box::pin(async move {
            let incoming_traceparent = req
                .headers()
                .get(TRACEPARENT_HEADER)
                .and_then(|v| v.to_str().ok())
                .and_then(Traceparent::parse);

            let span = if let Some(ref tp) = incoming_traceparent {
                tracing::info_span!(
                    "http.request",
                    otel.kind = "server",
                    trace_id = %tp.trace_id,
                    parent_id = %tp.parent_id,
                )
            } else {
                tracing::info_span!("http.request", otel.kind = "server")
            };

            let mut response = {
                let _guard = span.enter();
                next.run(req).await
            };

            // Echo traceparent back in response
            if let Some(tp) = incoming_traceparent {
                if let Ok(val) = HeaderValue::from_str(&tp.to_header_value()) {
                    response.headers_mut().insert(TRACEPARENT_HEADER, val);
                }
            }

            response
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Traceparent parsing tests ---

    #[test]
    fn test_parse_valid_traceparent() {
        let tp =
            Traceparent::parse("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01").unwrap();
        assert_eq!(tp.version, 0);
        assert_eq!(tp.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(tp.parent_id, "00f067aa0ba902b7");
        assert_eq!(tp.trace_flags, 1);
    }

    #[test]
    fn test_parse_unsampled_traceparent() {
        let tp =
            Traceparent::parse("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00").unwrap();
        assert_eq!(tp.trace_flags, 0);
    }

    #[test]
    fn test_parse_invalid_too_few_parts() {
        assert!(Traceparent::parse("00-abc-01").is_none());
    }

    #[test]
    fn test_parse_invalid_too_many_parts() {
        assert!(Traceparent::parse("00-a-b-c-d-e").is_none());
    }

    #[test]
    fn test_parse_invalid_trace_id_length() {
        assert!(
            Traceparent::parse("00-4bf92f3577b34da6a3ce929d0e0e473-00f067aa0ba902b7-01").is_none()
        );
    }

    #[test]
    fn test_parse_invalid_parent_id_length() {
        assert!(
            Traceparent::parse("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b-01").is_none()
        );
    }

    #[test]
    fn test_parse_all_zero_trace_id_invalid() {
        assert!(
            Traceparent::parse("00-00000000000000000000000000000000-00f067aa0ba902b7-01").is_none()
        );
    }

    #[test]
    fn test_parse_all_zero_parent_id_invalid() {
        assert!(
            Traceparent::parse("00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01").is_none()
        );
    }

    #[test]
    fn test_parse_non_hex_trace_id() {
        assert!(
            Traceparent::parse("00-4bf92f3577b34da6a3ce929d0e0eXXXX-00f067aa0ba902b7-01").is_none()
        );
    }

    #[test]
    fn test_parse_non_hex_version() {
        assert!(
            Traceparent::parse("zz-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01").is_none()
        );
    }

    #[test]
    fn test_to_header_value_roundtrip() {
        let original = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let tp = Traceparent::parse(original).unwrap();
        assert_eq!(tp.to_header_value(), original);
    }

    #[test]
    fn test_to_header_value_preserves_flags() {
        let tp = Traceparent {
            version: 0,
            trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".into(),
            parent_id: "00f067aa0ba902b7".into(),
            trace_flags: 0,
        };
        assert_eq!(
            tp.to_header_value(),
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00"
        );
    }

    // --- Middleware construction tests ---

    #[test]
    fn test_traceparent_middleware_new() {
        let _mw = TraceparentMiddleware::new();
    }

    #[test]
    fn test_traceparent_middleware_default() {
        let _mw: TraceparentMiddleware = Default::default();
    }
}
