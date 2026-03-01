//! Request extractors for parsing incoming HTTP requests.
//!
//! Extractors are types that implement [`FromRequest`] or [`FromRequestParts`]
//! and can be used as handler parameters to automatically parse request data.

use bytes::Bytes;
use http::Request;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;
use validator::Validate;

use crate::context::RequestContext;
use crate::error::Error;
use crate::response::{BoxBody, IntoResponse};
use crate::state::AppState;

const JSON_CONTENT_TYPE: &str = "application/json";
const FORM_CONTENT_TYPE: &str = "application/x-www-form-urlencoded";

/// Extracts and deserializes JSON request bodies.
///
/// Parses the request body as JSON into the specified type `T`.
/// Returns 400 Bad Request if parsing fails.
///
/// # Examples
///
/// ```ignore
/// use rapina::prelude::*;
///
/// #[derive(Deserialize)]
/// struct CreateUser {
///     name: String,
///     email: String,
/// }
///
/// #[post("/users")]
/// async fn create_user(body: Json<CreateUser>) -> Json<User> {
///     // Use body.name, body.email...
/// }
/// ```
#[derive(Debug)]
pub struct Json<T>(pub T);

/// Extracts a single path parameter from the URL.
///
/// Parses a path segment into the specified type `T`.
/// Returns 400 Bad Request if parsing fails.
///
/// # Examples
///
/// ```ignore
/// use rapina::prelude::*;
///
/// #[get("/users/:id")]
/// async fn get_user(id: Path<u64>) -> String {
///     format!("User ID: {}", *id) // deref to access value of Path
/// }
/// ```
#[derive(Debug)]
pub struct Path<T>(pub T);

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

/// Extracts and deserializes URL-encoded form data.
///
/// Parses `application/x-www-form-urlencoded` request bodies.
/// Returns 400 Bad Request if content-type is wrong or parsing fails.
///
/// # Examples
///
/// ```ignore
/// use rapina::prelude::*;
///
/// #[derive(Deserialize)]
/// struct LoginForm {
///     username: String,
///     password: String,
/// }
///
/// #[post("/login")]
/// async fn login(form: Form<LoginForm>) -> String {
///     format!("Welcome, {}", form.username)
/// }
/// ```
#[derive(Debug)]
pub struct Form<T>(pub T);

/// Provides access to request headers.
///
/// Extracts all HTTP headers from the request.
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
/// # Examples
///
/// ```ignore
/// use rapina::prelude::*;
///
/// #[derive(Clone)]
/// struct AppConfig {
///     db_url: String,
/// }
///
/// #[get("/config")]
/// async fn get_config(state: State<AppConfig>) -> String {
///     state.db_url
/// }
/// ```
#[derive(Debug)]
pub struct State<T>(pub T);

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

/// Wraps an extractor and validates the extracted value.
///
/// Uses the `validator` crate to run validation rules on the inner value.
/// Returns 422 Validation Error if validation fails.
///
/// # Examples
///
/// ```ignore
/// use rapina::prelude::*;
///
/// #[derive(Deserialize, Validate)]
/// struct CreateUser {
///     #[validate(email)]
///     email: String,
///     #[validate(length(min = 8))]
///     password: String,
/// }
///
/// #[post("/users")]
/// async fn create_user(body: Validated<Json<CreateUser>>) -> String {
///     // data is guaranteed to be valid
///     format!("Created user: {}", body.email)
/// }
/// ```
#[derive(Debug)]
pub struct Validated<T>(pub T);

/// Type alias for path parameters extracted from the URL.
pub type PathParams = HashMap<String, String>;

/// Trait for extractors that consume the request body.
///
/// Implement this trait for extractors that need access to the full request,
/// including the body. Only one body-consuming extractor can be used per handler.
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
/// Multiple parts-only extractors can be used in a single handler.
pub trait FromRequestParts: Sized + Send {
    /// Extract the value from request parts.
    fn from_request_parts(
        parts: &http::request::Parts,
        params: &PathParams,
        state: &Arc<AppState>,
    ) -> impl std::future::Future<Output = Result<Self, Error>> + Send;
}

