# migrate run commands Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `rapina migrate up|down|status|fresh|reset|init` commands that shell out to a user-owned `src/bin/rapina_migrate.rs` binary, plus scaffold that binary in `rapina new` templates.

**Architecture:** `rapina-cli` cannot link the user's `Migrator` type, so each migrate subcommand shells out to `cargo run -q --bin rapina_migrate -- <subcommand>`. The user's binary calls `rapina::migration::run_cli::<Migrator>()` which parses args, reads `DATABASE_URL`, and dispatches to sea-orm-migration. `rapina migrate init` scaffolds `src/bin/rapina_migrate.rs`; `rapina new` does the same for db-enabled templates.

**Tech Stack:** Rust, sea-orm-migration 1.x (`MigratorTrait`), tokio, `std::process::Command` for shell-out.

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `rapina/src/migration.rs` | Modify | Add `MigrateCommand`, `parse_args`, `run_cli<M>()` |
| `rapina/tests/migration_test.rs` | Modify | Add `run_cli` arg-parse + integration tests |
| `rapina-cli/src/commands/migrate.rs` | Modify | Add `init_migrate_bin`, `check_migrate_bin`, `run_migrate_cmd` |
| `rapina-cli/src/main.rs` | Modify | Add `Up`, `Down`, `Status`, `Fresh`, `Reset`, `Init` to `MigrateCommands` + handlers |
| `rapina-cli/src/commands/templates/mod.rs` | Modify | Add `generate_migrate_bin_rs()` helper |
| `rapina-cli/src/commands/templates/crud.rs` | Modify | Call `write_migrate_bin` — always (crud always has db) |
| `rapina-cli/src/commands/templates/rest_api.rs` | Modify | Call `write_migrate_bin` when `db_type.is_some()` |
| `rapina-cli/src/commands/templates/auth.rs` | Modify | Call `write_migrate_bin` when `db_type.is_some()` |

---

## Task 1: `MigrateCommand` + `parse_args` in `rapina/src/migration.rs`

**Files:**
- Modify: `rapina/src/migration.rs`
- Test: `rapina/tests/migration_test.rs`

- [ ] **Step 1: Write failing unit tests for `parse_args`**

Add to `rapina/tests/migration_test.rs` (inside a new `mod parse_args_tests` block, after existing tests):

```rust
#[cfg(feature = "sqlite")]
mod parse_args_tests {
    // parse_args is pub(crate) in the lib — test it via the re-export
    // We test the public behavior through known string slices.
    // These are plain unit tests (no async, no DB).

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_parse_up() {
        let cmd = rapina::migration::parse_args(&s(&["up"])).unwrap();
        assert_eq!(cmd, rapina::migration::MigrateCommand::Up);
    }

    #[test]
    fn test_parse_down_default_steps() {
        let cmd = rapina::migration::parse_args(&s(&["down"])).unwrap();
        assert_eq!(cmd, rapina::migration::MigrateCommand::Down { steps: 1 });
    }

    #[test]
    fn test_parse_down_with_steps() {
        let cmd = rapina::migration::parse_args(&s(&["down", "--steps", "3"])).unwrap();
        assert_eq!(cmd, rapina::migration::MigrateCommand::Down { steps: 3 });
    }

    #[test]
    fn test_parse_status() {
        let cmd = rapina::migration::parse_args(&s(&["status"])).unwrap();
        assert_eq!(cmd, rapina::migration::MigrateCommand::Status);
    }

    #[test]
    fn test_parse_fresh() {
        let cmd = rapina::migration::parse_args(&s(&["fresh"])).unwrap();
        assert_eq!(cmd, rapina::migration::MigrateCommand::Fresh);
    }

    #[test]
    fn test_parse_reset() {
        let cmd = rapina::migration::parse_args(&s(&["reset"])).unwrap();
        assert_eq!(cmd, rapina::migration::MigrateCommand::Reset);
    }

    #[test]
    fn test_parse_unknown_subcommand() {
        let err = rapina::migration::parse_args(&s(&["migrate"])).unwrap_err();
        assert!(err.contains("Unknown"));
    }

    #[test]
    fn test_parse_empty_args() {
        let err = rapina::migration::parse_args(&[]).unwrap_err();
        assert!(err.contains("Usage"));
    }

    #[test]
    fn test_parse_down_invalid_steps() {
        let err = rapina::migration::parse_args(&s(&["down", "--steps", "abc"])).unwrap_err();
        assert!(err.contains("Invalid steps"));
    }

    #[test]
    fn test_parse_down_missing_steps_value() {
        let err = rapina::migration::parse_args(&s(&["down", "--steps"])).unwrap_err();
        assert!(err.contains("--steps requires"));
    }
}
```

