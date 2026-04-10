+++
title = "JWKS Authentication"
description = "JWT validation using Json Web Key Sets (JWKS) and OIDC discovery endpoints"
weight = 4
date = 2026-03-28
+++

Rapina supports validating JWTs issued by **external identity providers** (Google, Auth0, Keycloak, Azure AD, Okta, etc.) using their public **JSON Web Key Sets (JWKS)**. Instead of sharing a symmetric secret, your application fetches the provider's public keys and uses them to cryptographically verify incoming tokens.

This is provided as an optional feature (`jwks`) and works independently from the [`AuthConfig`/`JWT_SECRET` mechanism ](authentication.md).

This feature is generally preferred over the [`AuthConfig`/`JWT_SECRET` mechanism ](authentication.md) for production environments in case you are using an external identity provider.

---

## Background: How JWKS Validation Works

A **JSON Web Token (JWT)** consists of three Base64-encoded parts separated by dots:

```
header.payload.signature
```

The **header** contains metadata about the token, including:
- `alg` — the signing algorithm (e.g. `RS256`)
- `kid` — the **key ID**, a reference to the specific key the issuer used to sign this token

The **payload** contains claims: `sub` (subject/user ID), `iss` (issuer), `aud` (audience), `exp` (expiration), etc.

The **signature** is created by the issuer using their private key. To verify it, you need the corresponding public key.

A **JWKS (JSON Web Key Set)** is a JSON document published by the identity provider that lists their current public keys, each identified by a `kid`. Rapina fetches this document, caches it in memory, and uses the matching key to verify incoming tokens.

### OIDC Discovery

Many providers support **OpenID Connect Discovery**: a well-known URL (typically `/.well-known/openid-configuration`) returns a JSON document that, among other things, contains a `jwks_uri` field pointing to the actual JWKS endpoint. Rapina's `JwksClient::oidc(...)` handles this two-step fetch automatically.

### Comparison with `AuthConfig`

| | `AuthConfig` (built-in) | JWKS                                               |
|---|---|----------------------------------------------------|
| Key type | Symmetric (`JWT_SECRET`) | Asymmetric (RSA/EC public key)                     |
| Token issuer | Your application | External Identity Provider                         |
| Key distribution | Environment variable | Fetched from URL at runtime                        |
| Use case | Your app issues and verifies its own tokens | Third-party tokens (Google, Auth0, Keycloak, etc.) |

---

## Setup

Add the `jwks` feature to your `Cargo.toml`:

```toml
[dependencies]
rapina = { version = "0.11", features = ["jwks"] }
```

This pulls in Rapina's `cron-scheduler` for automatic periodic cache refresh and `hyper-rustls` for HTTPS fetching of the JWKS endpoint using your system's native root CA certificates.

---

## JWKS Client

Rapina's `JwksClient` is responsible for fetching and caching the JSON Web Key Set from the identity provider. It is registered as application state and used automatically by the `JsonWebToken` extractor on every request.

### Caching and Automatic Refresh

The JWKS content is **cached in memory** so that each incoming request does not trigger a network call to the identity provider. The cache is refreshed automatically based on a **cron schedule** you provide when creating the client.

When the application starts:
1. Rapina **warms up the cache** by immediately fetching the JWKS from the configured endpoint.
2. Rapina **schedules a background cronjob** that periodically refreshes the cache according to the cron schedule.

If the cache warmup fails on startup (e.g. the identity provider is temporarily unavailable), the `JsonWebToken` extractor will **fall back to a live fetch** on the first request. If that also fails, it returns a `500 Internal Server Error`.

There are two variants of the JWKS client:

### Direct JWKS URL

Use this when you know the exact URL of the JWKS endpoint:

```rust
use rapina::jwt::JwksClient;

let jwks_client = JwksClient::direct(
    "https://www.googleapis.com/oauth2/v3/certs".to_string(),
    "0 */5 * * * *".to_string(), // Refresh every 5 minutes
);
```

> ⚠️ **The JWKS endpoint url must contain the HTTPS scheme, i.e. start with `https://`**. The lack of transport-layer security can have a severe impact on the security of the Rapina backend and its protected resources. Rapina will reject urls with plain HTTP scheme during startup.

### OIDC Discovery

Use this when the provider publishes an OpenID Connect discovery document. Rapina will first fetch the discovery document, extract the `jwks_uri` field, and then fetch the actual JWKS:

```rust
use rapina::jwt::JwksClient;

let jwks_client = JwksClient::oidc(
    "https://accounts.google.com/.well-known/openid-configuration".to_string(),
    "0 */5 * * * *".to_string(), // Refresh every 5 minutes
);
```

