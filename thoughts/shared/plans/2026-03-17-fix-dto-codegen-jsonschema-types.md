# Fix DTO Codegen JsonSchema Types

## Overview

`rapina import database` (and `rapina add resource`) generates `CreateX`/`UpdateX` DTOs that derive `JsonSchema` but import `Uuid`, `Decimal`, etc. via `use rapina::sea_orm::prelude::*`. The sea_orm re-exports don't carry `JsonSchema` impls, so compilation fails. Same root cause as #256, already fixed in the proc-macro layer but not in the CLI codegen path.

## Current State Analysis

The `generate_dto()` function in `rapina-cli/src/commands/codegen.rs:338-370` detects whether any DTO field uses a non-primitive type and conditionally emits a single glob import:

```rust
let needs_sea_orm_import = fields.iter().any(|f| {
    matches!(f.rust_type.as_str(), "Uuid" | "DateTimeUtc" | "Date" | "Decimal" | "Json")
});

let extra_import = if needs_sea_orm_import {
    "use rapina::sea_orm::prelude::*;\n"
} else {
    ""
};
```

This brings all five types from `sea_orm::prelude`, but `Uuid` and `Decimal` from that path lack `JsonSchema` impls.

### Key Discoveries:
- Proc-macro fix already applied at `rapina-macros/src/schema/types.rs:54,58` — uses `rapina::uuid::Uuid` and `rapina::rust_decimal::Decimal`
- `rapina` re-exports the original crates at `rapina/src/lib.rs:165,167`: `pub use uuid;` and `pub use rust_decimal;`
- `schemars` is configured with features `["chrono04", "uuid1", "rust_decimal1"]` at `rapina/Cargo.toml:52`
- `DateTimeUtc`, `Date`, and `Json` are type aliases in sea_orm (not newtypes), so `JsonSchema` propagates correctly through them
- The `FieldInfo.rust_type` field carries bare string names like `"Uuid"`, `"Decimal"` — set in both `import.rs:153,157` and `add.rs` parse_field()
- Existing tests at `add.rs:290-343` cover basic DTO generation but don't test non-primitive types

## Desired End State

Generated `dto.rs` files use `rapina::uuid::Uuid` and `rapina::rust_decimal::Decimal` instead of the sea_orm re-exports, while keeping `DateTimeUtc`, `Date`, and `Json` from `sea_orm::prelude`. All generated DTOs compile with `#[derive(JsonSchema)]`.

### Verification:
- New unit tests confirm correct imports are generated for each type combination
- `cargo test -p rapina-cli` passes
- `cargo clippy -p rapina-cli` passes
- `cargo fmt --check -p rapina-cli` passes

## What We're NOT Doing

- Changing `FieldInfo.rust_type` values upstream (that would affect entity/migration codegen too)
- Changing the proc-macro layer (already fixed)
- Changing the OpenAPI import path (`import_openapi.rs` — uses its own type system with `chrono::DateTime<chrono::Utc>` directly)
- Adding `pub use chrono` to rapina's lib.rs
- Modifying how entity models are generated

## Implementation Approach

TDD: write failing tests first that assert the correct imports, then fix the code to make them pass.

## Phase 1: Add Failing Tests

### Overview
Add test cases to `rapina-cli/src/commands/add.rs` that cover DTO generation with Uuid, Decimal, DateTimeUtc, Date, and Json fields. These tests will fail because the current code emits `use rapina::sea_orm::prelude::*` instead of type-specific imports.

### Changes Required:

#### 1. New test: Uuid and Decimal use correct import paths
**File**: `rapina-cli/src/commands/add.rs`
**Changes**: Add test after `test_generate_dto_nullable_fields`

```rust
#[test]
fn test_generate_dto_uuid_decimal_imports() {
    let fields = vec![
        FieldInfo {
            name: "id".to_string(),
            rust_type: "Uuid".to_string(),
            schema_type: "Uuid".to_string(),
            column_method: String::new(),
            nullable: false,
        },
        FieldInfo {
            name: "price".to_string(),
            rust_type: "Decimal".to_string(),
            schema_type: "Decimal".to_string(),
            column_method: String::new(),
            nullable: false,
        },
    ];
    let content = codegen::generate_dto("Product", &fields);

    // Must use original crate paths, not sea_orm re-exports
    assert!(content.contains("use rapina::uuid::Uuid;"));
    assert!(content.contains("use rapina::rust_decimal::Decimal;"));
    // Must NOT use the glob import
    assert!(!content.contains("sea_orm::prelude::*"));
}
```

#### 2. New test: DateTimeUtc/Date/Json still import from sea_orm prelude
**File**: `rapina-cli/src/commands/add.rs`
**Changes**: Add test