- [ ] **Step 2: Run tests — expect compile errors (items not yet defined)**

```bash
cargo test -p rapina --features sqlite parse_args_tests 2>&1 | head -30
```

Expected: compile errors mentioning `parse_args`, `MigrateCommand` not found.

- [ ] **Step 3: Add `MigrateCommand` and `parse_args` to `rapina/src/migration.rs`**

Add after the existing `pub use` block (after line 63) and before `run_pending`:

```rust
/// Subcommands understood by `run_cli`.
#[derive(Debug, PartialEq)]
pub enum MigrateCommand {
    Up,
    Down { steps: u32 },
    Status,
    Fresh,
    Reset,
}

/// Parse CLI args (everything after the binary name) into a `MigrateCommand`.
///
/// Expected forms:
///   up
///   down [--steps N]
///   status
///   fresh
///   reset
pub fn parse_args(args: &[String]) -> Result<MigrateCommand, String> {
    match args.first().map(|s| s.as_str()) {
        Some("up") => Ok(MigrateCommand::Up),
        Some("down") => {
            let steps = parse_steps(&args[1..])?;
            Ok(MigrateCommand::Down { steps })
        }
        Some("status") => Ok(MigrateCommand::Status),
        Some("fresh") => Ok(MigrateCommand::Fresh),
        Some("reset") => Ok(MigrateCommand::Reset),
        Some(other) => Err(format!(
            "Unknown subcommand: '{}'. Valid: up | down [--steps N] | status | fresh | reset",
            other
        )),
        None => Err(
            "No subcommand given. Usage: rapina_migrate <up|down|status|fresh|reset>".to_string(),
        ),
    }
}

fn parse_steps(args: &[String]) -> Result<u32, String> {
    if args.is_empty() {
        return Ok(1);
    }
    match args[0].as_str() {
        "--steps" => {
            if args.len() < 2 {
                return Err("--steps requires a value".to_string());
            }
            args[1]
                .parse::<u32>()
                .map_err(|_| format!("Invalid steps value: '{}'", args[1]))
        }
        other => Err(format!("Unexpected argument: '{}'", other)),
    }
}
```

- [ ] **Step 4: Run tests — expect PASS**

```bash
cargo test -p rapina --features sqlite parse_args_tests 2>&1
```

Expected: all 9 tests pass.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add rapina/src/migration.rs rapina/tests/migration_test.rs
git commit -m "feat(migration): add MigrateCommand enum and parse_args"
```

---

## Task 2: `run_cli<M>()` in `rapina/src/migration.rs`

**Files:**
- Modify: `rapina/src/migration.rs`
- Test: `rapina/tests/migration_test.rs`

- [ ] **Step 1: Write failing integration tests for `run_cli`**

Add to `rapina/tests/migration_test.rs` (after the `parse_args_tests` block):

```rust
#[cfg(feature = "sqlite")]
mod run_cli_tests {
    // run_cli reads DATABASE_URL from env and std::env::args — we can't easily inject args.
    // Test dispatch by calling the underlying functions directly, same pattern as
    // the existing migration_test.rs tests. run_cli itself is tested via CLI integration
    // (Task 3 in the plan). Here we verify the dispatch helpers compile and work.

    use rapina::migration::prelude::*;
    use rapina::sea_orm::Database;

    mod test_migration {
        use super::*;

        #[derive(DeriveMigrationName)]
        pub struct Migration;

