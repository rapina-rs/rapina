//! GraphQLResponder
//! Uses async-graphql's `Response` as `GraphQLResponseInner` alias
//! Wrapped around the `GraphQLResponse` structure to work around the orphan rule

use crate::response::{APPLICATION_JSON, BoxBody, IntoResponse, full};
use async_graphql::Response as GraphQLResponseInner;
use std::ops::Deref;

/// GraphQL response responder.
///
/// Implements `IntoResponse`. Always returns HTTP 200 with
/// `Content-Type: application/json`, per the GraphQL-over-HTTP spec.
/// Field-level resolver errors are carried inside the response body's
/// `errors` array — they never become 4xx or 5xx HTTP statuses.
#[derive(Debug)]
pub struct GraphQLResponse(pub GraphQLResponseInner);

impl GraphQLResponse {
    /// Consumes the responder and returns the inner `async_graphql::Response`.
    pub fn into_inner(self) -> GraphQLResponseInner {
        self.0
    }
}

impl Deref for GraphQLResponse {
    type Target = GraphQLResponseInner;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl IntoResponse for GraphQLResponse {
    fn into_response(self) -> http::Response<BoxBody> {
        let body = serde_json::to_vec(&self.0).unwrap_or_default();
        http::Response::builder()
            .status(http::StatusCode::OK)
            .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
            .body(full(body))
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn field_level_error_still_returns_200() {
        let response = async_graphql::Response::from_errors(vec![async_graphql::ServerError::new(
            "Failed", None,
        )]);
        let http_response = GraphQLResponse(response).into_response();

        assert_eq!(http_response.status(), 200);

        let body_bytes = http_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();

        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(json.get("errors").is_some());
    }
}
