//! Header extractors.
//!
//! `Header<T>` extracts a single HTTP header and parses it into `T` via the
//! [`FromHeaderStr`] trait.  The header name is derived automatically from the
//! Rust identifier (snake_case → kebab-case) or can be overridden with the
//! `#[header("explicit-name")]` attribute on the handler parameter.
//!
//! [`Headers`] extracts the whole [`http::HeaderMap`] instead, for when the
//! header names are only known at runtime.
//!
//! # Examples
//!
//! ```ignore
//! use rapina::prelude::*;
//!
//! #[get("/hello")]
//! async fn handler(x_request_id: Header<String>) -> String {
//!     format!("trace: {}", *x_request_id)
//! }
//! ```
//!
//! Optional header:
//!
//! ```ignore
//! #[get("/hello")]
//! async fn handler(x_forwarded_for: Option<Header<String>>) -> String {
//!     x_forwarded_for.map(|h| h.into_inner()).unwrap_or_default()
//! }
//! ```

use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;

use crate::error::Error;
use crate::extract::{FromRequestParts, PathParams};
use crate::state::AppState;

/// A typed, named header extractor.
///
/// Wraps the parsed header value and carries the header name used during
/// extraction.  The name is set by the proc-macro at compile time.
#[derive(Debug, Clone)]
pub struct Header<T> {
    value: T,
    name: &'static str,
}

impl<T> Header<T> {
    /// Creates a new `Header` wrapper (used internally by the extractor impl).
    ///
    /// # Note
    ///
    /// The `name` parameter must be `&'static str` because the proc-macro
    /// emits string literals at compile time. Runtime construction from a
    /// dynamic string is intentionally not supported — this type is designed
    /// for macro-generated extraction only.
    pub fn new(name: &'static str, value: T) -> Self {
        Self { value, name }
    }

    /// Returns the HTTP header name used to extract this value.
    pub fn header_name(&self) -> &'static str {
        self.name
    }

    /// Consumes the extractor and returns the inner parsed value.
    pub fn into_inner(self) -> T {
        self.value
    }

    /// Creates a `Header` wrapper from a runtime `String` name.
    ///
    /// Intended for unit tests and wrapper extractors that delegate to
    /// `Header<T>` with a dynamically constructed header name.  The
    /// macro-generated path always uses [`Header::new`] with a `&'static str`
    /// literal and is unaffected by this constructor.
    #[doc(hidden)]
    pub fn from_string(name: String, value: T) -> Self {
        let name: &'static str = Box::leak(name.into_boxed_str());
        Self { value, name }
    }
}

impl<T> Deref for Header<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

// ── FromHeaderStr trait ──────────────────────────────────────────────────────

/// Trait for types that can be parsed from an HTTP header value string.
///
/// Implement this for custom types.  Blanket impls are provided for `String`,
/// `uuid::Uuid`, and all primitive integer types.
///
/// # Naming
///
/// This trait is intentionally named `FromHeaderStr` (not `HeaderValue`) to
/// avoid a name collision with [`http::HeaderValue`], which is re-exported by
/// many HTTP crates and is commonly imported via glob (`use http::*`).
pub trait FromHeaderStr: Sized {
    /// Parse the raw header string into `Self`.
    ///
    /// Return `Err` with a human-readable reason on failure.
    fn from_header_str(s: &str) -> Result<Self, String>;
}

impl FromHeaderStr for String {
    fn from_header_str(s: &str) -> Result<Self, String> {
        Ok(s.to_owned())
    }
}

impl FromHeaderStr for uuid::Uuid {
    fn from_header_str(s: &str) -> Result<Self, String> {
        uuid::Uuid::parse_str(s).map_err(|e| e.to_string())
    }
}

macro_rules! impl_header_value_fromstr {
    ($($ty:ty),+ $(,)?) => {
        $(impl FromHeaderStr for $ty {
            fn from_header_str(s: &str) -> Result<Self, String> {
                <$ty as FromStr>::from_str(s).map_err(|e| e.to_string())
            }
        })+
    };
}

impl_header_value_fromstr!(
    i8, i16, i32, i64, i128, u8, u16, u32, u64, u128, f32, f64, bool,
);