        #[async_trait]
        impl MigrationTrait for Migration {
            async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
                manager
                    .create_table(
                        Table::create()
                            .table(RunCliTable::Table)
                            .col(
                                ColumnDef::new(RunCliTable::Id)
                                    .integer()
                                    .not_null()
                                    .auto_increment()
                                    .primary_key(),
                            )
                            .col(ColumnDef::new(RunCliTable::Name).string().not_null())
                            .to_owned(),
                    )
                    .await
            }

            async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
                manager
                    .drop_table(Table::drop().table(RunCliTable::Table).to_owned())
                    .await
            }
        }

        #[derive(DeriveIden)]
        enum RunCliTable {
            Table,
            Id,
            Name,
        }
    }

    rapina::migrations! {
        test_migration,
    }

    #[tokio::test]
    async fn test_dispatch_up() {
        let conn = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&conn, None).await.unwrap();
    }

    #[tokio::test]
    async fn test_dispatch_fresh() {
        let conn = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&conn, None).await.unwrap();
        Migrator::fresh(&conn).await.unwrap();
    }

    #[tokio::test]
    async fn test_dispatch_refresh() {
        let conn = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&conn, None).await.unwrap();
        Migrator::refresh(&conn).await.unwrap();
    }

    #[tokio::test]
    async fn test_dispatch_down() {
        let conn = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&conn, None).await.unwrap();
        Migrator::down(&conn, Some(1)).await.unwrap();
    }

    #[tokio::test]
    async fn test_dispatch_status() {
        let conn = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::status(&conn).await.unwrap();
    }
}
```

- [ ] **Step 2: Run tests — expect PASS (all dispatch through MigratorTrait)**

```bash
cargo test -p rapina --features sqlite run_cli_tests 2>&1
```

Expected: 5 tests pass. This confirms the MigratorTrait methods (`fresh`, `refresh`) exist.

- [ ] **Step 3: Add `run_cli<M>()` to `rapina/src/migration.rs`**

Add at the bottom of the file (after `status`):

```rust
/// Entry point for the `rapina_migrate` binary.
///
/// Reads `DATABASE_URL` from the environment, parses `std::env::args()`,
/// connects to the database, and dispatches the requested migration command.
///
/// # Usage (in `src/bin/rapina_migrate.rs`)
/// ```rust,ignore
/// #[path = "../migrations/mod.rs"]
/// mod migrations;
///
/// #[tokio::main]
/// async fn main() {
///     rapina::migration::run_cli::<migrations::Migrator>().await;
/// }
/// ```
#[cfg(feature = "database")]
pub async fn run_cli<M: MigratorTrait>() {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();

    let db_url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("Error: DATABASE_URL environment variable is not set.");
            eprintln!("       Set it in your .env file or export it before running.");
            std::process::exit(1);
        }
    };

    let cmd = match parse_args(&raw_args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    let conn = match sea_orm::Database::connect(&db_url).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: Could not connect to database: {e}");
            std::process::exit(1);
        }
    };

    let result: Result<(), sea_orm::DbErr> = match &cmd {
        MigrateCommand::Up => M::up(&conn, None).await,
        MigrateCommand::Down { steps } => M::down(&conn, Some(*steps)).await,
        MigrateCommand::Status => M::status(&conn).await,
        MigrateCommand::Fresh => M::fresh(&conn).await,
        MigrateCommand::Reset => M::refresh(&conn).await,
    };

    match result {
        Ok(()) => match cmd {
            MigrateCommand::Up => println!("Migrations applied successfully."),
            MigrateCommand::Down { steps } => {
                println!("Rolled back {steps} migration(s).")
            }
            MigrateCommand::Status => {}
            MigrateCommand::Fresh => {
                println!("Database cleared and migrations re-applied (fresh).")
            }
            MigrateCommand::Reset => println!("Migrations reset (down all + up all)."),
        },
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}
```

**Note:** `run_cli` is gated on `#[cfg(feature = "database")]` because it uses `sea_orm::Database`. `parse_args` and `MigrateCommand` have no feature gate (they are pure logic).

- [ ] **Step 4: Verify compile**

