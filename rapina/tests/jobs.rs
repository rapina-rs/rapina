//! Integration tests for the `#[job]` macro.
//!
//! IMPORTANT: `inventory` collects from the entire test binary.
//! All `#[job]` handlers across all test files share the same `JobDescriptor`
//! collection. Use globally unique function names to avoid `job_type` collisions.

#![cfg(feature = "database")]

use std::sync::{Arc, Mutex};

use rapina::jobs::{JobDescriptor, JobRequest};
use rapina::prelude::*;
use rapina::state::AppState;

// ── Payload types ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct JobEmailPayload {
    to: String,
    subject: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct JobReportPayload {
    user_id: u64,
}

// ── Job definitions ──────────────────────────────────────────────────────────

// No attributes — all defaults.
#[job]
async fn job_test_basic(payload: JobEmailPayload) -> JobResult {
    let _ = payload;
    Ok(())
}

// Custom queue and max_retries.
#[job(queue = "emails", max_retries = 5)]
async fn job_test_email(payload: JobEmailPayload) -> JobResult {
    let _ = payload;
    Ok(())
}

// Dependency injection via State<T>. Uses a Mutex counter so the test can
// verify the handler actually ran.
#[job(queue = "reports")]
async fn job_test_with_di(payload: JobReportPayload, counter: State<Mutex<u32>>) -> JobResult {
    let _ = payload;
    *counter.lock().unwrap() += 1;
    Ok(())
}

// Intentional failure — lets us verify error propagation through the wrapper.
#[job]
async fn job_test_fails(payload: JobEmailPayload) -> JobResult {
    let _ = payload;
    Err(rapina::error::Error::internal("job failed intentionally"))
}

// Multiple calls — used to verify the handle wrapper can be invoked repeatedly
// with independent state.
#[job]
async fn job_test_multi_call(payload: JobReportPayload, counter: State<Mutex<u32>>) -> JobResult {
    let _ = payload;
    *counter.lock().unwrap() += 1;
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn find_descriptor(job_type: &str) -> Option<&'static JobDescriptor> {
    rapina::inventory::iter::<JobDescriptor>().find(|d| d.job_type == job_type)
}

// ── Helper function tests ────────────────────────────────────────────────────

#[test]
fn helper_no_attrs_uses_defaults() {
    let req: JobRequest = job_test_basic(JobEmailPayload {
        to: "a@b.com".into(),
        subject: "hi".into(),
    });

    assert_eq!(req.job_type, "job_test_basic");
    assert_eq!(req.queue, "default");
    assert_eq!(req.max_retries, 3);
}

#[test]
fn helper_custom_queue_and_retries() {
    let req = job_test_email(JobEmailPayload {
        to: "x@y.com".into(),
        subject: "test".into(),
    });

    assert_eq!(req.job_type, "job_test_email");
    assert_eq!(req.queue, "emails");
    assert_eq!(req.max_retries, 5);
}

#[test]
fn helper_serializes_payload_fields() {
    let req = job_test_email(JobEmailPayload {
        to: "user@example.com".into(),
        subject: "Welcome".into(),
    });

    assert_eq!(req.payload["to"], "user@example.com");
    assert_eq!(req.payload["subject"], "Welcome");
}

#[test]
fn helper_payload_roundtrips_through_json() {
    let original = JobEmailPayload {
        to: "round@trip.com".into(),
        subject: "roundtrip".into(),
    };
    let req = job_test_basic(JobEmailPayload {
        to: original.to.clone(),
        subject: original.subject.clone(),
    });

    let recovered: JobEmailPayload = serde_json::from_value(req.payload).unwrap();
    assert_eq!(recovered, original);
}

#[test]
fn helper_max_retries_is_i32() {
    // i32 must match the INTEGER column in rapina_jobs — same as JobRow::max_retries.
    let req = job_test_basic(JobEmailPayload {
        to: String::new(),
        subject: String::new(),
    });
    let _: i32 = req.max_retries; // compile-time check
    assert_eq!(req.max_retries, 3);
}

