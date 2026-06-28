//! GraphQL integration via [`async-graphql`].
//!
//! Gated behind the `graphql` feature flag. Provides two newtype wrappers that
//! bridge `async-graphql` request/response types into Rapina's extractor and
//! responder system, so handlers can receive and return GraphQL payloads
//! without dealing with HTTP plumbing.
//!
//! - [`GraphQLRequest`] — `FromRequest` extractor that handles both **POST**
//!   (JSON body) and **GET** (query string) per the [GraphQL-over-HTTP spec].
//!   Malformed input returns 400.
//! - [`GraphQLResponse`] — `IntoResponse` responder that always returns HTTP
//!   200 with `Content-Type: application/json`. Field-level resolver errors
//!   live in the response body's `errors` array, never in the HTTP status.
//!
//! [`async-graphql`]: https://docs.rs/async-graphql
//! [GraphQL-over-HTTP spec]: https://graphql.github.io/graphql-over-http/
//!
//! # Example
//!
//! ```rust,ignore
//! use rapina::prelude::*;
//! use rapina::graphql::{GraphQLRequest, GraphQLResponse};
//! use async_graphql::{EmptyMutation, EmptySubscription, Object, Schema};
//!
//! struct Query;
//!
//! #[Object]
//! impl Query {
//!     async fn hello(&self) -> &str {
//!         "world"
//!     }
//! }
//!
//! type AppSchema = Schema<Query, EmptyMutation, EmptySubscription>;
//!
//! #[post("/graphql")]
//! #[public]
//! async fn graphql_handler(
//!     State(schema): State<AppSchema>,
//!     req: GraphQLRequest,
//! ) -> GraphQLResponse {
//!     GraphQLResponse(schema.execute(req.into_inner()).await)
//! }
//!
//! #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     let schema = Schema::build(Query, EmptyMutation, EmptySubscription).finish();
//!
//!     Rapina::new()
//!         .state(schema)
//!         .router(Router::new())
//!         .discover()
//!         .listen("127.0.0.1:3000")
//!         .await
//! }
//! ```

use crate::extract::FromRequest;
use crate::response::{APPLICATION_JSON, BoxBody, IntoResponse, full};
use async_graphql::{Request as GraphQLRequestInner, Response as GraphQLResponseInner, Variables};
use http_body_util::BodyExt;
use serde::Deserialize;
use std::ops::Deref;

/// Intermediate struct for GET query-param extraction.
/// Extensions intentionally unsupported for now, implement later for APQ/persistedQuery
#[derive(Deserialize)]
struct GraphQLParams {
    query: String,
    #[serde(default)]
    variables: Option<String>,
    #[serde(default, rename = "operationName")]
    operation_name: Option<String>,
}

/// GraphQL request extractor.
///
/// Implements `FromRequest`. Accepts:
///
/// - **POST** with a JSON body containing `query`, `variables`, and
///   `operationName`.
/// - **GET** with the same fields as URL query string params; the
///   `variables` param is URL-encoded JSON.
///
/// Any other HTTP method or malformed input returns a 400 via
/// [`Error::bad_request`](crate::error::Error::bad_request).
#[derive(Debug)]
pub struct GraphQLRequest(pub GraphQLRequestInner);

/// GraphQL response responder.
///
/// Implements `IntoResponse`. Always returns HTTP 200 with
/// `Content-Type: application/json`, per the GraphQL-over-HTTP spec.
/// Field-level resolver errors are carried inside the response body's
/// `errors` array — they never become 4xx or 5xx HTTP statuses.
#[derive(Debug)]
pub struct GraphQLResponse(pub GraphQLResponseInner);

impl GraphQLRequest {
    /// Consumes the extractor and returns the inner `async_graphql::Request`.
    pub fn into_inner(self) -> GraphQLRequestInner {
        self.0
    }
}

impl GraphQLResponse {
    /// Consumes the responder and returns the inner `async_graphql::Response`.
    pub fn into_inner(self) -> GraphQLResponseInner {
        self.0
    }
}

