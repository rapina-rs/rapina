+++
title = "OpenAPI"
description = "Auto-generated OpenAPI 3.0 specs from route metadata"
weight = 10
date = 2026-03-04
+++

Rapina generates an OpenAPI 3.0.3 spec from your route metadata at startup. Call `.openapi()` on the app builder and the spec is served at `/__rapina/openapi.json`. Handler function names become operation IDs, `Json<T>` return types generate response schemas via `schemars`, and `#[errors(ErrorType)]` documents error responses automatically.

Route macros accept optional metadata attributes — `id`, `summary`, `description`, `tags`, `deprecated` — to enrich the generated spec without any separate annotation files.

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

## Operation Metadata

All HTTP verb macros accept a set of optional key-value parameters after the path for richer OpenAPI descriptions:

| Parameter | Type | Default | Description |
|---|---|---|---|
| `id` | string literal | handler name | Stable `operationId`. Use dotted notation like `"users.create"`. |
| `summary` | string literal | humanized handler name | Short one-line summary shown in Swagger UI and docs generators. |
| `description` | string literal | first rustdoc `///` line | Longer prose description of the operation. |
| `tags` | array of string literals | `[]` | Tags for grouping operations in Swagger UI. |
| `deprecated` | boolean | `false` | Marks the operation as deprecated. |
| `group` | string literal | — | Path prefix joined at compile time (see [routing](/docs/core-concepts/routing/)). |

Any combination of parameters is valid. Unknown keys produce a compile error.

### Examples

```rust
// Explicit operation ID and summary:
#[get("/users", id = "users.list", summary = "List all users")]
async fn list_users() -> Json<Vec<User>> { /* ... */ }

// Full metadata on a POST endpoint:
#[post(
    "/users",
    id = "users.create",
    summary = "Create a user",
    description = "Creates a new user account. The email must be unique.",
    tags = ["users"],
)]
async fn create_user(body: Json<CreateUser>) -> Json<User> { /* ... */ }

// Tagging with multiple tags:
#[get("/admin/reports", tags = ["admin", "reports"])]
async fn get_reports() -> Json<Vec<Report>> { /* ... */ }

// Marking a legacy endpoint as deprecated:
#[get("/v1/users", deprecated = true, tags = ["v1"])]
async fn list_users_v1() -> Json<Vec<UserV1>> { /* ... */ }
```

### Operation IDs

When `id` is set it becomes the `operationId` in the spec. Without it, the handler function name is used. Rapina validates uniqueness at startup — two public routes with the same effective `operationId` (whether from `id` or the handler name) cause a panic with a clear message.

Using dotted notation (`"resource.action"`) is recommended when you have multiple API versions or nested resources, because it makes generated SDK clients easier to read.

### Summaries

When `summary` is omitted, Rapina generates one automatically by humanizing the handler name: underscores become spaces and the first letter is capitalized. For most handlers this is good enough; use `summary` when the function name is abbreviated or doesn't read naturally.

### Descriptions

`description` maps to the OpenAPI `Operation::description` field — a longer prose explanation rendered below the summary in Swagger UI. You can also write it as a rustdoc comment on the function and Rapina will pick up the first non-empty line:

```rust
/// Creates a new user account. The email address must be unique across
/// all tenants. Returns 409 if the email is already in use.
#[post("/users", tags = ["users"])]
async fn create_user(body: Json<CreateUser>) -> Json<User> { /* ... */ }
```

When both `description = "..."` on the macro and a `///` comment are present, the explicit attribute takes precedence.

### Tags

Tags are arrays of string literals. They group operations in Swagger UI's sidebar and in most API documentation generators. A single route can belong to multiple tags:

```rust
#[get("/admin/audit-log", tags = ["admin", "audit"])]
async fn get_audit_log() -> Json<Vec<AuditEntry>> { /* ... */ }
```

Tags don't affect routing or authentication — they're purely documentation metadata.

---

## Response Schemas

When a handler returns `Json<T>` or `Result<Json<T>>`, Rapina uses `schemars` to generate the JSON Schema for `T` and embeds it in the 200 response. Any other return type (`StatusCode`, `String`, etc.) produces a bare "Success" response with no schema.

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

## Swagger UI

Rapina can serve the interactive [Swagger UI](https://swagger.io/tools/swagger-ui/) explorer alongside your spec. Enable it with the `swagger-ui` Cargo feature.

### Setup

Add the feature to your dependency:

```toml
[dependencies]
rapina = { version = "0.13", features = ["swagger-ui"] }
```

Then call `.swagger_ui()` on the app builder. It must be combined with `.openapi()`:

```rust
#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .openapi("My API", "1.0.0")
        .swagger_ui()               // serves at /__rapina/swagger/
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```

The UI is available at `http://localhost:3000/__rapina/swagger/` and automatically points at `/__rapina/openapi.json`.

### Custom path

Use `.swagger_ui_at()` to choose a different URL:

```rust
Rapina::new()
    .openapi("My API", "1.0.0")
    .swagger_ui_at("/docs/")
    .discover()
    .listen("127.0.0.1:3000")
    .await
```

Any path works as long as it doesn't conflict with your application routes. Trailing slashes are recommended but not required.

### CDN dependency

The Swagger UI bundle is loaded from the [unpkg](https://unpkg.com) CDN at runtime (`swagger-ui-dist@5`). The Rapina binary itself stays small — no static assets are embedded. In air-gapped environments you'll need to either proxy the CDN requests or serve the bundle yourself and point a custom HTML page at the spec URL.

### Development vs. production

Swagger UI is useful during development. For production deployments that don't need the explorer, simply omit `.swagger_ui()` — the feature flag has no runtime overhead when the method isn't called. If you want to expose it in staging but not production, wrap the call in a config check:

```rust
let app = Rapina::new()
    .openapi("My API", "1.0.0")
    .discover();

#[cfg(feature = "swagger-ui")]
let app = if config.enable_swagger_ui {
    app.swagger_ui()
} else {
    app
};

app.listen("127.0.0.1:3000").await
```

---

## Handler Names and Operation IDs

By default, handler function names are used as the `operationId` in the spec, and a humanized `summary` is derived automatically:

| Function | Default `operationId` | Default `summary` |
|----------|----------------------|-------------------|
| `list_users` | `list_users` | List users |
| `get_user` | `get_user` | Get user |
| `create_order` | `create_order` | Create order |

Override either with the `id` and `summary` macro parameters:

```rust
#[get("/users", id = "users.list", summary = "Browse users")]
async fn list_users() -> Json<Vec<User>> { /* ... */ }
```

Keep default handler names descriptive — `get_user` reads better than `user` in both the spec and any generated SDK clients. Use the `id` override when you need a stable identifier that won't change even if you rename the function, or when dotted namespacing helps organize a large API.

Path parameters are extracted automatically from `:param` segments in the route path and documented as required path parameters in the spec. `"/users/:id"` becomes `"/users/{id}"` with a required `id` parameter.

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