This is the **recommended approach** for standard OIDC providers, as it is more robust: if the provider rotates their JWKS URL, the discovery document is updated automatically and your application continues to work.

> ⚠️ **The OIDC discovery url must contain the HTTPS scheme, i.e. start with `https://`**. The lack of transport-layer security can have a severe impact on the security of the Rapina backend and its protected resources. Rapina will reject urls with plain HTTP scheme during startup.

**OIDC discovery flow:**
1. Fetch `discovery_url` → parse `jwks_uri`
2. Fetch `jwks_uri` → get the `JwkSet`
3. Cache the `JwkSet` in memory
4. Look up the JWK matching the token's `kid`
5. Verify the token signature

### Cron Schedule Format

The `refresh_schedule` parameter accepts a **6-field cron expression** (seconds granularity).
Rapina's [Cron Scheduler](cron-scheduler.md#cron-expression-syntax) docs outline more information and common examples.

---

## `JsonWebToken<T>` Extractor

`JsonWebToken<T>` is a request extractor that:

1. Reads the `Authorization` header (strips an optional `Bearer ` prefix)
2. Parses the JWT header to get the `kid` and `alg`
3. Reads the JWKS from the in-memory cache (falls back to a live fetch if the cache is empty)
4. Finds the matching JWK by `kid`
5. Validates the token using the configured `Validation` settings
6. Returns the decoded claims as `JsonWebToken<T>`

### Standard Claims

`JsonWebToken<T>` always exposes these standard JWT claims as struct fields:

| Field | Type | Description |
|---|---|---|
| `sub` | `String` | Subject (the user/entity the token was issued for) |
| `iss` | `Option<String>` | Issuer (who issued the token) |
| `aud` | `Option<String>` | Audience (who the token is intended for) |
| `exp` | `usize` | Expiration time (Unix timestamp) |
| `iat` | `Option<usize>` | Issued at (Unix timestamp) |
| `nbf` | `Option<usize>` | Not before (Unix timestamp) |

### Custom Claims

The generic parameter `T` allows you to extract provider-specific claims alongside the standard ones. Define a struct and derive `Deserialize`:

```rust
use rapina::prelude::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct MyClaims {
    pub email: String,
    pub name: String,
}

#[get("/profile")]
async fn profile(token: JsonWebToken<MyClaims>) -> Json<String> {
    // Standard claims:
    println!("Subject: {}", token.sub);
    // Custom claims:
    Json(token.claims.email.clone())
}
```

If you do not need any custom claims, use `JsonWebToken`. This defaults to `JsonWebToken<DefaultClaims>`. `DefaultClaims` is an empty struct:

```rust
#[get("/ping")]
async fn ping(token: JsonWebToken) -> StatusCode {
    println!("Authenticated as: {}", token.sub);
    StatusCode::Ok
}
```

---

## Token Validation

Rapina provides a `default_validation()` function that returns a sensible baseline `Validation` configuration:

```rust
use rapina::jwt;

let validation = jwt::default_validation();
```

The defaults are:

| Setting | Value | Meaning |
|---|---|---|
| `leeway` | 10 seconds | Tolerated clock skew between issuer and your server |
| `validate_aud` | `true` | The `aud` claim **must** be present and match |
| `validate_exp` | `true` | The token must not be expired |
| `validate_nbf` | `true` | The token must not be used before its `nbf` time |

The algorithm (`alg`) is always taken from the JWT header itself — you do not need to set it manually.

### Audience Validation

> ⚠️ **Always configure audience validation in production.** Without it, a valid token issued for a different application (same Identity Provider, different `aud`) would be accepted by your server.

```rust
let mut validation = jwt::default_validation();
validation.set_audience(&["https://api.yourapp.com"]);
```

You can configure multiple audiences too.
The audience value is provider-specific. For Google, it is typically your OAuth 2.0 client ID. For Auth0, it is the API identifier you configured in the dashboard.

If you are prototyping and want to disable audience validation temporarily:

```rust
// ⚠️ Development only — do not use in production!
validation.validate_aud = false;
```

### Issuer Validation

> ⚠️ **Always configure issuer validation in production.** Validating it ensures tokens come from the exact identity provider you trust.

