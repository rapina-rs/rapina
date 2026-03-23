# `--bless` Snapshot Testing Implementation Plan

## Overview

Add snapshot/golden-file testing to rapina so users can capture API response bodies and compare against them on subsequent runs. The CLI gets a `--bless` flag that signals "save new snapshots"; the framework's `TestResponse` gets an `.assert_snapshot(name)` method that redacts dynamic fields, saves/compares `.snap` files, and produces clear diff output on mismatch.

## Current State Analysis

- `rapina test` is implemented in `rapina-cli/src/commands/test.rs`. It builds a `cargo test` (or `cargo llvm-cov`) command via `build_test_command()` and spawns it as a subprocess. Configuration flows through `TestConfig { coverage, watch, filter }`.
- CLI args are defined via clap derive in `rapina-cli/src/main.rs:84-93` as fields on the `Commands::Test` variant, then mapped to `TestConfig` at `main.rs:316-329`.
- The framework provides `rapina::testing::TestClient` (`rapina/src/testing/client.rs`) which spawns a real TCP server and returns `TestResponse { status, headers, body }`.
- `TestResponse` has `.text()`, `.json::<T>()`, `.try_json::<T>()`, `.bytes()` methods.
- Dynamic fields that need redaction: `trace_id` (UUID v4 from `RequestContext::new()` in `context.rs:12`), any UUID-shaped values, ISO 8601 timestamps.
- No snapshot testing infrastructure exists today.

### Key Discoveries:
- `TestResponse` fields are private — new methods must be added on the struct itself (`client.rs:246-250`)
- The CLI communicates with the test subprocess only via env vars and process exit code — no IPC. An env var (`RAPINA_BLESS=1`) is the right mechanism.
- `serde_json` is already a dependency of the `rapina` crate (`Cargo.toml:33`)
- The `testing` module is unconditionally compiled (no feature gate) at `lib.rs:112`
- Snapshot files should live in the user's project under `snapshots/` (gitignitted or committed, user's choice)

## Desired End State

After implementation:

1. Users add `.assert_snapshot("name")` calls to their integration tests:
   ```rust
   let response = client.get("/users/1").send().await;
   response.assert_snapshot("get_user");
   ```

2. Running `rapina test --bless` saves `snapshots/get_user.snap` with redacted content:
   ```
   HTTP 200 OK
   Content-Type: application/json

   {
     "id": 1,
     "name": "Alice",
     "created_at": "[TIMESTAMP]",
     "trace_id": "[UUID]"
   }
   ```

3. Running `rapina test` (without `--bless`) compares against saved snapshots. On mismatch, the test panics with a readable diff showing expected vs actual.

4. Dynamic values (UUIDs, timestamps) are automatically replaced with stable placeholders so snapshots don't break between runs.

### Verification:
- `cargo test -p rapina` passes (unit tests for redaction, snapshot compare)
- `cargo clippy -p rapina -p rapina-cli -- -D warnings` passes (ignoring pre-existing `singularize` warning)
- `cargo fmt --check` passes
- Manual: create a small rapina project, write a test with `.assert_snapshot()`, run `rapina test --bless`, verify `.snap` file created, run `rapina test` and verify it passes, change a handler response and verify `rapina test` fails with a clear diff

## What We're NOT Doing

- Custom redaction rules (user-defined field patterns) — can be added later
- Snapshot approval UI (interactive accept/reject) — `--bless` overwrites all
- Header snapshots — body only for now (headers vary too much across runs)
- Non-JSON response snapshots — only JSON bodies get pretty-printed and redacted; non-JSON bodies are saved as-is
- Integration with `insta` crate — this is a standalone implementation built into the framework

## Implementation Approach

The feature spans two crates:
- **`rapina-cli`**: Adds `--bless` flag, passes `RAPINA_BLESS=1` env var to the cargo subprocess
- **`rapina`**: New `testing/snapshot.rs` module with redaction + compare logic, and a new `.assert_snapshot()` method on `TestResponse`

Communication between CLI and framework is via the `RAPINA_BLESS` environment variable — simple, no IPC needed.

---

## Phase 1: CLI — `--bless` Flag

### Overview
Add the `--bless` argument to `rapina test` and pass it as an environment variable to the test subprocess.