impl Deref for GraphQLRequest {
    type Target = GraphQLRequestInner;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Deref for GraphQLResponse {
    type Target = GraphQLResponseInner;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromRequest for GraphQLRequest {
    async fn from_request(
        req: http::Request<hyper::body::Incoming>,
        _params: &crate::extract::PathParams,
        _state: &std::sync::Arc<crate::state::AppState>,
    ) -> Result<Self, crate::error::Error> {
        let method = req.method().clone();
        let query_string = req.uri().query().unwrap_or("").to_string();

        match method {
            http::Method::POST => {
                let bytes = req
                    .into_body()
                    .collect()
                    .await
                    .map_err(|_| crate::error::Error::bad_request("Failed to read request body"))?
                    .to_bytes();

                let inner: GraphQLRequestInner = serde_json::from_slice(&bytes).map_err(|e| {
                    crate::error::Error::bad_request(format!("Invalid GraphQL JSON: {}", e))
                })?;

                Ok(GraphQLRequest(inner))
            }

            http::Method::GET => {
                let params: GraphQLParams =
                    serde_urlencoded::from_str(&query_string).map_err(|e| {
                        crate::error::Error::bad_request(format!("Invalid query string: {}", e))
                    })?;

                let mut inner: GraphQLRequestInner = GraphQLRequestInner::new(params.query);

                if let Some(vars_str) = params.variables {
                    let value: serde_json::Value =
                        serde_json::from_str(&vars_str).map_err(|e| {
                            crate::error::Error::bad_request(format!(
                                "Invalid variables JSON: {}",
                                e
                            ))
                        })?;
                    inner = inner.variables(Variables::from_json(value));
                }

                if let Some(name) = params.operation_name {
                    inner = inner.operation_name(name);
                }

                Ok(GraphQLRequest(inner))
            }

            _ => Err(crate::error::Error::method_not_allowed(
                "Method not allowed in GraphQL",
            )),
        }
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
    use crate::test::{TestRequest, empty_params, empty_state};

    #[tokio::test]
    async fn post_json_body_extracts_request() {
        let req = TestRequest::post("/graphql")
            .json(&serde_json::json!({
                "query": "{hello}",
                "variables": {"name": "world" },
                "operationName": "Hello"
            }))
            .into_incoming_request()
            .await;
        let result = GraphQLRequest::from_request(req, &empty_params(), &empty_state()).await;
        assert!(result.is_ok());
        let request = result.unwrap().0;
        assert_eq!(request.query, "{hello}");
        assert_eq!(request.operation_name.as_deref(), Some("Hello"));
    }

    #[tokio::test]
    async fn get_query_string_extracts_request() {
        let req = TestRequest::get("/graphql?query=%7B+hello+%7D&operationName=Hello")
            .into_incoming_request()
            .await;

        let result = GraphQLRequest::from_request(req, &empty_params(), &empty_state()).await;
        assert!(result.is_ok());
        let request = result.unwrap().0;
        assert_eq!(request.query, "{ hello }");
    }

    #[tokio::test]
    async fn get_with_variables_and_operation_name() {
        let req = TestRequest::get(
            "/graphql?query=%7B+hello+%7D&variables=%7B%22name%22%3A%22world%22%7D&operationName=Hello",
        )
        .into_incoming_request()
        .await;

        let result = GraphQLRequest::from_request(req, &empty_params(), &empty_state()).await;
        assert!(result.is_ok());
        let request = result.unwrap().0;
        assert_eq!(
            request.variables,
            Variables::from_json(serde_json::json!({ "name" : "world" }))
        );
        assert_eq!(request.operation_name.as_deref(), Some("Hello"));
    }

    #[tokio::test]
    async fn malformed_body_returns_400() {
        let req = TestRequest::post("/graphql")
            .header("content-type", "application/json")
            .body(b"invalid json".as_ref())
            .into_incoming_request()
            .await;

        let result = GraphQLRequest::from_request(req, &empty_params(), &empty_state()).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status(), 400);
    }

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

    #[tokio::test]
    async fn unsupported_method_returns_405() {
        let req = TestRequest::delete("/graphql")
            .into_incoming_request()
            .await;

        let result = GraphQLRequest::from_request(req, &empty_params(), &empty_state()).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status(), 405);
    }
}