```bash
cargo build -p rapina --features sqlite 2>&1
```

Expected: compiles without errors.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add rapina/src/migration.rs rapina/tests/migration_test.rs
git commit -m "feat(migration): add run_cli<M>() for migrate binary dispatch"
```

---

## Task 3: `init_migrate_bin`, `check_migrate_bin`, `run_migrate_cmd` in `rapina-cli`

**Files:**
- Modify: `rapina-cli/src/commands/migrate.rs`

- [ ] **Step 1: Write failing tests**

Add at the bottom of `rapina-cli/src/commands/migrate.rs` (inside existing `#[cfg(test)] mod tests`):

```rust
    #[test]
    fn test_generate_migrate_bin_content() {
        let content = generate_migrate_bin_rs();
        assert!(content.contains("#[path = \"../migrations/mod.rs\"]"));
        assert!(content.contains("mod migrations"));
        assert!(content.contains("rapina::migration::run_cli::<migrations::Migrator>()"));
        assert!(content.contains("#[tokio::main]"));
    }

    #[test]
    fn test_check_migrate_bin_missing() {
        let dir = tempfile::tempdir().unwrap();
        let err = check_migrate_bin(dir.path()).unwrap_err();
        assert!(err.contains("rapina migrate init"));
    }

    #[test]
    fn test_check_migrate_bin_present() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("src").join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join("rapina_migrate.rs"), "fn main() {}").unwrap();
        assert!(check_migrate_bin(dir.path()).is_ok());
    }

    #[test]
    fn test_init_migrate_bin_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src").join("migrations")).unwrap();
        init_migrate_bin(dir.path()).unwrap();
        let bin_path = dir.path().join("src").join("bin").join("rapina_migrate.rs");
        assert!(bin_path.exists());
        let content = std::fs::read_to_string(bin_path).unwrap();
        assert!(content.contains("run_cli"));
    }

    #[test]
    fn test_init_migrate_bin_already_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src").join("bin")).unwrap();
        let bin_path = dir.path().join("src").join("bin").join("rapina_migrate.rs");
        std::fs::write(&bin_path, "existing").unwrap();
        let err = init_migrate_bin(dir.path()).unwrap_err();
        assert!(err.contains("already exists"));
    }
```

- [ ] **Step 2: Run tests — expect compile errors**

```bash
cargo test -p rapina-cli 2>&1 | grep -E "error|FAILED" | head -20
```

Expected: errors for undefined `generate_migrate_bin_rs`, `check_migrate_bin`, `init_migrate_bin`.

- [ ] **Step 3: Implement the three functions in `rapina-cli/src/commands/migrate.rs`**

Add after the existing `add_to_migrations_macro` function and before the `#[cfg(test)]` block:

```rust
/// Content for `src/bin/rapina_migrate.rs`.
pub(crate) fn generate_migrate_bin_rs() -> String {
    r#"//! Migration runner — generated by `rapina migrate init`.
//! Apply, roll back, or inspect migrations without touching the web server.
//!
//! Run via the Rapina CLI:
//!   rapina migrate up
//!   rapina migrate down [--steps N]
//!   rapina migrate status
//!   rapina migrate fresh
//!   rapina migrate reset

#[path = "../migrations/mod.rs"]
mod migrations;

#[tokio::main]
async fn main() {
    rapina::migration::run_cli::<migrations::Migrator>().await;
}
"#
    .to_string()
}

/// Returns `Ok(())` if `src/bin/rapina_migrate.rs` exists relative to `project_root`,
/// or an error with instructions to run `rapina migrate init`.
pub(crate) fn check_migrate_bin(project_root: &std::path::Path) -> Result<(), String> {
    let bin_path = project_root
        .join("src")
        .join("bin")
        .join("rapina_migrate.rs");
    if bin_path.exists() {
        Ok(())
    } else {
        Err(
            "migrate binary not found (src/bin/rapina_migrate.rs).\n\
             Run 'rapina migrate init' to set it up for this project."
                .to_string(),
        )
    }
}

/// Create `src/bin/rapina_migrate.rs` in `project_root`.
/// Returns an error if the file already exists.
pub fn init_migrate_bin(project_root: &std::path::Path) -> Result<(), String> {
    let bin_dir = project_root.join("src").join("bin");
    fs::create_dir_all(&bin_dir)
        .map_err(|e| format!("Failed to create src/bin/: {e}"))?;

    let bin_path = bin_dir.join("rapina_migrate.rs");
    if bin_path.exists() {
        return Err(format!(
            "src/bin/rapina_migrate.rs already exists. Remove it first if you want to regenerate."
        ));
    }

    fs::write(&bin_path, generate_migrate_bin_rs())
        .map_err(|e| format!("Failed to write rapina_migrate.rs: {e}"))?;
    println!(
        "  {} Created {}",
        "✓".green(),
        "src/bin/rapina_migrate.rs".cyan()
    );
    println!();
    println!("  Run migrations with:");
    println!("    rapina migrate up");
    println!("    rapina migrate down [--steps N]");
    println!("    rapina migrate status");
    println!("    rapina migrate fresh");
    println!("    rapina migrate reset");
    println!();
    Ok(())
}

/// Shell out to `cargo run -q --bin rapina_migrate -- <args>`.
///
/// Inherits the current process environment (including DATABASE_URL loaded from .env).
/// Streams stdout/stderr directly to the terminal.
pub fn run_migrate_cmd(subcommand_args: &[&str]) -> Result<(), String> {
    check_migrate_bin(std::path::Path::new("."))?;

    let status = std::process::Command::new("cargo")
        .args(["run", "-q", "--bin", "rapina_migrate", "--"])
        .args(subcommand_args)
        .status()
        .map_err(|e| format!("Failed to run cargo: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "rapina_migrate exited with status {}",
            status.code().unwrap_or(-1)
        ))
    }
}
```

- [ ] **Step 4: Run tests — expect PASS**

```bash
cargo test -p rapina-cli 2>&1
```

Expected: all tests pass, including the 4 new ones.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add rapina-cli/src/commands/migrate.rs
git commit -m "feat(cli): add init_migrate_bin, check_migrate_bin, run_migrate_cmd"
```

---

## Task 4: Wire new subcommands in `rapina-cli/src/main.rs`

**Files:**
- Modify: `rapina-cli/src/main.rs`

- [ ] **Step 1: Extend `MigrateCommands` enum**

In `rapina-cli/src/main.rs`, find:

```rust
#[derive(Subcommand)]
enum MigrateCommands {
    /// Generate a new migration file
    New {
        /// Name of the migration (e.g., create_users)
        name: String,
    },
}
```

Replace with:

```rust
#[derive(Subcommand)]
enum MigrateCommands {
    /// Generate a new migration file
    New {
        /// Name of the migration (e.g., create_users)
        name: String,
    },
    /// Set up the migrate binary for this project (creates src/bin/rapina_migrate.rs)
    Init,
    /// Apply all pending migrations
    Up,
    /// Roll back migrations (default: 1 step)
    Down {
        /// Number of migrations to roll back
        #[arg(long, default_value = "1")]
        steps: u32,
    },
    /// Show applied and pending migrations
    Status,
    /// Drop all tables and re-run all migrations (destructive)
    Fresh,
    /// Roll back all migrations then re-apply them
    Reset,
}
```

- [ ] **Step 2: Wire the new arms in the `Migrate` match block**

Find in `main`:

```rust
        Some(Commands::Migrate { command }) => {
            let result = match command {
                MigrateCommands::New { name } => commands::migrate::new_migration(&name),
            };
            if let Err(e) = result {
                eprintln!("{} {}", "Error:".red().bold(), e);
                std::process::exit(1);
            }
        }
