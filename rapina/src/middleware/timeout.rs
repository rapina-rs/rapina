use std::time::Duration;

use hyper::body::Incoming;
use hyper::{Request, Response};

use crate::context::RequestContext;
use crate::error::Error;
use crate::response::{BoxBody, IntoResponse};

use super::{BoxFuture, Middleware, Next};

/// Middleware that enforces a maximum duration for each request.
///
/// If a handler does not respond within the configured duration the request is
/// cancelled and a `408 Request Timeout` is returned to the client.
/// Defaults to 30 seconds.
///
/// # Example
///
/// ```rust,ignore
/// Rapina::new()
///     .with(TimeoutMiddleware::new(Duration::from_secs(10)))
/// ```
#[derive(Debug, Clone)]
pub struct TimeoutMiddleware {
    pub(crate) duration: Duration,
}

impl TimeoutMiddleware {
    pub fn new(duration: Duration) -> Self {
        Self { duration }
    }
}

impl Default for TimeoutMiddleware {
    fn default() -> Self {
        Self::new(Duration::from_secs(30))
    }
}

impl Middleware for TimeoutMiddleware {
    fn handle<'a>(
        &'a self,
        req: Request<Incoming>,
        _ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        Box::pin(async move {
            match tokio::time::timeout(self.duration, next.run(req)).await {
                Ok(response) => response,
                Err(_) => Error::request_timeout("request timeout").into_response(),
            }
        })
    }
}
