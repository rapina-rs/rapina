//! Integration tests for `#[authorize]` macro.
//!
//! The modules below exercise the possible code-generation "branches" of the route macro:
//!
//! - handlers with no arguments
//! - handlers with one non-header argument
//! - handlers containing only typed-header arguments
//! - handlers with multiple/mixed arguments

use http::StatusCode;
use rapina::extract::{FromRequestParts, PathParams};
use rapina::prelude::*;
use rapina::state::AppState;
use rapina::testing::TestClient;
use serde::Deserialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

// ── Shared fixtures ──────────────────────────────────────────────────────────

/// Tracks whether authorization policies and route handlers execute
#[derive(Clone, Default)]
struct Counters {
    policy_calls: Arc<AtomicUsize>,
    handler_calls: Arc<AtomicUsize>,
}

impl Counters {
    fn policy_calls(&self) -> usize {
        self.policy_calls.load(Ordering::SeqCst)
    }

    fn handler_calls(&self) -> usize {
        self.handler_calls.load(Ordering::SeqCst)
    }

    fn record_policy_call(&self) {
        self.policy_calls.fetch_add(1, Ordering::SeqCst);
    }

    fn record_handler_call(&self) {
        self.handler_calls.fetch_add(1, Ordering::SeqCst);
    }
}

/// Authorization-only extractor used to test extraction success and failure
struct RequiredAuthorizationHeader;

impl FromRequestParts for RequiredAuthorizationHeader {
    async fn from_request_parts(
        parts: &http::request::Parts,
        _params: &PathParams,
        _state: &Arc<AppState>,
    ) -> Result<Self> {
        match parts.headers.get("x-authorization-test") {
            Some(value) if value == "allowed" => Ok(Self),
            _ => Err(Error::unauthorized(
                "missing or invalid x-authorization-test header",
            )),
        }
    }
}

fn test_counters() -> Counters {
    Counters::default()
}

fn test_app() -> Rapina {
    Rapina::new().with_introspection(false).discover()
}

fn test_app_with_counters(counters: Counters) -> Rapina {
    Rapina::new()
        .with_introspection(false)
        .state(counters)
        .discover()
}

// ── No-handler-arguments branch ─────────────────────────────────────────────

mod zero_argument_branch {
    use super::*;

    #[derive(Deserialize)]
    struct GateQuery {
        allow: bool,
    }

    struct MissingAuthorizationState;

    // Policies

    async fn zero_dependency_policy() -> Result<()> {
        Ok(())
    }

    async fn query_policy(query: &Query<GateQuery>) -> Result<()> {
        if query.allow {
            Ok(())
        } else {
            Err(Error::forbidden("allow=true is required"))
        }
    }

    async fn missing_state_policy(_state: &State<MissingAuthorizationState>) -> Result<()> {
        Ok(())
    }

    // Handlers

