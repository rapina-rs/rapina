use hyper::body::Incoming;
use hyper::{Request, Response};

use crate::context::RequestContext;
use crate::error::Error;
use crate::response::{BoxBody, IntoResponse};

use super::{BoxFuture, Middleware, Next};

const DEFAULT_MAX_SIZE: usize = 1024 * 1024; // 1MB

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
