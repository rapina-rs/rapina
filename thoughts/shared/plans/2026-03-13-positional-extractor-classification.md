# Replace String-Based Extractor Classification with Positional Convention

## Overview

Replace the `is_parts_only_extractor` function in `rapina-macros` that classifies handler arguments by string-matching type names (e.g., checking if the name contains "Path", "Query", "State") with the axum-style positional convention: all arguments except the last use `FromRequestParts::from_request_parts`, the last argument uses `FromRequest::from_request`.

This eliminates false positives (user type `UserPathInfo` misclassified as parts-only) and false negatives (custom parts-only extractor with an unrecognized name treated as body-consuming). The blanket `impl<T: FromRequestParts> FromRequest for T` at `rapina/src/extract.rs:641` ensures parts-only extractors work correctly regardless of position.

## Current State Analysis

### The Problem

**File**: `rapina-macros/src/lib.rs:347-357`

```rust
fn is_parts_only_extractor(type_str: &str) -> bool {
    type_str.contains("Path")
        || type_str.contains("Query")
        || type_str.contains("Headers")
        || type_str.contains("State")
        || type_str.contains("Context")
        || type_str.contains("CurrentUser")
        || type_str.contains("Db")
        || type_str.contains("Cookie")
        || type_str.contains("Relay")
}
```

Called at `rapina-macros/src/lib.rs:252` during macro expansion to decide whether each handler argument gets routed through `FromRequestParts::from_request_parts` (parts-only) or `FromRequest::from_request` (body-consuming).

The string matching breaks when:
- A user type named `UserPathInfo` implements `FromRequest` (body-consuming) but gets misclassified as parts-only because the name contains "Path"
- A custom parts-only extractor with an unrecognized name (e.g., `AuthToken`) gets classified as body-consuming

### The Blanket Impl

**File**: `rapina/src/extract.rs:641-650`

```rust
impl<T: FromRequestParts> FromRequest for T {
    async fn from_request(req: Request<Incoming>, params: &PathParams, state: &Arc<AppState>) -> Result<Self, Error> {
        let (parts, _body) = req.into_parts();
        Self::from_request_parts(&parts, params, state).await
    }
}
```

Calling `FromRequest::from_request` on a parts-only extractor works correctly — destructures request, discards body, delegates to `from_request_parts`. This is what makes positional convention work.

### Relay Macro

**File**: `rapina-macros/src/lib.rs:467-548`

Already treats all arguments (after `RelayEvent`) as `FromRequestParts`. Synthesizes fake request parts via `Request::new(()).into_parts()` — no HTTP body. **No changes needed.**

### Key Discoveries:
- Body consumers (`Json<T>`, `Form<T>`, `Validated<Json<T>>`, `Validated<Form<T>>`) implement `FromRequest` directly — they do NOT implement `FromRequestParts`
- If a user puts `Json<T>` anywhere except last, the compiler errors because `Json<T>` doesn't implement `FromRequestParts`
- The `test_multiple_body_extractors_panics` test at line 778 expects a macro-time panic — with positional classification this moves to a compile-time trait error instead

## Desired End State

1. `is_parts_only_extractor` is deleted
2. `route_macro_core` uses position to classify arguments:
   - **0 args**: No extraction code emitted (unchanged)
   - **1 arg**: Emit `FromRequest::from_request` directly on the request (no destructuring)
   - **N args (N > 1)**: Destructure into `(parts, body)`. Args 1..N-1 emit `FromRequestParts::from_request_parts`. Last arg: reassemble via `Request::from_parts`, emit `FromRequest::from_request`
3. `#[relay]` macro unchanged
4. All existing tests pass (except `test_multiple_body_extractors_panics`, which is removed)
5. User types with names containing "Path", "Query", etc. are no longer misclassified

### Verification:
- `cargo test -p rapina-macros` passes
- `cargo test -p rapina` passes
- `cargo check --workspace` succeeds

## What We're NOT Doing

- Not changing `#[relay]` — intentionally parts-only, correct as-is
- Not adding `#[parts_only]` attributes or marker traits
- Not changing `FromRequest` or `FromRequestParts` trait definitions
- Not adding custom compile-time error messages for body consumers in non-last position — the compiler's trait error is clear enough

## Implementation Approach

TDD: write tests first that describe the desired positional behavior, then implement the changes to make them pass.

## Phase 1: Write Tests (RED)

### Overview
Write new tests and update existing test assertions to describe the positional convention. These tests will fail against the current implementation.

### Changes Required:

#### `rapina-macros/src/lib.rs` — Test modifications

**Update** `test_generates_handler_with_extractors` (line 737):

