# Import: Generate Relationship Fields from Foreign Keys

## Overview

The `rapina import database` command detects foreign keys and already builds `RelationshipInfo` records (`BelongsTo`/`HasMany`), but the code generation ignores them — FK columns stay as plain integers. This plan wires the existing relationship detection into the schema code generation so that imported tables produce proper `BelongsTo`/`HasMany` fields in a single combined `schema!` block.

## Current State Analysis

- **FK detection works**: `resolve_relationships()` at `import.rs:427` builds a `HashMap<String, Vec<RelationshipInfo>>` with correct `BelongsTo` and `HasMany` entries.
- **Generation ignores it**: `generate_for_table()` at `import.rs:496` takes `_relationships` (unused). Each table gets its own `schema!` block with only scalar fields.
- **The macro supports it**: `schema!` already handles `author: User` (BelongsTo), `author: Option<User>` (optional BelongsTo), and `posts: Vec<Post>` (HasMany) — see `schema_test.rs:11-33`.

### Key Discoveries:
- `generate_for_table()` calls `codegen::update_entity_file()` which calls `generate_schema_block()` — this is where scalar fields are rendered. It has no concept of relationship fields. (`codegen.rs:417-451`)
- Each table currently gets an independent `schema!` block appended to `entity.rs` (`codegen.rs:601-638`)
- FK columns like `user_id` are currently emitted as `user_id: i32` scalar fields
- The `RelationshipInfo` struct already carries `field_name` (e.g., `"user"` for BelongsTo, `"posts"` for HasMany) and `related_pascal` (e.g., `"User"`, `"Post"`)

## Desired End State

Given tables `users(id, email)` and `posts(id, title, user_id FK->users.id)`, the import should generate:

```rust
schema! {
    User {
        email: String,
        posts: Vec<Post>,
    }

    Post {
        title: String,
        author: User,
    }
}
```

Instead of the current:

```rust
schema! {
    User {
        email: String,
    }
}

schema! {
    Post {
        title: String,
        user_id: i32,
    }
}
```

### Verification:
- `cargo test -p rapina-cli` passes
- Importing a database with FK relationships produces a single `schema!` block with BelongsTo/HasMany fields
- FK columns (e.g., `user_id`) are replaced by relationship fields (e.g., `user: User`), not duplicated
- Nullable FK columns produce `Option<Entity>` BelongsTo fields
- Tables with no FK relationships to any other imported table still work (scalar-only fields)

## What We're NOT Doing

- **Multi-column FK support** — already skipped in `resolve_relationships()` (line 440-442)
- **SQLite FK support** — SQLite FK introspection is blocked by `pub(crate)` fields in sea-schema (line 302-304)
- **Grouping into multiple schema blocks** — all imported tables go into one block
- **Changes to the `schema!` macro** — it already handles all relationship syntax
- **Changes to migration generation** — migrations should still reflect the raw columns; the schema macro generates the FK column from the BelongsTo field

## Implementation Approach

The changes are contained in two files: `import.rs` and `codegen.rs`. The core idea:

1. Stop generating per-table schema blocks and entity file updates
2. Instead, collect all table data, then generate a single combined `schema!` block
3. When building fields for a table, replace FK columns with BelongsTo fields and append HasMany fields

## Phase 1: Add Relationship-Aware Schema Generation to codegen.rs

### Overview
Add a new function that generates a combined multi-entity `schema!` block with relationship fields.

### Changes Required:

#### 1. New struct for relationship fields in codegen.rs

**File**: `rapina-cli/src/commands/codegen.rs`

Add a new struct alongside `FieldInfo` to represent relationship fields:

```rust
pub(crate) struct RelationFieldInfo {
    pub name: String,
    /// e.g., "User", "Post"
    pub target_entity: String,
    pub kind: RelationFieldKind,
}

pub(crate) enum RelationFieldKind {
    BelongsTo,
    OptionalBelongsTo,
    HasMany,
}
```

#### 2. New struct for a full entity definition

**File**: `rapina-cli/src/commands/codegen.rs`

```rust
pub(crate) struct EntityBlock {
    pub pascal_name: String,
    pub fields: Vec<FieldInfo>,
    pub relations: Vec<RelationFieldInfo>,
    pub timestamps: Option<String>,
    pub primary_key: Option<Vec<String>>,
}
```

#### 3. New function: `generate_combined_schema_block`

**File**: `rapina-cli/src/commands/codegen.rs`

A new public function that renders multiple entities into a single `schema! {}` block:

