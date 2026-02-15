+++
title = "Error Handling"
description = "Standardized error responses with trace IDs"
weight = 4
date = 2025-02-13
+++

Rapina provides standardized error handling with consistent response formats and trace IDs for debugging.

## Error Response Format

All errors return a consistent JSON envelope:

```json
{
  "error": {
    "code": "NOT_FOUND",
    "message": "user not found"
  },
  "trace_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

The `trace_id` is automatically generated for each request and can be used to correlate logs and debug issues.

## Built-in Error Constructors

```rust
Error::bad_request("invalid input")      // 400
Error::unauthorized("login required")    // 401
Error::forbidden("access denied")        // 403
Error::not_found("user not found")       // 404
Error::conflict("already exists")        // 409
Error::validation("invalid email")       // 422
Error::rate_limited("too many requests") // 429
Error::internal("something went wrong")  // 500
```

## Using Errors in Handlers

Return `Result<T, Error>` or just `Result<T>` from handlers:

```rust
#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> Result<Json<User>> {
    let id = id.into_inner();

    if id == 0 {
        return Err(Error::bad_request("id cannot be zero"));
    }

    let user = find_user(id)
        .ok_or_else(|| Error::not_found("user not found"))?;

    Ok(Json(user))
}
```

## Adding Details

Add structured details to errors:

```rust
Error::validation("invalid input")
    .with_details(serde_json::json!({
        "field": "email",
        "reason": "invalid format"
    }))
```

Response:

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "invalid input",
    "details": {
      "field": "email",
      "reason": "invalid format"
    }
  },
  "trace_id": "..."
}
```

## Domain Errors

Define typed domain errors with automatic API conversion:

```rust
enum UserError {
    NotFound(u64),
    EmailTaken(String),
}

impl IntoApiError for UserError {
    fn into_api_error(self) -> Error {
        match self {
            UserError::NotFound(id) => Error::not_found(format!("user {} not found", id)),
            UserError::EmailTaken(email) => Error::conflict(format!("email {} taken", email)),
        }
    }
}
```

Use with the `?` operator:

```rust
#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> Result<Json<User>, UserError> {
    let id = id.into_inner();
    let user = find_user(id).ok_or(UserError::NotFound(id))?;
    Ok(Json(user))
}
```

## Documented Errors

Document error responses for OpenAPI generation:

```rust
use rapina::prelude::*;

struct GetUserHandler;

impl DocumentedError for GetUserHandler {
    fn error_responses() -> Vec<ErrorVariant> {
        vec![
            ErrorVariant::new(400, "BAD_REQUEST", "Invalid user ID"),
            ErrorVariant::new(404, "NOT_FOUND", "User not found"),
        ]
    }
}
```

## Error Codes

| HTTP Status | Code | Use Case |
|------------|------|----------|
| 400 | `BAD_REQUEST` | Invalid input, malformed request |
| 401 | `UNAUTHORIZED` | Missing or invalid authentication |
| 403 | `FORBIDDEN` | Authenticated but not allowed |
| 404 | `NOT_FOUND` | Resource doesn't exist |
| 409 | `CONFLICT` | Resource already exists |
| 422 | `VALIDATION_ERROR` | Input validation failed |
| 429 | `RATE_LIMITED` | Too many requests |
| 500 | `INTERNAL_ERROR` | Server error |
