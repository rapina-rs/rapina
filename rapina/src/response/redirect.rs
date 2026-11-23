//! HTTP redirect response type.

use http::{HeaderValue, Response, StatusCode, header};

use super::{BoxBody, IntoResponse, empty};
use crate::error::Error;

/// An HTTP redirect response.
///
/// Implements [`IntoResponse`] so handlers can return redirects directly:
///
/// ```
/// use rapina::prelude::*;
///
/// #[get("/old")]
/// async fn old() -> Redirect {
///     Redirect::to("/new") // 303 See Other
/// }
/// ```
///
/// Coming from FastAPI: its `RedirectResponse` defaults to `307`, which is
/// [`Redirect::temporary`] here.
#[derive(Debug, Clone)]
pub struct Redirect {
    status: StatusCode,
    location: String,
}

impl Redirect {
    /// Creates a `303 See Other` redirect to `uri`.
    ///
    /// This is the default for POST-then-GET flows (e.g., submitting a form
    /// or finishing a login, then redirecting to the next page with `GET`).
    pub fn to(uri: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SEE_OTHER,
            location: uri.into(),
        }
    }

    /// Creates a `307 Temporary Redirect` to `uri`.
    ///
    /// Preserves the HTTP method and request body when the client follows
    /// the redirect. Equivalent to FastAPI's default `RedirectResponse`.
    pub fn temporary(uri: impl Into<String>) -> Self {
        Self {
            status: StatusCode::TEMPORARY_REDIRECT,
            location: uri.into(),
        }
    }

    /// Creates a `308 Permanent Redirect` to `uri`.
    ///
    /// Preserves the HTTP method and request body when the client follows
    /// the redirect.
    pub fn permanent(uri: impl Into<String>) -> Self {
        Self {
            status: StatusCode::PERMANENT_REDIRECT,
            location: uri.into(),
        }
    }

    /// Creates a `302 Found` redirect to `uri`.
    pub fn found(uri: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FOUND,
            location: uri.into(),
        }
    }

    /// Creates a `301 Moved Permanently` redirect to `uri`.
    pub fn moved(uri: impl Into<String>) -> Self {
        Self {
            status: StatusCode::MOVED_PERMANENTLY,
            location: uri.into(),
        }
    }
}

impl IntoResponse for Redirect {
    fn into_response(self) -> Response<BoxBody> {
        let location = match HeaderValue::try_from(self.location) {
            Ok(val) => val,
            Err(err) => {
                return Error::internal(format!("invalid redirect location: {err}"))
                    .into_response();
            }
        };

        Response::builder()
            .status(self.status)
            .header(header::LOCATION, location)
            .body(empty())
            .unwrap_or_else(|err| {
                Error::internal(format!("failed to build redirect response: {err}"))
                    .into_response()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn test_redirect_to_see_other() {
        let res = Redirect::to("/new").into_response();
        assert_eq!(res.status(), StatusCode::SEE_OTHER);
        assert_eq!(res.headers().get(header::LOCATION).unwrap(), "/new");
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn test_redirect_temporary() {
        let res = Redirect::temporary("/temp").into_response();
        assert_eq!(res.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(res.headers().get(header::LOCATION).unwrap(), "/temp");
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn test_redirect_permanent() {
        let res = Redirect::permanent("/perm").into_response();
        assert_eq!(res.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(res.headers().get(header::LOCATION).unwrap(), "/perm");
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn test_redirect_found() {
        let res = Redirect::found("/found").into_response();
        assert_eq!(res.status(), StatusCode::FOUND);
        assert_eq!(res.headers().get(header::LOCATION).unwrap(), "/found");
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn test_redirect_moved() {
        let res = Redirect::moved("/moved").into_response();
        assert_eq!(res.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(res.headers().get(header::LOCATION).unwrap(), "/moved");
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty());
    }

    #[test]
    fn test_redirect_accepts_owned_string() {
        let res = Redirect::to(String::from("https://example.com/callback")).into_response();
        assert_eq!(res.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            res.headers().get(header::LOCATION).unwrap(),
            "https://example.com/callback"
        );
    }

    #[tokio::test]
    async fn test_redirect_invalid_location_returns_internal_error() {
        let res = Redirect::to("/bad\r\nheader").into_response();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(res.headers().get(header::LOCATION).is_none());
    }
}