    #[get("/authorization/zero-arguments/no-dependencies")]
    #[authorize(zero_dependency_policy)]
    async fn zero_dependency_handler() -> &'static str {
        "authorized"
    }

    #[get("/authorization/zero-arguments/query")]
    #[authorize(query_policy(Query<GateQuery>))]
    async fn query_authorized_handler() -> &'static str {
        "authorized"
    }

    #[get("/authorization/zero-arguments/missing-state")]
    #[authorize(missing_state_policy(State<MissingAuthorizationState>))]
    async fn missing_state_handler() -> &'static str {
        "must not execute"
    }

    // Tests

    #[tokio::test]
    async fn zero_dependency_policy_allows_request() {
        let client = TestClient::new(test_app()).await;

        let response = client
            .get("/authorization/zero-arguments/no-dependencies")
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text(), "authorized");
    }

    #[tokio::test]
    async fn authorization_only_dependency_is_extracted() {
        let client = TestClient::new(test_app()).await;

        let response = client
            .get("/authorization/zero-arguments/query?allow=true")
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text(), "authorized");
    }

    #[tokio::test]
    async fn authorization_only_policy_denial_skips_handler() {
        let client = TestClient::new(test_app()).await;

        let response = client
            .get("/authorization/zero-arguments/query?allow=false")
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn authorization_dependency_extraction_failure_skips_handler() {
        let client = TestClient::new(test_app()).await;

        let response = client
            .get("/authorization/zero-arguments/missing-state")
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}

// ── Single non-header argument branch ────────────────────────────────────────

mod single_non_header_branch {
    use super::*;

    // Policies

    async fn allow_with_state(counters: &State<Counters>) -> Result<()> {
        counters.record_policy_call();
        Ok(())
    }

    async fn deny_with_state(counters: &State<Counters>) -> Result<()> {
        counters.record_policy_call();
        Err(Error::forbidden("access denied by authorization policy"))
    }

    async fn header_policy(
        _header: &RequiredAuthorizationHeader,
        counters: &State<Counters>,
    ) -> Result<()> {
        counters.record_policy_call();
        Ok(())
    }

    // Handlers

    #[get("/authorization/single-non-header/reused-state")]
    #[authorize(allow_with_state(State<Counters>))]
    async fn reused_state_handler(counters: State<Counters>) -> &'static str {
        counters.record_handler_call();
        "authorized"
    }

    #[get("/authorization/single-non-header/denied")]
    #[authorize(deny_with_state(State<Counters>))]
    async fn denied_handler(counters: State<Counters>) -> &'static str {
        counters.record_handler_call();
        "must not execute"
    }

    #[get("/authorization/single-non-header/extracted-dependency")]
    #[authorize(header_policy(
        RequiredAuthorizationHeader,
        State<Counters>,
    ))]
    async fn extracted_dependency_handler(counters: State<Counters>) -> &'static str {
        counters.record_handler_call();
        "authorized"
    }

    // Tests

    #[tokio::test]
    async fn matching_handler_dependency_is_reused() {
        let counters = test_counters();
        let client = TestClient::new(test_app_with_counters(counters.clone())).await;

        let response = client
            .get("/authorization/single-non-header/reused-state")
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text(), "authorized");
        assert_eq!(counters.policy_calls(), 1);
        assert_eq!(counters.handler_calls(), 1);
    }

    #[tokio::test]
    async fn denied_policy_skips_handler_body() {
        let counters = test_counters();
        let client = TestClient::new(test_app_with_counters(counters.clone())).await;

        let response = client
            .get("/authorization/single-non-header/denied")
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(counters.policy_calls(), 1);
        assert_eq!(
            counters.handler_calls(),
            0,
            "the handler must not execute after authorization is denied"
        );
    }

    #[tokio::test]
    async fn authorization_dependency_extraction_failure_skips_policy_and_handler() {
        let counters = test_counters();
        let client = TestClient::new(test_app_with_counters(counters.clone())).await;

        let response = client
            .get("/authorization/single-non-header/extracted-dependency")
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            counters.policy_calls(),
            0,
            "the policy must not run if one of its dependencies cannot be extracted"
        );
        assert_eq!(
            counters.handler_calls(),
            0,
            "the handler must not run if authorization dependency extraction fails"
        );
    }

    #[tokio::test]
    async fn successful_authorization_only_extraction_runs_policy_and_handler_once() {
        let counters = test_counters();
        let client = TestClient::new(test_app_with_counters(counters.clone())).await;

        let response = client
            .get("/authorization/single-non-header/extracted-dependency")
            .header("x-authorization-test", "allowed")
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text(), "authorized");
        assert_eq!(counters.policy_calls(), 1);
        assert_eq!(counters.handler_calls(), 1);
    }
}

// ── All typed-header arguments branch ────────────────────────────────────────

mod all_headers_branch {
    use super::*;

    // Policies

    async fn single_header_policy(request_id: &Header<String>) -> Result<()> {
        if request_id.as_str() == "allowed-request" {
            Ok(())
        } else {
            Err(Error::forbidden("request ID is not authorized"))
        }
    }

    async fn all_headers_policy(
        request_id: &Header<String>,
        retry_count: &Header<u32>,
        counters: &State<Counters>,
    ) -> Result<()> {
        counters.record_policy_call();

        if request_id.as_str() != "allowed-request" {
            return Err(Error::forbidden("request ID is not authorized"));
        }

        if **retry_count > 3 {
            return Err(Error::forbidden("retry count exceeds authorization limit"));
        }

        Ok(())
    }

