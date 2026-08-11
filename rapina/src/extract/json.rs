//! `Json<T>` extractor

use crate::error::Error;
use crate::extract::{FromRequest, PathParams, impl_deref};
use crate::response::{BoxBody, IntoResponse};
use crate::state::AppState;
use http::Request;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use serde::de::DeserializeOwned;
use std::sync::Arc;

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

impl<T> Json<T> {
    /// Consumes the extractor and returns the inner value.
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
        crate::error::json_response(self.0, &(self.1).0)
    }
}

impl<T: serde::Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> http::Response<BoxBody> {
        (http::StatusCode::OK, self).into_response()
    }
}

impl_deref!(Json);

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Data {
        name: String,
    }

    #[test]
    fn test_json_into_inner() {
        let json = Json("value".to_string());
        assert_eq!(json.into_inner(), "value");
    }
    #[test]
    fn test_json_deref() {
        let json = Json("value".to_string());
        assert_eq!(*json, "value");
    }

    #[test]
    fn test_json_autoderef() {
        let data = Data {
            name: "json test".to_string(),
        };

        let json = Json(data.clone());
        assert_eq!(json.name, data.name);
    }
}