By default, the jwt::default_validation() does not validate the iss claim. In production, you should always restrict which issuer(s) you accept:
```rust
let mut validation = jwt::default_validation();
// Only accept tokens issued by your specific IdP tenant
validation.set_issuer(&["https://accounts.google.com"]);

// For Auth0 / Okta:
// validation.set_issuer(&["https://your-tenant.eu.auth0.com/"]);
// validation.set_issuer(&["https://your-org.okta.com/"]);
```
The issuer URL must match exactly, including any trailing slash. Double check this before-hand as identity providers are inconsistent about this.

---

## Registering with the Application

Both `JwksClient` and `Validation` are registered as application state using `.state()`. The `JsonWebToken` extractor retrieves them automatically from state on every request.

```rust
use rapina::prelude::*;
use rapina::jwt::{self, JwksClient};

#[derive(Deserialize)]
struct MyClaims {
    pub email: String,
}

#[get("/email")]
async fn get_email(token: JsonWebToken<MyClaims>) -> Json<String> {
    Json(token.claims.email.clone())
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let router = Router::new().get("/email", get_email);

    // Configure the JWKS source (with a 5-minute refresh schedule)
    let jwks_client = JwksClient::oidc(
        "https://accounts.google.com/.well-known/openid-configuration".to_string(),
        "0 */5 * * * *".to_string(),
    );

    // Configure validation
    let mut validation = jwt::default_validation();
    validation.set_audience(&["your-google-client-id.apps.googleusercontent.com"]);
    validation.set_issuer(&["https://accounts.google.com"]);

    Rapina::new()
        .state(jwks_client)    // makes JwksClient available to the extractor
        .state(validation)     // makes Validation available to the extractor
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
```

Both `.state()` calls are required. If `JwksClient` is not registered, the extractor returns `500 Internal Server Error`.

On startup, Rapina will:
1. **Warm up the JWKS cache** by fetching the key set immediately
2. **Schedule a background cronjob** to refresh the cache, based on the configured schedule

This ensures the JWKS keys are available from the very first request without any cold-start latency.

---

## Error Responses

The `JsonWebToken` extractor produces the following errors:

| Condition                                                              | HTTP Status Code | Message                                              |
|------------------------------------------------------------------------|------------------|------------------------------------------------------|
| `Authorization` header missing                                         | 401              | `missing authorization header`                       |
| Header value is not valid UTF-8                                        | 401              | `authorization header could not be parsed as String` |
| JWT structure is invalid / not parseable                               | 401              | `invalid token`                                      |
| JWT is expired (header parse stage)                                    | 401              | `token expired`                                      |
| JWT header parse failed for another reason                             | 401              | `token header validation failed: <detail>`           |
| Token's `kid` is not present in the JWKS                               | 401              | `no matching JWK found for the given 'kid'`          |
| Token signature / claims validation failed                             | 401              | `failed to decode token: <detail>`                   |
| `JwksClient` not registered in state                                   | 500              | `internal authentication error`                      |
| JWKS server is unhealthy/unreachable (cache empty + live fetch failed) | 500              | `internal authentication error`                      |

All errors follow the standard Rapina error envelope:

```json
{
  "error": {
    "code": "UNAUTHORIZED",
    "message": "no matching JWK found for the given `kid`"
  },
  "trace_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

---

## Full Example: Google OAuth

For a complete example, please see [folder `jwt-validation` in the Rapina examples](https://github.com/rapina-rs/rapina/tree/main/rapina/examples/jwt-validation).

**To test this with a real Google token:**

1. Navigate to [Google OAuth Playground](https://developers.google.com/oauthplayground)
2. In "Step 1", enter scopes: `https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile`. This will authorize the Google OAuth Playground to access your email and profile information for the account you sign in with (see next step)
3. Press **Authorize APIs** and sign in with your Google account
4. Press **Exchange authorization code for tokens**
5. Copy the `id_token` value from the response
6. Make a request to your running server:
   ```bash
   curl http://localhost:3000/email \
     -H "Authorization: Bearer <your-id-token-here>"
   ```

---

## Provider Quick Reference

| Provider | OIDC Discovery URL |
|---|---|
| Google | `https://accounts.google.com/.well-known/openid-configuration` |
| Auth0 | `https://<your-domain>.auth0.com/.well-known/openid-configuration` |
| Keycloak | `https://<host>/realms/<realm>/.well-known/openid-configuration` |
| Azure AD | `https://login.microsoftonline.com/<tenant>/v2.0/.well-known/openid-configuration` |
| Okta | `https://<your-domain>.okta.com/oauth2/default/.well-known/openid-configuration` |

For providers that do not support OIDC discovery, use `JwksClient::direct(jwks_url, refresh_schedule)` with the direct JWKS URL from their documentation.