### Changes Required:

#### 1. Add `bless` field to clap args
**File**: `rapina-cli/src/main.rs:84-93`
**Changes**: Add `bless: bool` field to `Commands::Test`

```rust
Test {
    /// Generate coverage report (requires cargo-llvm-cov)
    #[arg(long)]
    coverage: bool,
    /// Watch for changes and re-run tests
    #[arg(short, long)]
    watch: bool,
    /// Update snapshot files (golden-file testing)
    #[arg(long)]
    bless: bool,
    /// Filter tests by name
    filter: Option<String>,
},
```

#### 2. Add `bless` to `TestConfig`
**File**: `rapina-cli/src/commands/test.rs:16-21`
**Changes**: Add `pub bless: bool` field

```rust
pub struct TestConfig {
    pub coverage: bool,
    pub watch: bool,
    pub bless: bool,
    pub filter: Option<String>,
}
```

#### 3. Wire up the match arm
**File**: `rapina-cli/src/main.rs:316-329`
**Changes**: Destructure and pass `bless`

```rust
Some(Commands::Test {
    coverage,
    watch,
    bless,
    filter,
}) => {
    let config = commands::test::TestConfig {
        coverage,
        watch,
        bless,
        filter,
    };
    // ...
}
```

#### 4. Set env var in subprocess
**File**: `rapina-cli/src/commands/test.rs` — `run_tests()` function
**Changes**: When spawning the cargo subprocess, set `RAPINA_BLESS=1` if `config.bless` is true

In `run_tests()`, change the `Command::new` call to conditionally add the env var:

```rust
let mut cmd = Command::new(&cmd);
cmd.args(&args)
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

if config.bless {
    cmd.env("RAPINA_BLESS", "1");
}

let mut child = cmd.spawn()
    .map_err(|e| format!("Failed to run tests: {}", e))?;
```

Also print a message when bless mode is active:

```rust
if config.bless {
    println!(
        "{} Blessing snapshots — new .snap files will be written",
        "INFO".custom_color(colors::blue()).bold()
    );
}
```

#### 5. Update `build_test_command` tests
**File**: `rapina-cli/src/commands/test.rs` — tests module
**Changes**: Existing tests still pass since `bless` defaults to `false` via `Default`. Add one test confirming `bless` doesn't affect the cargo args (it's env-var only, not a cargo flag).

### Success Criteria:

#### Automated Verification:
- [x] `cargo test -p rapina-cli commands::test::tests` passes
- [x] `cargo clippy -p rapina-cli -- -D warnings` passes (ignoring pre-existing warning)
- [x] `cargo fmt --check` passes

---

## Phase 2: Framework — Snapshot Module

### Overview
Create `rapina/src/testing/snapshot.rs` with the core snapshot logic: JSON redaction, file I/O, and diff comparison.

### Changes Required:

#### 1. Create snapshot module
**File**: `rapina/src/testing/snapshot.rs` (new file)

**Public API:**

```rust
/// Check if we're in bless mode (RAPINA_BLESS=1).
pub fn is_bless_mode() -> bool;

/// Redact dynamic values in a JSON value, replacing them with stable placeholders.
pub fn redact(value: &mut serde_json::Value);

/// Format a snapshot: status line + pretty-printed JSON body.
pub fn format_snapshot(status: u16, body: &str) -> String;

/// Assert a response matches its snapshot file.
/// In bless mode, writes the snapshot. Otherwise, compares and panics on diff.
pub fn assert_snapshot(name: &str, status: u16, body: &[u8]);
```

**Redaction strategy — `redact()`:**

