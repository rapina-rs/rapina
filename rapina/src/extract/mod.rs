//! Request extractors for parsing incoming HTTP requests.
//!
//! Extractors are types that implement [`FromRequest`] or [`FromRequestParts`]
//! and can be used as handler parameters to automatically parse request data.

use http::Request;
use hyper::body::Incoming;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

mod path;
pub use path::{Path, PathParams, extract_path_params};

mod form;
pub use form::Form;
pub mod header;
mod json;
pub use json::Json;
mod validated;
pub use validated::Validated;

pub use header::{
    __extract_header, __extract_optional_header, FromHeaderStr, Header, extract_header,
    extract_optional_header,
};

#[cfg(feature = "multipart")]
pub mod multipart;
#[cfg(feature = "multipart")]
pub use multipart::{Field, Multipart};

use crate::context::RequestContext;
use crate::error::Error;
use crate::state::AppState;

/// Extracts and deserializes query string parameters.
///
/// Parses the URL query string into a typed struct using `serde_urlencoded`.
/// Returns 400 Bad Request if parsing fails.
///
/// # Examples
///
/// ```ignore
/// use rapina::prelude::*;
///
/// #[derive(Deserialize)]
/// struct Pagination {
///     page: Option<u32>,
///     limit: Option<u32>,
/// }
///
/// #[get("/users")]
/// async fn list_users(query: Query<Pagination>) -> String {
///     let page = query.page.unwrap_or(1);
///     format!("Page: {}", page)
/// }
/// ```
#[derive(Debug)]
pub struct Query<T>(pub T);

/// Provides access to all request headers as a raw [`http::HeaderMap`].
///
/// Extracts the entire header map from the request. Use this when you need to
/// iterate over all headers or access multiple headers dynamically.
///
/// # Distinction from `Header<T>`
///
/// - `Headers` — gives you the full [`http::HeaderMap`]; access headers by
///   name at runtime. No parsing is performed automatically.
/// - [`Header<T>`](crate::extract::Header) — extracts and parses a single,
///   named header into a typed value `T` at compile time via the proc-macro.
///   Prefer `Header<T>` when you know the header name upfront.
///
/// # Examples
///
/// ```ignore
/// use rapina::prelude::*;
///
/// #[get("/auth")]
/// async fn check_auth(headers: Headers) -> Result<String> {
///     let auth = headers.get("authorization")
///         .ok_or_else(|| Error::unauthorized("missing auth header"))?;
///     Ok("Authenticated".to_string())
/// }
/// ```
#[derive(Debug)]
pub struct Headers(pub http::HeaderMap);

/// Extracts and deserializes cookies from the request.
///
/// Parses the `Cookie` header into a typed struct. Each field in the struct
/// corresponds to a cookie name. Returns 400 Bad Request if parsing fails.
///
/// Use `Option<Cookie<T>>` for optional cookie access.
///
/// # Examples
///
/// ```ignore
/// use rapina::prelude::*;
///
/// #[derive(Deserialize)]
/// struct Session {
///     session_id: String,
/// }
///
/// #[get("/dashboard")]
/// async fn dashboard(session: Cookie<Session>) -> Result<Json<Dashboard>> {
///     // Use session.session_id...
/// }
/// ```
#[derive(Debug)]
pub struct Cookie<T>(pub T);

/// Extracts application state.
///
/// Provides access to shared application state that was registered
/// with [`Rapina::state`](crate::app::Rapina::state).
///
/// The inner value is wrapped in an `Arc<T>`, so extraction is always
/// a cheap atomic reference-count bump rather than a deep clone.
/// This also removes the `Clone` requirement on `T`.
///
/// # Examples
///
/// ```ignore
/// use rapina::prelude::*;
///
/// struct AppConfig {
///     db_url: String,
/// }
///
/// #[get("/config")]
/// async fn get_config(state: State<AppConfig>) -> String {
///     state.db_url.clone()
/// }
/// ```
#[derive(Debug, Clone)]
pub struct State<T>(pub Arc<T>);

