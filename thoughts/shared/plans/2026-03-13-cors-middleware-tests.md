# CORS Middleware Unit Tests — Implementation Plan

## Overview

Add unit tests to `rapina/src/middleware/cors.rs` covering all CORS logic: preflight responses, origin validation, wildcard handling, disallowed origin rejection, and header injection on normal responses. No production code changes needed.

## Current State Analysis

- **CORS middleware** (`rapina/src/middleware/cors.rs:86-190`) has two internal methods that contain all the logic:
  - `preflight_response(&self, origin: &Option<HeaderValue>) -> Response<BoxBody>` — builds a 204 response with all `Access-Control-*` headers
  - `add_cors_headers(&self, response: &mut Response<BoxBody>, origin: &Option<HeaderValue>)` — injects `Access-Control-Allow-Origin` and `Vary` into an existing response
- **`Middleware::handle`** delegates to these methods based on whether the request is `OPTIONS`
- **Rate limit tests** (`rapina/src/middleware/rate_limit.rs:210-347`) demonstrate the pattern: test internal methods directly (e.g., `check_rate_limit`) without needing to construct a full middleware chain
- **`BoxBody`** is `Full<Bytes>` (`rapina/src/response.rs:17`), which implements `Default`

### Key Discoveries:
- `preflight_response` and `add_cors_headers` are private methods on `CorsMiddleware` — tests inside the same module (`#[cfg(test)] mod tests`) can access them
- Both methods accept `&Option<HeaderValue>` for the origin, making them easy to test without constructing full `Request` objects
- The `CorsConfig::with_origins` constructor sets sensible defaults (6 methods, 2 headers) — tests should verify these exact values
- When origin is disallowed (not in the allowed list), the response simply omits `Access-Control-Allow-Origin` — it doesn't return an error

## Desired End State

A `#[cfg(test)] mod tests` block in `cors.rs` with tests covering:

1. Preflight with `AllowedOrigins::Any` → `Access-Control-Allow-Origin: *`
2. Preflight with `AllowedOrigins::Exact` + matching origin → origin echoed back
3. Preflight with `AllowedOrigins::Exact` + non-matching origin → no `Access-Control-Allow-Origin`
4. Preflight with `AllowedOrigins::Exact` + no `Origin` header → no `Access-Control-Allow-Origin`
5. Preflight returns status 204
6. Preflight sets `Access-Control-Allow-Methods` correctly for both `Any` and `List`
7. Preflight sets `Access-Control-Allow-Headers` correctly for both `Any` and `List`
8. Preflight sets `Vary: Origin`
9. `add_cors_headers` on normal response with `Any` → `*`
10. `add_cors_headers` on normal response with allowed origin → origin echoed
11. `add_cors_headers` on normal response with disallowed origin → no header set
12. `CorsConfig::permissive()` produces expected config
13. `CorsConfig::with_origins()` produces expected defaults

### Verification:
```bash
cargo test -p rapina middleware::cors::tests
```
All tests pass with zero production code changes.

## What We're NOT Doing

- Not testing the `Middleware::handle` trait impl end-to-end (would require `Next`, `Router`, `AppState` — overkill for unit tests)
- Not adding integration/e2e tests
- Not modifying any production code

## Implementation Approach

Follow the rate_limit test pattern: test internal methods directly. Both `preflight_response` and `add_cors_headers` are self-contained and don't require async or the middleware chain.

## Phase 1: Write CORS Unit Tests

### Overview
Add a `#[cfg(test)] mod tests` block at the bottom of `cors.rs`.

### Changes Required:

#### 1. `rapina/src/middleware/cors.rs` — append test module

