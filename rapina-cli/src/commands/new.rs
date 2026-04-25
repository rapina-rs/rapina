//! Implementation of the `rapina new` command.

use colored::Colorize;
use std::fs;
use std::path::Path;

use super::agents::{AgentsFlags, generate_agents_md, generate_claude_md, generate_rapina_docs};
use super::templates;
use super::templates::DatabaseType;

/// Controls which AI assistant files `rapina new` generates.
///
/// All flags are additive on top of the default (generate everything).
/// `no_ai` takes precedence over all others and skips every AI file.
pub struct AiOptions {
    /// Skip all AI files: `AGENTS.md`, `CLAUDE.md`, `.cursor/rules`, `.rapina-docs/`.
    pub no_ai: bool,
    /// Skip `AGENTS.md` and `CLAUDE.md` only. `.cursor/rules` and `.rapina-docs/` are still generated.
    pub no_agents_md: bool,
    /// Skip `.rapina-docs/` only. `AGENTS.md` and `CLAUDE.md` are still generated.
    pub no_bundled_docs: bool,
    /// Generate `AGENTS.md` and `CLAUDE.md` but skip `.rapina-docs/` and `.cursor/rules`.
    /// Use this when you maintain your own bundled docs or point agents at an external source.
    pub agents_md_only: bool,
}

/// Execute the `new` command to create a new Rapina project.
///
/// `template` is `None` for the default starter and `Some("crud")` / `Some("auth")`
/// for the optional starter templates.
/// `db_type` specifies the database to configure for the project.
pub fn execute(
    name: &str,
    template: Option<&str>,
    db_type: Option<&DatabaseType>,
    opts: AiOptions,
) -> Result<(), String> {
    validate_project_name(name)?;

    let project_path = Path::new(name);
    if project_path.exists() {
        return Err(format!("Directory '{}' already exists", name));
    }

    println!();
    println!(
        "  {} {}",
        "Creating new Rapina project:".bright_cyan(),
        name.bold()
    );
    if let Some(ref db) = db_type {
        println!(
            "  {} Database: {}",
            "📦".bright_cyan(),
            db.to_string().bold()
        );
    }
    println!();

    let src_path = project_path.join("src");
    fs::create_dir_all(&src_path).map_err(|e| format!("Failed to create directory: {}", e))?;

    match template {
        None | Some("rest-api") => {
            templates::rest_api::generate(name, project_path, &src_path, db_type)?
        }
        Some("crud") => {
            // Clap validation ensures --db is present for crud template
            // Safe to unwrap because it has been validated in clap
            templates::crud::generate(name, project_path, &src_path, db_type.unwrap())?
        }
        Some("auth") => templates::auth::generate(name, project_path, &src_path, db_type)?,
        _ => unreachable!(),
    }

    // Create README.md
    let readme = generate_readme(name, db_type);
    fs::write(project_path.join("README.md"), readme)
        .map_err(|e| format!("Failed to write README.md: {}", e))?;
    println!("  {} Created {}", "✓".green(), "README.md".cyan());

    // Create AI assistant config files
    if !opts.no_ai {
        let flags = AgentsFlags {
            with_db: db_type.is_some(),
            with_websocket: false,
            with_jobs: false,
        };

        let write_agents = !opts.no_agents_md;
        let write_docs = !opts.no_bundled_docs && !opts.agents_md_only;
        let write_cursor = !opts.agents_md_only;

        if write_agents {
            fs::write(project_path.join("AGENTS.md"), generate_agents_md(&flags))
                .map_err(|e| format!("Failed to write AGENTS.md: {}", e))?;
            println!("  {} Created {}", "✓".green(), "AGENTS.md".cyan());

            fs::write(project_path.join("CLAUDE.md"), generate_claude_md())
                .map_err(|e| format!("Failed to write CLAUDE.md: {}", e))?;
            println!("  {} Created {}", "✓".green(), "CLAUDE.md".cyan());
        }

        if write_cursor {
            let cursor_dir = project_path.join(".cursor");
            fs::create_dir_all(&cursor_dir)
                .map_err(|e| format!("Failed to create .cursor/: {}", e))?;
            fs::write(cursor_dir.join("rules"), generate_cursor_rules())
                .map_err(|e| format!("Failed to write .cursor/rules: {}", e))?;
            println!("  {} Created {}", "✓".green(), ".cursor/rules".cyan());
        }

        if write_docs {
            generate_rapina_docs(project_path, &flags)?;
            println!("  {} Created {}", "✓".green(), ".rapina-docs/".cyan());
        }
    }

    println!();
    println!("  {} Project created successfully!", "🎉".bold());
    println!();
    println!("  {}:", "Next steps".bright_yellow());
    println!("    cd {}", name.cyan());
    if db_type.is_some() {
        println!("    # Configure your database URL in .env or source");
        println!("    export DATABASE_URL=\"your-database-url\"");
    }
    println!("    rapina dev");
    println!();

    Ok(())
}

