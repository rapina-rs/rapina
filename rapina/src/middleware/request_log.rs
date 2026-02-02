use hyper::body::Incoming;
use hyper::{Request, Response};
use tracing::{Instrument, info, info_span};

use crate::context::RequestContext;
use crate::response::BoxBody;

use super::{BoxFuture, Middleware, Next};

#[derive(Debug, Clone, Copy)]
pub struct RequestLogMiddleware;

impl RequestLogMiddleware {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RequestLogMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl Middleware for RequestLogMiddleware {
    fn handle<'a>(
        &'a self,
        req: Request<Incoming>,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        let trace_id = ctx.trace_id.clone();

        let span = info_span!(
            "request",
            method = %method,
            path = %path,
            trace_id = %trace_id,
        );

        Box::pin(
            async move {
                let response = next.run(req).await;
                let duration = ctx.elapsed();
                let status = response.status().as_u16();

                info!(
                    status = status,
                    duration_ms = duration.as_millis() as u64,
                    "request completed"
                );

                response
            }
            .instrument(span),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_log_middleware_new() {
        let _mw = RequestLogMiddleware::new();
    }

    #[test]
    fn test_request_log_middleware_default() {
        let _mw: RequestLogMiddleware = Default::default();
    }
}
