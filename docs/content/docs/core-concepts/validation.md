+++
title = "Validation"
description = "Automatic request validation with the Validated extractor"
weight = 5
date = 2026-03-04
+++

`Validated<T>` wraps an extractor and validates the extracted data using the [validator](https://docs.rs/validator) crate before it reaches your handler. If the payload fails validation, Rapina returns a `422 Unprocessable Entity` response with field-level error details automatically — your handler never executes.

## Basic Usage

Derive `Validate` on your request struct alongside `Deserialize`, then use `Validated<Json<T>>` as the handler parameter.

```rust
use rapina::prelude::*;

#[derive(Deserialize, Validate)]
struct CreateUser {
    #[validate(email)]
    email: String,
    #[validate(length(min = 8))]
    password: String,
}

#[post("/users")]
async fn create_user(body: Validated<Json<CreateUser>>) -> impl IntoResponse {
    // data is guaranteed valid here
    format!("Created user: {}", body.email)
}
```

Because `Validated<T>` implements `Deref`, you can access fields directly through `body.email`. If you need the owned inner value, call `body.into_inner()` to unwrap the `Json<CreateUser>`.

## Validation Rules

The `validator` crate provides these attributes through `#[validate(...)]`:

| Rule | Example | Description |
|------|---------|-------------|
| `email` | `#[validate(email)]` | Must be a valid email address |
| `url` | `#[validate(url)]` | Must be a valid URL |
| `length(min, max)` | `#[validate(length(min = 1, max = 100))]` | String length bounds |
| `range(min, max)` | `#[validate(range(min = 0, max = 150))]` | Numeric value bounds |
| `contains(pattern)` | `#[validate(contains(pattern = "@"))]` | Must contain substring |
| `regex(path)` | `#[validate(regex(path = *RE_USERNAME))]` | Must match a regex |
| `must_match(other)` | `#[validate(must_match(other = "password"))]` | Must equal another field |
| `custom(function)` | `#[validate(custom(function = "validate_name"))]` | Custom validation function |
| `nested` | `#[validate(nested)]` | Validate nested structs recursively |

See the [validator docs](https://docs.rs/validator) for the complete list.

## Form Validation

`Validated<Form<T>>` works exactly the same way for URL-encoded form data:

```rust
#[derive(Deserialize, Validate)]
struct LoginForm {
    #[validate(email)]
    email: String,
    #[validate(length(min = 8))]
    password: String,
}

#[post("/login")]
async fn login(form: Validated<Form<LoginForm>>) -> impl IntoResponse {
    format!("Welcome, {}", form.email)
}
```

## Error Response Format

When validation fails, Rapina returns a `422` response following the standard error envelope:

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "validation failed",
    "details": {
      "email": [
        {
          "code": "email",
          "message": null,
          "params": {
            "value": "not-an-email"
          }
        }
      ],
      "password": [
        {
          "code": "length",
          "message": null,
          "params": {
            "min": 8,
            "value": "short"
          }
        }
      ]
    }
  },
  "trace_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
}
```

The `details` object is keyed by field name. Each field contains an array of validation errors with the rule that failed (`code`), optional custom message, and the parameters that were checked. The `trace_id` ties the error back to the request for debugging.

## Full Example

A user registration endpoint with multiple validation rules:

```rust
use rapina::prelude::*;

#[derive(Deserialize, Validate)]
struct RegisterUser {
    #[validate(length(min = 1, max = 50))]
    name: String,
    #[validate(email)]
    email: String,
    #[validate(length(min = 8, max = 128))]
    password: String,
    #[validate(must_match(other = "password"))]
    password_confirmation: String,
    #[validate(range(min = 18, max = 150))]
    age: u32,
}

#[post("/v1/users/register")]
async fn register(body: Validated<Json<RegisterUser>>) -> impl IntoResponse {
    let user = body.into_inner().0;
    // All fields are valid — safe to persist
    Json(serde_json::json!({
        "message": "user registered",
        "email": user.email
    }))
}
```

Sending an invalid payload:

```bash
curl -X POST http://localhost:3000/v1/users/register \
  -H "Content-Type: application/json" \
  -d '{"name": "", "email": "bad", "password": "short", "password_confirmation": "nope", "age": 10}'
```

Returns `422 Unprocessable Entity` with every field that failed:

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "validation failed",
    "details": {
      "name": [{ "code": "length", "message": null, "params": { "min": 1, "max": 50, "value": "" } }],
      "email": [{ "code": "email", "message": null, "params": { "value": "bad" } }],
      "password": [{ "code": "length", "message": null, "params": { "min": 8, "max": 128, "value": "short" } }],
      "password_confirmation": [{ "code": "must_match", "message": null, "params": { "value": "nope", "other": "password" } }],
      "age": [{ "code": "range", "message": null, "params": { "min": 18.0, "max": 150.0, "value": 10 } }]
    }
  },
  "trace_id": "f47ac10b-58cc-4372-a567-0e02b2c3d479"
}
```
