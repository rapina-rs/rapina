//! GraphQL Request extractor
//! Uses async-graphql's `Request` as `GraphQLRequestInner` alias
//! wrapped around the `GraphQLRequest` struct to work around the `orphan rule`
//! Also declares the `GraphQLParams` for the query params extraction

use crate::extract::FromRequest;
use async_graphql::{Request as GraphQLRequestInner, Variables};
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
/// Any other HTTP method or malformed input returns a 405 via
/// [`Error::method_not_allowed`](crate::error::Error::method_not_allowed).
#[derive(Debug)]
pub struct GraphQLRequest(pub GraphQLRequestInner);

impl GraphQLRequest {
    /// Consumes the extractor and returns the inner `async_graphql::Request`.
    pub fn into_inner(self) -> GraphQLRequestInner {
        self.0
    }
}

impl Deref for GraphQLRequest {
    type Target = GraphQLRequestInner;
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
    async fn unsupported_method_returns_405() {
        let req = TestRequest::delete("/graphql")
            .into_incoming_request()
            .await;

        let result = GraphQLRequest::from_request(req, &empty_params(), &empty_state()).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status(), 405);
    }
}
