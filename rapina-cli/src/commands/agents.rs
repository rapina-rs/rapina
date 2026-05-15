//! Agent rules composition, marker parsing, and drift detection helpers.

use sha2::{Digest, Sha256};
use std::path::Path;

// ── Flags ────────────────────────────────────────────────────────────────────

/// Controls which optional fragment files are included when composing `AGENTS.md`.
///
/// Always-on fragments (`core`, `extractors`, `errors`, `testing`) are included regardless.
/// These flags gate the conditional fragments that are only relevant when certain
/// features are active in the project.
pub struct AgentsFlags {
    /// Include `migrations.md`. Set when the project has a database feature (`sqlite`, `postgres`, or `mysql`).
    pub with_db: bool,
    /// Include `websocket.md`. Set when the `websocket` feature is enabled.
    pub with_websocket: bool,
    /// Include `jobs.md`. Set when the `jobs` feature is enabled.
    pub with_jobs: bool,
}

/// Derive `AgentsFlags` by inspecting the `rapina` dependency's features in a parsed `Cargo.toml`.
///
/// Reads `dependencies.rapina.features` and maps feature names to flags:
/// - `sqlite` / `postgres` / `mysql` → `with_db`
/// - `websocket` → `with_websocket`
/// - `jobs` → `with_jobs`
///
/// In monorepo setups where the member declares `rapina.workspace = true` and the
/// real feature list lives in `[workspace.dependencies]` of the workspace root,
/// this single-document inspection misses the inherited features and silently
/// returns all-false flags. Use [`detect_flags_with_workspace`] when the
/// member's directory is known so workspace inheritance is resolved.
pub fn detect_flags(cargo: &toml::Value) -> AgentsFlags {
    let features = read_member_features(cargo);
    flags_from_features(features.iter().map(String::as_str))
}

/// Workspace-aware variant of [`detect_flags`].
///
/// `member_cargo` is the parsed `Cargo.toml` of the project that wants
/// `AGENTS.md`. `member_dir` is the directory that file lives in — used to walk
/// up the tree to find a workspace root.
///
/// Resolution order:
///
/// 1. Read `dependencies.rapina.features` from the member's `Cargo.toml`.
/// 2. If the member declares `rapina.workspace = true`, walk up from `member_dir`
///    to find the first `Cargo.toml` whose top level contains a `[workspace]`
///    table, then read `workspace.dependencies.rapina.features` from it.
/// 3. Union the two feature sets (a feature appearing in both is not
///    double-counted) and map the union onto `AgentsFlags`.
///
/// Errors reading or parsing the workspace `Cargo.toml` fall back to the
/// member-level features rather than aborting — the missing fragments are
/// preferable to a panic in `rapina doctor`-style commands.
pub fn detect_flags_with_workspace(cargo: &toml::Value, member_dir: &Path) -> AgentsFlags {
    let mut features = read_member_features(cargo);

    if member_inherits_rapina_from_workspace(cargo) {
        if let Some(workspace_features) = workspace_rapina_features(member_dir) {
            for feature in workspace_features {
                if !features.contains(&feature) {
                    features.push(feature);
                }
            }
        }
    }

    flags_from_features(features.iter().map(String::as_str))
}

/// Whether the member's `Cargo.toml` says `rapina.workspace = true` (i.e. defers
/// the dependency definition to the workspace root). The check is conservative
/// — anything other than the explicit `workspace = true` form is treated as an
/// inline dependency that already provides its own features.
fn member_inherits_rapina_from_workspace(cargo: &toml::Value) -> bool {
    cargo
        .get("dependencies")
        .and_then(|deps| deps.get("rapina"))
        .and_then(|rapina| match rapina {
            toml::Value::Table(t) => t.get("workspace").and_then(|w| w.as_bool()),
            _ => None,
        })
        .unwrap_or(false)
}

/// Walk up from `member_dir` looking for a `Cargo.toml` whose top level
/// contains a `[workspace]` table; return its
/// `[workspace.dependencies.rapina.features]` array, if any.
fn workspace_rapina_features(member_dir: &Path) -> Option<Vec<String>> {
    let mut dir = member_dir;
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.exists() {
            if let Ok(content) = std::fs::read_to_string(&candidate) {
                if let Ok(parsed) = toml::from_str::<toml::Value>(&content) {
                    if parsed.get("workspace").is_some() {
                        return Some(read_workspace_rapina_features(&parsed));
                    }
                }
            }
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent,
            _ => return None,
        }
    }
}