// ── Internal helper used by the proc-macro-generated FromRequestParts impls ──

/// Extract a typed header value from request parts.
///
/// Called by the proc-macro-generated `FromRequestParts` impl for `Header<T>`.
/// Returns a structured error on missing or unparseable headers.
pub fn extract_header<T: FromHeaderStr>(
    parts: &http::request::Parts,
    name: &'static str,
) -> Result<T, Error> {
    let raw = parts
        .headers
        .get(name)
        .ok_or_else(|| {
            Error::new(
                400,
                "MISSING_HEADER",
                format!("Missing required header: {name}"),
            )
            .with_details(serde_json::json!({ "header": name }))
        })?
        .to_str()
        .map_err(|_| {
            Error::new(
                400,
                "INVALID_HEADER",
                format!("Header '{name}' contains non-UTF-8 bytes"),
            )
            .with_details(serde_json::json!({
                "header": name,
                "reason": "non-UTF-8 bytes",
            }))
        })?;

    T::from_header_str(raw).map_err(|reason| {
        Error::new(
            400,
            "INVALID_HEADER",
            format!("Invalid value for header '{name}': {reason}"),
        )
        .with_details(serde_json::json!({
            "header": name,
            "reason": reason,
        }))
    })
}

// ── FromRequestParts for Header<T> ───────────────────────────────────────────
//
// The proc-macro generates a specialised impl per handler parameter that
// supplies the concrete header name at compile time.  The generic impl below
// is NOT automatically usable without a name — it exists so that integration
// tests can construct `Header<T>` directly via `extract_header`.
//
// For the generic runtime path we do NOT implement `FromRequestParts` for
// `Header<T>` without a name because there is no way to infer the name from
// `T` alone.  The macro supplies `name` through code generation.

// ── Optional header support ──────────────────────────────────────────────────

/// Extract an optional typed header.
///
/// Returns `None` if the header is absent, `Err` only if the header is present
/// but cannot be parsed.  Like [`extract_header`], returns the raw parsed `T`
/// rather than a wrapped `Header<T>` — the caller is responsible for wrapping
/// via [`Header::new`] when needed.
pub fn extract_optional_header<T: FromHeaderStr>(
    parts: &http::request::Parts,
    name: &'static str,
) -> Result<Option<T>, Error> {
    let raw = match parts.headers.get(name) {
        None => return Ok(None),
        Some(v) => v.to_str().map_err(|_| {
            Error::new(
                400,
                "INVALID_HEADER",
                format!("Header '{name}' contains non-UTF-8 bytes"),
            )
            .with_details(serde_json::json!({
                "header": name,
                "reason": "non-UTF-8 bytes",
            }))
        })?,
    };

    T::from_header_str(raw).map(Some).map_err(|reason| {
        Error::new(
            400,
            "INVALID_HEADER",
            format!("Invalid value for header '{name}': {reason}"),
        )
        .with_details(serde_json::json!({
            "header": name,
            "reason": reason,
        }))
    })
}

// ── Headers ──────────────────────────────────────────────────────────────────

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

