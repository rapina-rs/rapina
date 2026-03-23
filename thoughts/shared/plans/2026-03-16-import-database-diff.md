# `rapina import database --diff` Implementation Plan

## Overview

Add a `--diff` flag to `rapina import database` that compares entity definitions in `src/entity.rs` against the live database and reports schema drift — columns added/removed/changed, type mismatches, nullability differences, and untracked tables. Returns non-zero exit code when drift is detected for CI use.

## Current State Analysis

- `rapina import database` already introspects live databases via `sea-schema` into an `IntrospectedTable`/`IntrospectedColumn` IR (`import.rs:10-50`)
- Entity definitions use the `schema!` macro DSL in `src/entity.rs` with attributes like `#[table_name]`, `#[column]`, `#[timestamps]`, `#[primary_key]`
- The `NormalizedType` enum (`import.rs:34-50`) provides a database-agnostic type representation
- `codegen.rs` has the type mapping from schema types to column methods (e.g., `String` → `.string()`, `i32` → `.integer()`)
- `rapina openapi diff` provides a precedent for diff-style reporting with breaking/non-breaking changes and colored output

### Key Discoveries:
- `IntrospectedTable`/`IntrospectedColumn` are private structs in `import.rs` — they need to be reused by the diff logic, so either make them `pub(crate)` or keep the diff logic in the same module
- The `schema!` macro parser lives in `rapina-macros` and uses `syn` — but for the CLI diff we need a **text-level parser** for `entity.rs` since the CLI doesn't compile the user's code
- The `NormalizedType` → `FieldInfo` mapping in `normalized_to_field_info()` (`import.rs:133-168`) gives us the schema_type string (e.g., "String", "i32", "DateTime") which is exactly what appears in `entity.rs`
- Field type names in `entity.rs` map 1:1 to `ScalarType` variants in `types.rs:26-43`
- `filter_and_validate_tables()` already strips internal tables (`seaql_migrations`, etc.)

## Desired End State

Running `rapina import database --diff --url postgres://...` will:

1. Parse `src/entity.rs` to extract all entity definitions with their fields, types, and attribute overrides
2. Introspect the live database (reusing existing introspection)
3. Print a colored diff report showing:
   - Tables in DB with no matching entity ("untracked tables")
   - Entities in code with no matching DB table ("missing tables")
   - For matched tables: columns added/removed in DB, type changes, nullability changes
4. Exit with code 0 if no drift, code 1 if drift detected

Example output:
```
  → Comparing entity definitions against live database...
  ✓ Connected to database (12 tables discovered)
  ✓ Parsed 8 entities from src/entity.rs

  Drift report:

  ✗ Table "users" has drift:
    + column "phone" (varchar, nullable) exists in DB but not in entity
    ~ column "email" type mismatch: entity has String, DB has Text
    - column "legacy_id" exists in entity but not in DB

  ✗ Table "audit_logs" has drift:
    ~ column "metadata" nullability mismatch: entity has NOT NULL, DB has NULL

  ⚠ Untracked tables (in DB, no entity):
    • analytics_events
    • temp_imports

  ⚠ Missing tables (in entity, not in DB):
    • notifications

  Summary: 2 table(s) with drift, 2 untracked, 1 missing
```

### Verification:
- `cargo test -p rapina-cli --features import` passes with new tests
- `cargo clippy -p rapina-cli --features import` has no warnings
- Running `--diff` against a matching DB returns exit code 0
- Running `--diff` against a drifted DB returns exit code 1 with correct report

## What We're NOT Doing

- Auto-fixing drift (no migration generation from diff)
- Diffing indexes, constraints, or triggers — columns and types only
- Comparing migration history against DB state
- Supporting multiple entity files (only `src/entity.rs`)

## Implementation Approach

Keep all new logic in the existing `import.rs` file since `IntrospectedTable`/`IntrospectedColumn`/`NormalizedType` are private and the diff is conceptually part of the import command. Add a lightweight regex-based parser for `entity.rs` (no `syn` dependency in CLI) that extracts entity names, field names, types, and attribute overrides.

## Phase 1: Entity File Parser

