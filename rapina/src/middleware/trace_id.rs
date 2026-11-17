use hyper::body::Incoming;
use hyper::header::HeaderValue;
use hyper::{Request, Response};

use crate::context::RequestContext;
use crate::response::BoxBody;

use super::{BoxFuture, Middleware, Next};

pub const TRACE_ID_HEADER: &str = "x-trace-id";

/// Request ID header used by proxies and load balancers.
///
/// Read only when [`TRACE_ID_HEADER`] is absent.
pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// Longest incoming trace ID this middleware accepts, in bytes.
const MAX_TRACE_ID_LEN: usize = 128;

/// Whether an incoming value is safe to use as a trace ID.
///
/// The trace ID is echoed in every log line and in every error body, so a
/// client must not be able to inject arbitrary content through the header.
/// A value is accepted when it is non-empty, at most 128 bytes long and made of
/// `A-Z`, `a-z`, `0-9`, `-`, `_`, `.` and `:` only.
pub fn is_valid_trace_id(candidate: &str) -> bool {
    !candidate.is_empty()
        && candidate.len() <= MAX_TRACE_ID_LEN
        && candidate
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

/// Middleware that propagates a trace ID through the request/response cycle.
///
/// If the incoming request contains an `x-trace-id` header its value is used
/// as the trace ID for the request context; otherwise the auto-generated UUID
/// from [`RequestContext`] is kept. The final trace ID is echoed back to the
/// caller via an `x-trace-id` response header, enabling distributed tracing
/// across service boundaries.
///
/// [`RequestContext`]: crate::context::RequestContext
///
/// # Example
///
/// ```rust,ignore
/// Rapina::new()
///     .with(TraceIdMiddleware::new())
/// ```
#[derive(Debug, Clone, Copy)]
pub struct TraceIdMiddleware;

impl TraceIdMiddleware {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TraceIdMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl Middleware for TraceIdMiddleware {
    fn handle<'a>(
        &'a self,
        mut req: Request<Incoming>,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        Box::pin(async move {
            // An explicit x-trace-id wins, even when it is invalid: only a
            // missing header falls back to x-request-id, and a value that fails
            // validation leaves the generated ID in place.
            let incoming_trace_id = if req.headers().contains_key(TRACE_ID_HEADER) {
                req.headers().get(TRACE_ID_HEADER)
            } else {
                req.headers().get(REQUEST_ID_HEADER)
            }
            .and_then(|v| v.to_str().ok())
            .filter(|id| is_valid_trace_id(id))
            .map(String::from);

            let trace_id = if let Some(id) = incoming_trace_id {
                // Use the provided trace_id and update context in extensions
                let new_ctx = RequestContext::with_trace_id(id.clone());
                req.extensions_mut().insert(new_ctx);
                id
            } else {
                ctx.trace_id().to_owned()
            };

            let mut response = next.run(req).await;

            // Add x-trace-id to response headers
            if let Ok(header_value) = HeaderValue::from_str(&trace_id) {
                response.headers_mut().insert(TRACE_ID_HEADER, header_value);
            }

            response
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_id_middleware_new() {
        let _mw = TraceIdMiddleware::new();
    }

    #[test]
    fn test_trace_id_middleware_default() {
        let _mw: TraceIdMiddleware = Default::default();
    }

    #[test]
    fn test_accepts_typical_ids() {
        for id in [
            "abc",
            "550e8400-e29b-41d4-a716-446655440000",
            "trace_1.2:3",
            "0af7651916cd43dd8448eb211c80319c",
            &"a".repeat(MAX_TRACE_ID_LEN),
        ] {
            assert!(is_valid_trace_id(id), "{id:?} should be accepted");
        }
    }

    #[test]
    fn test_rejects_empty() {
        assert!(!is_valid_trace_id(""));
    }

    #[test]
    fn test_rejects_too_long() {
        assert!(!is_valid_trace_id(&"a".repeat(MAX_TRACE_ID_LEN + 1)));
    }

    #[test]
    fn test_rejects_separators_outside_the_allowed_set() {
        for id in ["a b", "a\tb", "a/b", "a,b", "a;b", "a=b", "a@b", "a\u{7f}b"] {
            assert!(!is_valid_trace_id(id), "{id:?} should be rejected");
        }
    }

    #[test]
    fn test_rejects_non_ascii() {
        assert!(!is_valid_trace_id("tr\u{e4}ce"));
        assert!(!is_valid_trace_id("trace\u{1f600}"));
    }
}
