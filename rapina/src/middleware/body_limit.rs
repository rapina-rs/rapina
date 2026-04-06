use hyper::body::Incoming;
use hyper::{Request, Response};

use crate::context::RequestContext;
use crate::error::Error;
use crate::response::{BoxBody, IntoResponse};

use super::{BoxFuture, Middleware, Next};

const DEFAULT_MAX_SIZE: usize = 1024 * 1024; // 1MB

/// Middleware that rejects requests whose `Content-Length` exceeds a limit.
///
/// Checks the `Content-Length` header before passing the request downstream.
/// Requests that exceed `max_size` receive a `400 Bad Request` response.
/// Defaults to 1 MB. Requests without a `Content-Length` header are passed
/// through unchecked.
///
/// # Example
///
/// ```rust,ignore
/// Rapina::new()
///     .with(BodyLimitMiddleware::new(5 * 1024 * 1024)) // 5 MB
/// ```
#[derive(Debug, Clone)]
pub struct BodyLimitMiddleware {
    pub(crate) max_size: usize,
}

impl BodyLimitMiddleware {
    pub fn new(max_size: usize) -> Self {
        Self { max_size }
    }
}

impl Default for BodyLimitMiddleware {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_SIZE)
    }
}

impl Middleware for BodyLimitMiddleware {
    fn handle<'a>(
        &'a self,
        req: Request<Incoming>,
        _ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        Box::pin(async move {
            let content_length = req
                .headers()
                .get(hyper::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<usize>().ok());

            if content_length.is_some_and(|len| len > self.max_size) {
                return Error::bad_request("body too large").into_response();
            }

            next.run(req).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_body_limit_middleware_new() {
        let mw = BodyLimitMiddleware::new(2048);
        assert_eq!(mw.max_size, 2048);
    }

    #[test]
    fn test_body_limit_middleware_default() {
        let mw = BodyLimitMiddleware::default();
        assert_eq!(mw.max_size, 1024 * 1024); // 1MB default
    }

    #[test]
    fn test_body_limit_custom_size() {
        let mw = BodyLimitMiddleware::new(5 * 1024 * 1024); // 5MB
        assert_eq!(mw.max_size, 5 * 1024 * 1024);
    }

    #[test]
    fn test_body_limit_zero_size() {
        let mw = BodyLimitMiddleware::new(0);
        assert_eq!(mw.max_size, 0);
    }
}
