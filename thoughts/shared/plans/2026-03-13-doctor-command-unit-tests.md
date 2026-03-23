# Doctor Command Unit Tests

## Overview

Add unit tests for the three untested diagnostic functions in `rapina-cli/src/commands/doctor.rs`: `check_response_schemas`, `check_error_documentation`, and `check_openapi_metadata`. These are pure functions operating on `serde_json::Value`, making them trivially testable without a running server.

## Current State Analysis

The file already has tests for `check_duplicate_routes` (3 tests, lines 281–332). The other three diagnostic functions have identical structure — iterate over JSON, collect issues, push to `DiagnosticResult`. The existing test pattern is: construct JSON input, create empty `DiagnosticResult`, call function, assert on `warnings`/`passed`/`errors` vectors.

### Key Discoveries:
- `DiagnosticResult` is a private struct with `warnings`, `errors`, and `passed` fields (`doctor.rs:8-12`)
- All checks skip routes with paths starting with `/__rapina` (`doctor.rs:67,103,137,190`)
- `check_openapi_metadata` takes `&Result<Value, String>` unlike the others which take `&Value` (`doctor.rs:171`)
- `check_response_schemas` checks for presence of `response_schema` key (`doctor.rs:65`)
- `check_error_documentation` checks for non-empty `error_responses` array (`doctor.rs:97-101`)
- `check_openapi_metadata` checks for `summary` or `description` on each operation (`doctor.rs:196-197`)

## Desired End State

The `#[cfg(test)] mod tests` block in `doctor.rs` contains comprehensive tests for all four diagnostic functions. Running `cargo test -p rapina-cli` passes with all new tests green.

## What We're NOT Doing

- Testing `execute()` (requires a running server)
- Testing `fetch_json()` (requires network)
- Testing `print_results()` (output formatting, no logic)
- Refactoring existing code

## Implementation Approach

Add tests directly into the existing `mod tests` block, following the established pattern.

## Phase 1: Add Unit Tests

### Overview
Add 10 new test functions covering all three diagnostic functions with passing, failing, and edge cases.

### Changes Required:

#### 1. `check_response_schemas` tests
**File**: `rapina-cli/src/commands/doctor.rs` (append to `mod tests`)

```rust
#[test]
fn check_response_schemas_passes_when_all_have_schemas() {
    let routes = serde_json::json!([
        {"method": "GET", "path": "/users", "response_schema": {"type": "array"}},
        {"method": "POST", "path": "/users", "response_schema": {"type": "object"}},
    ]);
    let mut result = DiagnosticResult { warnings: Vec::new(), errors: Vec::new(), passed: Vec::new() };
    check_response_schemas(&routes, &mut result);
    assert!(result.warnings.is_empty());
    assert_eq!(result.passed.len(), 1);
    assert_eq!(result.passed[0], "All routes have response schemas");
}

#[test]
fn check_response_schemas_warns_on_missing_schema() {
    let routes = serde_json::json!([
        {"method": "GET", "path": "/users"},
        {"method": "POST", "path": "/users", "response_schema": {"type": "object"}},
    ]);
    let mut result = DiagnosticResult { warnings: Vec::new(), errors: Vec::new(), passed: Vec::new() };
    check_response_schemas(&routes, &mut result);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("GET /users"));
    assert!(result.passed.is_empty());
}

#[test]
fn check_response_schemas_skips_internal_routes() {
    let routes = serde_json::json!([
        {"method": "GET", "path": "/__rapina/routes"},
    ]);
    let mut result = DiagnosticResult { warnings: Vec::new(), errors: Vec::new(), passed: Vec::new() };
    check_response_schemas(&routes, &mut result);
    assert!(result.warnings.is_empty());
    assert_eq!(result.passed.len(), 1);
}
```

#### 2. `check_error_documentation` tests