Walk the JSON tree recursively. For each string value, apply these replacements:
- UUID v4 pattern (`[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}`) → `"[UUID]"`
- ISO 8601 timestamps (`\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}`) with optional fractional seconds and timezone → `"[TIMESTAMP]"`
- Keys named `trace_id` regardless of value → `"[UUID]"` (explicit, even if value doesn't match UUID pattern)

Use `regex::Regex` with `lazy_static` or `std::sync::OnceLock` for compiled patterns.

**File I/O — `assert_snapshot()`:**

```rust
pub fn assert_snapshot(name: &str, status: u16, body: &[u8]) {
    // 1. Try to parse body as JSON; if valid, redact; if not, use as-is
    let display_body = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(mut val) => {
            redact(&mut val);
            serde_json::to_string_pretty(&val).unwrap()
        }
        Err(_) => String::from_utf8_lossy(body).to_string(),
    };

    let snapshot = format_snapshot(status, &display_body);
    let snap_path = PathBuf::from("snapshots").join(format!("{}.snap", name));

    if is_bless_mode() {
        // Create dir, write file
        fs::create_dir_all(snap_path.parent().unwrap()).unwrap();
        fs::write(&snap_path, &snapshot).unwrap();
        return;
    }

    // Compare mode
    let expected = fs::read_to_string(&snap_path)
        .unwrap_or_else(|_| panic!(
            "Snapshot '{}' not found at {}. Run with --bless to create it.",
            name, snap_path.display()
        ));

    if snapshot != expected {
        panic!(
            "Snapshot '{}' mismatch!\n\n--- expected ({})\n+++ actual\n\n{}",
            name,
            snap_path.display(),
            line_diff(&expected, &snapshot)
        );
    }
}
```

**Diff output — `line_diff()`:**

Simple line-by-line diff (no external crate). For each line:
- Lines only in expected: prefix with `- `
- Lines only in actual: prefix with `+ `
- Matching lines: prefix with `  `

Use a basic longest-common-subsequence or simpler sequential comparison. This doesn't need to be a perfect diff algorithm — just clear enough to spot the change.

```rust
fn line_diff(expected: &str, actual: &str) -> String {
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    let mut output = String::new();

    let max = expected_lines.len().max(actual_lines.len());
    for i in 0..max {
        match (expected_lines.get(i), actual_lines.get(i)) {
            (Some(e), Some(a)) if e == a => {
                output.push_str(&format!("  {}\n", e));
            }
            (Some(e), Some(a)) => {
                output.push_str(&format!("- {}\n", e));
                output.push_str(&format!("+ {}\n", a));
            }
            (Some(e), None) => {
                output.push_str(&format!("- {}\n", e));
            }
            (None, Some(a)) => {
                output.push_str(&format!("+ {}\n", a));
            }
            (None, None) => {}
        }
    }
    output
}
```

**Snapshot file format (`.snap`):**

```
HTTP 200 OK
Content-Type: application/json

{
  "id": 1,
  "name": "Alice",
  "created_at": "[TIMESTAMP]",
  "trace_id": "[UUID]"
}
```

The `format_snapshot` function builds this:

```rust
pub fn format_snapshot(status: u16, body: &str) -> String {
    let reason = reason_phrase(status);
    let content_type = if body.starts_with('{') || body.starts_with('[') {
        "application/json"
    } else {
        "text/plain"
    };
    format!("HTTP {} {}\nContent-Type: {}\n\n{}\n", status, reason, content_type, body)
}
```

#### 2. Add `regex` dependency
**File**: `rapina/Cargo.toml`
**Changes**: Add `regex` to `[dependencies]`

```toml
regex = "1"
```

#### 3. Register the module
**File**: `rapina/src/testing/mod.rs`
**Changes**: Add `mod snapshot;` and re-export

```rust
mod client;
mod snapshot;

pub use client::{TestClient, TestRequestBuilder, TestResponse};
pub use snapshot::assert_snapshot;
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo test -p rapina testing::snapshot::tests` passes
- [x] `cargo clippy -p rapina -- -D warnings` passes
- [x] `cargo fmt --check` passes

---

## Phase 3: `TestResponse` Integration

### Overview
Add `.assert_snapshot(name)` to `TestResponse` so users have a one-liner for snapshot assertions.

### Changes Required:

#### 1. Add method to `TestResponse`
**File**: `rapina/src/testing/client.rs`
**Changes**: Add `assert_snapshot` method to `impl TestResponse`

```rust
impl TestResponse {
    // ... existing methods ...

    /// Asserts the response body matches a saved snapshot.
    ///
    /// In bless mode (`RAPINA_BLESS=1`), saves the response as a new snapshot.
    /// Otherwise, compares against the existing snapshot and panics on mismatch.
    ///
    /// Dynamic values (UUIDs, timestamps) are automatically redacted.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let response = client.get("/users/1").send().await;
    /// response.assert_snapshot("get_user_by_id");
    /// ```
    pub fn assert_snapshot(&self, name: &str) {
        super::snapshot::assert_snapshot(name, self.status.as_u16(), &self.body);
    }
}
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo test -p rapina` passes
- [x] `cargo clippy -p rapina -- -D warnings` passes
- [x] `cargo fmt --check` passes

---

## Phase 4: Tests

### Overview
Unit tests for the snapshot module's pure functions, plus an integration test that exercises the full bless/compare flow using a temp directory.

### Changes Required:

#### 1. Unit tests in snapshot module
**File**: `rapina/src/testing/snapshot.rs` — `#[cfg(test)] mod tests`

