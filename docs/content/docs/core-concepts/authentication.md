+++
title = "Authentication"
description = "JWT authentication with protected-by-default routes"
weight = 3
date = 2025-02-13
+++

Rapina provides JWT authentication with a "protected by default" approach. All routes require authentication unless explicitly marked as public.

## Setup

Set the `JWT_SECRET` environment variable:

```bash
JWT_SECRET=your-secret-key-here
JWT_EXPIRATION=3600  # Optional, defaults to 3600 seconds
```

Enable authentication in your application:

```rust
use rapina::prelude::*;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    load_dotenv();

    let auth_config = AuthConfig::from_env()
        .expect("JWT_SECRET is required");

    Rapina::new()
        .with_auth(auth_config.clone())
        .state(auth_config)
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
```

## Public Routes

Mark routes that don't require authentication with `#[public]`:

```rust
#[public]
#[get("/health")]
async fn health() -> &'static str {
    "ok"
}

#[public]
#[post("/login")]
async fn login(body: Json<LoginRequest>, auth: State<AuthConfig>) -> Result<Json<TokenResponse>> {
    // Authenticate and return token
}
```

You can also register public routes programmatically:

```rust
Rapina::new()
    .with_auth(auth_config)
    .public_route("GET", "/health")
    .public_route("POST", "/login")
    // ...
```

## Protected Routes

All routes without `#[public]` require a valid JWT token:

```rust
#[get("/me")]
async fn me(user: CurrentUser) -> Json<UserResponse> {
    Json(UserResponse {
        id: user.id,
        // ...
    })
}
```

The `CurrentUser` extractor provides:
- `user.id` - The user ID from the JWT `sub` claim
- `user.claims` - The full JWT claims

## Creating Tokens

Use `AuthConfig` to create tokens:

```rust
#[public]
#[post("/login")]
async fn login(body: Json<LoginRequest>, auth: State<AuthConfig>) -> Result<Json<TokenResponse>> {
    let req = body.into_inner();
    let auth_config = auth.into_inner();

    // Validate credentials (example)
    if req.username == "admin" && req.password == "secret" {
        let token = auth_config.create_token(&req.username)?;
        Ok(Json(TokenResponse::new(token, auth_config.expiration())))
    } else {
        Err(Error::unauthorized("invalid credentials"))
    }
}
```

`TokenResponse` is provided by Rapina - no need to define it yourself.

## Making Authenticated Requests

Include the JWT in the `Authorization` header:

```bash
curl http://localhost:3000/me \
  -H "Authorization: Bearer eyJhbGciOiJIUzI1NiIs..."
```

## Error Responses

| Scenario | Status | Code |
|----------|--------|------|
| Missing token | 401 | `UNAUTHORIZED` |
| Invalid token | 401 | `UNAUTHORIZED` |
| Expired token | 401 | `UNAUTHORIZED` |

All errors include a `trace_id` for debugging:

```json
{
  "error": {
    "code": "UNAUTHORIZED",
    "message": "token expired"
  },
  "trace_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

## JWT Claims

The default claims structure:

```rust
pub struct Claims {
    pub sub: String,  // Subject (user ID)
    pub exp: u64,     // Expiration timestamp
    pub iat: u64,     // Issued at timestamp
}
```

Access claims in handlers:

```rust
#[get("/token/info")]
async fn token_info(user: CurrentUser) -> Json<Claims> {
    Json(user.claims)
}
```