// ── Inventory registration tests ─────────────────────────────────────────────

#[test]
fn descriptor_registered_for_basic_job() {
    assert!(
        find_descriptor("job_test_basic").is_some(),
        "job_test_basic should be registered"
    );
}

#[test]
fn descriptor_registered_for_email_job() {
    assert!(
        find_descriptor("job_test_email").is_some(),
        "job_test_email should be registered"
    );
}

#[test]
fn descriptor_registered_for_di_job() {
    assert!(
        find_descriptor("job_test_with_di").is_some(),
        "job_test_with_di should be registered"
    );
}

#[test]
fn descriptor_job_type_matches_helper() {
    // The job_type in the descriptor must be identical to what the helper embeds.
    let desc = find_descriptor("job_test_basic").unwrap();
    let req = job_test_basic(JobEmailPayload {
        to: String::new(),
        subject: String::new(),
    });
    assert_eq!(desc.job_type, req.job_type);
}

// ── Handle wrapper tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn handle_wrapper_executes_successfully() {
    let desc = find_descriptor("job_test_basic").unwrap();
    let state = Arc::new(AppState::new());
    let payload = serde_json::json!({ "to": "a@b.com", "subject": "hi" });

    let result = (desc.handle)(payload, state).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn handle_wrapper_bad_payload_returns_internal_error() {
    let desc = find_descriptor("job_test_basic").unwrap();
    let state = Arc::new(AppState::new());
    // Missing required fields — serde_json::from_value will fail.
    let payload = serde_json::json!({ "unexpected": true });

    let result = (desc.handle)(payload, state).await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    // Error message must include the job_type so it's diagnosable in logs.
    assert!(
        err.message().contains("job_test_basic"),
        "error message should include the job type name, got: {}",
        err.message()
    );
}

#[tokio::test]
async fn handle_wrapper_propagates_handler_error() {
    let desc = find_descriptor("job_test_fails").unwrap();
    let state = Arc::new(AppState::new());
    let payload = serde_json::json!({ "to": "x@y.com", "subject": "test" });

    let result = (desc.handle)(payload, state).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .message()
            .contains("job failed intentionally"),
        "handler error should propagate unchanged"
    );
}

#[tokio::test]
async fn handle_wrapper_injects_state_via_state_extractor() {
    let desc = find_descriptor("job_test_with_di").unwrap();
    let state = Arc::new(AppState::new().with(Mutex::new(0u32)));
    let payload = serde_json::json!({ "user_id": 42 });

    assert_eq!(*state.get::<Mutex<u32>>().unwrap().lock().unwrap(), 0);

    let result = (desc.handle)(payload, Arc::clone(&state)).await;
    assert!(result.is_ok());

    let count = *state.get::<Mutex<u32>>().unwrap().lock().unwrap();
    assert_eq!(count, 1, "handler should have incremented the counter once");
}

#[tokio::test]
async fn handle_wrapper_can_be_called_multiple_times() {
    let desc = find_descriptor("job_test_multi_call").unwrap();
    let state = Arc::new(AppState::new().with(Mutex::new(0u32)));

    for _ in 0..5 {
        let payload = serde_json::json!({ "user_id": 1 });
        (desc.handle)(payload, Arc::clone(&state)).await.unwrap();
    }

    let count = *state.get::<Mutex<u32>>().unwrap().lock().unwrap();
    assert_eq!(count, 5);
}

#[tokio::test]
async fn handle_wrapper_missing_state_returns_error() {
    // job_test_with_di needs State<Mutex<u32>> but the state is empty.
    // The extractor should return an error, not panic.
    let desc = find_descriptor("job_test_with_di").unwrap();
    let state = Arc::new(AppState::new()); // no Mutex<u32> registered
    let payload = serde_json::json!({ "user_id": 99 });

    let result = (desc.handle)(payload, state).await;
    assert!(
        result.is_err(),
        "missing state dependency should return Err, not panic"
    );
}
