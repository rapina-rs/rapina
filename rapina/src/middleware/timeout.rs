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
/// cancelled and a `500 Internal Server Error` is returned to the client.
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
                Err(_) => Error::internal("request timeout").into_response(),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_timeout_middleware_new() {
        let mw = TimeoutMiddleware::new(Duration::from_secs(60));
        assert_eq!(mw.duration, Duration::from_secs(60));
    }

    #[test]
    fn test_timeout_middleware_default() {
        let mw = TimeoutMiddleware::default();
        assert_eq!(mw.duration, Duration::from_secs(30));
    }

    #[test]
    fn test_timeout_millisecond_duration() {
        let mw = TimeoutMiddleware::new(Duration::from_millis(500));
        assert_eq!(mw.duration, Duration::from_millis(500));
    }

    #[test]
    fn test_timeout_zero_duration() {
        let mw = TimeoutMiddleware::new(Duration::ZERO);
        assert_eq!(mw.duration, Duration::ZERO);
    }
}
