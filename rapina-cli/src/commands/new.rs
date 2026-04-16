//! Implementation of the `rapina new` command.

use colored::Colorize;
use std::fs;
use std::path::Path;

use super::templates;
use super::templates::DatabaseType;

/// Execute the `new` command to create a new Rapina project.
///
/// `template` is `None` for the default starter and `Some("crud")` / `Some("auth")`
/// for the optional starter templates.
/// `db_type` specifies the database to configure for the project.
pub fn execute(
    name: &str,
    template: Option<&str>,
    db_type: Option<&DatabaseType>,
    no_ai: bool,
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
    if !no_ai {
        let agent_path = project_path.join("AGENT.md");
        fs::write(&agent_path, generate_agent_md())
            .map_err(|e| format!("Failed to write AGENT.md: {}", e))?;
        println!("  {} Created {}", "✓".green(), "AGENT.md".cyan());

        let claude_dir = project_path.join(".claude");
        fs::create_dir_all(&claude_dir).map_err(|e| format!("Failed to create .claude/: {}", e))?;
        fs::write(claude_dir.join("CLAUDE.md"), generate_claude_md())
            .map_err(|e| format!("Failed to write .claude/CLAUDE.md: {}", e))?;
        println!("  {} Created {}", "✓".green(), ".claude/CLAUDE.md".cyan());

        let cursor_dir = project_path.join(".cursor");
        fs::create_dir_all(&cursor_dir).map_err(|e| format!("Failed to create .cursor/: {}", e))?;
        fs::write(cursor_dir.join("rules"), generate_cursor_rules())
            .map_err(|e| format!("Failed to write .cursor/rules: {}", e))?;
        println!("  {} Created {}", "✓".green(), ".cursor/rules".cyan());
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

fn generate_agent_md() -> String {
    r#"# Rapina Project

This is a Rust web application built with [Rapina](https://github.com/rapina-rs/rapina), an opinionated web framework.

## Key Conventions

### Routes are protected by default
All routes require JWT authentication unless explicitly marked with `#[public]`:

```rust
#[public]
#[post("/auth/login")]
async fn login(body: Json<LoginRequest>) -> Result<Json<TokenResponse>> { ... }

// This route requires a valid JWT token
#[get("/me")]
async fn me(user: CurrentUser) -> Json<UserResponse> { ... }
```

### Handler pattern
Use proc macros for route registration. Handler names follow `verb_resource` convention:

```rust
#[get("/todos")]       async fn list_todos() -> ...
#[get("/todos/:id")]   async fn get_todo(id: Path<i32>) -> ...
#[post("/todos")]      async fn create_todo(body: Json<CreateTodo>) -> ...
#[put("/todos/:id")]   async fn update_todo(id: Path<i32>, body: Json<UpdateTodo>) -> ...
#[delete("/todos/:id")] async fn delete_todo(id: Path<i32>) -> ...
```

### Typed extractors
- `Json<T>` — request/response body (T must derive Serialize and/or Deserialize + JsonSchema)
- `Path<T>` — URL path parameter (`:id` syntax)
- `Query<T>` — query string parameters
- `State<T>` — shared application state
- `Validated<Json<T>>` — JSON body with validation (T must also derive Validate, returns 422 on failure)
- `CurrentUser` — authenticated user identity (requires auth to be configured)
- `Db` — database connection (requires database feature)

### Error handling
Return `Result<Json<T>>` from handlers. Use typed errors:

```rust
pub enum TodoError {
    DbError(DbError),
}

impl IntoApiError for TodoError {
    fn into_api_error(self) -> Error {
        match self {
            TodoError::DbError(e) => e.into_api_error(),
        }
    }
}
```

All error responses include a `trace_id` for debugging:
```json
{
  "error": { "code": "NOT_FOUND", "message": "Todo 42 not found" },
  "trace_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

### Project structure (feature-first)
```
src/
├── main.rs          # App bootstrap with builder pattern
├── entity.rs        # Database entities (schema! macro)
├── migrations/      # Database migrations
└── todos/           # Feature module (always plural)
    ├── mod.rs
    ├── handlers.rs  # Route handlers
    ├── dto.rs       # Request/response types
    └── error.rs     # Domain errors
```

### Builder pattern
```rust
Rapina::new()
    .with_tracing(TracingConfig::new())
    .middleware(RequestLogMiddleware::new())
    .with_cors(CorsConfig::permissive())
    .router(router)
    .listen("127.0.0.1:3000")
    .await
```

## CLI Commands
- `rapina dev` — run with auto-reload
- `rapina doctor` — diagnose project issues
- `rapina routes` — list all registered routes
- `rapina add resource <name>` — scaffold a new CRUD resource
"#
    .to_string()
}

fn generate_claude_md() -> String {
    r#"# Rapina Project Instructions

This is a Rust web application built with the Rapina framework.

## Framework Overview

Rapina is an opinionated Rust web framework built on hyper. Routes are protected by default (JWT auth) unless marked `#[public]`. All response types must derive `Serialize` + `JsonSchema` for OpenAPI generation. Error responses always include a `trace_id`.

## Conventions

### Adding a new endpoint

1. Create or edit the handler in `src/<feature>/handlers.rs`
2. Use the proc macro: `#[get("/path")]`, `#[post("/path")]`, `#[put("/path")]`, `#[delete("/path")]`
3. Mark public routes with `#[public]` above the method macro
4. Use `#[errors(ErrorType)]` to document error responses for OpenAPI
5. If using `.discover()`, the route is auto-registered. Otherwise add it to the router in `main.rs`

### Extractors (in handler function signatures)

```rust
// Body (only one per handler)
body: Json<T>              // JSON body, T: Deserialize + JsonSchema
body: Validated<Json<T>>   // JSON body with validation, T: Deserialize + JsonSchema + Validate
body: Form<T>              // Form data

// Parts (multiple allowed)
id: Path<i32>              // URL path param (:id syntax)
params: Query<T>           // Query string
headers: Headers           // Full header map
state: State<T>            // App state
user: CurrentUser          // Authenticated user (id, claims)
ctx: Context               // Request context (trace_id, start_time)
db: Db                     // Database connection (requires database feature)
jar: Cookie<T>             // Cookie values
```

### Handler naming convention
- `list_<resources>` — GET collection
- `get_<resource>` — GET single item
- `create_<resource>` — POST
- `update_<resource>` — PUT
- `delete_<resource>` — DELETE

### Builder pattern
```rust
Rapina::new()
    .with_tracing(TracingConfig::new())
    .middleware(RequestLogMiddleware::new())
    .with_cors(CorsConfig::permissive())
    .router(router)
    .listen("127.0.0.1:3000")
    .await
```

### Error handling pattern

Each feature module has its own error type:

```rust
// src/todos/error.rs
pub enum TodoError {
    DbError(DbError),
}

impl IntoApiError for TodoError {
    fn into_api_error(self) -> Error {
        match self {
            TodoError::DbError(e) => e.into_api_error(),
        }
    }
}

impl DocumentedError for TodoError {
    fn error_variants() -> Vec<ErrorVariant> {
        vec![
            ErrorVariant { status: 404, code: "NOT_FOUND", description: "Todo not found" },
            ErrorVariant { status: 500, code: "DATABASE_ERROR", description: "Database operation failed" },
        ]
    }
}
```

Use `Error::not_found()`, `Error::bad_request()`, `Error::unauthorized()`, etc. for quick errors.

### Project structure

Feature-first modules. Each feature directory is plural:

```
src/todos/handlers.rs    # not src/handlers/todos.rs
src/todos/dto.rs         # CreateTodo, UpdateTodo structs
src/todos/error.rs       # TodoError enum
src/todos/mod.rs         # pub mod dto; pub mod error; pub mod handlers;
```

Top-level shared files:
- `src/entity.rs` — all database entities via `schema!` macro
- `src/migrations/` — database migrations via `migrations!` macro

### DTOs
- Request types: `Create<Resource>`, `Update<Resource>` — derive `Deserialize` + `JsonSchema`
- Response types: derive `Serialize` + `JsonSchema`
- Update DTOs wrap fields in `Option<T>` for partial updates

### Testing

```rust
use rapina::testing::TestClient;

#[tokio::test]
async fn test_hello() {
    let app = Rapina::new().router(router);
    let client = TestClient::new(app).await;

    let res = client.get("/").send().await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: MessageResponse = res.json();
    assert_eq!(body.message, "Hello from Rapina!");
}
```

`TestClient` supports `.get()`, `.post()`, `.put()`, `.delete()`, `.patch()`. Request builder has `.json()`, `.header()`, `.body()`. Response has `.status()`, `.json::<T>()`, `.text()`.

## Build & Run

```bash
rapina dev              # development with auto-reload
cargo build --release   # production build
rapina doctor           # check for common issues
rapina routes           # list all routes
```
"#
    .to_string()
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
    fn test_generate_agent_md() {
        let content = generate_agent_md();
        assert!(content.contains("Rapina"));
        assert!(content.contains("#[public]"));
        assert!(content.contains("trace_id"));
        assert!(content.contains("Json<T>"));
        assert!(content.contains("IntoApiError"));
        assert!(content.contains("rapina add resource"));
    }

    #[test]
    fn test_generate_claude_md() {
        let content = generate_claude_md();
        assert!(content.contains("Rapina"));
        assert!(content.contains("TestClient"));
        assert!(content.contains("#[errors("));
        assert!(content.contains("Validated<Json<T>>"));
        assert!(content.contains("IntoApiError"));
        assert!(content.contains("DocumentedError"));
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
