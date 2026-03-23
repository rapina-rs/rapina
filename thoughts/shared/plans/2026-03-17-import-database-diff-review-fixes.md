# Fix Review Feedback on `import database --diff`

## Overview

Address review feedback on the `import database --diff` feature (commit 0536789). Four fixes: correct `to_snake_case` for consecutive uppercase, resolve FK column types from referenced entities instead of hardcoding `i32`, extract a shared introspection helper to eliminate duplication, and remove an unnecessary type annotation.

## Current State Analysis

- `to_snake_case` (entity_parser.rs:437) inserts an underscore before every uppercase char, producing `h_t_m_l_parser` for `HTMLParser` instead of `html_parser`. The `schema!` macro uses `heck::ToSnakeCase` internally, so the parser and compiler disagree on table names for entities with consecutive uppercase letters, causing false drift.
- `build_expected_columns` (import.rs:823-829) hardcodes `NormalizedType::I32` for all `belongs_to` FK columns. Entities with UUID primary keys produce a false type mismatch.
- `database_diff()` (import.rs:1103-1157) duplicates the entire runtime creation and URL-scheme dispatch block from `database()` (import.rs:598-649).
- `let tables: Vec<IntrospectedTable>` (import.rs:1106) has an unnecessary type annotation.

### Key Discoveries
- `heck 0.5` is used in `rapina-macros/src/schema/generate.rs` via `use heck::ToSnakeCase` — this is the authoritative snake_case behavior the CLI must match
- `ParsedEntity.primary_key` is `None` for default auto `i32` PK, or `Some(vec![...])` for custom PKs — when `Some`, the PK columns appear as regular fields with their `schema_type` intact
- `schema_type_to_normalized` (import.rs:784) maps field types like `"Uuid"` to `NormalizedType::Uuid`
- For belongs_to fields, `field.schema_type` stores the referenced entity name (e.g., `"User"`)

## Desired End State

1. `to_snake_case("HTMLParser")` returns `"html_parser"`, `to_snake_case("APIToken")` returns `"api_token"` — matching heck behavior
2. FK columns for belongs_to fields use the referenced entity's PK type (e.g., `Uuid` if the target has a UUID PK), falling back to `I32` when the referenced entity isn't found
3. A single `introspect_tables` helper is used by both `database()` and `database_diff()`
4. No unnecessary type annotation on the `tables` binding in `database_diff()`
5. All existing tests pass, new tests cover the fixed behaviors

## What We're NOT Doing

- **Entity file path changes** — The reviewer suggested `--diff` can't find entities because they're generated into feature module directories like `src/users/entity.rs`. This is incorrect: both `import database` (codegen.rs:597) and `import database --diff` (import.rs:1093) use the same hardcoded path `src/entity.rs`. No feature-module entity generation exists in the codebase. **Flag this back to the reviewer.**
- Not adding `heck` as a dependency to `rapina-cli` — we'll reimplement the algorithm to match heck's behavior without adding the dependency

## Implementation Approach

Three phases, each independently testable. Phase 1 is a pure refactor. Phases 2 and 3 fix bugs.

---

## Phase 1: Extract Shared Introspection Helper + Remove Type Annotation

### Overview
Eliminate code duplication between `database()` and `database_diff()` by extracting the runtime creation and URL-dispatch block into a shared helper.

### Changes Required

#### 1. Extract `introspect_tables` helper
**File**: `rapina-cli/src/commands/import.rs`

Add a new function that encapsulates the duplicated logic:

```rust
/// Create a tokio runtime, connect to the database, and return introspected tables.
fn introspect_tables(
    url: &str,
    schema_name: Option<&str>,
) -> Result<Vec<IntrospectedTable>, String> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("Failed to create async runtime: {}", e))?;

    rt.block_on(async {
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            #[cfg(feature = "import-postgres")]
            {
                let schema = schema_name.unwrap_or("public");
                introspect_postgres(url, schema).await
            }
            #[cfg(not(feature = "import-postgres"))]
            {
                let _ = schema_name;
                Err("Postgres support requires the import-postgres feature. \
                     Reinstall with: cargo install rapina-cli --features import-postgres"
                    .to_string())
            }
        } else if url.starts_with("mysql://") || url.starts_with("mariadb://") {
            #[cfg(feature = "import-mysql")]
            {
                let schema = schema_name
                    .or_else(|| url.rsplit('/').next())
                    .ok_or_else(|| {
                        "Could not determine database name from URL. Use --schema to specify it."
                            .to_string()
                    })?;
                introspect_mysql(url, schema).await
            }
            #[cfg(not(feature = "import-mysql"))]
            {
                let _ = schema_name;
                Err("MySQL support requires the import-mysql feature. \
                     Reinstall with: cargo install rapina-cli --features import-mysql"
                    .to_string())
            }
        } else if url.starts_with("sqlite://") || url.starts_with("sqlite:") {
            #[cfg(feature = "import-sqlite")]
            {
                let _ = schema_name;
                introspect_sqlite(url).await
            }
            #[cfg(not(feature = "import-sqlite"))]
            {
                let _ = schema_name;
                Err("SQLite support requires the import-sqlite feature. \
                     Reinstall with: cargo install rapina-cli --features import-sqlite"
                    .to_string())
            }
        } else {
            Err(format!(
                "Unsupported database URL scheme. Expected postgres://, mysql://, or sqlite:// -- got {:?}",
                url.split("://").next().unwrap_or("unknown")
            ))
        }
    })
}
```

