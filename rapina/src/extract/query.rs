//! `Query<T>`, the query string extractor.

use serde::de::DeserializeOwned;
use std::sync::Arc;

use crate::error::Error;
use crate::extract::{FromRequestParts, PathParams, impl_deref};
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

impl<T> Query<T> {
    /// Consumes the extractor and returns the inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl_deref!(Query);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{TestRequest, empty_params, empty_state};

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

    #[test]
    fn test_query_into_inner() {
        let query = Query("test".to_string());
        assert_eq!(query.into_inner(), "test");
    }

    #[test]
    fn test_query_deref() {
        let query = Query("test".to_string());
        assert_eq!(*query, "test");
    }
}