```rust
pub(crate) fn generate_combined_schema_block(entities: &[EntityBlock]) -> String {
    let mut entity_sections = Vec::new();

    for entity in entities {
        let mut attrs = String::new();

        if let Some(ref pk_cols) = entity.primary_key {
            attrs.push_str(&format!("\n    #[primary_key({})]", pk_cols.join(", ")));
        }

        if let Some(ref ts) = entity.timestamps {
            attrs.push_str(&format!("\n    #[timestamps({})]", ts));
        }

        // Scalar fields
        let mut field_lines: Vec<String> = entity
            .fields
            .iter()
            .map(|f| format!("        {}: {},", f.name, f.schema_type))
            .collect();

        // Relationship fields
        for rel in &entity.relations {
            let line = match rel.kind {
                RelationFieldKind::BelongsTo => {
                    format!("        {}: {},", rel.name, rel.target_entity)
                }
                RelationFieldKind::OptionalBelongsTo => {
                    format!("        {}: Option<{}>,", rel.name, rel.target_entity)
                }
                RelationFieldKind::HasMany => {
                    format!("        {}: Vec<{}>,", rel.name, rel.target_entity)
                }
            };
            field_lines.push(line);
        }

        entity_sections.push(format!(
            "    {attrs}\n    {pascal} {{\n{fields}\n    }}",
            attrs = attrs,
            pascal = entity.pascal_name,
            fields = field_lines.join("\n"),
        ));
    }

    format!(
        "\nschema! {{\n{}\n}}\n",
        entity_sections.join("\n\n"),
    )
}
```

#### 4. New function: `write_combined_entity_file`

**File**: `rapina-cli/src/commands/codegen.rs`

Writes/updates entity.rs with a single combined block, removing old blocks for entities being re-imported:

```rust
pub(crate) fn write_combined_entity_file(
    entities: &[EntityBlock],
    force: bool,
) -> Result<(), String> {
    write_combined_entity_file_in(entities, force, Path::new("src/entity.rs"))
}

fn write_combined_entity_file_in(
    entities: &[EntityBlock],
    force: bool,
    entity_path: &Path,
) -> Result<(), String> {
    let schema_block = generate_combined_schema_block(entities);

    if entity_path.exists() {
        let mut content = fs::read_to_string(entity_path)
            .map_err(|e| format!("Failed to read entity.rs: {}", e))?;

        if force {
            for entity in entities {
                content = remove_schema_block(&content, &entity.pascal_name);
            }
        }

        let needs_import =
            !content.contains("use rapina::prelude::*") && !content.contains("use rapina::schema");
        let prefix = if needs_import {
            "use rapina::schema;\n"
        } else {
            ""
        };

        let updated = format!("{}{}{}", prefix, content.trim_end(), schema_block);
        fs::write(entity_path, updated)
            .map_err(|e| format!("Failed to write entity.rs: {}", e))?;
    } else {
        let content = format!("use rapina::prelude::*;\n{}", schema_block);
        fs::write(entity_path, content)
            .map_err(|e| format!("Failed to create entity.rs: {}", e))?;
    }

    println!("  {} Updated {}", "✓".green(), "src/entity.rs".cyan());
    Ok(())
}
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo test -p rapina-cli` passes — new functions compile and existing tests still pass
- [x] New unit tests for `generate_combined_schema_block` pass (see Phase 3)

---

## Phase 2: Refactor import.rs to Use Combined Generation

### Overview
Change the import flow from per-table entity file updates to a single combined write.

### Changes Required:

#### 1. Build EntityBlock list with relationship fields

**File**: `rapina-cli/src/commands/import.rs`

Refactor `generate_for_table` to return an `EntityBlock` instead of writing to disk, and add a new function to build fields with relationships:

```rust
fn build_entity_block(
    table: &IntrospectedTable,
    relationships: &HashMap<String, Vec<RelationshipInfo>>,
) -> Result<EntityBlock, String> {
    let singular = codegen::singularize(&table.name);
    let pascal = codegen::to_pascal_case(&singular);

    let is_composite_pk = table.primary_key_columns.len() > 1;
    let is_default_pk = !is_composite_pk
        && table.columns.iter().any(|c| c.name == "id" && c.col_type == NormalizedType::I32);

    let skip_columns: Vec<&str> = if is_composite_pk || !is_default_pk {
        vec!["created_at", "updated_at"]
    } else {
        vec!["id", "created_at", "updated_at"]
    };

    // Collect FK column names so we can skip them as scalar fields
    let fk_columns: std::collections::HashSet<String> = relationships
        .get(&table.name)
        .map(|rels| {
            rels.iter()
                .filter(|r| matches!(r.kind, RelationKind::BelongsTo))
                .map(|r| format!("{}_id", r.field_name))
                .collect()
        })
        .unwrap_or_default();

    let mut fields = Vec::new();
    let mut skipped = 0;

    for col in &table.columns {
        if skip_columns.contains(&col.name.as_str()) {
            continue;
        }
        // Skip FK columns — they'll be replaced by BelongsTo fields
        if fk_columns.contains(&col.name) {
            continue;
        }
        match normalized_to_field_info(&col.name, &col.col_type, col.is_nullable) {
            Some(fi) => fields.push(fi),
            None => {
                if let NormalizedType::Unmappable(ref type_name) = col.col_type {
                    eprintln!(
                        "    {} column {:?}.{:?} ({}) has no schema! equivalent -- skipped",
                        "warn:".yellow(),
                        table.name, col.name, type_name
                    );
                }
                skipped += 1;
            }
        }
    }

    // Build relationship fields
    let mut relations = Vec::new();
    if let Some(rels) = relationships.get(&table.name) {
        for rel in rels {
            let kind = match &rel.kind {
                RelationKind::BelongsTo => {
                    // Check if the FK column is nullable
                    let fk_col_name = format!("{}_id", rel.field_name);
                    let is_nullable = table
                        .columns
                        .iter()
                        .find(|c| c.name == fk_col_name)
                        .map(|c| c.is_nullable)
                        .unwrap_or(false);
                    if is_nullable {
                        codegen::RelationFieldKind::OptionalBelongsTo
                    } else {
                        codegen::RelationFieldKind::BelongsTo
                    }
                }
                RelationKind::HasMany => codegen::RelationFieldKind::HasMany,
            };
            relations.push(codegen::RelationFieldInfo {
                name: rel.field_name.clone(),
                target_entity: rel.related_pascal.clone(),
                kind,
            });
        }
    }

    let timestamps = detect_timestamps(table).map(|s| s.to_string());
    let primary_key = if is_composite_pk || !is_default_pk {
        Some(table.primary_key_columns.clone())
    } else {
        None
    };

    println!(
        "  {} Imported table {:?} as {} ({} columns, {} relations, {} skipped)",
        "✓".green(),
        table.name,
        pascal.bright_cyan(),
        fields.len(),
        relations.len(),
        skipped
    );

    Ok(codegen::EntityBlock {
        pascal_name: pascal,
        fields,
        relations,
        timestamps,
        primary_key,
    })
}
```

#### 2. Update the `database()` entry point

**File**: `rapina-cli/src/commands/import.rs`

Replace the per-table loop with combined generation:

```rust
// In database(), replace the generate_for_table loop with:

let relationships = resolve_relationships(&tables);
let mut entity_blocks = Vec::new();
let mut imported = Vec::new();

for table in &tables {
    let singular = codegen::singularize(&table.name);
    let plural = &table.name;
    let pascal = codegen::to_pascal_case(&singular);
    let pascal_plural = codegen::to_pascal_case(plural);

    let block = build_entity_block(table, &relationships)?;
    entity_blocks.push(block);

    // Still generate migration and feature module per-table
    // (these don't need relationship awareness)
    let fields = /* scalar fields for this table, same as current logic */;
    codegen::create_migration_file(plural, &pascal_plural, &fields, pk_type)?;
    codegen::create_feature_module(&singular, plural, &pascal, &fields, pk_type, force)?;

    imported.push((table.name.clone(), pascal));
}

// Write all entities into a single combined schema! block
codegen::write_combined_entity_file(&entity_blocks, force)?;
```

Note: migration and feature module generation still happen per-table and use the full scalar field list (including FK columns like `user_id`), because migrations need the actual database columns. Only the `schema!` block in `entity.rs` uses relationship syntax.

#### 3. Remove `generate_for_table` function

It's fully replaced by `build_entity_block` + the combined write. The old `update_entity_file` function stays for non-import use cases (e.g., `rapina add resource`).

### Success Criteria:

#### Automated Verification:
- [x] `cargo test -p rapina-cli` passes
- [x] `cargo build -p rapina-cli` compiles

#### Manual Verification:
- [ ] Import a database with FK relationships — entity.rs contains a single `schema!` block with BelongsTo/HasMany fields
- [ ] FK columns are not duplicated (no `user_id: i32` alongside `user: User`)
- [ ] Tables without relationships still import correctly

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation.

---

## Phase 3: Tests

### Overview
Add unit tests for the new codegen functions and update existing import tests.

### Changes Required:

