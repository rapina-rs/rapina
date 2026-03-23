# `rapina import database --force` Implementation Plan (TDD)

## Overview

Add a `--force` flag to `rapina import database` that allows re-importing tables when generated files already exist. Currently the command hard-errors if `src/<plural>/` exists. With `--force`, it removes existing generated files and re-creates them from the live database schema.

## Current State Analysis

- `create_feature_module()` returns `Err` if `src/<plural>/` directory exists (`codegen.rs:504-508`)
- `update_entity_file()` always appends a new `schema!` block — re-import duplicates it (`codegen.rs:437-456`)
- `create_migration_file()` uses timestamps — no collision possible (`codegen.rs:476`)
- `migrations/mod.rs` gets a new `mod` line and `migrations!` entry each time — re-import duplicates these (`migrate.rs:94-129`)
- No `--force` flag exists anywhere on the `Database` subcommand

### Key Discoveries:
- Feature module guard: `codegen.rs:504-508`
- Entity append logic: `codegen.rs:437-456`
- Migration mod.rs update: `migrate.rs:94-129`
- CLI definition: `main.rs:119-129`
- Dispatch: `main.rs:267-283`
- Entry point: `import.rs:586-698`

## Desired End State

`rapina import database --url <url> --force` re-imports all (or filtered) tables cleanly:
1. Existing `src/<plural>/` directories are removed and re-created
2. Existing `schema!` blocks in `entity.rs` for the same entity are replaced (not duplicated)
3. Duplicate migration module entries in `migrations/mod.rs` are avoided
4. Without `--force`, behavior is unchanged (hard error on existing directory)

### Verification:
- `cargo test --features import -p rapina-cli` passes
- `cargo clippy -p rapina-cli --features import` clean
- Manual: run `rapina import database` twice — first succeeds, second errors; with `--force`, second succeeds and files are correct

## What We're NOT Doing

- No `--force` for `import openapi` (separate issue)
- No `--dry-run` for database import (separate feature)
- No interactive confirmation prompt before force-delete
- No backup of overwritten files

## Implementation Approach

TDD: write failing tests first, then implement to make them pass.

---

## Phase 1: Tests for `remove_schema_block`

### Overview
Write tests for a new `remove_schema_block(content, entity_name) -> String` function in `codegen.rs` that strips an existing `schema! { ... EntityName { ... } }` block from entity.rs content.

### Changes Required:

#### 1. Add test cases in `codegen.rs`
**File**: `rapina-cli/src/commands/codegen.rs`
**Changes**: Add tests to the existing `#[cfg(test)] mod tests` block

```rust
#[test]
fn test_remove_schema_block_removes_matching_entity() {
    let content = r#"use rapina::prelude::*;

schema! {
    Post {
        title: String,
    }
}

schema! {
    Comment {
        body: String,
    }
}
"#;
    let result = remove_schema_block(content, "Post");
    assert!(!result.contains("Post {"));
    assert!(result.contains("Comment {"));
    assert!(result.contains("schema! {"));
}

#[test]
fn test_remove_schema_block_no_match_returns_unchanged() {
    let content = r#"use rapina::prelude::*;

schema! {
    Post {
        title: String,
    }
}
"#;
    let result = remove_schema_block(content, "User");
    assert_eq!(result.trim(), content.trim());
}

#[test]
fn test_remove_schema_block_with_attributes() {
    let content = r#"use rapina::prelude::*;

schema! {
    #[primary_key(user_id, role_id)]
    #[timestamps(none)]
    UsersRole {
        user_id: i32,
        role_id: i32,
    }
}
"#;
    let result = remove_schema_block(content, "UsersRole");
    assert!(!result.contains("UsersRole"));
    assert!(!result.contains("schema!"));
}
```

#### 2. Add stub function
**File**: `rapina-cli/src/commands/codegen.rs`

```rust
pub(crate) fn remove_schema_block(content: &str, entity_name: &str) -> String {
    todo!()
}
```

### Success Criteria:

#### Automated Verification:
- [x] Tests compile: `cargo test --features import -p rapina-cli --no-run`
- [x] Tests fail (red): `cargo test --features import -p rapina-cli -- test_remove_schema_block` should show 3 failures

**Implementation Note**: Pause here — confirm tests fail before proceeding to Phase 2.

---

## Phase 2: Implement `remove_schema_block`

### Overview
Implement the function to make Phase 1 tests pass.

### Changes Required:

#### 1. Implement `remove_schema_block`
**File**: `rapina-cli/src/commands/codegen.rs`

The function needs to find `schema! {` blocks that contain `EntityName {` and remove the entire block (from `schema!` to the closing `}`). Strategy: iterate lines, track brace depth when inside a `schema!` block, and skip lines belonging to a block that matches `entity_name`.

