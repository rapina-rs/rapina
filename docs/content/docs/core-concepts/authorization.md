+++
title = "Authorization"
description = "Run route-level authorization policies before endpoint handlers"
weight = 4
date = 2026-07-17
+++

Authentication establishes **who** is making a request. Authorization determines **what that caller may do**.

Rapina's `#[authorize]` attribute attaches an async authorization policy to a route. Rapina extracts the policy's
declared dependencies, invokes the policy, and only runs the endpoint handler when the policy returns `Ok(())`.

If dependency extraction or the policy fails, Rapina converts the error into a response and does not execute the
handler.

## Basic Usage

An authorization policy is an async function that returns `Result<()>`:

```rust
use rapina::prelude::*;

struct Permissions;

impl Permissions {
    fn can_view_reports(&self, user_id: &str) -> bool {
        // Load or evaluate the user's permissions
        true
    }
}

async fn require_report_access(
    user: &CurrentUser,
    permissions: &State<Permissions>,
) -> Result<()> {
    if permissions.can_view_reports(&user.id) {
        Ok(())
    } else {
        Err(Error::forbidden("report access required"))
    }
}
```

Attach the policy to a route with `#[authorize]`:

```rust
#[get("/reports")]
#[authorize(require_report_access(CurrentUser, State<Permissions>))]
async fn reports(user: CurrentUser) -> Result<Json<Vec<Report>>> {
    Ok(Json(load_reports_for(&user.id).await?))
}
```

The types listed in `#[authorize]` correspond to the policy's parameters in the same order. Rapina passes each value to
the policy by shared reference. 