#### 2. Update `database()` to use the helper
**File**: `rapina-cli/src/commands/import.rs`

Replace lines 598-649 with:
```rust
let tables = introspect_tables(url, schema_name)?;
```

#### 3. Update `database_diff()` to use the helper and remove type annotation
**File**: `rapina-cli/src/commands/import.rs`

Replace lines 1103-1157 with:
```rust
let tables = introspect_tables(url, schema_name)?;
```

### Success Criteria

#### Automated Verification:
- [x] `cargo build -p rapina-cli` compiles cleanly
- [x] `cargo test -p rapina-cli` — all existing tests pass
- [x] `cargo clippy -p rapina-cli` — no new warnings

---

## Phase 2: Fix `to_snake_case` for Consecutive Uppercase

### Overview
Update the algorithm to handle runs of uppercase letters correctly, matching heck's behavior.

### Changes Required

#### 1. Replace `to_snake_case` implementation
**File**: `rapina-cli/src/commands/entity_parser.rs` (line 437)

Replace the current implementation with:

```rust
fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            let next_is_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if i > 0 && (next_is_lower || !chars[i - 1].is_uppercase()) {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap());
        } else {
            result.push(c);
        }
    }
    result
}
```

Logic: insert an underscore before an uppercase char only when:
- It's not the first character, AND
- Either the next char is lowercase (start of a new word after an acronym), OR the previous char is not uppercase (start of a new uppercase run)

This produces: `HTMLParser` → `html_parser`, `APIToken` → `api_token`, `BlogPost` → `blog_post`.

#### 2. Update test assertions
**File**: `rapina-cli/src/commands/entity_parser.rs` (line 464)

Change the existing test and add more cases:

```rust
#[test]
fn test_snake_case_simple() {
    assert_eq!(to_snake_case("User"), "user");
    assert_eq!(to_snake_case("BlogPost"), "blog_post");
    assert_eq!(to_snake_case("HTMLParser"), "html_parser");
    assert_eq!(to_snake_case("APIToken"), "api_token");
    assert_eq!(to_snake_case("SimpleXMLParser"), "simple_xml_parser");
    assert_eq!(to_snake_case("ID"), "id");
}
```

### Success Criteria

#### Automated Verification:
- [x] `cargo test -p rapina-cli -- test_snake_case` passes with updated assertions
- [x] `cargo test -p rapina-cli` — all existing tests pass

---

## Phase 3: Resolve FK Column Types from Referenced Entity

### Overview
When building expected columns for a belongs_to field, look up the referenced entity's PK type instead of assuming `i32`.

### Changes Required

#### 1. Update `build_expected_columns` signature
**File**: `rapina-cli/src/commands/import.rs`

Change `build_expected_columns` to accept all entities so it can look up referenced entity PK types:

```rust
fn build_expected_columns(entity: &ParsedEntity, all_entities: &[ParsedEntity]) -> Vec<ExpectedColumn> {
```

#### 2. Resolve FK type from referenced entity
**File**: `rapina-cli/src/commands/import.rs` (line 823-829)

Replace the hardcoded `NormalizedType::I32` block:

```rust
if field.is_belongs_to {
    let fk_type = resolve_fk_type(&field.schema_type, all_entities);
    columns.push(ExpectedColumn {
        name: field.column_name.clone(),
        expected_type: fk_type,
        nullable: field.optional,
    });
}
```

#### 3. Add `resolve_fk_type` helper
**File**: `rapina-cli/src/commands/import.rs`

