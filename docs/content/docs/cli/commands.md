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
- `.gitignore`
- `README.md`
- `AGENT.md` — AI assistant context (generic)
- `.claude/CLAUDE.md` — Claude-specific instructions
- `.cursor/rules` — Cursor rules

The AI config files teach assistants Rapina conventions (protected-by-default routing, extractors, error handling, project structure) so they generate correct code out of the box.

To skip AI config files:

```bash
rapina new my-app --no-ai
```

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
| `datetime` | `timestamptz` | `DateTime` | TIMESTAMPTZ (timezone-aware) |
| `naivedatetime` | `timestamp` | `NaiveDateTime` | TIMESTAMP (without timezone) |
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

## rapina import database

Import schema from a live database, generating entities, migrations, handlers, DTOs, and error types for each table:

```bash
rapina import database --url postgres://user:pass@localhost/mydb
```

Options:

| Flag | Description | Default |
|------|-------------|---------|
| `--url <URL>` | Database connection URL (or `DATABASE_URL` env) | *required* |
| `--tables <T1,T2>` | Only import specific tables (comma-separated) | all tables |
| `--schema <NAME>` | Database schema name | `public` (Postgres) |
| `--force` | Overwrite existing files (re-import after schema changes) | false |

Supported databases: PostgreSQL (`postgres://`), MySQL (`mysql://`), SQLite (`sqlite://`). Each requires the corresponding feature:

```bash
cargo install rapina-cli --features import-postgres
cargo install rapina-cli --features import-mysql
cargo install rapina-cli --features import-sqlite
```

For each valid table, the command generates the same files as `rapina add resource`: a feature module (`src/<plural>/`), a `schema!` block in `src/entity.rs`, and a timestamped migration.

Tables are skipped if they have no primary key, a composite primary key, or are internal migration tables (`seaql_migrations`, `sqlx_migrations`, `__diesel_schema_migrations`).

### Re-importing with `--force`

Without `--force`, the command errors if a feature module directory already exists. With `--force`:

- Existing `src/<plural>/` directories are removed and re-created
- Duplicate `schema!` blocks in `entity.rs` are replaced instead of appended
- A new migration file is always created (timestamps prevent collisions)

This is useful when the upstream database schema changes and you want to regenerate the Rapina code to match.

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
| `--bless` | Update snapshot files (golden-file testing) |
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

# Save or update response snapshots
rapina test --bless
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
- No duplicate handler paths (same method + path registered more than once; only the first match is used, others are shadowed)

Output:

```
  → Running API health checks on http://127.0.0.1:3000...

  ✓ All routes have response schemas
  ✓ No duplicate handler paths
  ⚠ Missing documentation: GET /users/:id
  ⚠ No documented errors: POST /users

  Summary: 2 passed, 2 warnings, 0 errors

  Consider addressing the warnings above.
```

If duplicate routes are detected, you'll see a warning like:

```
  ⚠ Duplicate route GET /users: handlers [list_users, other_list] — only the first match is used, others are shadowed
```

## rapina migrate new

Generate a new empty migration file:

```bash
rapina migrate new create_posts
```

This creates a timestamped migration file in `src/migrations/` and updates `mod.rs` with the module declaration and `migrations!` macro entry. The migration name must be lowercase with underscores.

> **Note:** `rapina add resource` already generates a pre-filled migration. Use `rapina migrate new` when you need a migration that isn't tied to a new resource (e.g., adding a column, creating an index).

## rapina jobs init

Set up the background jobs migration in your project:

```bash
rapina jobs init
```

This adds the framework's `create_rapina_jobs` migration to `src/migrations/mod.rs`. If the file doesn't exist, it creates one. If the migration is already configured, the command is a no-op.

The migration creates the `rapina_jobs` table used by the background jobs system. It uses a zero timestamp prefix so it always runs before your application migrations. See [Background Jobs](/docs/core-concepts/background-jobs/) for the full table schema and types.

> **Note:** The jobs migration requires PostgreSQL. It uses `gen_random_uuid()` and partial indexes, which are not available in MySQL or SQLite.

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