```rust
pub(crate) fn remove_schema_block(content: &str, entity_name: &str) -> String {
    let mut result = String::new();
    let mut lines = content.lines().peekable();
    let entity_pattern = format!("{} {{", entity_name);

    while let Some(line) = lines.next() {
        if line.trim_start().starts_with("schema! {") {
            // Collect the entire schema block
            let mut block_lines = vec![line.to_string()];
            let mut depth: i32 = line.matches('{').count() as i32
                - line.matches('}').count() as i32;

            while depth > 0 {
                if let Some(next) = lines.next() {
                    depth += next.matches('{').count() as i32
                        - next.matches('}').count() as i32;
                    block_lines.push(next.to_string());
                } else {
                    break;
                }
            }

            // Check if this block contains our entity
            let block_text = block_lines.join("\n");
            if !block_text.contains(&entity_pattern) {
                result.push_str(&block_text);
                result.push('\n');
            }
            // else: skip the block entirely
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }

    // Clean up excessive blank lines left behind
    while result.contains("\n\n\n") {
        result = result.replace("\n\n\n", "\n\n");
    }

    result
}
```

### Success Criteria:

#### Automated Verification:
- [x] All 3 `remove_schema_block` tests pass: `cargo test --features import -p rapina-cli -- test_remove_schema_block`

**Implementation Note**: Pause for confirmation before proceeding.

---

## Phase 3: Tests for force behavior in `create_feature_module` and `update_entity_file`

### Overview
Write tests that verify force=true overwrites existing directories and deduplicates entity blocks.

### Changes Required:

#### 1. Tests for `create_feature_module` with force
**File**: `rapina-cli/src/commands/codegen.rs`

```rust
#[test]
fn test_create_feature_module_errors_without_force_when_exists() {
    let dir = tempfile::tempdir().unwrap();
    let module_dir = dir.path().join("users");
    fs::create_dir_all(&module_dir).unwrap();
    fs::write(module_dir.join("mod.rs"), "old content").unwrap();

    let fields = vec![FieldInfo {
        name: "email".to_string(),
        rust_type: "String".to_string(),
        schema_type: "String".to_string(),
        column_method: String::new(),
        nullable: false,
    }];

    let result = create_feature_module_in("user", "users", "User", &fields, "i32", false, dir.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("already exists"));
}

#[test]
fn test_create_feature_module_overwrites_with_force() {
    let dir = tempfile::tempdir().unwrap();
    let module_dir = dir.path().join("users");
    fs::create_dir_all(&module_dir).unwrap();
    fs::write(module_dir.join("mod.rs"), "old content").unwrap();

    let fields = vec![FieldInfo {
        name: "email".to_string(),
        rust_type: "String".to_string(),
        schema_type: "String".to_string(),
        column_method: String::new(),
        nullable: false,
    }];

    let result = create_feature_module_in("user", "users", "User", &fields, "i32", true, dir.path());
    assert!(result.is_ok());
    let mod_content = fs::read_to_string(module_dir.join("mod.rs")).unwrap();
    assert!(mod_content.contains("pub mod"));
}
```

#### 2. Tests for `update_entity_file` with force (dedup)
**File**: `rapina-cli/src/commands/codegen.rs`

```rust
#[test]
fn test_update_entity_file_deduplicates_with_force() {
    let dir = tempfile::tempdir().unwrap();
    let entity_path = dir.path().join("entity.rs");
    fs::write(&entity_path, r#"use rapina::prelude::*;

schema! {
    Post {
        title: String,
    }
}
"#).unwrap();

    let fields = vec![FieldInfo {
        name: "title".to_string(),
        rust_type: "String".to_string(),
        schema_type: "String".to_string(),
        column_method: String::new(),
        nullable: false,
    }];

    update_entity_file_in("Post", &fields, None, None, true, &entity_path).unwrap();
    let content = fs::read_to_string(&entity_path).unwrap();
    // Should have exactly one schema! block for Post, not two
    assert_eq!(content.matches("Post {").count(), 1);
    assert!(content.contains("schema! {"));
}
```

### Notes
These tests require refactored function signatures that accept a base path (for testability). The current functions use hardcoded `Path::new("src/...")`. We'll add `_in` variants that accept a base path, and have the original functions delegate to them.

### Success Criteria:

#### Automated Verification:
- [x] Tests compile: `cargo test --features import -p rapina-cli --no-run`
- [x] Tests fail (red): the `_in` functions don't exist yet or have `todo!()`

**Implementation Note**: Pause here — confirm tests fail before proceeding.

---

## Phase 4: Implement force behavior

### Overview
Add `force: bool` parameter through the call chain and implement the overwrite/dedup logic.

### Changes Required:

#### 1. CLI flag
**File**: `rapina-cli/src/main.rs`

Add `--force` to the `Database` variant:
```rust
Database {
    #[arg(long, env = "DATABASE_URL")]
    url: String,
    #[arg(long, value_delimiter = ',')]
    tables: Option<Vec<String>>,
    #[arg(long)]
    schema: Option<String>,
    /// Overwrite existing files (useful for re-importing after schema changes)
    #[arg(long)]
    force: bool,
},
```