```rust
#[test]
fn test_generate_dto_sea_orm_types_import() {
    let fields = vec![
        FieldInfo {
            name: "created_at".to_string(),
            rust_type: "DateTimeUtc".to_string(),
            schema_type: "DateTime".to_string(),
            column_method: String::new(),
            nullable: false,
        },
        FieldInfo {
            name: "metadata".to_string(),
            rust_type: "Json".to_string(),
            schema_type: "Json".to_string(),
            column_method: String::new(),
            nullable: true,
        },
    ];
    let content = codegen::generate_dto("Event", &fields);

    assert!(content.contains("use rapina::sea_orm::prelude::{DateTimeUtc, Json};"));
    // No glob
    assert!(!content.contains("sea_orm::prelude::*"));
}
```

#### 3. New test: mixed types generate all required imports
**File**: `rapina-cli/src/commands/add.rs`
**Changes**: Add test

```rust
#[test]
fn test_generate_dto_mixed_types_imports() {
    let fields = vec![
        FieldInfo {
            name: "id".to_string(),
            rust_type: "Uuid".to_string(),
            schema_type: "Uuid".to_string(),
            column_method: String::new(),
            nullable: false,
        },
        FieldInfo {
            name: "amount".to_string(),
            rust_type: "Decimal".to_string(),
            schema_type: "Decimal".to_string(),
            column_method: String::new(),
            nullable: false,
        },
        FieldInfo {
            name: "created_at".to_string(),
            rust_type: "DateTimeUtc".to_string(),
            schema_type: "DateTime".to_string(),
            column_method: String::new(),
            nullable: false,
        },
        FieldInfo {
            name: "name".to_string(),
            rust_type: "String".to_string(),
            schema_type: "String".to_string(),
            column_method: String::new(),
            nullable: false,
        },
    ];
    let content = codegen::generate_dto("Order", &fields);

    assert!(content.contains("use rapina::uuid::Uuid;"));
    assert!(content.contains("use rapina::rust_decimal::Decimal;"));
    assert!(content.contains("use rapina::sea_orm::prelude::{DateTimeUtc};"));
    assert!(!content.contains("sea_orm::prelude::*"));
}
```

#### 4. New test: primitive-only DTOs have no extra imports
**File**: `rapina-cli/src/commands/add.rs`
**Changes**: Add test

```rust
#[test]
fn test_generate_dto_primitives_no_extra_imports() {
    let fields = vec![
        FieldInfo {
            name: "name".to_string(),
            rust_type: "String".to_string(),
            schema_type: "String".to_string(),
            column_method: String::new(),
            nullable: false,
        },
    ];
    let content = codegen::generate_dto("Simple", &fields);

    assert!(!content.contains("sea_orm"));
    assert!(!content.contains("uuid"));
    assert!(!content.contains("rust_decimal"));
}
```

### Success Criteria:

#### Automated Verification:
- [x] Tests compile: `cargo test -p rapina-cli --no-run`
- [x] Tests FAIL (red phase): `cargo test -p rapina-cli -- test_generate_dto_uuid test_generate_dto_sea_orm test_generate_dto_mixed test_generate_dto_primitives`

---

## Phase 2: Fix `generate_dto()` to Emit Correct Imports

### Overview
Replace the glob `use rapina::sea_orm::prelude::*` with type-specific imports, routing `Uuid` and `Decimal` through their original crates.

### Changes Required:

#### 1. Rewrite import generation in `generate_dto()`
**File**: `rapina-cli/src/commands/codegen.rs`
**Changes**: Replace lines 338-350 with type-specific import logic

```rust
// Build type-specific imports instead of sea_orm glob
let needs_uuid = fields.iter().any(|f| f.rust_type == "Uuid");
let needs_decimal = fields.iter().any(|f| f.rust_type == "Decimal");

let sea_orm_types: Vec<&str> = fields
    .iter()
    .filter_map(|f| match f.rust_type.as_str() {
        "DateTimeUtc" | "Date" | "Json" => Some(f.rust_type.as_str()),
        _ => None,
    })
    .collect::<std::collections::BTreeSet<_>>()
    .into_iter()
    .collect();

let mut extra_imports = Vec::new();
if needs_uuid {
    extra_imports.push("use rapina::uuid::Uuid;".to_string());
}
if needs_decimal {
    extra_imports.push("use rapina::rust_decimal::Decimal;".to_string());
}
if !sea_orm_types.is_empty() {
    extra_imports.push(format!(
        "use rapina::sea_orm::prelude::{{{}}};",
        sea_orm_types.join(", ")
    ));
}

let extra_import = if extra_imports.is_empty() {
    String::new()
} else {
    format!("{}\n", extra_imports.join("\n"))
};
```

The format string remains the same — `{extra_import}` is already interpolated in the right place.

### Success Criteria:

#### Automated Verification:
- [x] All new tests pass (green phase): `cargo test -p rapina-cli -- test_generate_dto`
- [x] All existing tests still pass: `cargo test -p rapina-cli` (86 passed)
- [x] `cargo fmt --check -p rapina-cli` passes
- [x] `cargo clippy -p rapina-cli` passes (only pre-existing warnings)