Single extractor is the last (only) arg, so it should now emit `FromRequest`, not `FromRequestParts`:

```rust
#[test]
fn test_generates_handler_with_extractors() {
    let path = quote!("/users/:id");
    let input = quote! {
        async fn get_user(id: rapina::extract::Path<u64>) -> String {
            format!("{}", id.into_inner())
        }
    };

    let output = route_macro_core("GET", path, input);
    let output_str = output.to_string();

    assert!(output_str.contains("struct get_user"));
    // Single arg is last arg — uses FromRequest (blanket impl handles parts-only)
    assert!(output_str.contains("FromRequest"));
    // Single arg should NOT destructure request into parts
    assert!(!output_str.contains("into_parts"));
}
```

**Remove** `test_multiple_body_extractors_panics` (line 777-791):

With positional classification the macro no longer panics at expansion time — the compiler catches misplaced body consumers via trait bounds. Delete the entire test.

**Add** `test_custom_type_name_not_misclassified`:

```rust
#[test]
fn test_custom_type_name_not_misclassified() {
    // UserPathInfo contains "Path" but should NOT be routed to FromRequestParts
    // Positional convention: single (last) arg always uses FromRequest
    let path = quote!("/users");
    let input = quote! {
        async fn handler(info: UserPathInfo) -> String {
            "ok".to_string()
        }
    };

    let output = route_macro_core("POST", path, input);
    let output_str = output.to_string();

    assert!(output_str.contains("FromRequest"));
    assert!(!output_str.contains("FromRequestParts"));
}
```

**Add** `test_multiple_parts_only_extractors_positional`:

```rust
#[test]
fn test_multiple_parts_only_extractors_positional() {
    // All parts-only extractors: first N-1 use FromRequestParts, last uses FromRequest
    let path = quote!("/users/:id");
    let input = quote! {
        async fn handler(
            id: rapina::extract::Path<u64>,
            query: rapina::extract::Query<Params>,
            headers: rapina::extract::Headers,
        ) -> String {
            "ok".to_string()
        }
    };

    let output = route_macro_core("GET", path, input);
    let output_str = output.to_string();

    // First two args use FromRequestParts
    assert!(output_str.contains("FromRequestParts"));
    // Last arg uses FromRequest (via blanket impl at runtime)
    assert!(output_str.contains("FromRequest"));
    // Request is destructured for multi-arg case
    assert!(output_str.contains("into_parts"));
    // Request is reassembled for last arg
    assert!(output_str.contains("from_parts"));
}
```

**Add** `test_two_body_extractors_no_macro_panic`:

```rust
#[test]
fn test_two_body_extractors_no_macro_panic() {
    // With positional convention, the macro does NOT panic for multiple body consumers.
    // Instead, it generates code where the first Json is bounded by FromRequestParts
    // (which it doesn't implement), so the compiler catches it at type-check time.
    let path = quote!("/users");
    let input = quote! {
        async fn handler(
            body1: rapina::extract::Json<String>,
            body2: rapina::extract::Json<String>
        ) -> String {
            "ok".to_string()
        }
    };

    // Should NOT panic — macro expansion succeeds, compiler catches the error later
    let output = route_macro_core("POST", path, input);
    let output_str = output.to_string();

    // First arg gets FromRequestParts (will fail at compile time since Json doesn't impl it)
    assert!(output_str.contains("FromRequestParts"));
    // Last arg gets FromRequest
    assert!(output_str.contains("FromRequest"));
}
```

### Success Criteria:

#### Automated Verification:
- [x] New/updated tests compile: `cargo test -p rapina-macros --no-run`
- [x] New tests FAIL against current implementation (confirming they test new behavior)

---

## Phase 2: Implement Positional Convention (GREEN)

### Overview
Replace `is_parts_only_extractor` with positional classification in `route_macro_core`.

### Changes Required:

#### 1. `rapina-macros/src/lib.rs` — Replace handler body generation

**Replace lines 240-295** (the `else` branch of `if args.is_empty()`) with positional logic:

