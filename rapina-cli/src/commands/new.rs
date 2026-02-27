//! Implementation of the `rapina new` command.

use colored::Colorize;
use std::fs;
use std::path::Path;

/// Execute the `new` command to create a new Rapina project.
pub fn execute(name: &str, no_ai: bool) -> Result<(), String> {
    // Validate project name
    validate_project_name(name)?;

    // Check if directory already exists
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
    println!();

    // Create project directory structure
    let src_path = project_path.join("src");
    fs::create_dir_all(&src_path).map_err(|e| format!("Failed to create directory: {}", e))?;

    // Create Cargo.toml
    let cargo_toml = generate_cargo_toml(name);
    let cargo_path = project_path.join("Cargo.toml");
    fs::write(&cargo_path, cargo_toml).map_err(|e| format!("Failed to write Cargo.toml: {}", e))?;
    println!("  {} Created {}", "✓".green(), "Cargo.toml".cyan());

    // Create src/main.rs
    let main_rs = generate_main_rs();
    let main_path = src_path.join("main.rs");
    fs::write(&main_path, main_rs).map_err(|e| format!("Failed to write main.rs: {}", e))?;
    println!("  {} Created {}", "✓".green(), "src/main.rs".cyan());

    // Create .gitignore
    let gitignore = generate_gitignore();
    let gitignore_path = project_path.join(".gitignore");
    fs::write(&gitignore_path, gitignore)
        .map_err(|e| format!("Failed to write .gitignore: {}", e))?;
    println!("  {} Created {}", "✓".green(), ".gitignore".cyan());

    // Create README.md
    let readme = generate_readme(name);
    let readme_path = project_path.join("README.md");
    fs::write(&readme_path, readme).map_err(|e| format!("Failed to write README.md: {}", e))?;
    println!("  {} Created {}", "✓".green(), "README.md".cyan());

    // Create AI assistant config files
    if !no_ai {
        let agent_md = generate_agent_md();
        let agent_path = project_path.join("AGENT.md");
        fs::write(&agent_path, agent_md).map_err(|e| format!("Failed to write AGENT.md: {}", e))?;
        println!("  {} Created {}", "✓".green(), "AGENT.md".cyan());

        let claude_dir = project_path.join(".claude");
        fs::create_dir_all(&claude_dir).map_err(|e| format!("Failed to create .claude/: {}", e))?;
        let claude_md = generate_claude_md();
        let claude_path = claude_dir.join("CLAUDE.md");
        fs::write(&claude_path, claude_md)
            .map_err(|e| format!("Failed to write .claude/CLAUDE.md: {}", e))?;
        println!("  {} Created {}", "✓".green(), ".claude/CLAUDE.md".cyan());

        let cursor_dir = project_path.join(".cursor");
        fs::create_dir_all(&cursor_dir).map_err(|e| format!("Failed to create .cursor/: {}", e))?;
        let cursor_rules = generate_cursor_rules();
        let cursor_path = cursor_dir.join("rules");
        fs::write(&cursor_path, cursor_rules)
            .map_err(|e| format!("Failed to write .cursor/rules: {}", e))?;
        println!("  {} Created {}", "✓".green(), ".cursor/rules".cyan());
    }

    println!();
    println!("  {} Project created successfully!", "🎉".bold());
    println!();
    println!("  {}:", "Next steps".bright_yellow());
    println!("    cd {}", name.cyan());
    println!("    rapina dev");
    println!();

    Ok(())
}
fn generate_readme(name: &str) -> String {
    format!(
        "# {name}\n\nA web application built with Rapina.\n\n## Getting started\n\n```bash\nrapina dev\n```\n\n## Routes\n\n- `GET /` — Hello world\n- `GET /health` — Health check\n"
    )
}

/// Validate that the project name is a valid Rust crate name.
fn validate_project_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Project name cannot be empty".to_string());
    }

    // Check if name starts with a digit
    if name.chars().next().unwrap().is_ascii_digit() {
        return Err("Project name cannot start with a digit".to_string());
    }

    // Check for valid characters (alphanumeric, underscore, hyphen)
    for c in name.chars() {
        if !c.is_alphanumeric() && c != '_' && c != '-' {
            return Err(format!(
                "Project name contains invalid character: '{}'. Only alphanumeric characters, underscores, and hyphens are allowed.",
                c
            ));
        }
    }

    // Check for reserved names
    let reserved = ["test", "self", "super", "crate", "Self"];
    if reserved.contains(&name) {
        return Err(format!("'{}' is a reserved Rust keyword", name));
    }

    Ok(())
}

/// Generate the content for Cargo.toml.
fn generate_cargo_toml(name: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"

[dependencies]
rapina = "{version}"
tokio = {{ version = "1", features = ["full"] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
hyper = "1"
"#
    )
}

/// Generate the content for src/main.rs.
fn generate_main_rs() -> String {
    r#"use rapina::prelude::*;
use rapina::middleware::RequestLogMiddleware;
use rapina::schemars;

#[derive(Serialize, JsonSchema)]
struct MessageResponse {
    message: String,
}

#[derive(Serialize, JsonSchema)]
struct HealthResponse {
    status: String,
    version: String,
}

#[get("/")]
async fn hello() -> Json<MessageResponse> {
    Json(MessageResponse {
        message: "Hello from Rapina!".to_string(),
    })
}

#[get("/health")]
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let router = Router::new()
        .get("/", hello)
        .get("/health", health);

    Rapina::new()
        .with_tracing(TracingConfig::new())
        .middleware(RequestLogMiddleware::new())
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
"#
    .to_string()
}

/// Generate the content for .gitignore.
fn generate_gitignore() -> String {
    r#"/target
Cargo.lock
"#
    .to_string()
}

/// Generate the content for AGENT.md (generic AI assistant context).
fn generate_agent_md() -> String {
    r#"# Rapina Project

This is a Rust web application built with [Rapina](https://github.com/arferreira/rapina), an opinionated web framework.

## Key Conventions

### Routes are protected by default
All routes require JWT authentication unless explicitly marked with `#[public]`:

```rust
#[public]
#[get("/health")]
async fn health() -> &'static str { "ok" }

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

/// Generate the content for .claude/CLAUDE.md (Claude Code specific instructions).
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

/// Generate the content for .cursor/rules (Cursor AI rules).
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
    }

    #[test]
    fn test_generate_claude_md() {
        let content = generate_claude_md();
        assert!(content.contains("Rapina"));
        assert!(content.contains("TestClient"));
        assert!(content.contains("#[errors("));
        assert!(content.contains("Validated<Json<T>>"));
    }

    #[test]
    fn test_generate_cursor_rules() {
        let content = generate_cursor_rules();
        assert!(content.contains("Rapina"));
        assert!(content.contains("#[public]"));
        assert!(content.contains("IntoApiError"));
    }
}