impl<T> Json<T> {
    /// Consumes the extractor and returns the inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Path<T> {
    /// Consumes the extractor and returns the inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Query<T> {
    /// Consumes the extractor and returns the inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Form<T> {
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
    /// Consumes the extractor and returns the inner value.
    pub fn into_inner(self) -> T {
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
        &self.0.trace_id
    }

    /// Returns the elapsed time since the request started.
    pub fn elapsed(&self) -> std::time::Duration {
        self.0.elapsed()
    }
}

impl<T> Validated<T> {
    /// Consumes the extractor and returns the validated inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T: DeserializeOwned + Send> FromRequest for Json<T> {
    async fn from_request(
        req: Request<Incoming>,
        _params: &PathParams,
        _state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        let body = req.into_body();
        let bytes = body
            .collect()
            .await
            .map_err(|_| Error::bad_request("Failed to read request body"))?
            .to_bytes();

        let value: T = serde_json::from_slice(&bytes)
            .map_err(|e| Error::bad_request(format!("Invalid JSON in request body: {}", e)))?;

        Ok(Json(value))
    }
}

impl<T: serde::Serialize> IntoResponse for (http::StatusCode, Json<T>) {
    fn into_response(self) -> http::Response<BoxBody> {
        let body = serde_json::to_vec(&(self.1).0).unwrap_or_default();
        http::Response::builder()
            .status(self.0)
            .header("content-type", JSON_CONTENT_TYPE)
            .body(http_body_util::Full::new(Bytes::from(body)))
            .unwrap()
    }
}

impl<T: serde::Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> http::Response<BoxBody> {
        (http::StatusCode::OK, self).into_response()
    }
}

impl<T: DeserializeOwned + Send> FromRequest for Form<T> {
    async fn from_request(
        req: Request<Incoming>,
        _params: &PathParams,
        _state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        let content_type = req
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok());

        if !content_type
            .map(|ct| ct.starts_with(FORM_CONTENT_TYPE))
            .unwrap_or(false)
        {
            return Err(Error::bad_request(format!(
                "Expected Content-Type '{}', got '{}'",
                FORM_CONTENT_TYPE,
                content_type.unwrap_or("none")
            )));
        }

        let body = req.into_body();
        let bytes = body
            .collect()
            .await
            .map_err(|_| Error::bad_request("Failed to read form data from request body"))?
            .to_bytes();

        let value: T = serde_urlencoded::from_bytes(&bytes)
            .map_err(|e| Error::bad_request(format!("Invalid URL-encoded form data: {}", e)))?;

        Ok(Form(value))
    }
}

impl<T: DeserializeOwned + Validate + Send> FromRequest for Validated<Json<T>> {
    async fn from_request(
        req: Request<Incoming>,
        params: &PathParams,
        state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        let json = Json::<T>::from_request(req, params, state).await?;
        json.0.validate().map_err(|e| {
            Error::validation("validation failed")
                .with_details(serde_json::to_value(e).unwrap_or_default())
        })?;
        Ok(Validated(json))
    }
}

impl<T: DeserializeOwned + Validate + Send> FromRequest for Validated<Form<T>> {
    async fn from_request(
        req: Request<Incoming>,
        params: &PathParams,
        state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        let form = Form::<T>::from_request(req, params, state).await?;
        form.0.validate().map_err(|e| {
            Error::validation("validation failed")
                .with_details(serde_json::to_value(e).unwrap_or_default())
        })?;
        Ok(Validated(form))
    }
}