/// Provides access to the request context.
///
/// Contains the `trace_id` and request start time for logging and tracing.
///
/// # Examples
///
/// ```ignore
/// use rapina::prelude::*;
///
/// #[get("/trace")]
/// async fn get_trace(ctx: Context) -> String {
///     format!("Trace ID: {}", ctx.trace_id())
/// }
/// ```
#[derive(Debug)]
pub struct Context(pub RequestContext);

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

impl<T> Query<T> {
    /// Consumes the extractor and returns the inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl Headers {
    /// Gets a header value by name.
    pub fn get(&self, key: &str) -> Option<&http::HeaderValue> {
        self.0.get(key)
    }

    /// Consumes the extractor and returns the inner HeaderMap.
    pub fn into_inner(self) -> http::HeaderMap {
        self.0
    }
}

impl<T> Cookie<T> {
    /// Consumes the extractor and returns the inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> State<T> {
    /// Consumes the extractor and returns the inner `Arc<T>`.
    pub fn into_inner(self) -> Arc<T> {
        self.0
    }
}

impl Context {
    /// Consumes the extractor and returns the inner RequestContext.
    pub fn into_inner(self) -> RequestContext {
        self.0
    }

    /// Returns the trace ID for this request.
    pub fn trace_id(&self) -> &str {
        self.0.trace_id()
    }

    /// Returns the elapsed time since the request started.
    pub fn elapsed(&self) -> std::time::Duration {
        self.0.elapsed()
    }
}

impl<T: Send + Sync + 'static> FromRequestParts for State<T> {
    async fn from_request_parts(
        _parts: &http::request::Parts,
        _params: &PathParams,
        state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        let arc = state.get_arc::<T>().ok_or_else(|| {
            Error::internal(format!(
                "State not registered for type '{}'. Did you forget to call .state() or .state_arc()?",
                std::any::type_name::<T>()
            ))
        })?;
        Ok(State(arc))
    }
}

impl FromRequestParts for Context {
    async fn from_request_parts(
        parts: &http::request::Parts,
        _params: &PathParams,
        _state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        parts
            .extensions
            .get::<RequestContext>()
            .cloned()
            .map(Context)
            .ok_or_else(|| {
                Error::internal(
                    "RequestContext missing from request extensions. \
                     The request pipeline did not initialize the request context.",
                )
            })
    }
}

impl<T: DeserializeOwned + Send> FromRequestParts for Query<T> {
    async fn from_request_parts(
        parts: &http::request::Parts,
        _params: &PathParams,
        _state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        let query = parts.uri.query().unwrap_or("");
        let value: T = serde_urlencoded::from_str(query)
            .map_err(|e| Error::bad_request(format!("Invalid query string parameters: {}", e)))?;
        Ok(Query(value))
    }
}

impl FromRequestParts for Headers {
    async fn from_request_parts(
        parts: &http::request::Parts,
        _params: &PathParams,
        _state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        Ok(Headers(parts.headers.clone()))
    }
}