    // Handlers

    /// Exercises the `single_is_header` classification while still entering the
    /// all-headers extraction branch.
    #[get("/authorization/all-headers/single")]
    #[authorize(single_header_policy(Header<String>))]
    async fn single_header_handler(#[header("x-request-id")] request_id: Header<String>) -> String {
        request_id.into_inner()
    }

    #[get("/authorization/all-headers/multiple")]
    #[authorize(all_headers_policy(
        Header<String>,
        Header<u32>,
        State<Counters>,
    ))]
    async fn all_headers_handler(
        #[header("x-request-id")] request_id: Header<String>,
        #[header("x-retry-count")] retry_count: Header<u32>,
    ) -> String {
        format!("{}:{}", request_id.into_inner(), retry_count.into_inner())
    }

    // Tests

    #[tokio::test]
    async fn single_typed_header_is_bound_before_policy_runs() {
        let client = TestClient::new(test_app()).await;

        let response = client
            .get("/authorization/all-headers/single")
            .header("x-request-id", "allowed-request")
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text(), "allowed-request");
    }

    #[tokio::test]
    async fn all_header_bindings_exist_before_policy_runs() {
        let counters = test_counters();
        let client = TestClient::new(test_app_with_counters(counters.clone())).await;

        let response = client
            .get("/authorization/all-headers/multiple")
            .header("x-request-id", "allowed-request")
            .header("x-retry-count", "2")
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text(), "allowed-request:2");
        assert_eq!(counters.policy_calls(), 1);
    }

    #[tokio::test]
    async fn all_headers_policy_denial_skips_handler() {
        let counters = test_counters();
        let client = TestClient::new(test_app_with_counters(counters.clone())).await;

        let response = client
            .get("/authorization/all-headers/multiple")
            .header("x-request-id", "denied-request")
            .header("x-retry-count", "2")
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(counters.policy_calls(), 1);
    }

    #[tokio::test]
    async fn all_headers_policy_can_reject_parsed_header_value() {
        let counters = test_counters();
        let client = TestClient::new(test_app_with_counters(counters.clone())).await;

        let response = client
            .get("/authorization/all-headers/multiple")
            .header("x-request-id", "allowed-request")
            .header("x-retry-count", "4")
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(counters.policy_calls(), 1);
    }

    #[tokio::test]
    async fn missing_required_header_fails_before_policy() {
        let counters = test_counters();
        let client = TestClient::new(test_app_with_counters(counters.clone())).await;

        let response = client
            .get("/authorization/all-headers/multiple")
            .header("x-request-id", "allowed-request")
            // x-retry-count deliberately omitted
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            counters.policy_calls(),
            0,
            "the policy must not run until all required headers are extracted"
        );
    }

    #[tokio::test]
    async fn malformed_typed_header_fails_before_policy() {
        let counters = test_counters();
        let client = TestClient::new(test_app_with_counters(counters.clone())).await;

        let response = client
            .get("/authorization/all-headers/multiple")
            .header("x-request-id", "allowed-request")
            .header("x-retry-count", "not-a-number")
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            counters.policy_calls(),
            0,
            "the policy must not run if typed-header parsing fails"
        );
    }
}

// ── Multiple/mixed arguments branch ─────────────────────────────────────────

mod multiple_arguments_branch {
    use super::*;

    #[derive(Deserialize, schemars::JsonSchema)]
    struct RequestBody {
        value: String,
    }

    // Policies

    async fn allow_with_state(counters: &State<Counters>) -> Result<()> {
        counters.record_policy_call();
        Ok(())
    }

    async fn deny_with_state(counters: &State<Counters>) -> Result<()> {
        counters.record_policy_call();
        Err(Error::forbidden("access denied by authorization policy"))
    }

    async fn state_and_header_policy(
        counters: &State<Counters>,
        request_id: &Header<String>,
    ) -> Result<()> {
        counters.record_policy_call();

        if request_id.as_str() == "allowed-request" {
            Ok(())
        } else {
            Err(Error::forbidden("request ID is not authorized"))
        }
    }

