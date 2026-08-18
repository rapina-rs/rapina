//! `Cookie<T>`, the cookie extractor.

use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::Error;
use crate::extract::{FromRequestParts, PathParams, impl_deref};
use crate::state::AppState;

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

impl<T> Cookie<T> {
    /// Consumes the extractor and returns the inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl_deref!(Cookie);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{TestRequest, empty_params, empty_state};

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
}