impl<T: DeserializeOwned + Send> FromRequestParts for Cookie<T> {
    async fn from_request_parts(
        parts: &http::request::Parts,
        _params: &PathParams,
        _state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        let cookie_header = parts
            .headers
            .get(http::header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        // Parse cookies into key=value pairs
        let cookies: HashMap<String, String> = cookie_header
            .split(';')
            .filter_map(|pair| {
                let mut parts = pair.trim().splitn(2, '=');
                let key = parts.next()?.to_string();
                let value = parts.next()?.to_string();
                if key.is_empty() {
                    None
                } else {
                    Some((key, value))
                }
            })
            .collect();

        // Serialize to JSON then deserialize to target type
        let json = serde_json::to_string(&cookies)
            .map_err(|e| Error::bad_request(format!("Failed to process cookies: {}", e)))?;

        let value: T = serde_json::from_str(&json)
            .map_err(|e| Error::bad_request(format!("Invalid or missing cookies: {}", e)))?;

        Ok(Cookie(value))
    }
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

impl<T> Deref for State<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl_deref!(Query);
impl_deref!(Cookie);

impl Deref for Context {
    type Target = RequestContext;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Deref for Headers {
    type Target = http::HeaderMap;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

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
            .get::<RequestContext>()
            .map(|ctx| ctx.trace_id().to_owned());

        Ok(crate::jobs::Jobs::new(pool, trace_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{TestRequest, empty_params, empty_state};

    #[derive(Debug, Clone, PartialEq)]
    struct Data {
        name: String,
    }

    // Query extractor tests
    #[tokio::test]
    async fn test_query_extractor_success() {
        #[derive(serde::Deserialize, PartialEq, Debug)]
        struct Params {
            page: u32,
            limit: u32,
        }

        let (parts, _) = TestRequest::get("/users?page=1&limit=10").into_parts();
        let result =
            Query::<Params>::from_request_parts(&parts, &empty_params(), &empty_state()).await;

        assert!(result.is_ok());
        let query = result.unwrap();
        assert_eq!(query.0.page, 1);
        assert_eq!(query.0.limit, 10);
    }

    #[tokio::test]
    async fn test_query_extractor_optional_fields() {
        #[derive(serde::Deserialize)]
        struct Params {
            page: Option<u32>,
            search: Option<String>,
        }

        let (parts, _) = TestRequest::get("/users?page=5").into_parts();
        let result =
            Query::<Params>::from_request_parts(&parts, &empty_params(), &empty_state()).await;

        assert!(result.is_ok());
        let query = result.unwrap();
        assert_eq!(query.0.page, Some(5));
        assert!(query.0.search.is_none());
    }

    #[tokio::test]
    async fn test_query_extractor_empty_query() {
        #[allow(dead_code)]
        #[derive(serde::Deserialize, Default)]
        struct Params {
            #[serde(default)]
            page: u32,
        }

        let (parts, _) = TestRequest::get("/users").into_parts();
        let result =
            Query::<Params>::from_request_parts(&parts, &empty_params(), &empty_state()).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_query_extractor_invalid_type() {
        #[allow(dead_code)]
        #[derive(serde::Deserialize, Debug)]
        struct Params {
            page: u32,
        }

        let (parts, _) = TestRequest::get("/users?page=notanumber").into_parts();
        let result =
            Query::<Params>::from_request_parts(&parts, &empty_params(), &empty_state()).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status(), 400);
    }

    #[tokio::test]
    async fn test_query_extractor_uuid() {
        #[derive(serde::Deserialize, PartialEq, Debug)]
        struct Params {
            id: uuid::Uuid,
        }

        let id = uuid::Uuid::new_v4();
        let (parts, _) = TestRequest::get(&format!("/users?id={id}")).into_parts();
        let result =
            Query::<Params>::from_request_parts(&parts, &empty_params(), &empty_state()).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().0.id, id);
    }

    #[tokio::test]
    async fn test_query_extractor_uuid_invalid() {
        #[allow(dead_code)]
        #[derive(serde::Deserialize, Debug)]
        struct Params {
            id: uuid::Uuid,
        }

        let (parts, _) = TestRequest::get("/users?id=not-a-uuid").into_parts();
        let result =
            Query::<Params>::from_request_parts(&parts, &empty_params(), &empty_state()).await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status(), 400);
    }

    // Headers extractor tests
    #[tokio::test]
    async fn test_headers_extractor() {
        let (parts, _) = TestRequest::get("/")
            .header("x-custom", "value")
            .header("authorization", "Bearer token")
            .into_parts();

        let result = Headers::from_request_parts(&parts, &empty_params(), &empty_state()).await;
        assert!(result.is_ok());

        let headers = result.unwrap();
        assert_eq!(headers.get("x-custom").unwrap().to_str().unwrap(), "value");
        assert_eq!(
            headers.get("authorization").unwrap().to_str().unwrap(),
            "Bearer token"
        );
    }

    #[tokio::test]
    async fn test_headers_extractor_missing_header() {
        let (parts, _) = TestRequest::get("/").into_parts();
        let result = Headers::from_request_parts(&parts, &empty_params(), &empty_state()).await;

        assert!(result.is_ok());
        let headers = result.unwrap();
        assert!(headers.get("x-nonexistent").is_none());
    }

    // Context extractor tests
    #[tokio::test]
    async fn test_context_extractor() {
        let (parts, _) = TestRequest::get("/").into_parts();
        let result = Context::from_request_parts(&parts, &empty_params(), &empty_state()).await;

        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert!(!ctx.trace_id().is_empty());
    }

    #[tokio::test]
    async fn test_context_extractor_with_custom_trace_id() {
        let custom_ctx = crate::context::RequestContext::with_trace_id("custom-123".to_string());
        let (parts, _) = TestRequest::get("/").into_parts_with_context(custom_ctx);

        let result = Context::from_request_parts(&parts, &empty_params(), &empty_state()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().trace_id(), "custom-123");
    }

    // State extractor tests
    #[tokio::test]
    async fn test_state_extractor_success() {
        struct AppConfig {
            name: String,
        }

        let state = crate::test::state_with(AppConfig {
            name: "test-app".to_string(),
        });
        let (parts, _) = TestRequest::get("/").into_parts();

        let result = State::<AppConfig>::from_request_parts(&parts, &empty_params(), &state).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name, "test-app");
    }

    #[tokio::test]
    async fn test_state_extractor_not_found() {
        #[derive(Debug)]
        struct MissingState;

        let state = empty_state();
        let (parts, _) = TestRequest::get("/").into_parts();

        let result =
            State::<MissingState>::from_request_parts(&parts, &empty_params(), &state).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status(), 500);
    }

    #[tokio::test]
    async fn test_state_extractor_arc_trait_object() {
        trait Greeter: Send + Sync {
            fn greet(&self) -> &'static str;
        }

        struct Hello;
        impl Greeter for Hello {
            fn greet(&self) -> &'static str {
                "hello"
            }
        }

        let greeter: std::sync::Arc<dyn Greeter> = std::sync::Arc::new(Hello);
        let state = std::sync::Arc::new(crate::state::AppState::new().with_arc(greeter));
        let (parts, _) = TestRequest::get("/").into_parts();

        let result = State::<std::sync::Arc<dyn Greeter>>::from_request_parts(
            &parts,
            &empty_params(),
            &state,
        )
        .await;

        assert!(result.is_ok());
        // Deref chain: State<Arc<dyn Greeter>> -> Arc<dyn Greeter> -> dyn Greeter
        assert_eq!(result.unwrap().greet(), "hello");
    }