fn read_member_features(cargo: &toml::Value) -> Vec<String> {
    cargo
        .get("dependencies")
        .and_then(|d| d.get("rapina"))
        .and_then(|r| r.get("features"))
        .and_then(|f| f.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn read_workspace_rapina_features(workspace_cargo: &toml::Value) -> Vec<String> {
    workspace_cargo
        .get("workspace")
        .and_then(|ws| ws.get("dependencies"))
        .and_then(|deps| deps.get("rapina"))
        .and_then(|r| r.get("features"))
        .and_then(|f| f.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn flags_from_features<'a>(features: impl Iterator<Item = &'a str>) -> AgentsFlags {
    let mut with_db = false;
    let mut with_websocket = false;
    let mut with_jobs = false;
    for feature in features {
        match feature {
            "sqlite" | "postgres" | "mysql" => with_db = true,
            "websocket" => with_websocket = true,
            "jobs" => with_jobs = true,
            _ => {}
        }
    }
    AgentsFlags {
        with_db,
        with_websocket,
        with_jobs,
    }
}

// ── Generation ───────────────────────────────────────────────────────────────

/// Compose `AGENTS.md` content from feature-flagged fragments and wrap it in marker tags.
///
/// The always-on fragments (`core`, `extractors`, `errors`, `testing`) are always included.
/// Conditional fragments are appended based on `flags`. The result is wrapped in
/// `<!-- BEGIN:rapina-agent-rules -->` / `<!-- END:rapina-agent-rules -->` markers with a
/// version stamp and SHA256 hash of the body content for drift detection.
pub fn generate_agents_md(flags: &AgentsFlags) -> String {
    let mut fragments: Vec<&str> = vec![
        include_str!("agents/core.md"),
        include_str!("agents/extractors.md"),
        include_str!("agents/errors.md"),
        include_str!("agents/testing.md"),
    ];
    if flags.with_db {
        fragments.push(include_str!("agents/migrations.md"));
    }
    if flags.with_websocket {
        fragments.push(include_str!("agents/websocket.md"));
    }
    if flags.with_jobs {
        fragments.push(include_str!("agents/jobs.md"));
    }
    let body = fragments.join("\n");
    wrap_with_markers(&body)
}

/// Wrap content in `<!-- BEGIN:rapina-agent-rules -->` / `<!-- END:rapina-agent-rules -->` markers.
///
/// The BEGIN marker embeds the current CLI version and a SHA256 hash of `content`.
/// The hash covers exactly the bytes between the markers, so drift detection can distinguish
/// between a clean version bump (hash still matches) and a user edit (hash no longer matches).
pub fn wrap_with_markers(content: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let hash = sha256_hex(content);
    format!(
        "<!-- BEGIN:rapina-agent-rules v{version} sha256:{hash} -->\n{content}\n<!-- END:rapina-agent-rules -->\n"
    )
}

/// Return the SHA256 digest of `s` as a lowercase hex string.
pub fn sha256_hex(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Write individual fragment files into `.rapina-docs/` at the given project root.
///
/// Always-on fragments are written unconditionally. Conditional fragments (`migrations.md`,
/// `websocket.md`, `jobs.md`) are written only when the corresponding flag is set.
/// This mirrors the fragment selection in `generate_agents_md` so agents browsing
/// `.rapina-docs/` see exactly the docs relevant to the project's feature set.
pub fn generate_rapina_docs(project_path: &Path, flags: &AgentsFlags) -> Result<(), String> {
    let docs_path = project_path.join(".rapina-docs");
    std::fs::create_dir_all(&docs_path)
        .map_err(|e| format!("Failed to create .rapina-docs/: {}", e))?;

    let always_on: &[(&str, &str)] = &[
        ("core.md", include_str!("agents/core.md")),
        ("extractors.md", include_str!("agents/extractors.md")),
        ("errors.md", include_str!("agents/errors.md")),
        ("testing.md", include_str!("agents/testing.md")),
    ];
    for (name, content) in always_on {
        let path = docs_path.join(name);
        let existing = std::fs::read(&path).unwrap_or_default();
        if existing != content.as_bytes() {
            std::fs::write(&path, content)
                .map_err(|e| format!("Failed to write .rapina-docs/{}: {}", name, e))?;
        }
    }

    if flags.with_db {
        let path = docs_path.join("migrations.md");
        let content = include_str!("agents/migrations.md");
        let existing = std::fs::read(&path).unwrap_or_default();
        if existing != content.as_bytes() {
            std::fs::write(&path, content)
                .map_err(|e| format!("Failed to write .rapina-docs/migrations.md: {}", e))?;
        }
    }
    if flags.with_websocket {
        let path = docs_path.join("websocket.md");
        let content = include_str!("agents/websocket.md");
        let existing = std::fs::read(&path).unwrap_or_default();
        if existing != content.as_bytes() {
            std::fs::write(&path, content)
                .map_err(|e| format!("Failed to write .rapina-docs/websocket.md: {}", e))?;
        }
    }
    if flags.with_jobs {
        let path = docs_path.join("jobs.md");
        let content = include_str!("agents/jobs.md");
        let existing = std::fs::read(&path).unwrap_or_default();
        if existing != content.as_bytes() {
            std::fs::write(&path, content)
                .map_err(|e| format!("Failed to write .rapina-docs/jobs.md: {}", e))?;
        }
    }

    Ok(())
}

// ── Marker parsing ────────────────────────────────────────────────────────────

/// The parsed contents of a `rapina-agent-rules` marker block in `AGENTS.md`.
pub struct ParsedBlock {
    /// CLI version recorded when the block was last written (e.g. `"0.11.0"`).
    /// Drift detection uses only the hash — version bumps alone don't trigger Stale.
    pub stored_version: String,
    /// SHA256 hex digest of `body` at write time. Used to detect user edits:
    /// if `sha256(body) != stored_hash`, the content was modified after generation.
    pub stored_hash: String,
    /// Content between the BEGIN and END marker lines, not including the markers themselves.
    pub body: String,
    /// Byte offset of the `<!-- BEGIN` marker in the source string. Used by `fix_agents` to splice.
    pub begin_pos: usize,
    /// Byte offset of the first byte after the `<!-- END -->` marker (including its trailing `\n`).
    /// Used by `fix_agents` to splice without re-searching.
    pub end_pos: usize,
}

/// Parse the rapina-agent-rules block from an AGENTS.md file.
/// Returns `None` if no block is found.
pub fn parse_agents_block(source: &str) -> Option<ParsedBlock> {
    let begin_prefix = "<!-- BEGIN:rapina-agent-rules ";
    let end_marker = "<!-- END:rapina-agent-rules -->";

    let begin_pos = source.find(begin_prefix)?;
    let begin_line_end = source[begin_pos..].find("-->")?;
    let begin_line = &source[begin_pos..begin_pos + begin_line_end + 3];

    // Parse version and hash from: <!-- BEGIN:rapina-agent-rules v0.11.0 sha256:abc... -->
    let inner = begin_line
        .trim_start_matches("<!-- BEGIN:rapina-agent-rules ")
        .trim_end_matches(" -->");
    let mut parts = inner.split_whitespace();
    let version = parts.next()?.trim_start_matches('v').to_string();
    let hash_part = parts.next()?;
    let stored_hash = hash_part.trim_start_matches("sha256:").to_string();

    // Extract body between markers
    let after_begin = begin_pos + begin_line_end + 3;
    let body_start = if source[after_begin..].starts_with('\n') {
        after_begin + 1
    } else {
        after_begin
    };

    let end_marker_start = source.find(end_marker)?;
    let body_raw = &source[body_start..end_marker_start];
    // The format string adds \n before <!-- END, so strip exactly one trailing \n
    // to recover the original content that was hashed.
    let body = body_raw.strip_suffix('\n').unwrap_or(body_raw).to_string();

    // end_pos points past the END marker and its trailing newline
    let end_pos_raw = end_marker_start + end_marker.len();
    let end_pos = if source[end_pos_raw..].starts_with('\n') {
        end_pos_raw + 1
    } else {
        end_pos_raw
    };

    Some(ParsedBlock {
        stored_version: version,
        stored_hash,
        body,
        begin_pos,
        end_pos,
    })
}

// ── Drift detection ───────────────────────────────────────────────────────────

/// Result of comparing the on-disk `AGENTS.md` against the current bundled fragments.
pub enum DriftStatus {
    /// SHA256 of the on-disk block body matches what the current CLI would generate. No action needed.
    UpToDate,
    /// The on-disk content is unchanged since it was last written by Rapina
    /// (stored hash matches actual body), but it differs from what the current CLI would generate.
    /// Safe to refresh with `rapina doctor --fix-agents`.
    Stale {
        /// CLI version that last wrote the block (e.g. `"0.11.0"`).
        stored_version: String,
    },
    /// The stored hash in the marker no longer matches the actual on-disk body —
    /// a user edited content inside the markers. Refuse to auto-fix without `--force`.
    UserEdited {
        on_disk_body: String,
        current_body: String,
    },
    /// `AGENTS.md` does not exist. Run `rapina doctor --fix-agents` to generate it.
    Missing,
    /// `AGENTS.md` exists but contains no `rapina-agent-rules` block.
    NoBlock,
    /// Not in a Rapina project (`Cargo.toml` with `rapina` dependency not found).
    NotInProject,
}

/// Compare the on-disk `AGENTS.md` against the current bundled fragments.
///
/// `base` is the project root directory (the directory that should contain `AGENTS.md`
/// and `Cargo.toml`). Reads `Cargo.toml` to detect which optional fragments apply,
/// then applies three-way logic:
/// 1. `sha256(on_disk_body) == sha256(current_body)` → `UpToDate`
/// 2. `sha256(on_disk_body) == stored_hash` (unedited but stale) → `Stale`
/// 3. `sha256(on_disk_body) != stored_hash` (user edited) → `UserEdited`
pub fn check_drift(base: &Path) -> DriftStatus {
    let source = match std::fs::read_to_string(base.join("AGENTS.md")) {
        Ok(s) => s,
        Err(_) => return DriftStatus::Missing,
    };

    let block = match parse_agents_block(&source) {
        Some(b) => b,
        None => {
            if source.contains("<!-- BEGIN:rapina-agent-rules ") {
                eprintln!(
                    "AGENTS.md has an unclosed marker. Run: rapina doctor --fix-agents --force"
                );
            }
            return DriftStatus::NoBlock;
        }
    };

    // Detect current project flags. `verify_rapina_project` reads
    // `./Cargo.toml`, so the cwd is the member directory we need to walk up
    // from when resolving workspace-inherited features.
    let flags = match super::verify_rapina_project() {
        Ok(cargo) => match std::env::current_dir() {
            Ok(cwd) => detect_flags_with_workspace(&cargo, &cwd),
            // If we can't resolve cwd we still have member-level features.
            Err(_) => detect_flags(&cargo),
        },
        Err(_) => return DriftStatus::NotInProject,
    };

    // What we'd generate now (body only, between markers)
    let current_full = generate_agents_md(&flags);
    let current_block =
        parse_agents_block(&current_full).expect("generated AGENTS.md must have a block");
    let current_body = current_block.body;
    let current_hash = sha256_hex(&current_body);

    let on_disk_hash = sha256_hex(&block.body);

    if on_disk_hash == current_hash {
        DriftStatus::UpToDate
    } else if on_disk_hash == block.stored_hash {
        // Content matches what Rapina last wrote — safe to refresh
        DriftStatus::Stale {
            stored_version: block.stored_version,
        }
    } else {
        // Hash in marker != actual on-disk content → user edited
        DriftStatus::UserEdited {
            on_disk_body: block.body,
            current_body,
        }
    }
}

/// Return the content to write to `CLAUDE.md`.
///
/// Includes the `@AGENTS.md` include directive (Claude Code CLI syntax) plus a brief
/// human-readable header so the file is understandable outside Claude Code.
pub fn generate_claude_md() -> &'static str {
    "# Claude Rules\n\
     \n\
     This project uses [Rapina](https://userapina.com), a Rust web framework.\n\
     \n\
     For Rapina-specific conventions (route handlers, extractors, error handling, testing),\n\
     see [AGENTS.md](./AGENTS.md).\n\
     \n\
     @AGENTS.md\n"
}

/// Rewrite the `rapina-agent-rules` block in `AGENTS.md` with current bundled content.
///
/// Preserves any text outside the markers (e.g. custom rules the user added above or below).
/// Refuses to overwrite user edits inside the markers unless `force` is `true` — detected by
/// comparing `sha256(on_disk_body)` against `stored_hash` in the marker.
///
/// Also creates `CLAUDE.md` (if absent) and refreshes `.rapina-docs/` to match the new block.
pub fn fix_agents(base: &Path, force: bool) -> Result<(), String> {
    let agents_path = base.join("AGENTS.md");
    let source = match std::fs::read_to_string(&agents_path) {
        Ok(s) => s,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                String::new()
            } else {
                return Err(format!("failed to read {}: {e}", agents_path.display()));
            }
        }
    };

    let cargo_path = base.join("Cargo.toml");
    let flags = match std::fs::read_to_string(&cargo_path) {
        // Cargo.toml found — detect which optional fragments to include.
        // If the file can't be parsed, fall back to always-on fragments only.
        Ok(content) => toml::from_str::<toml::Value>(&content)
            .map(|parsed| detect_flags_with_workspace(&parsed, base))
            .unwrap_or_else(|e| {
                eprintln!(
                    "Warning: could not parse Cargo.toml ({e}), generating base fragments only"
                );
                AgentsFlags {
                    with_db: false,
                    with_websocket: false,
                    with_jobs: false,
                }
            }),
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                // No Cargo.toml — refuse rather than silently generating a flags-less file.
                return Err(format!(
                    "no Cargo.toml found in {}. Run this command from a Rust project directory.",
                    base.display()
                ));
            } else {
                // Any other IO error (permissions, etc.) should fail loudly.
                return Err(format!("failed to read {}: {e}", cargo_path.display()));
            }
        }
    };

    // Check for user edits inside markers before touching anything
    let existing_block = if source.is_empty() {
        None
    } else {
        parse_agents_block(&source)
    };

    if let Some(ref block) = existing_block {
        let on_disk_hash = sha256_hex(&block.body);
        if on_disk_hash != block.stored_hash && !force {
            return Err("AGENTS.md has been edited inside the markers. \
                 Move custom rules outside the markers, then re-run. \
                 Use --force to overwrite anyway."
                .to_string());
        }
    }

    let new_block = generate_agents_md(&flags);

    // Replace old block using the stored positions (no second parse, no unwraps)
    let new_content = if let Some(block) = existing_block {
        let before = &source[..block.begin_pos];
        let after = &source[block.end_pos..];
        format!("{}{}{}", before, new_block, after)
    } else {
        new_block
    };

    std::fs::write(&agents_path, new_content)
        .map_err(|e| format!("Failed to write AGENTS.md: {}", e))?;

    // Create CLAUDE.md only if absent — never overwrite user customisations
    let claude_path = base.join("CLAUDE.md");
    if !claude_path.exists() {
        std::fs::write(&claude_path, generate_claude_md())
            .map_err(|e| format!("Failed to write CLAUDE.md: {}", e))?;
    }

    // Refresh .rapina-docs/ to match the regenerated block
    generate_rapina_docs(base, &flags)?;

    Ok(())
}