// ── README ───────────────────────────────────────────────────────────────────

fn generate_readme(name: &str, db_type: Option<&DatabaseType>) -> String {
    let db_section = if let Some(db) = db_type {
        match db {
            DatabaseType::Sqlite => {
                r#"
## Database

This project uses **SQLite** for data persistence. The database file is created automatically at `app.db`.

To configure a different SQLite database or adjust connection pool settings, edit `src/main.rs`:

```rust
.with_database(DatabaseConfig::new("sqlite://app.db?mode=rwc"))
```

Run migrations:
```bash
rapina migrate new create_your_table
```
"#
            }
            DatabaseType::Postgres => {
                r#"
## Database

This project is configured for **PostgreSQL**. Set the `DATABASE_URL` environment variable before running:

```bash
export DATABASE_URL="postgres://user:password@localhost:5432/dbname"
```

Or create a `.env` file:
```env
DATABASE_URL=postgres://user:password@localhost:5432/dbname
```

Run migrations:
```bash
rapina migrate new create_your_table
```
"#
            }
            DatabaseType::Mysql => {
                r#"
## Database

This project is configured for **MySQL**. Set the `DATABASE_URL` environment variable before running:

```bash
export DATABASE_URL="mysql://user:password@localhost:3306/dbname"
```

Or create a `.env` file:
```env
DATABASE_URL=mysql://user:password@localhost:3306/dbname
```

Run migrations:
```bash
rapina migrate new create_your_table
```
"#
            }
        }
    } else {
        ""
    };

    format!(
        "# {name}\n\nA web application built with Rapina.\n\n## Getting started\n\n```bash\nrapina dev\n```\n\n## Routes\n\n- `GET /` — Hello world\n- `GET /__rapina/health` — Health check (built-in)\n{db_section}"
    )
}

fn generate_cursor_rules() -> String {
    r#"# Rapina Framework Rules

This is a Rust project using the Rapina web framework.

## Route Handlers

- Use proc macros: `#[get("/path")]`, `#[post("/path")]`, `#[put("/path")]`, `#[delete("/path")]`
- All routes require JWT auth by default. Use `#[public]` for public routes
- Handler names: `list_todos`, `get_todo`, `create_todo`, `update_todo`, `delete_todo`
- Use `#[errors(ErrorType)]` to document error responses

## Extractors

- `Json<T>` for request/response bodies (T: Serialize/Deserialize + JsonSchema)
- `Validated<Json<T>>` for validated bodies (T: also Validate, returns 422)
- `Path<T>` for URL params (`:id` syntax)
- `Query<T>` for query strings
- `State<T>` for shared app state
- `CurrentUser` for the authenticated user
- `Db` for database connection
- Only one body extractor per handler

## Error Handling

- Return `Result<Json<T>>` from handlers
- Use `Error::not_found()`, `Error::bad_request()`, etc.
- Each feature has a typed error enum implementing `IntoApiError` + `DocumentedError`
- All errors include `trace_id` in the response

## Project Structure

Feature-first modules (plural names):
```
src/todos/handlers.rs   — route handlers
src/todos/dto.rs        — CreateTodo, UpdateTodo (Deserialize + JsonSchema)
src/todos/error.rs      — TodoError with IntoApiError + DocumentedError
src/todos/mod.rs        — pub mod dto; pub mod error; pub mod handlers;
src/entity.rs           — schema! macro for DB entities
src/migrations/         — database migrations
```

## Response Types

- Derive `Serialize` + `JsonSchema` on all response structs
- Derive `Deserialize` + `JsonSchema` on request structs
- Update DTOs use `Option<T>` for partial updates

## Builder Pattern

```rust
Rapina::new()
    .with_tracing(TracingConfig::new())
    .middleware(RequestLogMiddleware::new())
    .discover()  // auto-discover handlers
    .listen("127.0.0.1:3000")
    .await
```

## CLI

- `rapina dev` — development server with auto-reload
- `rapina doctor` — diagnose issues
- `rapina routes` — list routes
- `rapina add resource <name>` — scaffold CRUD resource
"#
    .to_string()
}

