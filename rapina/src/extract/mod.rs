//! Request extractors for parsing incoming HTTP requests.
//!
//! Extractors are types that implement [`FromRequest`] or [`FromRequestParts`]
//! and can be used as handler parameters to automatically parse request data.

use http::Request;
use hyper::body::Incoming;
use std::sync::Arc;

mod path;
pub use path::{Path, PathParams, extract_path_params};

mod context;
pub use context::Context;
mod cookie;
pub use cookie::Cookie;
mod form;
pub use form::Form;
pub mod header;
mod json;
pub use json::Json;
mod query;
pub use query::Query;
mod state;
pub use state::State;
mod validated;
pub use validated::Validated;

pub use header::{
    __extract_header, __extract_optional_header, FromHeaderStr, Header, Headers, extract_header,
    extract_optional_header,
};

#[cfg(feature = "multipart")]
pub mod multipart;
#[cfg(feature = "multipart")]
pub use multipart::{Field, Multipart};

use crate::error::Error;
use crate::state::AppState;

/// Trait for extractors that consume the request body.
///
/// Implement this trait for extractors that need access to the full request,
/// including the body. Only one body-consuming extractor can be used per handler,
/// and it **must be the last parameter** in the handler function signature.
pub trait FromRequest: Sized {
    /// Extract the value from the request.
    fn from_request(
        req: Request<Incoming>,
        params: &PathParams,
        state: &Arc<AppState>,
    ) -> impl std::future::Future<Output = Result<Self, Error>> + Send;
}

/// Trait for extractors that only need request metadata.
///
/// Implement this trait for extractors that don't need the request body,
/// such as path parameters, query strings, or headers.
/// Multiple parts-only extractors can be used in a single handler
/// and must appear before any body-consuming extractor.
pub trait FromRequestParts: Sized + Send {
    /// Extract the value from request parts.
    fn from_request_parts(
        parts: &http::request::Parts,
        params: &PathParams,
        state: &Arc<AppState>,
    ) -> impl std::future::Future<Output = Result<Self, Error>> + Send;
}

impl<T: FromRequestParts> FromRequest for T {
    async fn from_request(
        req: Request<Incoming>,
        params: &PathParams,
        state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        let (parts, _body) = req.into_parts();
        Self::from_request_parts(&parts, params, state).await
    }
}

macro_rules! impl_deref {
    ($name:ident) => {
        impl<T> ::std::ops::Deref for $name<T> {
            type Target = T;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    };
}

pub(crate) use impl_deref;