    async fn header_and_state_policy(
        request_id: &Header<String>,
        counters: &State<Counters>,
    ) -> Result<()> {
        counters.record_policy_call();

        if request_id.as_str() == "allowed-request" {
            Ok(())
        } else {
            Err(Error::forbidden("request ID is not authorized"))
        }
    }

    // Handlers

    /// Exercises the multiple-argument path whose last argument consumes the full
    /// request body through `FromRequest`
    #[post("/authorization/multiple/body-last")]
    #[authorize(allow_with_state(State<Counters>))]
    async fn body_last_handler(
        counters: State<Counters>,
        body: Json<RequestBody>,
    ) -> Result<Json<String>> {
        counters.record_handler_call();
        Ok(Json(body.into_inner().value))
    }

    #[post("/authorization/multiple/body-last-denied")]
    #[authorize(deny_with_state(State<Counters>))]
    async fn denied_body_last_handler(
        counters: State<Counters>,
        body: Json<RequestBody>,
    ) -> Result<Json<String>> {
        counters.record_handler_call();
        Ok(Json(body.into_inner().value))
    }

    /// Exercises the multiple-argument path whose last argument is a typed header and
    /// therefore does not consume the request body
    #[get("/authorization/multiple/header-last")]
    #[authorize(state_and_header_policy(
        State<Counters>,
        Header<String>,
    ))]
    async fn header_last_handler(
        counters: State<Counters>,
        #[header("x-request-id")] request_id: Header<String>,
    ) -> String {
        counters.record_handler_call();
        request_id.into_inner()
    }

    /// Exercises typed-header generation in the leading `parts_extractions` collection
    /// while the final argument uses normal `FromRequest`
    #[get("/authorization/multiple/header-first")]
    #[authorize(header_and_state_policy(
        Header<String>,
        State<Counters>,
    ))]
    async fn header_first_handler(
        #[header("x-request-id")] request_id: Header<String>,
        counters: State<Counters>,
    ) -> String {
        counters.record_handler_call();
        request_id.into_inner()
    }

    // Tests

    #[tokio::test]
    async fn authorization_preserves_body_for_last_body_extractor() {
        let counters = test_counters();
        let client = TestClient::new(test_app_with_counters(counters.clone())).await;

        let response = client
            .post("/authorization/multiple/body-last")
            .header("content-type", "application/json")
            .body(r#"{"value":"body preserved"}"#)
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text(), "\"body preserved\"");
        assert_eq!(counters.policy_calls(), 1);
        assert_eq!(counters.handler_calls(), 1);
    }

    #[tokio::test]
    async fn denied_policy_skips_multiple_argument_body_handler() {
        let counters = test_counters();
        let client = TestClient::new(test_app_with_counters(counters.clone())).await;

        let response = client
            .post("/authorization/multiple/body-last-denied")
            .header("content-type", "application/json")
            .body(r#"{"value":"must not be returned"}"#)
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(counters.policy_calls(), 1);
        assert_eq!(
            counters.handler_calls(),
            0,
            "the handler must not execute after authorization is denied"
        );
    }

    #[tokio::test]
    async fn typed_header_as_last_argument_is_bound_before_policy() {
        let counters = test_counters();
        let client = TestClient::new(test_app_with_counters(counters.clone())).await;

        let response = client
            .get("/authorization/multiple/header-last")
            .header("x-request-id", "allowed-request")
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text(), "allowed-request");
        assert_eq!(counters.policy_calls(), 1);
        assert_eq!(counters.handler_calls(), 1);
    }

    #[tokio::test]
    async fn typed_header_as_first_argument_is_bound_before_policy() {
        let counters = test_counters();
        let client = TestClient::new(test_app_with_counters(counters.clone())).await;

        let response = client
            .get("/authorization/multiple/header-first")
            .header("x-request-id", "allowed-request")
            .send()
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text(), "allowed-request");
        assert_eq!(counters.policy_calls(), 1);
        assert_eq!(counters.handler_calls(), 1);
    }
}