/// Validate that the project name is a valid Rust crate name.
fn validate_project_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Project name cannot be empty".to_string());
    }

    if name.chars().next().unwrap().is_ascii_digit() {
        return Err("Project name cannot start with a digit".to_string());
    }

    for c in name.chars() {
        if !c.is_alphanumeric() && c != '_' && c != '-' {
            return Err(format!(
                "Project name contains invalid character: '{}'. Only alphanumeric characters, underscores, and hyphens are allowed.",
                c
            ));
        }
    }

    let reserved = ["test", "self", "super", "crate", "Self"];
    if reserved.contains(&name) {
        return Err(format!("'{}' is a reserved Rust keyword", name));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_project_name_valid() {
        assert!(validate_project_name("my-app").is_ok());
        assert!(validate_project_name("my_app").is_ok());
        assert!(validate_project_name("myapp").is_ok());
        assert!(validate_project_name("myapp123").is_ok());
    }

    #[test]
    fn test_validate_project_name_invalid() {
        assert!(validate_project_name("").is_err());
        assert!(validate_project_name(".").is_err());
        assert!(validate_project_name("123app").is_err());
        assert!(validate_project_name("my app").is_err());
        assert!(validate_project_name("my.app").is_err());
        assert!(validate_project_name("self").is_err());
    }

    #[test]
    fn test_generate_agents_md_base() {
        let flags = AgentsFlags {
            with_db: false,
            with_websocket: false,
            with_jobs: false,
        };
        let content = generate_agents_md(&flags);
        assert!(content.contains("Rapina"));
        assert!(content.contains("#[public]"));
        assert!(content.contains("trace_id"));
        assert!(content.contains("Json<T>"));
        assert!(content.contains("IntoApiError"));
        assert!(content.contains("DocumentedError"));
        assert!(content.contains("TestClient"));
        assert!(content.contains("State<T>"));
        assert!(content.contains("rapina add resource"));
        assert!(content.contains("Don't"));
        assert!(content.contains("BEGIN:rapina-agent-rules"));
        assert!(content.contains("END:rapina-agent-rules"));
        assert!(content.contains("sha256:"));
        assert!(!content.contains("migrations.md") && !content.contains("rapina migrate up"));
    }

    #[test]
    fn test_generate_agents_md_with_db() {
        let flags = AgentsFlags {
            with_db: true,
            with_websocket: false,
            with_jobs: false,
        };
        let content = generate_agents_md(&flags);
        assert!(content.contains("rapina migrate up"));
        assert!(content.contains("sea-orm-cli"));
    }

    #[test]
    fn test_agents_md_hash_is_stable() {
        let flags = AgentsFlags {
            with_db: false,
            with_websocket: false,
            with_jobs: false,
        };
        let a = generate_agents_md(&flags);
        let b = generate_agents_md(&flags);
        assert_eq!(a, b);
    }

    #[test]
    fn test_generate_rapina_docs() {
        let dir = tempfile::tempdir().unwrap();
        let flags = AgentsFlags {
            with_db: true,
            with_websocket: false,
            with_jobs: false,
        };
        generate_rapina_docs(dir.path(), &flags).unwrap();
        assert!(dir.path().join(".rapina-docs/core.md").exists());
        assert!(dir.path().join(".rapina-docs/extractors.md").exists());
        assert!(dir.path().join(".rapina-docs/errors.md").exists());
        assert!(dir.path().join(".rapina-docs/testing.md").exists());
        assert!(dir.path().join(".rapina-docs/migrations.md").exists());
        assert!(!dir.path().join(".rapina-docs/websocket.md").exists());
        assert!(!dir.path().join(".rapina-docs/jobs.md").exists());
    }

    #[test]
    fn test_generate_cursor_rules() {
        let content = generate_cursor_rules();
        assert!(content.contains("Rapina"));
        assert!(content.contains("#[public]"));
        assert!(content.contains("IntoApiError"));
        assert!(content.contains("DocumentedError"));
        assert!(content.contains("rapina add resource"));
    }

    #[test]
    fn test_generate_readme_without_db() {
        let content = generate_readme("test-app", None);
        assert!(content.contains("# test-app"));
        assert!(content.contains("rapina dev"));
        assert!(!content.contains("## Database"));
    }

    #[test]
    fn test_generate_readme_with_sqlite() {
        let content = generate_readme("test-app", Some(&DatabaseType::Sqlite));
        assert!(content.contains("## Database"));
        assert!(content.contains("**SQLite**"));
        assert!(content.contains("app.db"));
    }

    #[test]
    fn test_generate_readme_with_postgres() {
        let content = generate_readme("test-app", Some(&DatabaseType::Postgres));
        assert!(content.contains("## Database"));
        assert!(content.contains("**PostgreSQL**"));
        assert!(content.contains("DATABASE_URL"));
        assert!(content.contains("postgres://"));
    }

    #[test]
    fn test_generate_readme_with_mysql() {
        let content = generate_readme("test-app", Some(&DatabaseType::Mysql));
        assert!(content.contains("## Database"));
        assert!(content.contains("**MySQL**"));
        assert!(content.contains("DATABASE_URL"));
        assert!(content.contains("mysql://"));
    }
}