```rust
#[test]
fn check_error_documentation_passes_when_all_documented() {
    let routes = serde_json::json!([
        {"method": "GET", "path": "/users", "error_responses": [{"status": 404}]},
        {"method": "POST", "path": "/users", "error_responses": [{"status": 422}]},
    ]);
    let mut result = DiagnosticResult { warnings: Vec::new(), errors: Vec::new(), passed: Vec::new() };
    check_error_documentation(&routes, &mut result);
    assert!(result.warnings.is_empty());
    assert_eq!(result.passed.len(), 1);
    assert_eq!(result.passed[0], "All routes have documented errors");
}

#[test]
fn check_error_documentation_warns_on_missing_errors() {
    let routes = serde_json::json!([
        {"method": "GET", "path": "/users"},
        {"method": "DELETE", "path": "/users/:id", "error_responses": []},
    ]);
    let mut result = DiagnosticResult { warnings: Vec::new(), errors: Vec::new(), passed: Vec::new() };
    check_error_documentation(&routes, &mut result);
    assert_eq!(result.warnings.len(), 2);
    assert!(result.warnings[0].contains("GET /users"));
    assert!(result.warnings[1].contains("DELETE /users/:id"));
    assert!(result.passed.is_empty());
}

#[test]
fn check_error_documentation_skips_internal_routes() {
    let routes = serde_json::json!([
        {"method": "GET", "path": "/__rapina/openapi"},
    ]);
    let mut result = DiagnosticResult { warnings: Vec::new(), errors: Vec::new(), passed: Vec::new() };
    check_error_documentation(&routes, &mut result);
    assert!(result.warnings.is_empty());
    assert_eq!(result.passed.len(), 1);
}
```

#### 3. `check_openapi_metadata` tests

```rust
#[test]
fn check_openapi_metadata_passes_when_all_documented() {
    let openapi: Result<Value, String> = Ok(serde_json::json!({
        "paths": {
            "/users": {
                "get": {"summary": "List users"},
                "post": {"description": "Create a user"}
            }
        }
    }));
    let mut result = DiagnosticResult { warnings: Vec::new(), errors: Vec::new(), passed: Vec::new() };
    check_openapi_metadata(&openapi, &mut result);
    assert!(result.warnings.is_empty());
    assert_eq!(result.passed.len(), 1);
    assert_eq!(result.passed[0], "All operations have descriptions");
}

#[test]
fn check_openapi_metadata_warns_on_missing_docs() {
    let openapi: Result<Value, String> = Ok(serde_json::json!({
        "paths": {
            "/users": {
                "get": {},
                "post": {"summary": "Create a user"}
            }
        }
    }));
    let mut result = DiagnosticResult { warnings: Vec::new(), errors: Vec::new(), passed: Vec::new() };
    check_openapi_metadata(&openapi, &mut result);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("GET /users"));
    assert!(result.passed.is_empty());
}

#[test]
fn check_openapi_metadata_warns_when_openapi_not_enabled() {
    let openapi: Result<Value, String> = Err("not found".to_string());
    let mut result = DiagnosticResult { warnings: Vec::new(), errors: Vec::new(), passed: Vec::new() };
    check_openapi_metadata(&openapi, &mut result);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("not enabled"));
}

#[test]
fn check_openapi_metadata_skips_internal_paths() {
    let openapi: Result<Value, String> = Ok(serde_json::json!({
        "paths": {
            "/__rapina/routes": {
                "get": {}
            }
        }
    }));
    let mut result = DiagnosticResult { warnings: Vec::new(), errors: Vec::new(), passed: Vec::new() };
    check_openapi_metadata(&openapi, &mut result);
    assert!(result.warnings.is_empty());
    assert_eq!(result.passed.len(), 1);
}
```

### Success Criteria:

#### Automated Verification:
- [x] Code compiles: `cargo check -p rapina-cli`
- [x] All tests pass: `cargo test -p rapina-cli -- doctor`
- [x] Formatting: `cargo fmt -p rapina-cli`
- [x] Clippy passes: `cargo clippy -p rapina-cli`

#### Manual Verification:
- None required — pure unit tests.