impl Deref for Headers {
    type Target = http::HeaderMap;
    fn deref(&self) -> &Self::Target {
        &self.0
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

// ── Macro-generated FromRequestParts helper ──────────────────────────────────
//
// The rapina-macros crate calls this when it emits the blanket impl for a
// handler parameter `foo: Header<T>`.  We expose the two helpers as public API
// so the generated code compiles without needing to reach into private modules.

#[doc(hidden)]
pub use extract_header as __extract_header;
#[doc(hidden)]
pub use extract_optional_header as __extract_optional_header;

#[cfg(test)]
mod tests {
    use crate::test::{TestRequest, empty_params, empty_state};

    use super::*;

    fn parts_with_header(name: &str, value: &str) -> http::request::Parts {
        let (parts, _) = TestRequest::get("/").header(name, value).into_parts();
        parts
    }

    fn parts_without_header() -> http::request::Parts {
        TestRequest::get("/").into_parts().0
    }

    // ── extract_header ────────────────────────────────────────────────────────

    #[test]
    fn test_extract_string_header_present() {
        let parts = parts_with_header("x-request-id", "abc-123");
        let v = extract_header::<String>(&parts, "x-request-id").unwrap();
        assert_eq!(v, "abc-123");
    }

    #[test]
    fn test_extract_string_header_missing_returns_400() {
        let parts = parts_without_header();
        let err = extract_header::<String>(&parts, "x-request-id").unwrap_err();
        assert_eq!(err.status(), 400);
        assert_eq!(err.code(), "MISSING_HEADER");
        let details = err.details().unwrap();
        assert_eq!(details["header"], "x-request-id");
    }

    #[test]
    fn test_extract_u64_header_valid() {
        let parts = parts_with_header("x-retry-count", "3");
        let v = extract_header::<u64>(&parts, "x-retry-count").unwrap();
        assert_eq!(v, 3);
    }

    #[test]
    fn test_extract_u64_header_malformed_returns_400() {
        let parts = parts_with_header("x-retry-count", "not-a-number");
        let err = extract_header::<u64>(&parts, "x-retry-count").unwrap_err();
        assert_eq!(err.status(), 400);
        assert_eq!(err.code(), "INVALID_HEADER");
        let details = err.details().unwrap();
        assert_eq!(details["header"], "x-retry-count");
        assert!(details["reason"].is_string());
    }

    #[test]
    fn test_extract_uuid_header_valid() {
        let id = uuid::Uuid::new_v4();
        let parts = parts_with_header("x-correlation-id", &id.to_string());
        let v = extract_header::<uuid::Uuid>(&parts, "x-correlation-id").unwrap();
        assert_eq!(v, id);
    }

    #[test]
    fn test_extract_uuid_header_malformed() {
        let parts = parts_with_header("x-correlation-id", "not-a-uuid");
        let err = extract_header::<uuid::Uuid>(&parts, "x-correlation-id").unwrap_err();
        assert_eq!(err.status(), 400);
        assert_eq!(err.code(), "INVALID_HEADER");
    }

    // ── extract_optional_header ───────────────────────────────────────────────

    #[test]
    fn test_optional_header_present() {
        let parts = parts_with_header("x-request-id", "abc");
        let result = extract_optional_header::<String>(&parts, "x-request-id").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "abc");
    }

    #[test]
    fn test_optional_header_absent_returns_none() {
        let parts = parts_without_header();
        let result = extract_optional_header::<String>(&parts, "x-request-id").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_optional_header_malformed_returns_400() {
        let parts = parts_with_header("x-count", "not-a-number");
        let err = extract_optional_header::<u32>(&parts, "x-count").unwrap_err();
        assert_eq!(err.status(), 400);
        assert_eq!(err.code(), "INVALID_HEADER");
    }

    // ── Header<T> struct ─────────────────────────────────────────────────────

    #[test]
    fn test_header_into_inner() {
        let h = Header::new("x-foo", "bar".to_string());
        assert_eq!(h.into_inner(), "bar");
    }

    #[test]
    fn test_header_deref() {
        let h = Header::new("x-count", 42u64);
        assert_eq!(*h, 42);
    }

    #[test]
    fn test_header_name() {
        let h = Header::new("x-request-id", "id".to_string());
        assert_eq!(h.header_name(), "x-request-id");
    }

    // ── HeaderValue impls ─────────────────────────────────────────────────────

    #[test]
    fn test_header_value_bool() {
        assert!(bool::from_header_str("true").unwrap());
        assert!(!bool::from_header_str("false").unwrap());
        assert!(bool::from_header_str("yes").is_err());
    }

    #[test]
    fn test_header_value_f64() {
        assert_eq!(f64::from_header_str("1.5").unwrap(), 1.5_f64);
        assert!(f64::from_header_str("abc").is_err());
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

    #[test]
    fn test_headers_into_inner() {
        let headers = Headers(http::HeaderMap::new());
        let inner = headers.into_inner();
        assert!(inner.is_empty());
    }

    #[test]
    fn test_headers_autoderef() {
        let headers = Headers(http::HeaderMap::new());
        assert!(headers.is_empty());
    }
}