    // into_inner tests
    #[test]
    fn test_query_into_inner() {
        let query = Query("test".to_string());
        assert_eq!(query.into_inner(), "test");
    }

    #[test]
    fn test_headers_into_inner() {
        let headers = Headers(http::HeaderMap::new());
        let inner = headers.into_inner();
        assert!(inner.is_empty());
    }

    #[test]
    fn test_state_into_inner() {
        let state = State(Arc::new("value".to_string()));
        let arc = state.into_inner();
        assert_eq!(*arc, "value");
    }

    #[test]
    fn test_context_into_inner() {
        let ctx = crate::context::RequestContext::with_trace_id("test".to_string());
        let context = Context(ctx);
        assert_eq!(context.into_inner().trace_id(), "test");
    }

    #[test]
    fn test_context_elapsed() {
        let ctx = crate::context::RequestContext::new();
        let context = Context(ctx);
        // Verify elapsed() returns a Duration (compile-time check)
        let _elapsed: std::time::Duration = context.elapsed();
    }

    // deref tests
    #[test]
    fn test_query_deref() {
        let query = Query("test".to_string());
        assert_eq!(*query, "test");
    }

    #[test]
    fn test_state_deref() {
        let state = State(Arc::new("value".to_string()));
        assert_eq!(*state, "value");
    }