For a complete example that authorizes an user after the user was authenticated based on a Json Web Token, see the [`jwt-validation` example](https://github.com/rapina-rs/rapina/tree/main/rapina/examples/jwt-validation) on GitHub.

## Attribute Order

The route macro must appear above `#[authorize]`:

```rust
#[get("/reports")]
#[authorize(require_report_access(CurrentUser, State<Permissions>))]
async fn reports(user: CurrentUser) -> Result<Json<Vec<Report>>> {
    // ...
}
```

This ordering is required because the route macro processes the authorization attribute while expanding the handler.

The following ordering is invalid:

```rust
// Compile error: #[authorize] must be below the route macro.
#[authorize(require_report_access(CurrentUser, State<Permissions>))]
#[get("/reports")]
async fn reports(user: CurrentUser) -> Result<Json<Vec<Report>>> {
    // ...
}
```

Using `#[authorize]` without a Rapina route macro also ends in a compile error.

## Policy Signature

A policy has the following general shape:

```rust
async fn policy(dependency_one: &ExtractorOne, dependency_two: &ExtractorTwo) -> Result<()> {
    // Return Ok(()) to continue.
    // Return an Error to stop the request.
}
```

Declare the policy path and dependency types in `#[authorize]`:

```rust
#[authorize(policy(
    ExtractorOne,
    ExtractorTwo
))]
```

The dependency order must match the policy parameter order.

Rapina passes policy dependencies by shared reference. Policy parameters must therefore accept `&T`, rather than owned
`T`:

```rust
// Correct
async fn policy(user: &CurrentUser) -> Result<()> {
    Ok(())
}

// Incorrect
async fn policy(user: CurrentUser) -> Result<()> {
    Ok(())
}
```

## Policies Without Dependencies

A policy that needs no request or application dependencies omits the inner dependency list:

```rust
async fn require_feature_enabled() -> Result<()> {
    if is_feature_enabled() {
        Ok(())
    } else {
        Err(Error::forbidden("feature is disabled"))
    }
}

#[get("/preview")]
#[authorize(require_feature_enabled)]
async fn preview() -> &'static str {
    "preview"
}
```

A zero-dependency policy does not authenticate the request by itself. If the route must authenticate a caller, enable
Rapina authentication or declare an authentication extractor as a policy dependency, for example the `JsonWebToken`
extractor.

## Reusing Handler Dependencies

When a policy dependency type exactly matches a handler parameter type, Rapina extracts the value once and reuses the
handler binding for authorization:

```rust
async fn can_edit(user: &CurrentUser, permissions: &State<Permissions>) -> Result<()> {
    if permissions.can_edit(&user.id) {
        Ok(())
    } else {
        Err(Error::forbidden("edit permission required"))
    }
}

#[put("/documents/:id")]
#[authorize(can_edit(CurrentUser, State<Permissions>))]
async fn update_document(
    user: CurrentUser,
    permissions: State<Permissions>,
    id: Path<u64>,
    body: Json<UpdateDocument>,
) -> Result<Json<Document>> {
    Ok(Json(
        update_document_for(&user.id, *id, body.into_inner()).await?,
    ))
}
```

The `CurrentUser` and `State<Permissions>` values are available to the policy before the endpoint body runs, then remain
available to the handler. Rapina only extracts the values once and passes the same reference around multiple times, if
necessary, to not have a performance impact of calling the same extractors multiple times for each request.

### Handler parameter patterns

A reused handler parameter must use a simple identifier pattern:

```rust
// Supported
async fn handler(state: State<AppState>) {
    // ...
}
```

Destructured parameters cannot be reused by an authorization policy:

```rust
// Not supported when State<AppState> is also declared in #[authorize].
async fn handler(State(state): State<AppState>) {
    // ...
}
```

If the policy does not reuse that parameter, the handler's normal pattern rules still apply.

## Authorization-Only Dependencies

A policy may require a dependency that the endpoint handler does not otherwise need. Rapina extracts that dependency
without requiring it in the handler signature:

```rust
#[derive(Deserialize)]
struct AccessQuery {
    tenant: String,
}

async fn can_access_tenant(
    user: &CurrentUser,
    query: &Query<AccessQuery>,
    permissions: &State<Permissions>,
) -> Result<()> {
    if permissions.can_access(&user.id, &query.tenant) {
        Ok(())
    } else {
        Err(Error::forbidden("tenant access denied"))
    }
}

#[get("/dashboard")]
#[authorize(can_access_tenant(CurrentUser, Query<AccessQuery>, State<Permissions>))]
async fn dashboard() -> Result<Json<Dashboard>> {
    Ok(Json(load_dashboard().await?))
}
```

Here, none of the policy dependencies must appear in `dashboard`'s parameter list.

Authorization-only dependencies must implement `FromRequestParts`. Common examples include:

- `State<T>`
- `CurrentUser`
- `JsonWebToken<T>`
- `Path<T>`
- `Query<T>`
- `Headers`
- `Cookie<T>`

A custom extractor that implements `FromRequestParts` may also be used.

## Body Extractors

Authorization-only dependencies cannot consume the request body. Types such as these are not valid authorization-only
dependencies:

- `Json<T>`
- `Form<T>`
- `Validated<Json<T>>`
- `Validated<Form<T>>`
- other custom `FromRequest` extractors that consume the body

The body must remain available for the endpoint handler.

If authorization depends on body content, consider one of these alternatives:

1. Move the required authorization input to the path, query string, headers, token claims, or application state.
2. Perform the body-dependent check inside the handler after validation.
3. Introduce a dedicated request-parts extractor for metadata that can be evaluated without consuming the body.

Body extractors may remain normal handler parameters while the policy uses other dependencies:

```rust
#[post("/documents")]
#[authorize(can_edit(
    CurrentUser,
    State<Permissions>,
))]
async fn create_document(
    user: CurrentUser,
    permissions: State<Permissions>,
    body: Json<CreateDocument>,
) -> Result<Json<Document>> {
    Ok(Json(
        create_document_for(&user.id, body.into_inner()).await?,
    ))
}
```

## Error Behavior

Rapina does not force every authorization failure to return the same 403 status. The policy chooses the error, and Rapina
converts it into a response through `IntoResponse`:

```rust
async fn require_active_account(user: &CurrentUser) -> Result<()> {
    if user_is_disabled(&user.id) {
        Err(Error::forbidden("account disabled"))
    } else {
        Ok(())
    }
}
```

Typical outcomes are:

| Failure                                                | Typical status              |
|--------------------------------------------------------|-----------------------------|
| Missing or invalid authentication dependency           | `401 Unauthorized`          |
| Authenticated caller lacks permission                  | `403 Forbidden`             |
| Malformed path, query, cookie, or header dependency    | `400 Bad Request`           |
| Missing application state or another server dependency | `500 Internal Server Error` |

Execution order for all requests:

1. Extract authorization-only dependencies.
2. Extract and bind handler dependencies that the policy reuses.
3. Invoke the authorization policy.
4. Execute the endpoint body only if the policy returns `Ok(())`.

If dependency extraction fails, neither the policy nor the endpoint body runs. If the policy returns an error, the
endpoint body does not run. 

## Authentication and Authorization

`#[authorize]` does not automatically enable authentication.

A policy may authorize requests using any available dependency, including application state, query parameters, headers,
cookies, or token extractors. If the policy must identify a caller, declare an authentication dependency such as
`CurrentUser` or `JsonWebToken<T>`.

For Rapina-issued JWTs:

```rust
async fn require_owner(
    user: &CurrentUser,
    resource: &Path<u64>,
    permissions: &State<Permissions>,
) -> Result<()> {
    if permissions.is_owner(&user.id, **resource) {
        Ok(())
    } else {
        Err(Error::forbidden("resource ownership required"))
    }
}

#[get("/documents/:id")]
#[authorize(require_owner(
    CurrentUser,
    Path<u64>,
    State<Permissions>,
))]
async fn document(
    user: CurrentUser,
    id: Path<u64>,
) -> Result<Json<Document>> {
    Ok(Json(load_document_for(&user.id, *id).await?))
}
```

See [Authentication](/docs/core-concepts/authentication/) for configuring Rapina's protected-by-default authentication.

## JWT and JWKS Authorization

`#[authorize]` also works with externally issued JWTs validated through JWKS:

```rust
use rapina::jwt::JsonWebToken;
use rapina::prelude::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct Claims {
    email: String,
    roles: Vec<String>,
}

async fn require_support_role(
    token: &JsonWebToken<Claims>,
) -> Result<()> {
    if token
        .claims
        .roles
        .iter()
        .any(|role| role == "support")
    {
        Ok(())
    } else {
        Err(Error::forbidden("support role required"))
    }
}

#[get("/support/tickets")]
#[authorize(require_support_role(
    JsonWebToken<Claims>,
))]
async fn tickets(
    token: JsonWebToken<Claims>,
) -> Result<Json<Vec<Ticket>>> {
    Ok(Json(
        load_tickets_for(&token.claims.email).await?,
    ))
}
```

The policy receives a `JsonWebToken<Claims>` only after the extractor has successfully:

1. read the `Authorization` header;
2. selected the matching JWK;
3. verified the token signature;
4. validated the configured claims;
5. deserialized the custom claims.

Configure `JwksClient` and JWT validation as described in [JWKS Authentication](/docs/core-concepts/jwks/).

## Policies in Other Modules

The policy may be defined in another module:

```rust
// src/authz.rs

use rapina::prelude::*;

pub async fn require_admin(
    user: &CurrentUser,
) -> Result<()> {
    if user_is_admin(&user.id) {
        Ok(())
    } else {
        Err(Error::forbidden("admin role required"))
    }
}
```

Reference the module path from the route:

```rust
// src/routes.rs

mod authz;

#[get("/admin")]
#[authorize(authz::require_admin(
    CurrentUser,
))]
async fn admin(user: CurrentUser) -> &'static str {
    "admin"
}
```

The policy must be visible from the module containing the route.

## `#[public]` Is Incompatible

A route cannot be both public and policy-protected. Combining `#[public]` and `#[authorize]` is a compile error:

```rust
// Compile error
#[public]
#[get("/admin")]
#[authorize(require_admin(
    CurrentUser,
))]
async fn admin(user: CurrentUser) -> &'static str {
    "admin"
}
```

Choose one access model for each route:

- `#[public]` for unauthenticated access;
- a normal protected route for authentication without an additional policy;
- `#[authorize(...)]` for a route-level policy.

## Type Matching

Dependency reuse uses normalized Rust syntax. It does not resolve aliases or determine semantic Rust type equality.

Use the same type spelling in the attribute and handler signature:

```rust
// Reused because the spelling matches.
#[get("/example")]
#[authorize(policy(
    State<AppState>,
))]
async fn example(state: State<AppState>) {
    // ...
}
```

The following types may resolve to the same Rust type, but Rapina treats them as different for dependency reuse:

```rust
#[get("/example")]
#[authorize(policy(
    rapina::extract::State<AppState>,
))]
async fn example(state: State<AppState>) {
    // ...
}
```

When the spelling does not match, Rapina attempts to extract another authorization-only dependency through
`FromRequestParts`. This may duplicate work and may fail to compile for extractors that require route-specific
generation.

Use consistent imports and type spelling between `#[authorize]` and the handler signature.

## Typed Headers

A policy can reuse typed headers declared by the endpoint handler:

```rust
async fn check_request(
    request_id: &Header<String>,
    retry_count: &Header<u32>,
) -> Result<()> {
    if request_id.is_empty() || **retry_count > 3 {
        Err(Error::forbidden("request is not allowed"))
    } else {
        Ok(())
    }
}

#[get("/operation")]
#[authorize(check_request(
    Header<String>,
    Header<u32>,
))]
async fn operation(
    #[header("x-request-id")]
    request_id: Header<String>,
    #[header("x-retry-count")]
    retry_count: Header<u32>,
) -> String {
    format!(
        "{}:{}",
        request_id.into_inner(),
        retry_count.into_inner(),
    )
}
```

`Header<T>` cannot be extracted independently by authorization because its name comes from the handler parameter.
Declare it on the handler and use exactly the same type spelling in `#[authorize]`.

Dependency reuse is type-based. If a handler contains multiple parameters with the same `Header<T>` type, the policy
declaration cannot distinguish them by name. Prefer distinct parsed types or a custom `FromRequestParts` extractor when
a policy needs multiple logically different headers with the same value type.