```

Replace with:

```rust
        Some(Commands::Migrate { command }) => {
            let result = match command {
                MigrateCommands::New { name } => commands::migrate::new_migration(&name),
                MigrateCommands::Init => {
                    commands::migrate::init_migrate_bin(std::path::Path::new("."))
                }
                MigrateCommands::Up => commands::migrate::run_migrate_cmd(&["up"]),
                MigrateCommands::Down { steps } => {
                    let steps_str = steps.to_string();
                    commands::migrate::run_migrate_cmd(&["down", "--steps", &steps_str])
                }
                MigrateCommands::Status => commands::migrate::run_migrate_cmd(&["status"]),
                MigrateCommands::Fresh => commands::migrate::run_migrate_cmd(&["fresh"]),
                MigrateCommands::Reset => commands::migrate::run_migrate_cmd(&["reset"]),
            };
            if let Err(e) = result {
                eprintln!("{} {}", "Error:".red().bold(), e);
                std::process::exit(1);
            }
        }
```

- [ ] **Step 3: Build to verify**

```bash
cargo build -p rapina-cli 2>&1
```

Expected: compiles without errors or warnings.

- [ ] **Step 4: Smoke test help output**

```bash
cargo run -p rapina-cli -- migrate --help 2>&1
```

Expected output includes: `up`, `down`, `status`, `fresh`, `reset`, `init`, `new`.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add rapina-cli/src/main.rs
git commit -m "feat(cli): wire migrate up|down|status|fresh|reset|init subcommands"
```

---

## Task 5: Add `generate_migrate_bin_rs` helper and scaffold in templates

**Files:**
- Modify: `rapina-cli/src/commands/templates/mod.rs`
- Modify: `rapina-cli/src/commands/templates/crud.rs`
- Modify: `rapina-cli/src/commands/templates/rest_api.rs`
- Modify: `rapina-cli/src/commands/templates/auth.rs`

- [ ] **Step 1: Write failing tests for template scaffolding**

Add to the `#[cfg(test)] mod tests` block in `rapina-cli/src/commands/templates/mod.rs`:

```rust
    #[test]
    fn test_generate_migrate_bin_rs_content() {
        let content = generate_migrate_bin_rs();
        assert!(content.contains("#[path = \"../migrations/mod.rs\"]"));
        assert!(content.contains("mod migrations"));
        assert!(content.contains("run_cli::<migrations::Migrator>()"));
        assert!(content.contains("#[tokio::main]"));
    }
```

Add to `rapina-cli/src/commands/templates/crud.rs` tests (add a new `#[cfg(test)]` block at the bottom if one doesn't exist — check first):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_crud_generate_creates_migrate_bin() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        generate(
            "myapp",
            dir.path(),
            &src,
            &crate::commands::templates::DatabaseType::Sqlite,
        )
        .unwrap();
        assert!(src.join("bin").join("rapina_migrate.rs").exists());
    }
}
```

Add to `rapina-cli/src/commands/templates/rest_api.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_rest_api_with_db_creates_migrate_bin() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        generate(
            "myapp",
            dir.path(),
            &src,
            Some(&crate::commands::templates::DatabaseType::Sqlite),
        )
        .unwrap();
        assert!(src.join("bin").join("rapina_migrate.rs").exists());
    }

    #[test]
    fn test_rest_api_without_db_no_migrate_bin() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        generate("myapp", dir.path(), &src, None).unwrap();
        assert!(!src.join("bin").join("rapina_migrate.rs").exists());
    }
}
```

Add to `rapina-cli/src/commands/templates/auth.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_auth_with_db_creates_migrate_bin() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        generate(
            "myapp",
            dir.path(),
            &src,
            Some(&crate::commands::templates::DatabaseType::Sqlite),
        )
        .unwrap();
        assert!(src.join("bin").join("rapina_migrate.rs").exists());
    }

    #[test]
    fn test_auth_without_db_no_migrate_bin() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        generate("myapp", dir.path(), &src, None).unwrap();
        assert!(!src.join("bin").join("rapina_migrate.rs").exists());
    }
}
```

- [ ] **Step 2: Run tests — expect compile errors**

```bash
cargo test -p rapina-cli 2>&1 | grep -E "error\[|FAILED" | head -20
```

Expected: errors for undefined `generate_migrate_bin_rs` in templates/mod.rs, and test failures for missing bin files.

- [ ] **Step 3: Add `generate_migrate_bin_rs` to `rapina-cli/src/commands/templates/mod.rs`**

Add at the end of the public functions section (before the `#[cfg(test)]` block):

