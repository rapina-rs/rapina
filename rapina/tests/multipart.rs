#![cfg(feature = "multipart")]

use http::StatusCode;
use rapina::extract::{FromRequest, Multipart, PathParams};
use rapina::hyper::body::Incoming;
use rapina::prelude::*;
use rapina::state::AppState;
use rapina::testing::TestClient;
use std::sync::Arc;

#[tokio::test]
async fn test_multipart_extraction() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(Router::new().route(
            http::Method::POST,
            "/upload",
            |req: http::Request<Incoming>, params: PathParams, state: Arc<AppState>| async move {
                let mut multipart = Multipart::from_request(req, &params, &state).await.unwrap();
                let mut names = Vec::new();
                while let Some(field) = multipart.next_field().await.unwrap() {
                    names.push(field.name().unwrap_or_default().to_string());
                }
                names.join(",")
            },
        ));

    let client = TestClient::new(app).await;

    let boundary = "boundary";
    let body = format!(
        "--{boundary}\r\n\
         Content-Disposition: form-data; name=\"foo\"\r\n\
         \r\n\
         bar\r\n\
         --{boundary}\r\n\
         Content-Disposition: form-data; name=\"baz\"\r\n\
         \r\n\
         qux\r\n\
         --{boundary}--\r\n"
    );

    let response = client
        .post("/upload")
        .header(
            "content-type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .body(body)
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "foo,baz");
}

#[tokio::test]
async fn test_multipart_file_upload() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(Router::new().route(
            http::Method::POST,
            "/upload",
            |req: http::Request<Incoming>, params: PathParams, state: Arc<AppState>| async move {
                let mut multipart = Multipart::from_request(req, &params, &state).await.unwrap();
                let mut result = String::new();
                while let Some(field) = multipart.next_field().await.unwrap() {
                    let name = field.name().unwrap_or_default().to_string();
                    let file_name = field.file_name().unwrap_or_default().to_string();
                    let content_type = field.content_type().unwrap_or_default().to_string();
                    let text = field.text().await.unwrap();
                    result.push_str(&format!(
                        "{}:{}:{}:{};",
                        name, file_name, content_type, text
                    ));
                }
                result
            },
        ));

    let client = TestClient::new(app).await;

    let boundary = "boundary";
    let body = format!(
        "--{boundary}\r\n\
         Content-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\n\
         Content-Type: text/plain\r\n\
         \r\n\
         hello world\r\n\
         --{boundary}--\r\n"
    );

    let response = client
        .post("/upload")
        .header(
            "content-type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .body(body)
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "file:test.txt:text/plain:hello world;");
}

#[tokio::test]
async fn test_multipart_invalid_request() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(Router::new().route(
            http::Method::POST,
            "/upload",
            |req: http::Request<Incoming>, params: PathParams, state: Arc<AppState>| async move {
                match Multipart::from_request(req, &params, &state).await {
                    Ok(_) => "ok".into_response(),
                    Err(e) => e.into_response(),
                }
            },
        ));

    let client = TestClient::new(app).await;

    // Missing boundary
    let response = client
        .post("/upload")
        .header("content-type", "multipart/form-data")
        .body("test")
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Not multipart
    let response = client
        .post("/upload")
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