Tests for `redact()`:
- `test_redact_uuid` — replaces UUID v4 strings with `[UUID]`
- `test_redact_timestamp` — replaces ISO 8601 timestamps with `[TIMESTAMP]`
- `test_redact_trace_id_key` — redacts `trace_id` field regardless of value format
- `test_redact_nested` — handles nested objects and arrays
- `test_redact_non_string_untouched` — numbers, bools, nulls pass through
- `test_redact_no_false_positives` — normal strings like `"hello"` are not redacted

Tests for `format_snapshot()`:
- `test_format_snapshot_json` — produces correct format with status line and content type
- `test_format_snapshot_200` — `HTTP 200 OK`
- `test_format_snapshot_404` — `HTTP 404 Not Found`

Tests for `line_diff()`:
- `test_diff_identical` — no diff markers
- `test_diff_changed_line` — shows `- ` and `+ ` lines
- `test_diff_added_line` — shows `+ ` for new lines
- `test_diff_removed_line` — shows `- ` for removed lines

Tests for `assert_snapshot()` (using temp dirs):
- `test_bless_creates_file` — set `RAPINA_BLESS=1`, call assert_snapshot, verify file exists with correct content
- `test_compare_passes_on_match` — write a snapshot, call assert_snapshot with matching content
- `test_compare_panics_on_mismatch` — write a snapshot, call assert_snapshot with different content, `#[should_panic]`
- `test_compare_panics_when_missing` — no snapshot file exists, `#[should_panic(expected = "not found")]`

Note: The file I/O tests need to use `std::env::set_current_dir` with a temp dir, or the `assert_snapshot` function should accept an optional base path for testability. Prefer the latter — add an internal `assert_snapshot_in(name, status, body, base_path)` that `assert_snapshot` delegates to with `"snapshots"` as default.

### Success Criteria:

#### Automated Verification:
- [x] `cargo test -p rapina testing::snapshot` passes
- [x] `cargo test -p rapina-cli commands::test::tests` passes
- [x] `cargo clippy -p rapina -p rapina-cli -- -D warnings` passes (ignoring pre-existing warning)
- [x] `cargo fmt --check` passes

#### Manual Verification:
- [ ] Create a minimal rapina project with a JSON endpoint
- [ ] Run `rapina test --bless` — verify `snapshots/*.snap` files are created with redacted content
- [ ] Run `rapina test` — verify tests pass against saved snapshots
- [ ] Change a handler's response — verify `rapina test` fails with a readable diff
- [ ] Run `rapina test --bless` again — verify updated snapshot, tests pass again

---

## Testing Strategy

### Unit Tests:
- Redaction: UUID patterns, timestamp patterns, trace_id key, nested JSON, arrays, non-string values, no false positives
- Format: correct status line, content type detection
- Diff: identical, changed, added, removed lines
- File I/O: bless creates file, compare matches, compare panics on mismatch, compare panics when missing

### Integration Tests:
- Full round-trip: `TestClient` → `.send()` → `.assert_snapshot()` in bless mode → verify file → compare mode → verify pass

## References

- `rapina-cli/src/commands/test.rs` — CLI test command
- `rapina-cli/src/main.rs:84-93` — CLI arg definitions
- `rapina/src/testing/client.rs` — `TestClient`, `TestResponse`
- `rapina/src/testing/mod.rs` — module re-exports
- `rapina/src/context.rs` — `RequestContext` with `trace_id`
- `rapina/src/error.rs:68-143` — error response formats containing `trace_id`