```rust
    } else {
        let inner_block = &func.block;

        if args.len() == 1 {
            // Single arg: pass request directly to FromRequest
            let arg = &args[0];
            if let FnArg::Typed(pat_type) = arg
                && let Pat::Ident(pat_ident) = &*pat_type.pat
            {
                let arg_name = &pat_ident.ident;
                let arg_type = &pat_type.ty;
                quote! {
                    let #arg_name = match <#arg_type as rapina::extract::FromRequest>::from_request(__rapina_req, &__rapina_params, &__rapina_state).await {
                        Ok(v) => v,
                        Err(e) => return rapina::response::IntoResponse::into_response(e),
                    };
                    let __rapina_result #return_type_annotation = (async #inner_block).await;
                    let __rapina_response = rapina::response::IntoResponse::into_response(__rapina_result);
                    #cache_header_injection
                    __rapina_response
                }
            } else {
                unreachable!("handler argument must be a typed pattern")
            }
        } else {
            // Multiple args: all but last use FromRequestParts, last uses FromRequest
            let mut parts_extractions = Vec::new();

            for arg in &args[..args.len() - 1] {
                if let FnArg::Typed(pat_type) = arg
                    && let Pat::Ident(pat_ident) = &*pat_type.pat
                {
                    let arg_name = &pat_ident.ident;
                    let arg_type = &pat_type.ty;
                    parts_extractions.push(quote! {
                        let #arg_name = match <#arg_type as rapina::extract::FromRequestParts>::from_request_parts(&__rapina_parts, &__rapina_params, &__rapina_state).await {
                            Ok(v) => v,
                            Err(e) => return rapina::response::IntoResponse::into_response(e),
                        };
                    });
                }
            }

            let last_arg = args.last().unwrap();
            let last_extraction = if let FnArg::Typed(pat_type) = last_arg
                && let Pat::Ident(pat_ident) = &*pat_type.pat
            {
                let arg_name = &pat_ident.ident;
                let arg_type = &pat_type.ty;
                quote! {
                    let __rapina_req = rapina::http::Request::from_parts(__rapina_parts, __rapina_body);
                    let #arg_name = match <#arg_type as rapina::extract::FromRequest>::from_request(__rapina_req, &__rapina_params, &__rapina_state).await {
                        Ok(v) => v,
                        Err(e) => return rapina::response::IntoResponse::into_response(e),
                    };
                }
            } else {
                unreachable!("handler argument must be a typed pattern")
            };

            quote! {
                let (__rapina_parts, __rapina_body) = __rapina_req.into_parts();
                #(#parts_extractions)*
                #last_extraction
                let __rapina_result #return_type_annotation = (async #inner_block).await;
                let __rapina_response = rapina::response::IntoResponse::into_response(__rapina_result);
                #cache_header_injection
                __rapina_response
            }
        }
    };
```

#### 2. `rapina-macros/src/lib.rs` — Delete `is_parts_only_extractor`

**Delete lines 347-357** (the entire function).

### Success Criteria:

#### Automated Verification:
- [x] `cargo test -p rapina-macros` — all macro unit tests pass (99 passed)
- [x] `cargo test -p rapina` — all integration tests pass (273+ passed)
- [x] `cargo check --workspace` — full workspace compiles
- [x] `cargo clippy --workspace` — no new warnings
- [x] `grep -r is_parts_only_extractor` returns nothing (only in plan doc and updated comments)

**Implementation Note**: After Phase 2 automated verification passes, pause for manual confirmation before considering the task complete.

#### Manual Verification:
- [ ] Review generated token output for a multi-extractor handler to confirm positional logic
- [ ] Confirm relay tests still pass unchanged

---

## Testing Strategy

### Unit Tests (rapina-macros):
- Single parts-only extractor → emits `FromRequest` (blanket impl handles it)
- Single body extractor → emits `FromRequest`
- Multiple extractors with body last → first N-1 emit `FromRequestParts`, last emits `FromRequest`
- Multiple parts-only extractors → all but last emit `FromRequestParts`, last emits `FromRequest`
- Custom type name containing "Path"/"Query"/etc. → classified by position, not name
- Two body consumers → macro doesn't panic, generates code that fails at compile time
- Relay macro unchanged

### Integration Tests (rapina):
- Existing `tests/extractors.rs` exercises actual HTTP requests with various extractor combinations
- Existing `tests/routing.rs` exercises path parameter extraction
- Existing `tests/relay.rs` and `tests/relay_channels.rs` verify relay handlers still work

### Edge Cases:
- Handler with 0 args — unchanged, already handled
- Handler with 1 parts-only arg — works via blanket impl
- Handler with only parts-only args — all but last use `FromRequestParts`, last uses `FromRequest` via blanket
- Body consumer not in last position — compiler error (correct behavior)

## Performance Considerations

- Single-arg parts-only extractors now go through `FromRequest` → blanket impl → `FromRequestParts`, adding one extra `Request::into_parts()`. Negligible.
- Multi-arg handlers with only parts-only extractors: last one goes through blanket impl's extra `into_parts()` + reassembly. Also negligible.
- No runtime cost difference for the common case (parts-only args first, body consumer last).