```rust
/// Determine the FK column type by looking up the referenced entity's primary key type.
/// Falls back to I32 if the entity isn't found or has a default PK.
fn resolve_fk_type(referenced_entity_name: &str, all_entities: &[ParsedEntity]) -> NormalizedType {
    let referenced = all_entities.iter().find(|e| e.name == referenced_entity_name);

    match referenced {
        Some(entity) => match &entity.primary_key {
            // Default PK → i32
            None => NormalizedType::I32,
            // Custom PK → look up the first PK column's type in the entity's fields
            Some(pk_cols) => {
                if let Some(pk_col) = pk_cols.first() {
                    entity
                        .fields
                        .iter()
                        .find(|f| f.name == *pk_col)
                        .and_then(|f| schema_type_to_normalized(&f.schema_type))
                        .unwrap_or(NormalizedType::I32)
                } else {
                    NormalizedType::I32
                }
            }
        },
        // Referenced entity not in parsed set → default i32
        None => NormalizedType::I32,
    }
}
```

#### 4. Update `compare_table` to pass entities through
**File**: `rapina-cli/src/commands/import.rs`

Update `compare_table` signature and call site:

```rust
fn compare_table(entity: &ParsedEntity, db_table: &IntrospectedTable, all_entities: &[ParsedEntity]) -> TableDrift {
    let expected = build_expected_columns(entity, all_entities);
    // ... rest unchanged
```

Update the call in `compute_drift` (line 905):

```rust
let drift = compare_table(entity, db_table, entities);
```

#### 5. Add tests for FK type resolution
**File**: `rapina-cli/src/commands/import.rs`

```rust
#[test]
fn test_drift_belongs_to_uuid_pk() {
    // Event has a UUID PK, Ticket belongs_to Event
    let event = ParsedEntity {
        name: "Event".to_string(),
        table_name: "events".to_string(),
        fields: vec![ParsedField {
            name: "id".to_string(),
            column_name: "id".to_string(),
            schema_type: "Uuid".to_string(),
            optional: false,
            is_belongs_to: false,
            is_has_many: false,
        }],
        has_created_at: false,
        has_updated_at: false,
        primary_key: Some(vec!["id".to_string()]),
    };
    let ticket = ParsedEntity {
        name: "Ticket".to_string(),
        table_name: "tickets".to_string(),
        fields: vec![ParsedField {
            name: "event".to_string(),
            column_name: "event_id".to_string(),
            schema_type: "Event".to_string(),
            optional: false,
            is_belongs_to: true,
            is_has_many: false,
        }],
        has_created_at: false,
        has_updated_at: false,
        primary_key: None,
    };
    let entities = vec![event, ticket];

    // DB has event_id as UUID — should NOT flag as drift
    let db_table = IntrospectedTable {
        name: "tickets".to_string(),
        columns: vec![
            IntrospectedColumn { name: "id".to_string(), col_type: NormalizedType::I32, is_nullable: false },
            IntrospectedColumn { name: "event_id".to_string(), col_type: NormalizedType::Uuid, is_nullable: false },
        ],
        primary_key_columns: vec!["id".to_string()],
        foreign_keys: vec![],
    };

    let drift = compare_table(&entities[1], &db_table, &entities);
    assert!(drift.type_mismatches.is_empty(), "UUID FK should not produce a type mismatch");
}

#[test]
fn test_resolve_fk_type_default_pk() {
    assert_eq!(resolve_fk_type("Unknown", &[]), NormalizedType::I32);
}
```

#### 6. Update existing test helper calls
Any existing tests that call `build_expected_columns` or `compare_table` directly will need the extra `&[]` or `&entities` argument. Check test helpers `make_entity` and all `compare_table`/`build_expected_columns` call sites in the test module.

### Success Criteria

#### Automated Verification:
- [x] `cargo test -p rapina-cli -- test_drift_belongs_to_uuid_pk` passes
- [x] `cargo test -p rapina-cli -- test_resolve_fk_type` passes
- [x] `cargo test -p rapina-cli` — all existing tests pass (including updated call sites)
- [x] `cargo clippy -p rapina-cli` — no new warnings

---

## Disputed Review Item

**Item #3 (import.rs:1090-1093)** — The reviewer states that `import database` generates entities into feature module directories like `src/users/entity.rs`, so the diff tool can't find them.

**This is incorrect.** Both `import database` (codegen.rs:597) and `import database --diff` (import.rs:1093) use the same hardcoded path `src/entity.rs`. The seed command (seed.rs:441) also uses `src/entity.rs`. Example projects (`examples/todo-app/src/entity.rs`, `examples/url-shortener/.../src/entity.rs`) confirm the flat single-file pattern.

No feature-module entity generation (`src/users/entity.rs`) exists anywhere in the codebase.

**Action**: Flag back to the reviewer for clarification.

## References

- Original commit: `0536789 feat(cli): add import database --diff for schema drift detection`
- Previous plan: `thoughts/shared/plans/2026-03-16-import-database-diff.md`
- heck usage in macros: `rapina-macros/src/schema/generate.rs` (line 3, `use heck::ToSnakeCase`)
