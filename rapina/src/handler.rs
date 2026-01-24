//! Handler trait for named route handlers.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use http::Request;
use hyper::body::Incoming;

use crate::extract::PathParams;
use crate::response::BoxBody;
use crate::state::AppState;

type BoxFuture = Pin<Box<dyn Future<Output = hyper::Response<BoxBody>> + Send>>;

/// A named request handler.
///
/// Implemented by route macros (`#[get]`, `#[post]`, etc.) to provide
/// both handler logic and name for OpenAPI generation.
pub trait Handler: Clone + Send + Sync + 'static {
    /// Handler name used as operationId in OpenAPI.
    const NAME: &'static str;

    /// Handle the request.
    fn call(&self, req: Request<Incoming>, params: PathParams, state: Arc<AppState>) -> BoxFuture;
}