Update dispatch to pass `force`:
```rust
ImportCommands::Database { url, tables, schema, force } => {
    commands::import::database(&url, tables.as_deref(), schema.as_deref(), force)
}
```

#### 2. Thread `force` through import.rs
**File**: `rapina-cli/src/commands/import.rs`

- `database()` signature: add `force: bool` parameter
- Pass `force` to `generate_for_table()`
- `generate_for_table()` signature: add `force: bool`
- Pass `force` to `codegen::update_entity_file()` and `codegen::create_feature_module()`

#### 3. Implement `create_feature_module` force logic
**File**: `rapina-cli/src/commands/codegen.rs`

Add internal `create_feature_module_in()` that accepts `force: bool` and `base: &Path`:

```rust
pub(crate) fn create_feature_module(
    singular: &str, plural: &str, pascal: &str,
    fields: &[FieldInfo], pk_type: &str, force: bool,
) -> Result<(), String> {
    create_feature_module_in(singular, plural, pascal, fields, pk_type, force, Path::new("src"))
}

fn create_feature_module_in(
    singular: &str, plural: &str, pascal: &str,
    fields: &[FieldInfo], pk_type: &str, force: bool, base: &Path,
) -> Result<(), String> {
    let module_dir = base.join(plural);

    if module_dir.exists() {
        if !force {
            return Err(format!(
                "Directory 'src/{}/' already exists. Remove it first, choose a different resource name, or use --force to overwrite.",
                plural
            ));
        }
        fs::remove_dir_all(&module_dir)
            .map_err(|e| format!("Failed to remove existing directory: {}", e))?;
        println!("  {} Removed existing {}", "↻".yellow(), format!("src/{}/", plural).cyan());
    }

    // ... rest of existing creation logic, using `module_dir` ...
}
```

#### 4. Implement `update_entity_file` force logic
**File**: `rapina-cli/src/commands/codegen.rs`

Add internal `update_entity_file_in()` that accepts `force: bool` and entity path:

```rust
pub(crate) fn update_entity_file(
    pascal: &str, fields: &[FieldInfo],
    timestamps: Option<&str>, primary_key: Option<&[String]>, force: bool,
) -> Result<(), String> {
    update_entity_file_in(pascal, fields, timestamps, primary_key, force, Path::new("src/entity.rs"))
}

fn update_entity_file_in(
    pascal: &str, fields: &[FieldInfo],
    timestamps: Option<&str>, primary_key: Option<&[String]>,
    force: bool, entity_path: &Path,
) -> Result<(), String> {
    let schema_block = generate_schema_block(pascal, fields, timestamps, primary_key);

    if entity_path.exists() {
        let mut content = fs::read_to_string(entity_path)
            .map_err(|e| format!("Failed to read entity.rs: {}", e))?;

        if force {
            content = remove_schema_block(&content, pascal);
        }

        let needs_import =
            !content.contains("use rapina::prelude::*") && !content.contains("use rapina::schema");
        let prefix = if needs_import { "use rapina::schema;\n" } else { "" };

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

#### 5. Handle duplicate migration mod entries
**File**: `rapina-cli/src/commands/migrate.rs`

In `update_mod_rs`, before adding a new `mod <name>;` line, check if a `mod m..._create_<plural>;` already exists. If force, skip adding a duplicate module declaration and migrations! entry.

### Success Criteria:

#### Automated Verification:
- [x] All Phase 1-3 tests pass (green): `cargo test --features import -p rapina-cli`
- [x] Clippy clean: `cargo clippy -p rapina-cli --features import`
- [x] Build succeeds: `cargo build -p rapina-cli --features import`

#### Manual Verification:
- [ ] `rapina import database --url <url>` without `--force` still errors on existing dirs
- [ ] `rapina import database --url <url> --force` re-imports cleanly
- [ ] `entity.rs` has no duplicate `schema!` blocks after re-import
- [ ] `--help` shows the new `--force` flag with description

**Implementation Note**: After completing this phase and all automated verification passes, pause for manual testing.

---

## Testing Strategy

### Unit Tests (all in `codegen.rs::tests`):
- `remove_schema_block` — match, no-match, with attributes
- `create_feature_module_in` — error without force, overwrite with force
- `update_entity_file_in` — dedup with force

### Integration-level (manual):
- Full round-trip: import → re-import with `--force` against a real database
- Verify generated code compiles

## References

- Issue context: Follow-up from #170 / #240
- Feature module guard: `rapina-cli/src/commands/codegen.rs:504-508`
- Entity append: `rapina-cli/src/commands/codegen.rs:437-456`
- CLI definition: `rapina-cli/src/main.rs:119-129`
- Entry point: `rapina-cli/src/commands/import.rs:586-698`
