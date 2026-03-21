+++
title = "OpenAPI"
description = "Auto-generated OpenAPI 3.0 specs from route metadata"
weight = 10
date = 2026-03-04
+++

Rapina generates an OpenAPI 3.0.3 spec from your route metadata at startup. Call `.openapi()` on the app builder and the spec is served at `/__rapina/openapi.json`. Handler function names become operation IDs, `Json<T>` return types generate response schemas via `schemars`, and `#[errors(ErrorType)]` documents error responses automatically.

## Enabling OpenAPI

Pass a title and version to `.openapi()` on the Rapina builder:

```rust
use rapina::prelude::*;

#[derive(Serialize, Clone, JsonSchema)]
struct User {
    id: u64,
    name: String,
    email: String,
}

#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> Result<Json<User>> {
    let id = *id;
    Ok(Json(User {
        id,
        name: "Antonio".to_string(),
        email: "antonio@example.com".to_string(),
    }))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .openapi("My API", "1.0.0")
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```

Response types must derive `JsonSchema` from the `schemars` crate (re-exported through `rapina::prelude`). Without it the spec is still generated, but the 200 response won't include a schema.

---

## Response Schemas

When a handler returns `Json<T>` or `Result<Json<T>>`, Rapina uses `schemars::schema_for!` to generate the JSON Schema for `T` and embeds it in the 200 response. Any other return type (`StatusCode`, `String`, etc.) produces a bare "Success" response with no schema.

```rust
#[derive(Serialize, Clone, JsonSchema)]
struct UserResponse {
    id: u64,
    name: String,
    email: String,
    active: bool,
}

#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> Result<Json<UserResponse>> {
    // ...
}
```

The generated spec fragment for this handler:

```json
{
  "responses": {
    "200": {
      "description": "Success",
      "content": {
        "application/json": {
          "schema": {
            "type": "object",
            "required": ["id", "name", "email", "active"],
            "properties": {
              "id": { "type": "integer", "format": "uint64", "minimum": 0 },
              "name": { "type": "string" },
              "email": { "type": "string" },
              "active": { "type": "boolean" }
            }
          }
        }
      }
    }
  }
}
```

---

## Documenting Errors

The `#[errors(ErrorType)]` attribute on a handler links it to a type that implements `DocumentedError`. Each error variant becomes a separate status code entry in the spec.

### Define a domain error

```rust
use rapina::prelude::*;

pub enum OrderError {
    NotFound,
    OutOfStock,
}

impl IntoApiError for OrderError {
    fn into_api_error(self) -> Error {
        match self {
            OrderError::NotFound => Error::not_found("order not found"),
            OrderError::OutOfStock => Error::conflict("item out of stock"),
        }
    }
}

impl DocumentedError for OrderError {
    fn error_variants() -> Vec<ErrorVariant> {
        vec![
            ErrorVariant {
                status: 404,
                code: "NOT_FOUND",
                description: "Order not found",
            },
            ErrorVariant {
                status: 409,
                code: "OUT_OF_STOCK",
                description: "Item is out of stock",
            },
        ]
    }
}
```

`DocumentedError` requires `IntoApiError` as a supertrait. `IntoApiError` handles runtime conversion to `rapina::error::Error`; `DocumentedError` provides compile-time metadata for spec generation.

### Use it on a handler

```rust
#[get("/orders/:id")]
#[errors(OrderError)]
async fn get_order(id: Path<u64>) -> Result<Json<Order>> {
    // ...
}
```

The `#[errors]` attribute goes after the HTTP verb macro. The resulting spec includes a response entry for each status code:

```json
{
  "404": {
    "description": "Order not found",
    "content": {
      "application/json": {
        "schema": { "$ref": "#/components/schemas/ErrorResponse" }
      }
    }
  },
  "409": {
    "description": "Item is out of stock",
    "content": {
      "application/json": {
        "schema": { "$ref": "#/components/schemas/ErrorResponse" }
      }
    }
  }
}
```

All error responses reference the standard `ErrorResponse` schema in `components/schemas`, which matches Rapina's [error envelope format](/docs/core-concepts/errors/).

---

## The Spec Endpoint

`GET /__rapina/openapi.json` is registered automatically when you call `.openapi()`. The endpoint is public — it doesn't require authentication even when auth middleware is enabled. The response is pretty-printed JSON.

If `.openapi()` was not called, the endpoint isn't registered. Requests to `/__rapina/openapi.json` return 404.

Internal routes under `/__rapina/` are excluded from the generated spec, so the OpenAPI endpoint itself won't appear in your API documentation.

---

## CLI Tools

The `rapina` CLI ships three subcommands for working with OpenAPI specs. All three require a running development server and accept `--host` (default `127.0.0.1`) and `--port` / `-p` (default `3000`, also reads `$RAPINA_PORT` or `$SERVER_PORT`).

### Export

Fetches the spec from your running server and writes it to a file or stdout:

```sh
# Print to stdout
rapina openapi export

# Write to file
rapina openapi export -o openapi.json
```

### Check

Compares a committed spec file against the running server. Useful in CI to ensure the checked-in spec stays synchronized with the implementation:

```sh
rapina openapi check              # compares openapi.json (default)
rapina openapi check api-spec.json  # custom file path
```

On mismatch it prints a diff and exits non-zero, with a hint to run `rapina openapi export -o openapi.json` to update.

### Diff

Compares the current spec against a base branch and detects breaking changes:

```sh
rapina openapi diff --base main
rapina openapi diff --base main api-spec.json
```

The command exits non-zero only if there are breaking changes. Non-breaking changes print a warning but exit 0.

| Change | Classification |
|--------|---------------|
| Removed endpoint | Breaking |
| Removed HTTP method from endpoint | Breaking |
| Removed response field | Breaking |
| Response field type changed | Breaking |
| Added endpoint | Non-breaking |
| Added HTTP method to endpoint | Non-breaking |
| Added response field | Non-breaking |

---

## Handler Names and Operation IDs

Handler function names are used directly as the `operationId` in the spec. The function name is also humanized into a `summary` — underscores become spaces and the first letter is capitalized.

| Function | `operationId` | `summary` |
|----------|---------------|-----------|
| `list_users` | `list_users` | List users |
| `get_user` | `get_user` | Get user |
| `create_order` | `create_order` | Create order |

Keep handler names descriptive. `get_user` reads better than `user` in both the spec and the generated documentation.

Path parameters are extracted automatically from `:param` segments in the route path and documented as required path parameters in the spec. `"/users/:id"` becomes `"/users/{id}"` with a required `id` parameter.
