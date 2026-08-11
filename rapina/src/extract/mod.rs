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

// Database extractor (requires "database" feature)
#[cfg(feature = "database")]
impl FromRequestParts for crate::database::Db {
    async fn from_request_parts(
        _parts: &http::request::Parts,
        _params: &PathParams,
        state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        use sea_orm::DatabaseConnection;

        let conn = state.get::<DatabaseConnection>().ok_or_else(|| {
            Error::internal(
                "Database connection not configured. Did you forget to call .with_database()?",
            )
        })?;
        Ok(crate::database::Db::new(conn.clone()))
    }
}

// Jobs extractor (requires "database" feature)
#[cfg(feature = "database")]
impl FromRequestParts for crate::jobs::Jobs {
    async fn from_request_parts(
        parts: &http::request::Parts,
        _params: &PathParams,
        state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        use sea_orm::DatabaseConnection;

        let pool = state
            .get::<DatabaseConnection>()
            .ok_or_else(|| {
                Error::internal(
                    "Database connection not configured. Did you forget to call .with_database()?",
                )
            })?
            .clone();

        let trace_id = parts
            .extensions
            .get::<crate::context::RequestContext>()
            .map(|ctx| ctx.trace_id().to_owned());

        Ok(crate::jobs::Jobs::new(pool, trace_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{TestRequest, empty_params, empty_state};

    #[cfg(feature = "database")]
    #[tokio::test]
    async fn jobs_extractor_missing_db_returns_500() {
        let (parts, _) = TestRequest::get("/").into_parts();
        let result =
            crate::jobs::Jobs::from_request_parts(&parts, &empty_params(), &empty_state()).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status(), 500);
    }
}