#### 1. Tests for `generate_combined_schema_block`

**File**: `rapina-cli/src/commands/codegen.rs` (in `#[cfg(test)] mod tests`)

```rust
#[test]
fn test_generate_combined_schema_block_with_relationships() {
    let entities = vec![
        EntityBlock {
            pascal_name: "User".to_string(),
            fields: vec![FieldInfo {
                name: "email".to_string(),
                rust_type: "String".to_string(),
                schema_type: "String".to_string(),
                column_method: String::new(),
                nullable: false,
            }],
            relations: vec![RelationFieldInfo {
                name: "posts".to_string(),
                target_entity: "Post".to_string(),
                kind: RelationFieldKind::HasMany,
            }],
            timestamps: None,
            primary_key: None,
        },
        EntityBlock {
            pascal_name: "Post".to_string(),
            fields: vec![FieldInfo {
                name: "title".to_string(),
                rust_type: "String".to_string(),
                schema_type: "String".to_string(),
                column_method: String::new(),
                nullable: false,
            }],
            relations: vec![RelationFieldInfo {
                name: "author".to_string(),
                target_entity: "User".to_string(),
                kind: RelationFieldKind::BelongsTo,
            }],
            timestamps: None,
            primary_key: None,
        },
    ];

    let block = generate_combined_schema_block(&entities);
    assert!(block.contains("schema! {"));
    assert!(block.contains("User {"));
    assert!(block.contains("email: String,"));
    assert!(block.contains("posts: Vec<Post>,"));
    assert!(block.contains("Post {"));
    assert!(block.contains("title: String,"));
    assert!(block.contains("author: User,"));
    // Only one schema! block
    assert_eq!(block.matches("schema! {").count(), 1);
}

#[test]
fn test_generate_combined_schema_block_optional_belongs_to() {
    let entities = vec![EntityBlock {
        pascal_name: "Comment".to_string(),
        fields: vec![FieldInfo {
            name: "body".to_string(),
            rust_type: "String".to_string(),
            schema_type: "Text".to_string(),
            column_method: String::new(),
            nullable: false,
        }],
        relations: vec![RelationFieldInfo {
            name: "author".to_string(),
            target_entity: "User".to_string(),
            kind: RelationFieldKind::OptionalBelongsTo,
        }],
        timestamps: None,
        primary_key: None,
    }];

    let block = generate_combined_schema_block(&entities);
    assert!(block.contains("author: Option<User>,"));
}

#[test]
fn test_generate_combined_schema_block_no_relations() {
    let entities = vec![EntityBlock {
        pascal_name: "Setting".to_string(),
        fields: vec![FieldInfo {
            name: "key".to_string(),
            rust_type: "String".to_string(),
            schema_type: "String".to_string(),
            column_method: String::new(),
            nullable: false,
        }],
        relations: vec![],
        timestamps: Some("none".to_string()),
        primary_key: None,
    }];

    let block = generate_combined_schema_block(&entities);
    assert!(block.contains("Setting {"));
    assert!(block.contains("key: String,"));
    assert!(block.contains("#[timestamps(none)]"));
    assert!(!block.contains("Vec<"));
    assert!(!block.contains("Option<"));
}
```

#### 2. Tests for `write_combined_entity_file_in`

**File**: `rapina-cli/src/commands/codegen.rs` (in `#[cfg(test)] mod tests`)

```rust
#[test]
fn test_write_combined_entity_file_creates_new() {
    let dir = tempfile::tempdir().unwrap();
    let entity_path = dir.path().join("entity.rs");

    let entities = vec![EntityBlock {
        pascal_name: "User".to_string(),
        fields: vec![FieldInfo {
            name: "email".to_string(),
            rust_type: "String".to_string(),
            schema_type: "String".to_string(),
            column_method: String::new(),
            nullable: false,
        }],
        relations: vec![],
        timestamps: None,
        primary_key: None,
    }];

    write_combined_entity_file_in(&entities, false, &entity_path).unwrap();
    let content = fs::read_to_string(&entity_path).unwrap();
    assert!(content.contains("use rapina::prelude::*;"));
    assert!(content.contains("schema! {"));
    assert!(content.contains("User {"));
}

#[test]
fn test_write_combined_entity_file_force_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let entity_path = dir.path().join("entity.rs");
    fs::write(&entity_path, "use rapina::prelude::*;\n\nschema! {\n    User {\n        name: String,\n    }\n}\n").unwrap();

    let entities = vec![EntityBlock {
        pascal_name: "User".to_string(),
        fields: vec![FieldInfo {
            name: "email".to_string(),
            rust_type: "String".to_string(),
            schema_type: "String".to_string(),
            column_method: String::new(),
            nullable: false,
        }],
        relations: vec![],
        timestamps: None,
        primary_key: None,
    }];

    write_combined_entity_file_in(&entities, true, &entity_path).unwrap();
    let content = fs::read_to_string(&entity_path).unwrap();
    assert_eq!(content.matches("User {").count(), 1);
    assert!(content.contains("email: String,"));
    assert!(!content.contains("name: String,"));
}
```