### Overview
Build a text-level parser that reads `src/entity.rs` and extracts entity definitions into a structure we can compare against introspected tables.

### Changes Required:

#### 1. New file: `rapina-cli/src/commands/entity_parser.rs`

A standalone module that parses `schema!` blocks from entity files.

```rust
use std::collections::HashMap;
use std::fs;

/// An entity definition extracted from src/entity.rs
#[derive(Debug, Clone)]
pub struct ParsedEntity {
    /// PascalCase name as written in schema! (e.g., "User")
    pub name: String,
    /// Resolved table name (from #[table_name] or auto-pluralized)
    pub table_name: String,
    /// Fields declared in the entity (excludes auto-generated id, timestamps)
    pub fields: Vec<ParsedField>,
    /// Whether created_at is expected
    pub has_created_at: bool,
    /// Whether updated_at is expected
    pub has_updated_at: bool,
    /// Primary key columns (None = default auto i32 "id")
    pub primary_key: Option<Vec<String>>,
}

/// A field extracted from an entity definition
#[derive(Debug, Clone)]
pub struct ParsedField {
    /// Field name as written in schema!
    pub name: String,
    /// The resolved column name (from #[column] or field name, with _id suffix for belongs_to)
    pub column_name: String,
    /// The schema type string (e.g., "String", "i32", "bool", "User" for belongs_to)
    pub schema_type: String,
    /// Whether the field is Option<T>
    pub optional: bool,
    /// Whether this is a belongs_to relationship (generates an _id FK column)
    pub is_belongs_to: bool,
    /// Whether this is a has_many relationship (Vec<T>, no column in DB)
    pub is_has_many: bool,
}
```

The parser will:
1. Read `src/entity.rs`
2. Find all `schema! { ... }` blocks using brace-depth counting (same approach as `remove_schema_block` in `codegen.rs:542`)
3. Within each block, parse entity-level attributes (`#[table_name]`, `#[timestamps]`, `#[primary_key]`)
4. Parse field definitions with their attributes (`#[column]`, `#[unique]`, `#[index]`)
5. Classify field types:
   - Known scalars (String, Text, i32, i64, f32, f64, bool, Uuid, DateTime, NaiveDateTime, Date, Decimal, Json) → scalar column
   - `Vec<X>` → has_many (no DB column)
   - `Option<X>` where X is unknown → belongs_to (generates `{name}_id` column)
   - Unknown `X` → belongs_to (generates `{name}_id` column)
6. Use `codegen::pluralize()` and `codegen::singularize()` for table name resolution

The scalar type list matches `ScalarType::from_ident()` in `rapina-macros/src/schema/types.rs:26-43`.

#### 2. Register module in `rapina-cli/src/commands/mod.rs`