```rust
/// Generate the content for `src/bin/rapina_migrate.rs`.
pub fn generate_migrate_bin_rs() -> String {
    crate::commands::migrate::generate_migrate_bin_rs()
}

/// Write `src/bin/rapina_migrate.rs` into `src_path/../bin/`.
/// `src_path` is the `src/` directory of the generated project.
pub fn write_migrate_bin(src_path: &Path) -> Result<(), String> {
    let bin_dir = src_path.join("bin");
    fs::create_dir_all(&bin_dir)
        .map_err(|e| format!("Failed to create src/bin/: {e}"))?;
    write_file(
        &bin_dir.join("rapina_migrate.rs"),
        &generate_migrate_bin_rs(),
        "src/bin/rapina_migrate.rs",
    )
}
```

- [ ] **Step 4: Call `write_migrate_bin` in `crud.rs`**

In `rapina-cli/src/commands/templates/crud.rs`, update `generate` function. After the `write_file` call for `src/migrations/m20240101_000001_create_items.rs` (around line 51), add:

```rust
    write_migrate_bin(src_path)?;
```

Also add to the imports at the top of the file:

```rust
use super::write_migrate_bin;
```

- [ ] **Step 5: Call `write_migrate_bin` in `rest_api.rs`**

In `rapina-cli/src/commands/templates/rest_api.rs`, update `generate`. After the `.env` write block (after the `if let Some(db)` block), add:

```rust
    if db_type.is_some() {
        write_migrate_bin(src_path)?;
    }
```

Also add `write_migrate_bin` to the `use super::` import.

- [ ] **Step 6: Call `write_migrate_bin` in `auth.rs`**

In `rapina-cli/src/commands/templates/auth.rs`, update `generate`. After the `.env` write block, add:

```rust
    if db_type.is_some() {
        write_migrate_bin(src_path)?;
    }
```

Also add `write_migrate_bin` to the `use super::` import.

- [ ] **Step 7: Run all tests**

```bash
cargo test -p rapina-cli 2>&1
```

Expected: all tests pass, including the new template tests.

- [ ] **Step 8: Run all rapina lib tests**

```bash
cargo test -p rapina --features sqlite 2>&1
```

Expected: all tests pass.

- [ ] **Step 9: Commit**

```bash
cargo fmt --all
git add rapina-cli/src/commands/templates/mod.rs \
        rapina-cli/src/commands/templates/crud.rs \
        rapina-cli/src/commands/templates/rest_api.rs \
        rapina-cli/src/commands/templates/auth.rs
git commit -m "feat(templates): scaffold src/bin/rapina_migrate.rs in db-enabled templates"
```

---

## Self-Review

### Spec coverage

| Spec requirement | Task |
|---|---|
| `rapina migrate up/down/status/fresh/reset` | Task 4 |
| `rapina::migration::run_cli<M>()` | Task 2 |
| `rapina migrate init` scaffolds bin | Task 3 |
| Shell-out to `cargo run --bin rapina_migrate` | Task 3 |
| Error if bin missing | Task 3 (`check_migrate_bin`) |
| `rapina new` crud scaffolds bin | Task 5 |
| `rapina new` rest-api with --db scaffolds bin | Task 5 |
| `rapina new` auth with --db scaffolds bin | Task 5 |
| `run_migrations()` startup untouched | No change needed ✓ |
| Tests | Tasks 1, 2, 3, 5 |

### Type consistency

- `MigrateCommand::Down { steps: u32 }` defined Task 1, used Task 2 ✓
- `check_migrate_bin(project_root: &Path)` defined Task 3, used Task 4 ✓
- `run_migrate_cmd(subcommand_args: &[&str])` defined Task 3, used Task 4 ✓
- `generate_migrate_bin_rs()` defined in `migrate.rs` Task 3, re-exported via `templates/mod.rs` Task 5 ✓
- `write_migrate_bin(src_path: &Path)` defined Task 5 mod.rs, called in crud/rest_api/auth Task 5 ✓

### No placeholders: confirmed none.