impl<T: Clone + Send + Sync + 'static> FromRequestParts for State<T> {
    async fn from_request_parts(
        _parts: &http::request::Parts,
        _params: &PathParams,
        state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        let value = state.get::<T>().ok_or_else(|| {
            Error::internal(format!(
                "State not registered for type '{}'. Did you forget to call .state()?",
                std::any::type_name::<T>()
            ))
        })?;
        Ok(State(value.clone()))
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

impl<T: FromStr + Send> FromRequestParts for Path<T>
where
    T::Err: std::fmt::Display,
{
    async fn from_request_parts(
        _parts: &http::request::Parts,
        params: &PathParams,
        _state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        let (param_name, value) = params.iter().next().ok_or_else(|| {
            Error::bad_request(
                "Missing path parameter. Ensure your route pattern includes a parameter like /:id",
            )
        })?;

        let parsed = value.parse::<T>().map_err(|e| {
            Error::bad_request(format!(
                "Path parameter '{}' must be a valid {}, got '{}': {}",
                param_name,
                std::any::type_name::<T>(),
                value,
                e
            ))
        })?;

        Ok(Path(parsed))
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

pub fn extract_path_params(pattern: &str, path: &str) -> Option<PathParams> {
    let pattern_parts: Vec<&str> = pattern.split('/').collect();
    let path_parts: Vec<&str> = path.split('/').collect();

    if pattern_parts.len() != path_parts.len() {
        return None;
    }

    let mut params = HashMap::new();

    for (pattern_part, path_part) in pattern_parts.iter().zip(path_parts.iter()) {
        if let Some(param_name) = pattern_part.strip_prefix(':') {
            params.insert(param_name.to_string(), path_part.to_string());
        } else if pattern_part != path_part {
            return None;
        }
    }

    Some(params)
}

macro_rules! impl_deref {
    ($name:ident) => {
        impl<T> Deref for $name<T> {
            type Target = T;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    };
}

impl_deref!(State);
impl_deref!(Json);
impl_deref!(Path);
impl_deref!(Query);
impl_deref!(Form);
impl_deref!(Cookie);
impl_deref!(Validated);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{TestRequest, empty_params, empty_state, params};

    #[derive(Debug, Clone, PartialEq)]
    struct Data {
        name: String,
    }

    // Path params extraction tests
    #[test]
    fn test_extract_path_params_exact_match() {
        let result = extract_path_params("/users", "/users");
        assert!(result.is_some());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_extract_path_params_single_param() {
        let result = extract_path_params("/users/:id", "/users/123");
        assert!(result.is_some());
        let params = result.unwrap();
        assert_eq!(params.get("id"), Some(&"123".to_string()));
    }

    #[test]
    fn test_extract_path_params_multiple_params() {
        let result = extract_path_params("/users/:user_id/posts/:post_id", "/users/1/posts/42");
        assert!(result.is_some());
        let params = result.unwrap();
        assert_eq!(params.get("user_id"), Some(&"1".to_string()));
        assert_eq!(params.get("post_id"), Some(&"42".to_string()));
    }

    #[test]
    fn test_extract_path_params_no_match_different_length() {
        let result = extract_path_params("/users/:id", "/users/123/extra");
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_path_params_no_match_different_static() {
        let result = extract_path_params("/users/:id", "/posts/123");
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_path_params_root() {
        let result = extract_path_params("/", "/");
        assert!(result.is_some());
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
        assert_eq!(err.status, 400);
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

    // Path extractor tests
    #[tokio::test]
    async fn test_path_extractor_u64() {
        let (parts, _) = TestRequest::get("/users/123").into_parts();
        let params = params(&[("id", "123")]);

        let result = Path::<u64>::from_request_parts(&parts, &params, &empty_state()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, 123);
    }

    #[tokio::test]
    async fn test_path_extractor_string() {
        let (parts, _) = TestRequest::get("/users/john").into_parts();
        let params = params(&[("name", "john")]);

        let result = Path::<String>::from_request_parts(&parts, &params, &empty_state()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "john");
    }

    #[tokio::test]
    async fn test_path_extractor_invalid_type() {
        let (parts, _) = TestRequest::get("/users/notanumber").into_parts();
        let params = params(&[("id", "notanumber")]);

        let result = Path::<u64>::from_request_parts(&parts, &params, &empty_state()).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status, 400);
    }

    #[tokio::test]
    async fn test_path_extractor_missing_param() {
        let (parts, _) = TestRequest::get("/users").into_parts();
        let params = empty_params();

        let result = Path::<u64>::from_request_parts(&parts, &params, &empty_state()).await;
        assert!(result.is_err());
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
        #[derive(Clone)]
        struct AppConfig {
            name: String,
        }

        let state = crate::test::state_with(AppConfig {
            name: "test-app".to_string(),
        });
        let (parts, _) = TestRequest::get("/").into_parts();

        let result = State::<AppConfig>::from_request_parts(&parts, &empty_params(), &state).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0.name, "test-app");
    }

    #[tokio::test]
    async fn test_state_extractor_not_found() {
        #[derive(Clone, Debug)]
        struct MissingState;

        let state = empty_state();
        let (parts, _) = TestRequest::get("/").into_parts();

        let result =
            State::<MissingState>::from_request_parts(&parts, &empty_params(), &state).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status, 500);
    }

    // into_inner tests
    #[test]
    fn test_json_into_inner() {
        let json = Json("value".to_string());
        assert_eq!(json.into_inner(), "value");
    }

    #[test]
    fn test_path_into_inner() {
        let path = Path(42u64);
        assert_eq!(path.into_inner(), 42);
    }

    #[test]
    fn test_query_into_inner() {
        let query = Query("test".to_string());
        assert_eq!(query.into_inner(), "test");
    }

    #[test]
    fn test_form_into_inner() {
        let form = Form("data".to_string());
        assert_eq!(form.into_inner(), "data");
    }

    #[test]
    fn test_headers_into_inner() {
        let headers = Headers(http::HeaderMap::new());
        let inner = headers.into_inner();
        assert!(inner.is_empty());
    }

    #[test]
    fn test_state_into_inner() {
        let state = State("value".to_string());
        assert_eq!(state.into_inner(), "value");
    }

    #[test]
    fn test_context_into_inner() {
        let ctx = crate::context::RequestContext::with_trace_id("test".to_string());
        let context = Context(ctx);
        assert_eq!(context.into_inner().trace_id, "test");
    }

    #[test]
    fn test_context_elapsed() {
        let ctx = crate::context::RequestContext::new();
        let context = Context(ctx);
        // Verify elapsed() returns a Duration (compile-time check)
        let _elapsed: std::time::Duration = context.elapsed();
    }

    #[test]
    fn test_validated_into_inner() {
        let validated = Validated("value".to_string());
        assert_eq!(validated.into_inner(), "value");
    }

    #[test]
    fn test_validated_with_struct() {
        #[derive(Debug, PartialEq)]
        struct Data {
            name: String,
        }

        let validated = Validated(Data {
            name: "test".to_string(),
        });
        assert_eq!(
            validated.into_inner(),
            Data {
                name: "test".to_string()
            }
        );
    }

    // deref tests
    #[test]
    fn test_json_deref() {
        let json = Json("value".to_string());
        assert_eq!(*json, "value");
    }

    #[test]
    fn test_path_deref() {
        let path = Path(42u64);
        assert_eq!(*path, 42);
    }

    #[test]
    fn test_query_deref() {
        let query = Query("test".to_string());
        assert_eq!(*query, "test");
    }

    #[test]
    fn test_form_deref() {
        let form = Form("data".to_string());
        assert_eq!(*form, "data");
    }

    #[test]
    fn test_state_deref() {
        let state = State("value".to_string());
        assert_eq!(*state, "value");
    }

    #[test]
    fn test_validated_deref() {
        let validated = Validated("value".to_string());
        assert_eq!(*validated, "value");
    }

    #[test]
    fn test_validated_deref_with_struct() {
        let validated = Validated(Data {
            name: "test".to_string(),
        });
        assert_eq!(
            *validated,
            Data {
                name: "test".to_string()
            }
        );
    }

    // autoderef tests
    #[test]
    fn test_json_autoderef() {
        let data = Data {
            name: "json test".to_string(),
        };

        let json = Json(data.clone());
        assert_eq!(json.name, data.name);
    }

    #[test]
    fn test_state_autoderef() {
        let data = Data {
            name: "state test".to_string(),
        };

        let state = State(data.clone());
        assert_eq!(state.name, data.name);
    }

    #[test]
    fn test_form_autoderef() {
        let data = Data {
            name: "form test".to_string(),
        };

        let form = Form(data.clone());
        assert_eq!(form.name, data.name);
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
        assert_eq!(ctx.trace_id, "test");
    }

    #[test]
    fn test_validated_autoderef() {
        let data = Data {
            name: "test".to_string(),
        };

        let validated = Validated(data.clone());
        assert_eq!(validated.name, data.name);
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
        assert_eq!(err.status, 400);
        assert!(err.message.contains("session_id"));
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
}
