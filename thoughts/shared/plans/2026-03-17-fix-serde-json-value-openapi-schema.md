# Fix `Option<serde_json::Value>` OpenAPI Schema Generation

## Overview

When a DTO contains a field of type `Option<serde_json::Value>`, the OpenAPI export produces `"opts": true` instead of a valid schema. This is because `schemars` 1.x with default settings generates JSON Schema 2020-12 boolean schemas (`true` = "any value"), which are **invalid in OpenAPI 3.0.x**. OpenAPI 3.0 requires schema objects, not bare booleans.

## Current State Analysis

- `schema_for!` is used in exactly one place: the generated code in route macros (`rapina-macros/src/lib.rs:206`)
- It uses **default** `SchemaSettings` which target JSON Schema 2020-12 (allows boolean schemas)
- The generated schema flows through `Route` → `RouteInfo` → `build_openapi_spec()` with **no post-processing**
- `schemars` 1.x provides `SchemaSettings::openapi3()` which applies `ReplaceBoolSchemas` transform (`true` → `{}`, `false` → `{"not": {}}`)

### Key Discoveries:
- Only `response_schema()` uses `schema_for!` — there's no request body schema generation yet (`rapina-macros/src/lib.rs:206`)
- No existing tests verify actual schema content for complex types (`rapina/src/openapi/spec.rs:328-404` tests all pass `None` for `response_schema`)
- The `Schema::Inline(serde_json::Value)` enum variant serializes the value as-is with no transformation (`rapina/src/openapi/spec.rs:143`)

## Desired End State

A DTO like:
```rust
#[derive(Deserialize, JsonSchema)]
struct MyDto {
    opts: Option<serde_json::Value>,
}
```

Produces valid OpenAPI 3.0 schema:
```json
{
    "properties": {
        "opts": {}
    }
}
```

Instead of the current invalid output:
```json
{
    "properties": {
        "opts": true
    }
}
```

### Verification:
- A new unit test confirms `serde_json::Value` fields produce `{}` not `true`
- Existing tests continue to pass
- `cargo test` passes across all crates

## What We're NOT Doing

- Adding request body schema generation (separate feature)
- Hoisting response schemas into `components/schemas` with `$ref` (separate concern)
- Migrating to OpenAPI 3.1 (which does support boolean schemas natively)

## Implementation Approach

Add a helper function in `rapina` that generates schemas using OpenAPI 3.0-compatible settings, then call it from the macro instead of `schema_for!`. This keeps the macro simple and makes the logic testable.

## Phase 1: Add OpenAPI-compatible schema helper

### Changes Required:

#### 1. Add helper function
**File**: `rapina/src/openapi/spec.rs`

Add a public helper that generates OpenAPI 3.0-compatible schemas:

```rust
/// Generate a JSON Schema for type `T` using OpenAPI 3.0-compatible settings.
///
/// This uses `SchemaSettings::openapi3()` which replaces boolean schemas
/// (`true`/`false`) with object equivalents (`{}`/`{"not": {}}`) that are
/// valid in OpenAPI 3.0.x.
pub fn openapi_schema_for<T: schemars::JsonSchema>() -> serde_json::Value {
    let schema = schemars::generate::SchemaSettings::openapi3()
        .into_generator()
        .into_root_schema_for::<T>();
    serde_json::to_value(schema).unwrap()
}
```

#### 2. Re-export the helper for macro use
**File**: `rapina/src/lib.rs`

Add a `#[doc(hidden)]` re-export so the macro can call it:

```rust
#[doc(hidden)]
pub use openapi::spec::openapi_schema_for;
```

#### 3. Update the route macro to use the helper
**File**: `rapina-macros/src/lib.rs` (line 206)

Change from:
```rust
Some(serde_json::to_value(rapina::schemars::schema_for!(#inner_type)).unwrap())
```

