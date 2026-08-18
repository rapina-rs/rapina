//! `Form<T>` extractor

use crate::error::Error;
use crate::extract::{FromRequest, PathParams, impl_deref};
use crate::response::FORM_CONTENT_TYPE;
use crate::state::AppState;
use http::Request;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use serde::de::DeserializeOwned;
use std::sync::Arc;

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

impl<T> Form<T> {
    /// Consumes the extractor and returns the inner value.
    pub fn into_inner(self) -> T {
        self.0
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

impl_deref!(Form);

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Data {
        name: String,
    }

    #[test]
    fn test_form_into_inner() {
        let form = Form("data".to_string());
        assert_eq!(form.into_inner(), "data");
    }

    #[test]
    fn test_form_deref() {
        let form = Form("data".to_string());
        assert_eq!(*form, "data");
    }

    #[test]
    fn test_form_autoderef() {
        let data = Data {
            name: "form test".to_string(),
        };

        let form = Form(data.clone());
        assert_eq!(form.name, data.name);
    }
}
