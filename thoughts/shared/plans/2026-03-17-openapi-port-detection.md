# OpenAPI Port Detection Implementation Plan

## Overview

Fix the OpenAPI CLI commands (`export`, `check`, `diff`) so they use the same `RAPINA_PORT` env var as the rest of the CLI, and add `.env` file scanning so the port is auto-detected from project configuration when not explicitly provided.

## Current State Analysis

- OpenAPI subcommands use `env = "SERVER_PORT"` while `dev`, `routes`, and `doctor` use `env = "RAPINA_PORT"` (`main.rs:191-219` vs `main.rs:41,58,77`)
- `rapina dev --port 8080` sets `RAPINA_PORT=8080` on the child process (`dev.rs:180`), but `rapina openapi export` reads `SERVER_PORT` — so they don't communicate
- `dotenvy` is an optional dep behind the `seed` feature; no commands currently load `.env` before clap parses args
- No `.env` scanning exists for port auto-detection

### Key Discoveries:
- The port is only used to **fetch** the spec from a running server (`openapi.rs:110`), not embedded in the generated spec
- `build_openapi_url(host, port)` in `common/urls.rs:7` assembles the fetch URL
- All port flags default to `3000` and accept `-p` short form
- The `verify_rapina_project()` function already reads `Cargo.toml` (`commands/mod.rs:22`)

## Desired End State

All port-using CLI commands (`dev`, `routes`, `doctor`, `openapi export/check/diff`) share a consistent port resolution chain:

1. `--port` flag (explicit CLI arg) — highest priority
2. `RAPINA_PORT` env var (from shell environment)
3. `SERVER_PORT` env var (backwards compatibility for OpenAPI commands)
4. `.env` file `RAPINA_PORT` value
5. `.env` file `PORT` value
6. Default `3000` — lowest priority

Verification: `rapina openapi export` with no flags, with `RAPINA_PORT=8080` in `.env`, connects to port 8080.

## What We're NOT Doing

- Not adding port detection by scanning Rust source code
- Not adding a `[rapina]` section to `Cargo.toml` for port config
- Not changing the `rapina` library's port handling (only CLI)
- Not changing the generated OpenAPI spec content (no `servers:` block changes)

## Implementation Approach

Two phases: first fix the env var inconsistency (minimal, safe), then add `.env` scanning.

---

## Phase 1: Unify env var to `RAPINA_PORT`

### Overview
Change the three OpenAPI subcommands from `SERVER_PORT` to `RAPINA_PORT` so they match every other port-using command.

### Changes Required:

#### 1. CLI arg definitions
**File**: `rapina-cli/src/main.rs`
**Changes**: Replace `env = "SERVER_PORT"` with `env = "RAPINA_PORT"` on lines 191, 203, 218. Keep `SERVER_PORT` as a backwards-compatible fallback handled in `main()` before clap parsing.

```rust
// Export (line 191)
#[arg(short, long, env = "RAPINA_PORT", default_value = "3000")]
port: u16,

// Check (line 203)
#[arg(short, long, env = "RAPINA_PORT", default_value = "3000")]
port: u16,

// Diff (line 218)
#[arg(short, long, env = "RAPINA_PORT", default_value = "3000")]
port: u16,
```

#### 1b. `SERVER_PORT` backwards compatibility fallback
**File**: `rapina-cli/src/main.rs`
**Changes**: Before `Cli::parse()`, if `RAPINA_PORT` is not set but `SERVER_PORT` is, copy `SERVER_PORT` into `RAPINA_PORT` so clap picks it up.

```rust
fn main() {
    // Backwards compat: SERVER_PORT → RAPINA_PORT
    if std::env::var("RAPINA_PORT").is_err() {
        if let Ok(port) = std::env::var("SERVER_PORT") {
            std::env::set_var("RAPINA_PORT", &port);
        }
    }

    let cli = Cli::parse();
    // ...
}
```

#### 2. Documentation
**File**: `docs/content/docs/core-concepts/openapi.md`
**Changes**: Update line 187 — change `$SERVER_PORT` reference to `$RAPINA_PORT`.

```markdown
All three require a running development server and accept `--host` (default `127.0.0.1`) and `--port` / `-p` (default `3000`, also reads `$RAPINA_PORT`).
```

**File**: `docs/content/docs/cli/commands.md`
**Changes**: Add `--port` and `--host` flags to the OpenAPI export options table (lines 288-291).

### Success Criteria:

#### Automated Verification:
- [x] Project compiles: `cargo build -p rapina-cli`
- [x] Existing tests pass: `cargo test -p rapina-cli`
- [ ] `rapina openapi export --help` shows `RAPINA_PORT` env var, not `SERVER_PORT`