#### 3. Update `test_resolve_relationships` in import.rs

The existing test at `import.rs:990` already validates FK detection and should still pass unchanged.

#### 4. Add test for `build_entity_block`

**File**: `rapina-cli/src/commands/import.rs` (in `#[cfg(test)] mod tests`)

```rust
#[test]
fn test_build_entity_block_replaces_fk_with_belongs_to() {
    let tables = vec![
        IntrospectedTable {
            name: "users".into(),
            columns: vec![IntrospectedColumn {
                name: "id".into(),
                col_type: NormalizedType::I32,
                is_nullable: false,
            }],
            primary_key_columns: vec!["id".into()],
            foreign_keys: vec![],
        },
        IntrospectedTable {
            name: "posts".into(),
            columns: vec![
                IntrospectedColumn { name: "id".into(), col_type: NormalizedType::I32, is_nullable: false },
                IntrospectedColumn { name: "title".into(), col_type: NormalizedType::Str, is_nullable: false },
                IntrospectedColumn { name: "user_id".into(), col_type: NormalizedType::I32, is_nullable: false },
            ],
            primary_key_columns: vec!["id".into()],
            foreign_keys: vec![IntrospectedForeignKey {
                columns: vec!["user_id".into()],
                referenced_table: "users".into(),
                referenced_columns: vec!["id".into()],
            }],
        },
    ];

    let rels = resolve_relationships(&tables);
    let block = build_entity_block(&tables[1], &rels).unwrap();

    // Should NOT have user_id as a scalar field
    assert!(!block.fields.iter().any(|f| f.name == "user_id"));
    // Should have title as a scalar field
    assert!(block.fields.iter().any(|f| f.name == "title"));
    // Should have a BelongsTo relation
    assert_eq!(block.relations.len(), 1);
    assert_eq!(block.relations[0].name, "user");
    assert_eq!(block.relations[0].target_entity, "User");
    assert!(matches!(block.relations[0].kind, codegen::RelationFieldKind::BelongsTo));
}

#[test]
fn test_build_entity_block_nullable_fk_becomes_optional() {
    let tables = vec![
        IntrospectedTable {
            name: "users".into(),
            columns: vec![IntrospectedColumn {
                name: "id".into(),
                col_type: NormalizedType::I32,
                is_nullable: false,
            }],
            primary_key_columns: vec!["id".into()],
            foreign_keys: vec![],
        },
        IntrospectedTable {
            name: "posts".into(),
            columns: vec![
                IntrospectedColumn { name: "id".into(), col_type: NormalizedType::I32, is_nullable: false },
                IntrospectedColumn { name: "user_id".into(), col_type: NormalizedType::I32, is_nullable: true },
            ],
            primary_key_columns: vec!["id".into()],
            foreign_keys: vec![IntrospectedForeignKey {
                columns: vec!["user_id".into()],
                referenced_table: "users".into(),
                referenced_columns: vec!["id".into()],
            }],
        },
    ];

    let rels = resolve_relationships(&tables);
    let block = build_entity_block(&tables[1], &rels).unwrap();

    assert!(matches!(block.relations[0].kind, codegen::RelationFieldKind::OptionalBelongsTo));
}
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo test -p rapina-cli` — all new and existing tests pass
- [x] `cargo clippy -p rapina-cli` — no warnings

---

## Migration Notes

- **Backwards compatible**: The existing `generate_schema_block` and `update_entity_file` functions remain untouched for use by `rapina add resource` and other commands.
- **No breaking changes to CLI flags** — the `rapina import database` command signature is unchanged.
- **SQLite imports are unaffected** — they already skip FK detection (foreign_keys is always empty), so they'll produce scalar-only entity blocks with no relationships.

## References

- Existing relationship test: `rapina/tests/schema_test.rs:11-33`
- FK resolution: `rapina-cli/src/commands/import.rs:427-474`
- Current schema generation: `rapina-cli/src/commands/codegen.rs:417-451`
- Schema macro types: `rapina-macros/src/schema/types.rs:79-86`