/// Produce a simple line-level diff between `old` and `new` for human display.
///
/// Lines present only in `old` are prefixed with `- `; lines present only in `new`
/// with `+ `. Lines present in both (accounting for duplicates) are omitted. This is not a
/// true LCS diff — it uses count-based multiset difference, sufficient for `AGENTS.md` drift display.
pub fn simple_diff(old: &str, new: &str) -> String {
    use std::collections::HashMap;

    let mut old_counts: HashMap<&str, usize> = HashMap::new();
    let mut new_counts: HashMap<&str, usize> = HashMap::new();
    for line in old.lines() {
        *old_counts.entry(line).or_insert(0) += 1;
    }
    for line in new.lines() {
        *new_counts.entry(line).or_insert(0) += 1;
    }

    // Track how many minus/plus lines we have already emitted for each unique line.
    let mut removed_emitted: HashMap<&str, usize> = HashMap::new();
    let mut added_emitted: HashMap<&str, usize> = HashMap::new();

    // Output order follows source-line order (we iterate over .lines(), not the HashMaps),
    // so the result is deterministic even though the count tables use HashMap.
    let mut out = String::new();
    for line in old.lines() {
        let quota = old_counts
            .get(line)
            .copied()
            .unwrap_or(0)
            .saturating_sub(new_counts.get(line).copied().unwrap_or(0));
        let emitted = removed_emitted.entry(line).or_insert(0);
        if *emitted < quota {
            *emitted += 1;
            out.push_str(&format!("- {}\n", line));
        }
    }
    for line in new.lines() {
        let quota = new_counts
            .get(line)
            .copied()
            .unwrap_or(0)
            .saturating_sub(old_counts.get(line).copied().unwrap_or(0));
        let emitted = added_emitted.entry(line).or_insert(0);
        if *emitted < quota {
            *emitted += 1;
            out.push_str(&format!("+ {}\n", line));
        }
    }
    if out.is_empty() {
        out.push_str("(whitespace or ordering differs)\n");
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Writes a minimal Cargo.toml with a rapina dependency so fix_agents doesn't
    // error on the missing-Cargo.toml guard. Tests that care about feature flags
    // should write their own Cargo.toml instead of using this helper.
    fn write_minimal_cargo_toml(dir: &std::path::Path) {
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n\n[dependencies]\nrapina = \"0.1.0\"\n",
        )
        .unwrap();
    }

    #[test]
    fn test_parse_agents_block_roundtrip() {
        let flags = AgentsFlags {
            with_db: false,
            with_websocket: false,
            with_jobs: false,
        };
        let generated = generate_agents_md(&flags);
        let parsed = parse_agents_block(&generated).expect("must parse");
        assert!(!parsed.stored_version.is_empty());
        assert_eq!(parsed.stored_hash.len(), 64); // SHA256 hex
        assert!(parsed.body.contains("Rapina"));
    }

    #[test]
    fn test_parse_agents_block_hash_integrity() {
        let flags = AgentsFlags {
            with_db: false,
            with_websocket: false,
            with_jobs: false,
        };
        let generated = generate_agents_md(&flags);
        let parsed = parse_agents_block(&generated).expect("must parse");
        assert_eq!(parsed.stored_hash, sha256_hex(&parsed.body));
    }

    #[test]
    fn test_parse_agents_block_with_surrounding_content() {
        let flags = AgentsFlags {
            with_db: true,
            with_websocket: false,
            with_jobs: false,
        };
        let block = generate_agents_md(&flags);
        let source = format!("# Custom header\n\n{block}\n## Custom footer\n");
        let parsed = parse_agents_block(&source).expect("must parse");
        assert_eq!(parsed.stored_hash, sha256_hex(&parsed.body));
    }

    #[test]
    fn test_parse_agents_block_returns_none_when_missing() {
        assert!(parse_agents_block("# No markers here").is_none());
        assert!(parse_agents_block("").is_none());
    }

    #[test]
    fn test_detect_flags_with_sqlite() {
        let cargo: toml::Value = toml::from_str(
            r#"[dependencies]
rapina = { version = "0.11", features = ["sqlite"] }"#,
        )
        .unwrap();
        let flags = detect_flags(&cargo);
        assert!(flags.with_db);
        assert!(!flags.with_websocket);
        assert!(!flags.with_jobs);
    }

    #[test]
    fn test_detect_flags_with_websocket_and_jobs() {
        let cargo: toml::Value = toml::from_str(
            r#"[dependencies]
rapina = { version = "0.11", features = ["postgres", "websocket", "jobs"] }"#,
        )
        .unwrap();
        let flags = detect_flags(&cargo);
        assert!(flags.with_db);
        assert!(flags.with_websocket);
        assert!(flags.with_jobs);
    }

    #[test]
    fn test_detect_flags_no_features() {
        let cargo: toml::Value = toml::from_str(
            r#"[dependencies]
rapina = "0.11""#,
        )
        .unwrap();
        let flags = detect_flags(&cargo);
        assert!(!flags.with_db);
        assert!(!flags.with_websocket);
        assert!(!flags.with_jobs);
    }

    /// Regression for #545: when the member uses
    /// `rapina.workspace = true`, `detect_flags_with_workspace` must read
    /// features from the workspace root's `[workspace.dependencies]` rather
    /// than silently returning `with_db: false`.
    #[test]
    fn test_detect_flags_with_workspace_inherits_features_from_root() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(
            workspace.path().join("Cargo.toml"),
            r#"[workspace]
members = ["api"]

[workspace.dependencies]
rapina = { version = "0.11", features = ["postgres"] }
"#,
        )
        .unwrap();

        let member_dir = workspace.path().join("api");
        std::fs::create_dir_all(&member_dir).unwrap();
        let member_cargo: toml::Value = toml::from_str(
            r#"[package]
name = "api"
version = "0.1.0"

[dependencies]
rapina.workspace = true
"#,
        )
        .unwrap();

        let flags = detect_flags_with_workspace(&member_cargo, &member_dir);
        assert!(
            flags.with_db,
            "workspace.dependencies postgres should set with_db"
        );
        assert!(!flags.with_websocket);
        assert!(!flags.with_jobs);
    }

    /// When features are split across the member and the workspace root,
    /// `detect_flags_with_workspace` must merge them so every fragment that
    /// any layer asks for is enabled.
    #[test]
    fn test_detect_flags_with_workspace_merges_member_and_root_features() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(
            workspace.path().join("Cargo.toml"),
            r#"[workspace]
members = ["api"]

[workspace.dependencies]
rapina = { version = "0.11", features = ["postgres"] }
"#,
        )
        .unwrap();

        let member_dir = workspace.path().join("api");
        std::fs::create_dir_all(&member_dir).unwrap();
        // Cargo allows a member to add features on top of the workspace-inherited
        // set: `{ workspace = true, features = ["extra"] }`. The merge test
        // verifies that both layers of features contribute to the final flags.
        let member_cargo: toml::Value = toml::from_str(
            r#"[package]
name = "api"
version = "0.1.0"

[dependencies]
rapina = { workspace = true, features = ["websocket"] }
"#,
        )
        .unwrap();

        let flags = detect_flags_with_workspace(&member_cargo, &member_dir);
        assert!(flags.with_db);
        assert!(flags.with_websocket);
        assert!(!flags.with_jobs);
    }

    /// When the member package is also the workspace root, the workspace walk
    /// must inspect `member_dir` itself so root-owned workspace dependencies
    /// are inherited.
    #[test]
    fn test_detect_flags_with_workspace_when_member_is_workspace_root() {
        let workspace = tempfile::tempdir().unwrap();
        let cargo_toml = r#"[package]
name = "api"
version = "0.1.0"

[workspace]

[dependencies]
rapina = { workspace = true }

[workspace.dependencies]
rapina = { version = "0.11", features = ["postgres"] }
"#;
        std::fs::write(workspace.path().join("Cargo.toml"), cargo_toml).unwrap();
        let cargo_value: toml::Value = toml::from_str(cargo_toml).unwrap();

        let flags = detect_flags_with_workspace(&cargo_value, workspace.path());
        assert!(flags.with_db);
        assert!(!flags.with_websocket);
        assert!(!flags.with_jobs);
    }

    /// Duplicates between member and workspace must not be treated as an
    /// error: `flags_from_features` is set-style, so a feature appearing in
    /// both places maps to the same flag once.
    #[test]
    fn test_detect_flags_with_workspace_dedupes_overlapping_features() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(
            workspace.path().join("Cargo.toml"),
            r#"[workspace]
members = ["api"]

[workspace.dependencies]
rapina = { version = "0.11", features = ["postgres"] }
"#,
        )
        .unwrap();

        let member_dir = workspace.path().join("api");
        std::fs::create_dir_all(&member_dir).unwrap();
        let member_cargo: toml::Value = toml::from_str(
            r#"[package]
name = "api"
version = "0.1.0"

[dependencies]
rapina = { workspace = true, features = ["postgres"] }
"#,
        )
        .unwrap();

        let flags = detect_flags_with_workspace(&member_cargo, &member_dir);
        assert!(flags.with_db);
        assert!(!flags.with_websocket);
        assert!(!flags.with_jobs);
    }

    /// Non-workspace projects (i.e. the existing happy path) still take
    /// features only from the member's own `[dependencies]` block —
    /// `detect_flags_with_workspace` must NOT walk up looking for an unrelated
    /// parent `Cargo.toml` when the member declares `rapina` inline.
    #[test]
    fn test_detect_flags_with_workspace_ignores_root_when_member_declares_inline() {
        let outer = tempfile::tempdir().unwrap();
        // A parent Cargo.toml with a [workspace] table that defines a
        // different rapina feature set. The member doesn't reference it.
        std::fs::write(
            outer.path().join("Cargo.toml"),
            r#"[workspace]
members = ["api"]

[workspace.dependencies]
rapina = { version = "0.11", features = ["postgres", "websocket"] }
"#,
        )
        .unwrap();

        let member_dir = outer.path().join("api");
        std::fs::create_dir_all(&member_dir).unwrap();
        let member_cargo: toml::Value = toml::from_str(
            r#"[package]
name = "api"
version = "0.1.0"

[dependencies]
rapina = { version = "0.11", features = ["jobs"] }
"#,
        )
        .unwrap();

        let flags = detect_flags_with_workspace(&member_cargo, &member_dir);
        assert!(
            !flags.with_db,
            "inline rapina dep must not inherit workspace features"
        );
        assert!(!flags.with_websocket);
        assert!(flags.with_jobs);
    }

    /// If the workspace root's `Cargo.toml` is missing or unparseable, the
    /// resolver falls back to the member's own features rather than
    /// panicking.
    #[test]
    fn test_detect_flags_with_workspace_missing_root_falls_back_to_member() {
        let dir = tempfile::tempdir().unwrap();
        // No parent Cargo.toml at all. `member_dir` is just a leaf directory.
        let member_cargo: toml::Value = toml::from_str(
            r#"[package]
name = "api"
version = "0.1.0"

[dependencies]
rapina = { workspace = true }
"#,
        )
        .unwrap();

        let flags = detect_flags_with_workspace(&member_cargo, dir.path());
        assert!(!flags.with_db);
        assert!(!flags.with_websocket);
        assert!(!flags.with_jobs);
    }

    #[test]
    fn test_fix_agents_refuses_user_edits_without_force() {
        let dir = tempfile::tempdir().unwrap();
        write_minimal_cargo_toml(dir.path());
        let flags = AgentsFlags {
            with_db: false,
            with_websocket: false,
            with_jobs: false,
        };
        let generated = generate_agents_md(&flags);
        // Simulate user edit inside the markers (hash in header no longer matches body)
        let tampered = generated.replace("# Rapina Project", "# Rapina Project\n\nmy custom rule");
        std::fs::write(dir.path().join("AGENTS.md"), &tampered).unwrap();

        let err = fix_agents(dir.path(), false).unwrap_err();
        assert!(
            err.contains("edited inside the markers"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_fix_agents_force_overwrites_user_edits() {
        let dir = tempfile::tempdir().unwrap();
        write_minimal_cargo_toml(dir.path());
        let flags = AgentsFlags {
            with_db: false,
            with_websocket: false,
            with_jobs: false,
        };
        let generated = generate_agents_md(&flags);
        let tampered = generated.replace("# Rapina Project", "# Rapina Project\n\nmy custom rule");
        std::fs::write(dir.path().join("AGENTS.md"), &tampered).unwrap();

        fix_agents(dir.path(), true).unwrap();

        let result = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        // After force, hash should match body again
        let block = parse_agents_block(&result).expect("must parse");
        assert_eq!(sha256_hex(&block.body), block.stored_hash);
    }

    #[test]
    fn test_fix_agents_creates_fresh_when_missing() {
        // Tests the fresh-creation path (no existing AGENTS.md), not flag selection.
        let dir = tempfile::tempdir().unwrap();
        write_minimal_cargo_toml(dir.path());
        fix_agents(dir.path(), false).unwrap();
        assert!(dir.path().join("AGENTS.md").exists());
    }

    #[test]
    fn test_fix_agents_creates_claude_md_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        write_minimal_cargo_toml(dir.path());
        fix_agents(dir.path(), false).unwrap();
        assert!(dir.path().join("CLAUDE.md").exists());
        let content = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert!(content.contains("@AGENTS.md"));
    }

    #[test]
    fn test_fix_agents_does_not_overwrite_existing_claude_md() {
        let dir = tempfile::tempdir().unwrap();
        write_minimal_cargo_toml(dir.path());
        let custom = "# My custom claude rules\n";
        std::fs::write(dir.path().join("CLAUDE.md"), custom).unwrap();

        fix_agents(dir.path(), false).unwrap();

        let content = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert_eq!(content, custom);
    }

    #[test]
    fn test_fix_agents_populates_rapina_docs() {
        let dir = tempfile::tempdir().unwrap();
        write_minimal_cargo_toml(dir.path());
        fix_agents(dir.path(), false).unwrap();
        assert!(dir.path().join(".rapina-docs/core.md").exists());
        assert!(dir.path().join(".rapina-docs/extractors.md").exists());
        assert!(dir.path().join(".rapina-docs/errors.md").exists());
        assert!(dir.path().join(".rapina-docs/testing.md").exists());
    }

    #[test]
    fn test_fix_agents_preserves_surrounding_content() {
        let dir = tempfile::tempdir().unwrap();
        write_minimal_cargo_toml(dir.path());
        let flags = AgentsFlags {
            with_db: false,
            with_websocket: false,
            with_jobs: false,
        };
        let block = generate_agents_md(&flags);
        let source = format!("# Custom header\n\n{block}\n## Custom footer\n");
        std::fs::write(dir.path().join("AGENTS.md"), &source).unwrap();

        fix_agents(dir.path(), false).unwrap();

        let result = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(
            result.starts_with("# Custom header\n"),
            "header lost: {result}"
        );
        assert!(
            result.contains("## Custom footer\n"),
            "footer lost: {result}"
        );
    }

    #[test]
    fn test_fix_agents_updates_stale_block() {
        let dir = tempfile::tempdir().unwrap();

        // Step 1: Cargo.toml declares postgres — detect_flags will return with_db: true.
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n\n[dependencies]\nrapina = { version = \"0.1.0\", features = [\"postgres\"] }\n",
        ).unwrap();

        // Step 2: Write a stale AGENTS.md — block was generated without the db fragment.
        let stale_flags = AgentsFlags {
            with_db: false,
            with_websocket: false,
            with_jobs: false,
        };
        std::fs::write(
            dir.path().join("AGENTS.md"),
            generate_agents_md(&stale_flags),
        )
        .unwrap();

        // Step 3: fix_agents detects with_db: true from Cargo.toml and regenerates the block.
        fix_agents(dir.path(), false).unwrap();

        let result = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        let block = parse_agents_block(&result).expect("block must be present");

        // Step 4: The updated block must include the migrations fragment and have a valid hash.
        assert!(
            block.body.contains("migration"),
            "migrations fragment missing: {}",
            block.body
        );
        assert_eq!(sha256_hex(&block.body), block.stored_hash);
    }

    #[test]
    fn test_simple_diff_shows_changes() {
        let old = "line1\nline2\nline3";
        let new = "line1\nline3\nline4";
        let diff = simple_diff(old, new);
        assert!(diff.contains("- line2"));
        assert!(diff.contains("+ line4"));
        assert!(!diff.contains("line1"));
        assert!(!diff.contains("line3"));
    }

    #[test]
    fn test_simple_diff_handles_duplicate_lines() {
        // old has "x" twice, new has it once — one removal, no additions
        let old = "x\nx\ny";
        let new = "x\ny";
        let diff = simple_diff(old, new);
        assert!(diff.contains("- x"));
        assert!(!diff.contains("+ x"));
        // exactly one removal
        assert_eq!(diff.lines().filter(|l| l.starts_with("- x")).count(), 1);
    }

    #[test]
    fn test_generate_rapina_docs_skips_write_when_content_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let flags = AgentsFlags {
            with_db: false,
            with_websocket: false,
            with_jobs: false,
        };

        // First call: writes all files.
        generate_rapina_docs(dir.path(), &flags).unwrap();

        let core_path = dir.path().join(".rapina-docs/core.md");
        let mtime_before = std::fs::metadata(&core_path).unwrap().modified().unwrap();

        // Small sleep so that a write would produce a different mtime.
        // NOTE: assumes sub-second filesystem precision (APFS, ext4). On 1-second
        // resolution filesystems (HFS+, FAT32) a write within the same second would
        // be undetectable via mtime — content comparison cannot substitute here
        // because written bytes are identical either way.
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Second call: content identical → no write → mtime unchanged.
        generate_rapina_docs(dir.path(), &flags).unwrap();

        let mtime_after = std::fs::metadata(&core_path).unwrap().modified().unwrap();

        assert_eq!(
            mtime_before, mtime_after,
            "core.md mtime changed on second call — file was rewritten unnecessarily"
        );
    }

    #[test]
    fn test_generate_rapina_docs_writes_when_content_changed() {
        let dir = tempfile::tempdir().unwrap();
        let flags = AgentsFlags {
            with_db: false,
            with_websocket: false,
            with_jobs: false,
        };

        // First call: writes all files.
        generate_rapina_docs(dir.path(), &flags).unwrap();

        let core_path = dir.path().join(".rapina-docs/core.md");

        // Tamper with the file.
        std::fs::write(&core_path, b"tampered content").unwrap();

        let mtime_tampered = std::fs::metadata(&core_path).unwrap().modified().unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));

        // Second call: content differs → file must be rewritten.
        generate_rapina_docs(dir.path(), &flags).unwrap();

        let mtime_after = std::fs::metadata(&core_path).unwrap().modified().unwrap();

        assert_ne!(
            mtime_tampered, mtime_after,
            "core.md was NOT rewritten after content changed"
        );

        // Content must now match the embedded source.
        let written = std::fs::read_to_string(&core_path).unwrap();
        assert_eq!(written, include_str!("agents/core.md"));
    }

    #[test]
    fn test_generate_rapina_docs_skips_write_for_all_always_on_files() {
        let dir = tempfile::tempdir().unwrap();
        let flags = AgentsFlags {
            with_db: false,
            with_websocket: false,
            with_jobs: false,
        };

        generate_rapina_docs(dir.path(), &flags).unwrap();

        let names = ["core.md", "extractors.md", "errors.md", "testing.md"];
        let mtimes_before: Vec<_> = names
            .iter()
            .map(|name| {
                std::fs::metadata(dir.path().join(format!(".rapina-docs/{name}")))
                    .unwrap()
                    .modified()
                    .unwrap()
            })
            .collect();

        // NOTE: assumes sub-second filesystem precision (APFS, ext4). On 1-second
        // resolution filesystems (HFS+, FAT32) a write within the same second would
        // be undetectable via mtime — content comparison cannot substitute here
        // because written bytes are identical either way.
        std::thread::sleep(std::time::Duration::from_millis(10));
        generate_rapina_docs(dir.path(), &flags).unwrap();

        for (name, mtime_before) in names.iter().zip(mtimes_before.iter()) {
            let mtime_after = std::fs::metadata(dir.path().join(format!(".rapina-docs/{name}")))
                .unwrap()
                .modified()
                .unwrap();
            assert_eq!(
                *mtime_before, mtime_after,
                "{name} mtime changed on second call — unnecessary write"
            );
        }
    }

    #[test]
    fn test_generate_rapina_docs_conditional_db_skips_write_when_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let flags = AgentsFlags {
            with_db: true,
            with_websocket: false,
            with_jobs: false,
        };

        generate_rapina_docs(dir.path(), &flags).unwrap();

        let path = dir.path().join(".rapina-docs/migrations.md");
        let mtime_before = std::fs::metadata(&path).unwrap().modified().unwrap();

        // NOTE: assumes sub-second filesystem precision (APFS, ext4).
        std::thread::sleep(std::time::Duration::from_millis(10));
        generate_rapina_docs(dir.path(), &flags).unwrap();

        let mtime_after = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(
            mtime_before, mtime_after,
            "migrations.md rewritten unnecessarily"
        );
    }

    #[test]
    fn test_generate_rapina_docs_conditional_db_writes_when_changed() {
        let dir = tempfile::tempdir().unwrap();
        let flags = AgentsFlags {
            with_db: true,
            with_websocket: false,
            with_jobs: false,
        };

        generate_rapina_docs(dir.path(), &flags).unwrap();

        let path = dir.path().join(".rapina-docs/migrations.md");
        std::fs::write(&path, b"tampered").unwrap();
        let mtime_tampered = std::fs::metadata(&path).unwrap().modified().unwrap();

        // NOTE: assumes sub-second filesystem precision (APFS, ext4).
        std::thread::sleep(std::time::Duration::from_millis(10));
        generate_rapina_docs(dir.path(), &flags).unwrap();

        let mtime_after = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_ne!(
            mtime_tampered, mtime_after,
            "migrations.md not rewritten after tamper"
        );

        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(written, include_str!("agents/migrations.md"));
    }

    #[test]
    fn test_generate_rapina_docs_conditional_websocket_skips_write_when_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let flags = AgentsFlags {
            with_db: false,
            with_websocket: true,
            with_jobs: false,
        };

        generate_rapina_docs(dir.path(), &flags).unwrap();

        let path = dir.path().join(".rapina-docs/websocket.md");
        let mtime_before = std::fs::metadata(&path).unwrap().modified().unwrap();

        // NOTE: assumes sub-second filesystem precision (APFS, ext4).
        std::thread::sleep(std::time::Duration::from_millis(10));
        generate_rapina_docs(dir.path(), &flags).unwrap();

        let mtime_after = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(
            mtime_before, mtime_after,
            "websocket.md rewritten unnecessarily"
        );
    }

    #[test]
    fn test_generate_rapina_docs_conditional_websocket_writes_when_changed() {
        let dir = tempfile::tempdir().unwrap();
        let flags = AgentsFlags {
            with_db: false,
            with_websocket: true,
            with_jobs: false,
        };

        generate_rapina_docs(dir.path(), &flags).unwrap();

        let path = dir.path().join(".rapina-docs/websocket.md");
        std::fs::write(&path, b"tampered").unwrap();
        let mtime_tampered = std::fs::metadata(&path).unwrap().modified().unwrap();

        // NOTE: assumes sub-second filesystem precision (APFS, ext4).
        std::thread::sleep(std::time::Duration::from_millis(10));
        generate_rapina_docs(dir.path(), &flags).unwrap();

        let mtime_after = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_ne!(
            mtime_tampered, mtime_after,
            "websocket.md not rewritten after tamper"
        );

        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(written, include_str!("agents/websocket.md"));
    }

    #[test]
    fn test_generate_rapina_docs_conditional_jobs_skips_write_when_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let flags = AgentsFlags {
            with_db: false,
            with_websocket: false,
            with_jobs: true,
        };

        generate_rapina_docs(dir.path(), &flags).unwrap();

        let path = dir.path().join(".rapina-docs/jobs.md");
        let mtime_before = std::fs::metadata(&path).unwrap().modified().unwrap();

        // NOTE: assumes sub-second filesystem precision (APFS, ext4).
        std::thread::sleep(std::time::Duration::from_millis(10));
        generate_rapina_docs(dir.path(), &flags).unwrap();

        let mtime_after = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(mtime_before, mtime_after, "jobs.md rewritten unnecessarily");
    }

    #[test]
    fn test_generate_rapina_docs_conditional_jobs_writes_when_changed() {
        let dir = tempfile::tempdir().unwrap();
        let flags = AgentsFlags {
            with_db: false,
            with_websocket: false,
            with_jobs: true,
        };

        generate_rapina_docs(dir.path(), &flags).unwrap();

        let path = dir.path().join(".rapina-docs/jobs.md");
        std::fs::write(&path, b"tampered").unwrap();
        let mtime_tampered = std::fs::metadata(&path).unwrap().modified().unwrap();

        // NOTE: assumes sub-second filesystem precision (APFS, ext4).
        std::thread::sleep(std::time::Duration::from_millis(10));
        generate_rapina_docs(dir.path(), &flags).unwrap();

        let mtime_after = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_ne!(
            mtime_tampered, mtime_after,
            "jobs.md not rewritten after tamper"
        );

        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(written, include_str!("agents/jobs.md"));
    }

    #[test]
    fn test_generate_rapina_docs_all_flags_true_skips_write_when_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let flags = AgentsFlags {
            with_db: true,
            with_websocket: true,
            with_jobs: true,
        };

        generate_rapina_docs(dir.path(), &flags).unwrap();

        let names = [
            "core.md",
            "extractors.md",
            "errors.md",
            "testing.md",
            "migrations.md",
            "websocket.md",
            "jobs.md",
        ];
        let mtimes_before: Vec<_> = names
            .iter()
            .map(|name| {
                std::fs::metadata(dir.path().join(format!(".rapina-docs/{name}")))
                    .unwrap()
                    .modified()
                    .unwrap()
            })
            .collect();

        // NOTE: assumes sub-second filesystem precision (APFS, ext4).
        std::thread::sleep(std::time::Duration::from_millis(10));
        generate_rapina_docs(dir.path(), &flags).unwrap();

        for (name, mtime_before) in names.iter().zip(mtimes_before.iter()) {
            let mtime_after = std::fs::metadata(dir.path().join(format!(".rapina-docs/{name}")))
                .unwrap()
                .modified()
                .unwrap();
            assert_eq!(
                *mtime_before, mtime_after,
                "{name} mtime changed on second call with all flags true — unnecessary write"
            );
        }
    }

    #[test]
    fn test_generate_rapina_docs_conditional_flags_false_does_not_create_files() {
        let dir = tempfile::tempdir().unwrap();
        let flags = AgentsFlags {
            with_db: false,
            with_websocket: false,
            with_jobs: false,
        };

        generate_rapina_docs(dir.path(), &flags).unwrap();

        assert!(
            !dir.path().join(".rapina-docs/migrations.md").exists(),
            "migrations.md must not exist when with_db=false"
        );
        assert!(
            !dir.path().join(".rapina-docs/websocket.md").exists(),
            "websocket.md must not exist when with_websocket=false"
        );
        assert!(
            !dir.path().join(".rapina-docs/jobs.md").exists(),
            "jobs.md must not exist when with_jobs=false"
        );
    }
}
