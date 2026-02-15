use hyper::body::Incoming;
use hyper::header::HeaderValue;
use hyper::{Request, Response};

use crate::context::RequestContext;
use crate::response::BoxBody;

use super::{BoxFuture, Middleware, Next};

pub const TRACE_ID_HEADER: &str = "x-trace-id";

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
            // Check for incoming x-trace-id header for distributed tracing
            let incoming_trace_id = req
                .headers()
                .get(TRACE_ID_HEADER)
                .and_then(|v| v.to_str().ok())
                .map(String::from);

            let trace_id = if let Some(id) = incoming_trace_id {
                // Use the provided trace_id and update context in extensions
                let new_ctx = RequestContext::with_trace_id(id.clone());
                req.extensions_mut().insert(new_ctx);
                id
            } else {
                ctx.trace_id.clone()
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
}