    // autoderef tests
    #[test]
    fn test_state_autoderef() {
        let data = Data {
            name: "state test".to_string(),
        };

        let state = State(Arc::new(data.clone()));
        assert_eq!(state.name, data.name);
    }

    #[test]
    fn test_headers_autoderef() {
        let headers = Headers(http::HeaderMap::new());
        assert!(headers.is_empty());
    }

    #[test]
    fn test_context_autoderef() {
        let ctx = Context(crate::context::RequestContext::with_trace_id(
            "test".to_string(),
        ));
        assert_eq!(ctx.trace_id(), "test");
    }

    // Cookie extractor tests
    #[tokio::test]
    async fn test_cookie_extractor_success() {
        #[derive(serde::Deserialize, Debug, PartialEq)]
        struct Session {
            session_id: String,
        }

        let (parts, _) = TestRequest::get("/dashboard")
            .header("cookie", "session_id=abc123")
            .into_parts();

        let result =
            Cookie::<Session>::from_request_parts(&parts, &empty_params(), &empty_state()).await;

        assert!(result.is_ok());
        let cookie = result.unwrap();
        assert_eq!(cookie.0.session_id, "abc123");
    }

    #[tokio::test]
    async fn test_cookie_extractor_multiple_cookies() {
        #[derive(serde::Deserialize, Debug)]
        struct Cookies {
            session_id: String,
            user_id: String,
        }

        let (parts, _) = TestRequest::get("/")
            .header("cookie", "session_id=abc123; user_id=user456")
            .into_parts();

        let result =
            Cookie::<Cookies>::from_request_parts(&parts, &empty_params(), &empty_state()).await;

        assert!(result.is_ok());
        let cookies = result.unwrap();
        assert_eq!(cookies.0.session_id, "abc123");
        assert_eq!(cookies.0.user_id, "user456");
    }

    #[tokio::test]
    async fn test_cookie_extractor_optional_field() {
        #[derive(serde::Deserialize, Debug)]
        struct Cookies {
            session_id: String,
            tracking: Option<String>,
        }

        let (parts, _) = TestRequest::get("/")
            .header("cookie", "session_id=abc123")
            .into_parts();

        let result =
            Cookie::<Cookies>::from_request_parts(&parts, &empty_params(), &empty_state()).await;

        assert!(result.is_ok());
        let cookies = result.unwrap();
        assert_eq!(cookies.0.session_id, "abc123");
        assert!(cookies.0.tracking.is_none());
    }

    #[tokio::test]
    async fn test_cookie_extractor_missing_required() {
        // Struct never successfully deserializes in this test (testing error case)
        #[allow(dead_code)]
        #[derive(serde::Deserialize, Debug)]
        struct Session {
            session_id: String,
        }

        let (parts, _) = TestRequest::get("/").into_parts();

        let result =
            Cookie::<Session>::from_request_parts(&parts, &empty_params(), &empty_state()).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status(), 400);
        assert!(err.message().contains("session_id"));
    }

    #[tokio::test]
    async fn test_cookie_extractor_empty_header() {
        #[allow(dead_code)]
        #[derive(serde::Deserialize, Debug)]
        struct Session {
            session_id: Option<String>,
        }

        let (parts, _) = TestRequest::get("/").header("cookie", "").into_parts();

        let result =
            Cookie::<Session>::from_request_parts(&parts, &empty_params(), &empty_state()).await;

        assert!(result.is_ok());
        assert!(result.unwrap().0.session_id.is_none());
    }

    #[test]
    fn test_cookie_into_inner() {
        let cookie = Cookie("session".to_string());
        assert_eq!(cookie.into_inner(), "session");
    }

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
