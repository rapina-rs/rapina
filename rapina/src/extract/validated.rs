//! `Validated<T>` extractor

use crate::error::Error;
use crate::extract::{Form, Json};
use crate::extract::{FromRequest, PathParams, impl_deref};
use crate::state::AppState;
use http::Request;
use hyper::body::Incoming;
use serde::de::DeserializeOwned;
use std::sync::Arc;
use validator::Validate;

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

impl<T> Validated<T> {
    /// Consumes the extractor and returns the validated inner value.
    pub fn into_inner(self) -> T {
        self.0
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
            Error::validation("validation failed").with_details(
                serde_json::to_value(&e).unwrap_or_else(|err| {
                    tracing::warn!("Failed to serialize validation error details: {err}");
                    serde_json::Value::default()
                }),
            )
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
            Error::validation("validation failed").with_details(
                serde_json::to_value(&e).unwrap_or_else(|err| {
                    tracing::warn!("Failed to serialize validation error details: {err}");
                    serde_json::Value::default()
                }),
            )
        })?;
        Ok(Validated(form))
    }
}

impl_deref!(Validated);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{TestRequest, empty_params, empty_state};

    #[derive(Debug, Clone, PartialEq)]
    struct Data {
        name: String,
    }

    #[test]
    fn test_validated_into_inner() {
        let validated = Validated("value".to_string());
        assert_eq!(validated.into_inner(), "value");
    }

    #[test]
    fn test_validated_with_struct() {
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

    #[test]
    fn test_validated_autoderef() {
        let data = Data {
            name: "test".to_string(),
        };

        let validated = Validated(data.clone());
        assert_eq!(validated.name, data.name);
    }

    // Validated<Json<T>> extractor tests

    #[tokio::test]
    async fn test_validated_json_valid_input() {
        #[derive(serde::Deserialize, validator::Validate, Debug)]
        struct Payload {
            #[validate(length(min = 1))]
            name: String,
        }

        let req = TestRequest::post("/")
            .json(&serde_json::json!({ "name": "Alice" }))
            .into_incoming_request()
            .await;

        let result =
            Validated::<Json<Payload>>::from_request(req, &empty_params(), &empty_state()).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().into_inner().0.name, "Alice");
    }

    #[tokio::test]
    async fn test_validated_json_invalid_input_returns_422() {
        #[derive(serde::Deserialize, validator::Validate, Debug)]
        struct Payload {
            #[validate(length(min = 3))]
            name: String,
        }

        let req = TestRequest::post("/")
            .json(&serde_json::json!({ "name": "Al" }))
            .into_incoming_request()
            .await;

        let result =
            Validated::<Json<Payload>>::from_request(req, &empty_params(), &empty_state()).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status(), 422);
    }

    #[tokio::test]
    async fn test_validated_json_invalid_input_details_not_empty() {
        #[derive(serde::Deserialize, validator::Validate, Debug)]
        struct Payload {
            #[validate(email)]
            email: String,
        }

        let req = TestRequest::post("/")
            .json(&serde_json::json!({ "email": "not-an-email" }))
            .into_incoming_request()
            .await;

        let result =
            Validated::<Json<Payload>>::from_request(req, &empty_params(), &empty_state()).await;

        let err = result.unwrap_err();
        // details should be a non-null object with field errors, not an empty {}
        let details = err.details();
        assert!(details.is_some());
        let details = details.unwrap();
        assert!(details.is_object());
        assert!(!details.as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_validated_json_bad_json_returns_400() {
        let req = TestRequest::post("/")
            .header("content-type", "application/json")
            .body(b"{not valid json".as_ref())
            .into_incoming_request()
            .await;

        #[derive(serde::Deserialize, validator::Validate, Debug)]
        #[allow(dead_code)]
        struct Payload {
            name: String,
        }

        let result =
            Validated::<Json<Payload>>::from_request(req, &empty_params(), &empty_state()).await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status(), 400);
    }

    // Validated<Form<T>> extractor tests

    #[tokio::test]
    async fn test_validated_form_valid_input() {
        #[derive(serde::Deserialize, validator::Validate, Debug)]
        struct Payload {
            #[validate(length(min = 1))]
            name: String,
        }

        let req = TestRequest::post("/")
            .form(&serde_json::json!({ "name": "Bob" }))
            .into_incoming_request()
            .await;

        let result =
            Validated::<Form<Payload>>::from_request(req, &empty_params(), &empty_state()).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().into_inner().0.name, "Bob");
    }

    #[tokio::test]
    async fn test_validated_form_invalid_input_returns_422() {
        #[derive(serde::Deserialize, validator::Validate, Debug)]
        struct Payload {
            #[validate(length(min = 5))]
            name: String,
        }

        let req = TestRequest::post("/")
            .form(&serde_json::json!({ "name": "Bo" }))
            .into_incoming_request()
            .await;

        let result =
            Validated::<Form<Payload>>::from_request(req, &empty_params(), &empty_state()).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status(), 422);
    }

    #[tokio::test]
    async fn test_validated_form_invalid_input_details_not_empty() {
        #[derive(serde::Deserialize, validator::Validate, Debug)]
        struct Payload {
            #[validate(email)]
            email: String,
        }

        let req = TestRequest::post("/")
            .form(&serde_json::json!({ "email": "bad" }))
            .into_incoming_request()
            .await;

        let result =
            Validated::<Form<Payload>>::from_request(req, &empty_params(), &empty_state()).await;

        let err = result.unwrap_err();
        let details = err.details();
        assert!(details.is_some());
        let details = details.unwrap();
        assert!(details.is_object());
        assert!(!details.as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_validated_form_bad_content_type_returns_400() {
        let req = TestRequest::post("/")
            .header("content-type", "text/plain")
            .body(b"name=Bob".as_ref())
            .into_incoming_request()
            .await;

        #[derive(serde::Deserialize, validator::Validate, Debug)]
        #[allow(dead_code)]
        struct Payload {
            name: String,
        }

        let result =
            Validated::<Form<Payload>>::from_request(req, &empty_params(), &empty_state()).await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status(), 400);
    }
}
