+++
title = "Commands"
description = "Complete CLI command reference"
weight = 1
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