To:
```rust
Some(rapina::openapi_schema_for::<#inner_type>())
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo test -p rapina` — all existing tests pass
- [x] `cargo test -p rapina-macros` — all existing tests pass
- [x] `cargo test -p rapina-cli` — all existing tests pass
- [x] New unit test for `openapi_schema_for` with `serde_json::Value` passes

## Phase 2: Add test coverage

### Changes Required:

#### 1. Unit test for the helper function
**File**: `rapina/src/openapi/spec.rs` (in the existing `#[cfg(test)]` module)

```rust
#[test]
fn test_openapi_schema_for_serde_json_value() {
    // serde_json::Value should produce {} (any value), not boolean true
    let schema = openapi_schema_for::<serde_json::Value>();
    assert_eq!(schema, serde_json::json!({}));
}

#[test]
fn test_openapi_schema_for_option_serde_json_value() {
    // Option<serde_json::Value> should also produce a valid object schema, not true
    let schema = openapi_schema_for::<Option<serde_json::Value>>();
    // Should be an object schema, not a boolean
    assert!(schema.is_object());
}

#[test]
fn test_openapi_schema_for_struct_with_value_field() {
    #[derive(schemars::JsonSchema)]
    struct TestDto {
        opts: Option<serde_json::Value>,
    }
    let schema = openapi_schema_for::<TestDto>();
    let properties = schema.get("properties").unwrap();
    let opts = properties.get("opts").unwrap();
    // Must be an object (e.g., {}), not boolean true
    assert!(opts.is_object(), "opts field schema should be an object, got: {opts}");
}
```

#### 2. Integration test with build_openapi_spec
**File**: `rapina/src/openapi/spec.rs` (in the existing `#[cfg(test)]` module)

```rust
#[test]
fn test_build_openapi_spec_with_value_response_schema() {
    #[derive(schemars::JsonSchema)]
    struct DtoWithValue {
        data: String,
        opts: Option<serde_json::Value>,
    }
    let schema = openapi_schema_for::<DtoWithValue>();
    let routes = vec![RouteInfo::new("POST", "/items", "create_item", Some(schema), Vec::new())];
    let spec = build_openapi_spec("Test API", "1.0.0", &routes);

    let json = serde_json::to_value(&spec).unwrap();
    let opts_schema = &json["paths"]["/items"]["post"]["responses"]["200"]
        ["content"]["application/json"]["schema"]["properties"]["opts"];

    // Must not be boolean true — must be a valid schema object
    assert!(!opts_schema.is_boolean(), "opts should not be a boolean schema, got: {opts_schema}");
    assert!(opts_schema.is_object(), "opts should be an object schema, got: {opts_schema}");
}
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo test -p rapina` — all tests pass including new ones
- [x] `cargo test -p rapina-macros` — macro test that checks for `schema_for` in output is updated or still passes
- [x] `cargo clippy --all` — no warnings

#### Manual Verification:
- [ ] Create a test project with a DTO containing `Option<serde_json::Value>`, run `rapina openapi export -o openapi.json`, and verify the output contains `{}` instead of `true`

## Testing Strategy

### Unit Tests:
- `openapi_schema_for::<serde_json::Value>()` produces `{}`
- `openapi_schema_for::<Option<serde_json::Value>>()` produces a valid object schema
- Struct with `Option<serde_json::Value>` field produces object schema for that field
- End-to-end through `build_openapi_spec` with a Value-containing response schema

### Edge Cases:
- Nested `Option<Option<serde_json::Value>>` (unlikely but should still work)
- `Vec<serde_json::Value>` — items schema should be `{}` not `true`

## References

- schemars `SchemaSettings::openapi3()`: applies `ReplaceBoolSchemas` transform
- schemars `ReplaceBoolSchemas`: converts `true` → `{}`, `false` → `{"not": {}}`
- OpenAPI 3.0.3 spec: Schema Objects must be JSON objects, not bare booleans
- JSON Schema 2020-12: `true`/`false` are valid schemas (but not in OAS 3.0)