#### Manual Verification:
- [ ] Start server on port 8080: `RAPINA_PORT=8080 rapina dev`
- [ ] In another terminal: `RAPINA_PORT=8080 rapina openapi export` connects to 8080
- [ ] Backwards compat: `SERVER_PORT=8080 rapina openapi export` still works
- [ ] `RAPINA_PORT` takes priority over `SERVER_PORT` when both are set
- [ ] Without env var: `rapina openapi export` still defaults to 3000

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation before proceeding to Phase 2.

---

## Phase 2: Add `.env` file scanning for port auto-detection

### Overview
Load the project's `.env` file early in CLI startup so that `RAPINA_PORT` (or `PORT`) values from `.env` are available when clap resolves `env = "RAPINA_PORT"`. This benefits all port-using commands, not just OpenAPI.

### Changes Required:

#### 1. Make `dotenvy` non-optional
**File**: `rapina-cli/Cargo.toml`
**Changes**: Move `dotenvy` from optional to required, remove from `seed` feature deps.

```toml
# In [dependencies] — change from:
dotenvy = { version = "0.15", optional = true }
# To:
dotenvy = "0.15"

# In [features] — change from:
seed = ["dep:sea-orm", "dep:tokio", "dep:dotenvy", "dep:fastrand", "dep:uuid"]
# To:
seed = ["dep:sea-orm", "dep:tokio", "dep:fastrand", "dep:uuid"]
```

#### 2. Load `.env` before clap parsing
**File**: `rapina-cli/src/main.rs`
**Changes**: Add `dotenvy::dotenv().ok();` at the very start of `main()`, before `Cli::parse()`. This ensures env vars from `.env` are available for clap's `env = "..."` resolution. Also handle `PORT` → `RAPINA_PORT` fallback.

```rust
fn main() {
    // Load .env file if present (before clap parses, so env vars are available)
    dotenvy::dotenv().ok();

    // Fallback chain: RAPINA_PORT > SERVER_PORT > PORT
    if std::env::var("RAPINA_PORT").is_err() {
        if let Ok(port) = std::env::var("SERVER_PORT") {
            std::env::set_var("RAPINA_PORT", &port);
        } else if let Ok(port) = std::env::var("PORT") {
            std::env::set_var("RAPINA_PORT", &port);
        }
    }

    let cli = Cli::parse();
    // ... rest unchanged
}
```

#### 3. Remove redundant `dotenvy::dotenv()` call from seed command
**File**: `rapina-cli/src/commands/seed.rs`
**Changes**: Remove `dotenvy::dotenv().ok();` on line 200 since it's now called globally in `main()`.

### Success Criteria:

#### Automated Verification:
- [x] Project compiles: `cargo build -p rapina-cli`
- [x] All tests pass: `cargo test -p rapina-cli` (86 passed)
- [ ] Seed tests still pass (with seed feature): `cargo test -p rapina-cli --features seed-sqlite`

#### Manual Verification:
- [ ] Create `.env` with `RAPINA_PORT=9090`, run `rapina openapi export --help` — default should still show 3000 (env is resolved at runtime, not help text)
- [ ] With `.env` containing `RAPINA_PORT=9090` and server on 9090: `rapina openapi export` connects to 9090
- [ ] With `.env` containing `PORT=9090` (no `RAPINA_PORT`): `rapina openapi export` connects to 9090
- [ ] Explicit `--port 4000` overrides `.env` value
- [ ] Explicit `RAPINA_PORT=4000 rapina openapi export` overrides `.env` value
- [ ] `rapina dev` also respects `.env` port
- [ ] No `.env` file: all commands still default to 3000

---

## Testing Strategy

### Unit Tests:
- No new unit tests needed for Phase 1 (env var rename is a config change)
- Phase 2: Consider an integration test that writes a `.env`, spawns the CLI, and verifies port resolution — but this is optional given the manual verification is straightforward

### Manual Testing Steps:
1. Verify `rapina openapi export --help` shows `RAPINA_PORT`
2. Verify `SERVER_PORT` backwards compatibility still works
3. Verify `.env` scanning works for `dev`, `routes`, `doctor`, and all three `openapi` subcommands
4. Verify priority chain: `--port` > `RAPINA_PORT` env > `SERVER_PORT` env > `.env` RAPINA_PORT > `.env` PORT > 3000

## References

- CLI entry point: `rapina-cli/src/main.rs`
- OpenAPI commands: `rapina-cli/src/commands/openapi.rs`
- URL builder: `rapina-cli/src/common/urls.rs`
- Dev command (reference for env var pattern): `rapina-cli/src/commands/dev.rs:179-180`
- OpenAPI docs: `docs/content/docs/core-concepts/openapi.md:187`
- CLI commands docs: `docs/content/docs/cli/commands.md:278-291`
