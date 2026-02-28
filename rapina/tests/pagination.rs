//! Integration tests for the pagination module.

#![cfg(feature = "database")]

use http::StatusCode;
use rapina::pagination::{Paginate, PaginationConfig};
use rapina::prelude::*;
use rapina::testing::TestClient;
use std::sync::Arc;

// -- Paginate extractor via TestClient --

#[tokio::test]
async fn test_paginate_defaults_via_handler() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(Router::new().route(
            http::Method::GET,
            "/items",
            |req, params, state: Arc<rapina::state::AppState>| async move {
                let (parts, _) = req.into_parts();
                let p = Paginate::from_request_parts(&parts, &params, &state)
                    .await
                    .unwrap();
                Json(serde_json::json!({
                    "page": p.page,
                    "per_page": p.per_page,
                }))
            },
        ));

    let client = TestClient::new(app).await;
    let response = client.get("/items").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = response.json();
    assert_eq!(json["page"], 1);
    assert_eq!(json["per_page"], 20);
}

#[tokio::test]
async fn test_paginate_explicit_params_via_handler() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(Router::new().route(
            http::Method::GET,
            "/items",
            |req, params, state: Arc<rapina::state::AppState>| async move {
                let (parts, _) = req.into_parts();
                let p = Paginate::from_request_parts(&parts, &params, &state)
                    .await
                    .unwrap();
                Json(serde_json::json!({
                    "page": p.page,
                    "per_page": p.per_page,
                }))
            },
        ));

    let client = TestClient::new(app).await;
    let response = client.get("/items?page=3&per_page=50").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = response.json();
    assert_eq!(json["page"], 3);
    assert_eq!(json["per_page"], 50);
}

#[tokio::test]
async fn test_paginate_invalid_page_returns_422() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(Router::new().route(
            http::Method::GET,
            "/items",
            |req, params, state: Arc<rapina::state::AppState>| async move {
                let (parts, _) = req.into_parts();
                match Paginate::from_request_parts(&parts, &params, &state).await {
                    Ok(p) => Json(serde_json::json!({"page": p.page})).into_response(),
                    Err(e) => e.into_response(),
                }
            },
        ));

    let client = TestClient::new(app).await;
    let response = client.get("/items?page=0").send().await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_paginate_per_page_exceeds_max_returns_422() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(Router::new().route(
            http::Method::GET,
            "/items",
            |req, params, state: Arc<rapina::state::AppState>| async move {
                let (parts, _) = req.into_parts();
                match Paginate::from_request_parts(&parts, &params, &state).await {
                    Ok(p) => Json(serde_json::json!({"per_page": p.per_page})).into_response(),
                    Err(e) => e.into_response(),
                }
            },
        ));

    let client = TestClient::new(app).await;
    let response = client.get("/items?per_page=101").send().await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_paginate_respects_custom_config() {
    let app = Rapina::new()
        .with_introspection(false)
        .state(PaginationConfig {
            default_per_page: 25,
            max_per_page: 50,
        })
        .router(Router::new().route(
            http::Method::GET,
            "/items",
            |req, params, state: Arc<rapina::state::AppState>| async move {
                let (parts, _) = req.into_parts();
                match Paginate::from_request_parts(&parts, &params, &state).await {
                    Ok(p) => Json(serde_json::json!({"page": p.page, "per_page": p.per_page}))
                        .into_response(),
                    Err(e) => e.into_response(),
                }
            },
        ));

    let client = TestClient::new(app).await;

    // Default per_page should come from config
    let response = client.get("/items").send().await;
    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = response.json();
    assert_eq!(json["per_page"], 25);

    // Exceeding custom max returns 422
    let response = client.get("/items?per_page=51").send().await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // Within custom max is fine
    let response = client.get("/items?per_page=50").send().await;
    assert_eq!(response.status(), StatusCode::OK);
}

// -- Paginated<T> response --

use rapina::pagination::Paginated;

#[tokio::test]
async fn test_paginated_response_via_handler() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/items", |_, _, _| async move {
                Paginated {
                    data: vec!["a", "b", "c"],
                    page: 2,
                    per_page: 3,
                    total: 9,
                    total_pages: 3,
                    has_prev: true,
                    has_next: true,
                }
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/items").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("application/json")
    );

    let json: serde_json::Value = response.json();
    assert_eq!(json["data"], serde_json::json!(["a", "b", "c"]));
    assert_eq!(json["page"], 2);
    assert_eq!(json["per_page"], 3);
    assert_eq!(json["total"], 9);
    assert_eq!(json["total_pages"], 3);
    assert_eq!(json["has_prev"], true);
    assert_eq!(json["has_next"], true);
}

use rapina::extract::FromRequestParts;