**File**: `rapina/src/middleware/cors.rs`
**Changes**: Add `#[cfg(test)] mod tests { ... }` at the end of the file

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use http::{HeaderValue, Method, StatusCode, header};

    // --- Config constructors ---

    #[test]
    fn test_permissive_config() {
        let config = CorsConfig::permissive();
        assert!(matches!(config.allowed_origins, AllowedOrigins::Any));
        assert!(matches!(config.allowed_methods, AllowedMethods::Any));
        assert!(matches!(config.allowed_headers, AllowedHeaders::Any));
    }

    #[test]
    fn test_with_origins_config() {
        let config = CorsConfig::with_origins(vec!["https://example.com".into()]);
        match &config.allowed_origins {
            AllowedOrigins::Exact(origins) => {
                assert_eq!(origins, &vec!["https://example.com".to_string()]);
            }
            _ => panic!("expected Exact origins"),
        }
        match &config.allowed_methods {
            AllowedMethods::List(methods) => {
                assert_eq!(methods.len(), 6);
                assert!(methods.contains(&Method::GET));
                assert!(methods.contains(&Method::POST));
                assert!(methods.contains(&Method::PUT));
                assert!(methods.contains(&Method::PATCH));
                assert!(methods.contains(&Method::DELETE));
                assert!(methods.contains(&Method::OPTIONS));
            }
            _ => panic!("expected List methods"),
        }
        match &config.allowed_headers {
            AllowedHeaders::List(headers) => {
                assert_eq!(headers.len(), 2);
                assert!(headers.contains(&header::ACCEPT));
                assert!(headers.contains(&header::AUTHORIZATION));
            }
            _ => panic!("expected List headers"),
        }
    }

    // --- Preflight response ---

    #[test]
    fn test_preflight_returns_204() {
        let mw = CorsMiddleware::new(CorsConfig::permissive());
        let resp = mw.preflight_response(&None);
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[test]
    fn test_preflight_wildcard_origin() {
        let mw = CorsMiddleware::new(CorsConfig::permissive());
        let resp = mw.preflight_response(&None);
        assert_eq!(
            resp.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
            "*"
        );
    }

    #[test]
    fn test_preflight_allowed_origin_echoed() {
        let config = CorsConfig::with_origins(vec!["https://example.com".into()]);
        let mw = CorsMiddleware::new(config);
        let origin = Some(HeaderValue::from_static("https://example.com"));
        let resp = mw.preflight_response(&origin);
        assert_eq!(
            resp.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
            "https://example.com"
        );
    }

    #[test]
    fn test_preflight_disallowed_origin() {
        let config = CorsConfig::with_origins(vec!["https://example.com".into()]);
        let mw = CorsMiddleware::new(config);
        let origin = Some(HeaderValue::from_static("https://evil.com"));
        let resp = mw.preflight_response(&origin);
        assert!(resp.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
    }

    #[test]
    fn test_preflight_no_origin_header() {
        let config = CorsConfig::with_origins(vec!["https://example.com".into()]);
        let mw = CorsMiddleware::new(config);
        let resp = mw.preflight_response(&None);
        assert!(resp.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
    }

    #[test]
    fn test_preflight_methods_any() {
        let mw = CorsMiddleware::new(CorsConfig::permissive());
        let resp = mw.preflight_response(&None);
        assert_eq!(
            resp.headers().get(header::ACCESS_CONTROL_ALLOW_METHODS).unwrap(),
            "*"
        );
    }

    #[test]
    fn test_preflight_methods_list() {
        let config = CorsConfig::with_origins(vec!["https://example.com".into()]);
        let mw = CorsMiddleware::new(config);
        let origin = Some(HeaderValue::from_static("https://example.com"));
        let resp = mw.preflight_response(&origin);
        let methods = resp
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_METHODS)
            .unwrap()
            .to_str()
            .unwrap();
        for m in ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
            assert!(methods.contains(m), "missing method: {m}");
        }
    }

    #[test]
    fn test_preflight_headers_any() {
        let mw = CorsMiddleware::new(CorsConfig::permissive());
        let resp = mw.preflight_response(&None);
        assert_eq!(
            resp.headers().get(header::ACCESS_CONTROL_ALLOW_HEADERS).unwrap(),
            "*"
        );
    }

    #[test]
    fn test_preflight_headers_list() {
        let config = CorsConfig::with_origins(vec!["https://x.com".into()]);
        let mw = CorsMiddleware::new(config);
        let origin = Some(HeaderValue::from_static("https://x.com"));
        let resp = mw.preflight_response(&origin);
        let headers_val = resp
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(headers_val.contains("accept"), "missing accept header");
        assert!(headers_val.contains("authorization"), "missing authorization header");
    }

    #[test]
    fn test_preflight_vary_header() {
        let mw = CorsMiddleware::new(CorsConfig::permissive());
        let resp = mw.preflight_response(&None);
        assert_eq!(resp.headers().get(header::VARY).unwrap(), "Origin");
    }

    // --- add_cors_headers on normal responses ---

    fn empty_response() -> Response<BoxBody> {
        Response::builder()
            .status(StatusCode::OK)
            .body(BoxBody::default())
            .unwrap()
    }

    #[test]
    fn test_normal_response_wildcard_origin() {
        let mw = CorsMiddleware::new(CorsConfig::permissive());
        let mut resp = empty_response();
        mw.add_cors_headers(&mut resp, &None);
        assert_eq!(
            resp.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
            "*"
        );
    }

    #[test]
    fn test_normal_response_allowed_origin() {
        let config = CorsConfig::with_origins(vec!["https://example.com".into()]);
        let mw = CorsMiddleware::new(config);
        let mut resp = empty_response();
        let origin = Some(HeaderValue::from_static("https://example.com"));
        mw.add_cors_headers(&mut resp, &origin);
        assert_eq!(
            resp.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
            "https://example.com"
        );
    }

    #[test]
    fn test_normal_response_disallowed_origin() {
        let config = CorsConfig::with_origins(vec!["https://example.com".into()]);
        let mw = CorsMiddleware::new(config);
        let mut resp = empty_response();
        let origin = Some(HeaderValue::from_static("https://evil.com"));
        mw.add_cors_headers(&mut resp, &origin);
        assert!(resp.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
    }

    #[test]
    fn test_normal_response_vary_header() {
        let mw = CorsMiddleware::new(CorsConfig::permissive());
        let mut resp = empty_response();
        mw.add_cors_headers(&mut resp, &None);
        assert_eq!(resp.headers().get(header::VARY).unwrap(), "Origin");
    }
}
```

### Success Criteria:

#### Automated Verification:
- [x] All CORS tests pass: `cargo test -p rapina middleware::cors::tests`
- [x] Full test suite still passes: `cargo test -p rapina`
- [x] No clippy warnings: `cargo clippy -p rapina`

#### Manual Verification:
- [ ] Review that test names clearly describe what they verify
- [ ] Confirm no production code was modified

## Testing Strategy

### Unit Tests:
- Config constructors: verify `permissive()` and `with_origins()` produce correct config
- Preflight: status 204, wildcard origin, exact origin echo, disallowed origin omission, no-origin-header case, methods (any vs list), headers (any vs list), Vary header
- Normal response headers: wildcard, allowed origin, disallowed origin, Vary header

### Edge Cases Covered:
- No `Origin` header in request (both preflight and normal)
- Origin present but not in allowed list
- Multiple origins in config (only matching one echoed)

## References

- CORS middleware: `rapina/src/middleware/cors.rs`
- Rate limit test pattern: `rapina/src/middleware/rate_limit.rs:210-347`
- `BoxBody` type: `rapina/src/response.rs:17` (`Full<Bytes>`)