Add `pub(crate) mod entity_parser;` (no feature gate needed — it's pure text parsing with no DB dependencies).

### Success Criteria:

#### Automated Verification:
- [ ] `cargo test -p rapina-cli` passes with new parser unit tests
- [ ] Parser correctly handles: simple entities, entities with all attribute types, multiple schema! blocks, belongs_to/has_many fields, Option fields
- [ ] Parser correctly resolves table names (default pluralization + `#[table_name]` override)
- [ ] Parser correctly resolves column names (default + `#[column]` override + `_id` suffix for belongs_to)

#### Manual Verification:
- [ ] Parser produces correct output for `rapina/examples/todo-app/src/entity.rs`

---

## Phase 2: Diff Engine

### Overview
Compare parsed entities against introspected tables and produce a structured drift report.

### Changes Required:

#### 1. Diff logic in `rapina-cli/src/commands/import.rs`

Add types and comparison function at the bottom of `import.rs` (before tests):

```rust
// ---------------------------------------------------------------------------
// Schema drift detection
// ---------------------------------------------------------------------------

/// A column as understood by the entity definition in code.
#[derive(Debug)]
struct ExpectedColumn {
    /// Column name in the database
    name: String,
    /// NormalizedType that the code expects
    expected_type: NormalizedType,
    /// Whether the column is nullable
    nullable: bool,
}

/// Drift detected for a single table.
#[derive(Debug)]
struct TableDrift {
    table_name: String,
    entity_name: String,
    /// Columns in DB but not in entity
    extra_columns: Vec<IntrospectedColumn>,
    /// Columns in entity but not in DB
    missing_columns: Vec<ExpectedColumn>,
    /// Columns present in both but with type/nullability mismatch
    type_mismatches: Vec<TypeMismatch>,
}

#[derive(Debug)]
struct TypeMismatch {
    column_name: String,
    entity_type: NormalizedType,
    db_type: NormalizedType,
    entity_nullable: bool,
    db_nullable: bool,
}

/// Full drift report.
#[derive(Debug)]
struct DriftReport {
    /// Tables with column-level drift
    drifted_tables: Vec<TableDrift>,
    /// Tables in DB with no entity (excluding internal tables)
    untracked_tables: Vec<String>,
    /// Entities with no matching DB table
    missing_tables: Vec<String>,
}

impl DriftReport {
    fn has_drift(&self) -> bool {
        !self.drifted_tables.is_empty()
            || !self.untracked_tables.is_empty()
            || !self.missing_tables.is_empty()
    }
}
```

The comparison function `compute_drift()` will:
1. Convert `ParsedEntity` fields into `ExpectedColumn` entries:
   - Map schema type strings back to `NormalizedType` (reverse of `normalized_to_field_info`)
   - Add auto-generated columns: `id` (unless custom PK), `created_at`/`updated_at` per timestamp config
   - For belongs_to fields, add `{name}_id` column with type i32
2. Build a lookup map: table_name → entity
3. For each DB table:
   - If no matching entity → add to `untracked_tables`
   - If matching entity exists → compare columns
4. For each entity:
   - If no matching DB table → add to `missing_tables`
5. Column comparison by name:
   - In DB but not in expected → `extra_columns`
   - In expected but not in DB → `missing_columns`
   - In both → check type + nullability match → `type_mismatches` if different

Schema type → NormalizedType mapping (reverse of `normalized_to_field_info`):
```rust
fn schema_type_to_normalized(schema_type: &str) -> Option<NormalizedType> {
    match schema_type {
        "String" => Some(NormalizedType::Str),
        "Text" => Some(NormalizedType::Text),
        "i32" => Some(NormalizedType::I32),
        "i64" => Some(NormalizedType::I64),
        "f32" => Some(NormalizedType::F32),
        "f64" => Some(NormalizedType::F64),
        "bool" => Some(NormalizedType::Bool),
        "Uuid" => Some(NormalizedType::Uuid),
        "DateTime" => Some(NormalizedType::DateTimeUtc),
        "NaiveDateTime" => Some(NormalizedType::NaiveDateTime),
        "Date" => Some(NormalizedType::Date),
        "Decimal" => Some(NormalizedType::Decimal),
        "Json" => Some(NormalizedType::Json),
        _ => None, // belongs_to or unknown
    }
}
```

### Success Criteria:

#### Automated Verification:
- [ ] `cargo test -p rapina-cli` passes with new diff engine tests
- [ ] Tests cover: no drift (identical), extra DB column, missing DB column, type mismatch, nullability mismatch, untracked table, missing table, mixed drift
- [ ] `Unmappable` DB types are reported as extra columns (not false type mismatches)

---

## Phase 3: CLI Wiring + Output

### Overview
Add the `--diff` flag to the CLI, wire up the parser + introspection + diff engine, and format colored terminal output.

### Changes Required:

#### 1. Add `--diff` flag in `rapina-cli/src/main.rs`

In the `ImportCommands::Database` variant (~line 155):

```rust
/// Compare entity definitions against live database and report drift
#[arg(long)]
diff: bool,
```

#### 2. Wire up in `main.rs` match arm (~line 335)

Pass the `diff` flag through to the handler:

```rust
ImportCommands::Database {
    url,
    tables,
    schema,
    force,
    diff,
} => {
    #[cfg(feature = "import")]
    {
        if diff {
            commands::import::database_diff(
                &url,
                tables.as_deref(),
                schema.as_deref(),
            )
        } else {
            commands::import::database(
                &url,
                tables.as_deref(),
                schema.as_deref(),
                force,
            )
        }
    }
}
```

#### 3. Add `pub fn database_diff()` entry point in `import.rs`

```rust
pub fn database_diff(
    url: &str,
    table_filter: Option<&[String]>,
    schema_name: Option<&str>,
) -> Result<(), String> {
    codegen::verify_rapina_project()?;

    // 1. Parse entity file
    println!();
    println!("  {} Parsing entity definitions...", "→".bright_cyan());
    let entities = super::entity_parser::parse_entity_file()?;
    println!("  {} Parsed {} entity/entities from {}", "✓".green(), entities.len(), "src/entity.rs".cyan());

    // 2. Introspect live DB (reuse existing async introspection)
    println!("  {} Connecting to database...", "→".bright_cyan());
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("Failed to create async runtime: {}", e))?;
    let tables = rt.block_on(async { /* same URL dispatch as database() */ })?;
    let total = tables.len();

    // Apply same filtering (skip internal tables, but keep tables without PK for untracked reporting)
    let db_tables = filter_tables_for_diff(tables, table_filter);
    println!("  {} Discovered {} table(s)", "✓".green(), total);

    // 3. Compute drift
    let report = compute_drift(&entities, &db_tables);

    // 4. Print report
    print_drift_report(&report);

    if report.has_drift() {
        Err("Schema drift detected".to_string())
    } else {
        Ok(())
    }
}
```

Note: `filter_tables_for_diff()` is a lighter version of `filter_and_validate_tables()` — it removes internal tables but does NOT skip tables without PKs or with composite PKs, since those should appear as "untracked" if they have no entity.

#### 4. Report formatter in `import.rs`

```rust
fn print_drift_report(report: &DriftReport) {
    println!();

    if !report.has_drift() {
        println!("  {} No schema drift detected", "✓".green());
        return;
    }

    println!("  {}:", "Drift report".bright_yellow());
    println!();

    for drift in &report.drifted_tables {
        println!("  {} Table {:?} ({}) has drift:", "✗".red(), drift.table_name, drift.entity_name.bright_cyan());
        for col in &drift.extra_columns {
            let nullable = if col.is_nullable { "nullable" } else { "not null" };
            println!("    {} column {:?} ({:?}, {}) exists in DB but not in entity",
                "+".green(), col.name, col.col_type, nullable);
        }
        for col in &drift.missing_columns {
            println!("    {} column {:?} exists in entity but not in DB",
                "-".red(), col.name);
        }
        for m in &drift.type_mismatches {
            if m.entity_type != m.db_type {
                println!("    {} column {:?} type mismatch: entity has {:?}, DB has {:?}",
                    "~".yellow(), m.column_name, m.entity_type, m.db_type);
            }
            if m.entity_nullable != m.db_nullable {
                let entity_null = if m.entity_nullable { "NULL" } else { "NOT NULL" };
                let db_null = if m.db_nullable { "NULL" } else { "NOT NULL" };
                println!("    {} column {:?} nullability mismatch: entity has {}, DB has {}",
                    "~".yellow(), m.column_name, entity_null, db_null);
            }
        }
        println!();
    }

    if !report.untracked_tables.is_empty() {
        println!("  {} Untracked tables (in DB, no entity):", "⚠".yellow());
        for t in &report.untracked_tables {
            println!("    {} {}", "•".yellow(), t);
        }
        println!();
    }

    if !report.missing_tables.is_empty() {
        println!("  {} Missing tables (in entity, not in DB):", "⚠".yellow());
        for t in &report.missing_tables {
            println!("    {} {}", "•".yellow(), t);
        }
        println!();
    }

    // Summary line
    let mut parts = Vec::new();
    if !report.drifted_tables.is_empty() {
        parts.push(format!("{} table(s) with drift", report.drifted_tables.len()));
    }
    if !report.untracked_tables.is_empty() {
        parts.push(format!("{} untracked", report.untracked_tables.len()));
    }
    if !report.missing_tables.is_empty() {
        parts.push(format!("{} missing", report.missing_tables.len()));
    }
    println!("  {}: {}", "Summary".bright_yellow(), parts.join(", "));
    println!();
}
```

#### 5. Implement `NormalizedType` Display

Add `Display` impl for `NormalizedType` so the report can show human-readable type names instead of `Debug` output:

```rust
impl std::fmt::Display for NormalizedType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NormalizedType::Str => write!(f, "String"),
            NormalizedType::Text => write!(f, "Text"),
            NormalizedType::I32 => write!(f, "i32"),
            NormalizedType::I64 => write!(f, "i64"),
            NormalizedType::F32 => write!(f, "f32"),
            NormalizedType::F64 => write!(f, "f64"),
            NormalizedType::Bool => write!(f, "bool"),
            NormalizedType::Uuid => write!(f, "Uuid"),
            NormalizedType::DateTimeUtc => write!(f, "DateTime"),
            NormalizedType::NaiveDateTime => write!(f, "NaiveDateTime"),
            NormalizedType::Date => write!(f, "Date"),
            NormalizedType::Decimal => write!(f, "Decimal"),
            NormalizedType::Json => write!(f, "Json"),
            NormalizedType::Unmappable(s) => write!(f, "{}", s),
        }
    }
}
```

### Success Criteria:

#### Automated Verification:
- [ ] `cargo build -p rapina-cli --features import-postgres` compiles cleanly
- [ ] `cargo clippy -p rapina-cli --features import` has no warnings
- [ ] `cargo test -p rapina-cli --features import` passes all tests (existing + new)
- [ ] `--diff` and `--force` are mutually exclusive (error if both passed)

#### Manual Verification:
- [ ] Running `rapina import database --diff` against a matching DB shows "No schema drift detected" and exits 0
- [ ] Running against a DB with an extra column reports it correctly
- [ ] Running against a DB missing a column reports it correctly
- [ ] Untracked tables (no entity) are listed
- [ ] Missing tables (entity but no DB table) are listed
- [ ] Output is properly colored and formatted

---

## Testing Strategy

### Unit Tests (in `entity_parser.rs`):
- Parse simple entity with scalar fields
- Parse entity with `#[table_name]` override
- Parse entity with `#[column]` override on field
- Parse entity with `#[timestamps(none)]`, `#[timestamps(created_at)]`
- Parse entity with `#[primary_key(id)]` for Uuid PK
- Parse entity with belongs_to relationship (`author: User`)
- Parse entity with has_many relationship (`posts: Vec<Post>`)
- Parse entity with Option scalar field
- Parse multiple schema! blocks in one file
- Handle entity file that doesn't exist (error)
- Handle entity file with no schema! blocks (empty result)

### Unit Tests (in `import.rs` tests module):
- `compute_drift` with identical schema → empty report
- `compute_drift` with extra DB column → reported in `extra_columns`
- `compute_drift` with missing DB column → reported in `missing_columns`
- `compute_drift` with type mismatch → reported in `type_mismatches`
- `compute_drift` with nullability mismatch → reported in `type_mismatches`
- `compute_drift` with untracked table → in `untracked_tables`
- `compute_drift` with missing table → in `missing_tables`
- `compute_drift` handles timestamp columns correctly (auto-added based on entity config)
- `compute_drift` handles belongs_to FK columns (e.g., `author` field → expects `author_id` column)
- `compute_drift` ignores has_many fields (no DB column)
- `schema_type_to_normalized` maps all 13 scalar types correctly
- `DriftReport::has_drift()` returns false when all lists empty, true otherwise

### Integration Tests:
- None needed — this is a read-only comparison command, unit tests on the parser and diff engine provide sufficient coverage

## References

- Existing introspection: `rapina-cli/src/commands/import.rs:174-323`
- Schema block parsing pattern: `rapina-cli/src/commands/codegen.rs:542-582` (`remove_schema_block`)
- OpenAPI diff precedent: `rapina-cli/src/commands/openapi.rs:59-107`
- Scalar type list: `rapina-macros/src/schema/types.rs:26-43`
- Entity example: `rapina/examples/todo-app/src/entity.rs`
