+++
title = "Commands"
description = "Complete CLI command reference"
weight = 1
date = 2025-02-13
+++

## rapina new

Create a new Rapina project:

```bash
rapina new my-app
```

This creates:
- `Cargo.toml` with Rapina dependencies
- `src/main.rs` with a basic API
- `.env.example` with common variables
- `.gitignore`

## rapina add resource

Scaffold a complete CRUD resource with handlers, DTOs, error type, entity definition, and a database migration:

```bash
rapina add resource user name:string email:string active:bool
```

This creates:

```
src/users/mod.rs           # Module declarations
src/users/handlers.rs      # list, get, create, update, delete handlers
src/users/dto.rs           # CreateUser, UpdateUser request types
src/users/error.rs         # UserError with IntoApiError + DocumentedError
src/entity.rs              # Appends a schema! {} block (or creates the file)
src/migrations/m{TS}_create_users.rs   # Pre-filled migration
src/migrations/mod.rs      # Updated with mod + migrations! macro entry
```

Fields use a `name:type` format. Supported types:

| Type | Aliases | Rust Type | Column |
|------|---------|-----------|--------|
| `string` | | `String` | VARCHAR |
| `text` | | `String` | TEXT |
| `i32` | `integer` | `i32` | INTEGER |
| `i64` | `bigint` | `i64` | BIGINT |
| `f32` | `float` | `f32` | FLOAT |
| `f64` | `double` | `f64` | DOUBLE |
| `bool` | `boolean` | `bool` | BOOLEAN |
| `uuid` | | `Uuid` | UUID |
| `datetime` | | `DateTime` | TIMESTAMPTZ |
| `date` | | `Date` | DATE |
| `decimal` | | `Decimal` | DECIMAL |
| `json` | | `Json` | JSON |

The generated handlers follow Rapina conventions and are ready to wire into your router. The command prints the exact code you need to add to `main.rs`:

```
  Next steps:

  1. Add the module declaration to src/main.rs:

     mod users;
     mod entity;
     mod migrations;

  2. Register the routes in your Router:

     use users::handlers::{list_users, get_user, create_user, update_user, delete_user};

     let router = Router::new()
         .get("/users", list_users)
         .get("/users/:id", get_user)
         .post("/users", create_user)
         .put("/users/:id", update_user)
         .delete("/users/:id", delete_user);

  3. Enable the database feature in Cargo.toml:

     rapina = { version = "...", features = ["postgres"] }
```

The resource name must be lowercase with underscores (e.g., `user`, `blog_post`). Pluralization is automatic. If the resource directory already exists, the command fails with a clear error instead of overwriting.

## rapina dev

Start the development server with hot reload:

```bash
rapina dev
```

Options:

| Flag | Description | Default |
|------|-------------|---------|
| `-p, --port <PORT>` | Server port | 3000 |
| `--host <HOST>` | Server host | 127.0.0.1 |

Example:

```bash
rapina dev -p 8080 --host 0.0.0.0
```

## rapina test

Run tests with pretty output:

```bash
rapina test
```

Options:

| Flag | Description |
|------|-------------|
| `--coverage` | Generate coverage report (requires cargo-llvm-cov) |
| `-w, --watch` | Watch for changes and re-run tests |
| `[FILTER]` | Filter tests by name |

Examples:

```bash
# Run all tests
rapina test

# Run tests matching a pattern
rapina test user

# Watch mode - re-run on file changes
rapina test -w

# Generate coverage report
rapina test --coverage
```

Output:

```
  ✓ tests::it_works
  ✓ tests::user_creation
  ✗ tests::it_fails

──────────────────────────────────────────────────
FAIL 2 passed, 1 failed, 0 ignored
████████████████████████████░░░░░░░░░░░░
```

## rapina routes

List all registered routes from a running server:

```bash
rapina routes
```

Output:

```
  METHOD  PATH                  HANDLER
  ------  --------------------  ---------------
  GET     /                     hello
  GET     /health               health
  GET     /users/:id            get_user
  POST    /users                create_user

  4 route(s) registered
```

> **Note:** The server must be running for this command to work.

## rapina doctor

Run health checks on your API:

```bash
rapina doctor
```

Checks:
- Response schemas defined for all routes
- Error documentation present
- OpenAPI metadata (descriptions)

Output:

```
  Running API health checks...

  All routes have response schemas
  Missing documentation: GET /users/:id
  No documented errors: POST /users

  Summary: 1 passed, 2 warnings, 0 errors

  Consider addressing the warnings above.
```

## rapina migrate new

Generate a new empty migration file:

```bash
rapina migrate new create_posts
```

This creates a timestamped migration file in `src/migrations/` and updates `mod.rs` with the module declaration and `migrations!` macro entry. The migration name must be lowercase with underscores.

> **Note:** `rapina add resource` already generates a pre-filled migration. Use `rapina migrate new` when you need a migration that isn't tied to a new resource (e.g., adding a column, creating an index).

## rapina openapi export

Export the OpenAPI specification to a file:

```bash
rapina openapi export -o openapi.json
```

Options:

| Flag | Description | Default |
|------|-------------|---------|
| `-o, --output <FILE>` | Output file | openapi.json |

## rapina openapi check

Verify that the committed spec matches the current code:

```bash
rapina openapi check
```

Useful in CI to ensure the spec is always up to date.

## rapina openapi diff

Detect breaking changes against another branch:

```bash
rapina openapi diff --base main
```

Output:

```
  Comparing OpenAPI spec with main branch...

  Breaking changes:
    - Removed endpoint: /health
    - Removed method: DELETE /users/{id}

  Non-breaking changes:
    - Added endpoint: /posts
    - Added field 'avatar' in GET /users/{id}

Error: Found 2 breaking change(s)
```

The command exits with code 1 if breaking changes are detected.
