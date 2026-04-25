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
pub fn detect_flags(cargo: &toml::Value) -> AgentsFlags {
    let features: Vec<String> = cargo
        .get("dependencies")
        .and_then(|d| d.get("rapina"))
        .and_then(|r| r.get("features"))
        .and_then(|f| f.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    AgentsFlags {
        with_db: features
            .iter()
            .any(|f| f == "sqlite" || f == "postgres" || f == "mysql"),
        with_websocket: features.iter().any(|f| f == "websocket"),
        with_jobs: features.iter().any(|f| f == "jobs"),
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
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
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
        std::fs::write(docs_path.join(name), content)
            .map_err(|e| format!("Failed to write .rapina-docs/{}: {}", name, e))?;
    }

    if flags.with_db {
        std::fs::write(
            docs_path.join("migrations.md"),
            include_str!("agents/migrations.md"),
        )
        .map_err(|e| format!("Failed to write .rapina-docs/migrations.md: {}", e))?;
    }
    if flags.with_websocket {
        std::fs::write(
            docs_path.join("websocket.md"),
            include_str!("agents/websocket.md"),
        )
        .map_err(|e| format!("Failed to write .rapina-docs/websocket.md: {}", e))?;
    }
    if flags.with_jobs {
        std::fs::write(docs_path.join("jobs.md"), include_str!("agents/jobs.md"))
            .map_err(|e| format!("Failed to write .rapina-docs/jobs.md: {}", e))?;
    }

    Ok(())
}

// ── Marker parsing ────────────────────────────────────────────────────────────

/// The parsed contents of a `rapina-agent-rules` marker block in `AGENTS.md`.
pub struct ParsedBlock {
    /// CLI version recorded when the block was last written (e.g. `"0.11.0"`).
    /// Retained in the struct so callers can inspect it for debugging, but drift
    /// detection uses only the hash — version bumps alone don't trigger Stale.
    #[allow(dead_code)]
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
    Stale,
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
        None => return DriftStatus::NoBlock,
    };

    // Detect current project flags
    let flags = match super::verify_rapina_project() {
        Ok(cargo) => detect_flags(&cargo),
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
        DriftStatus::Stale
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
     This project uses [Rapina](https://rapina.dev), a Rust web framework.\n\
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
    let source = std::fs::read_to_string(&agents_path).unwrap_or_default();

    // Fall back to empty flags when Cargo.toml is absent or has no rapina dependency
    // (e.g. running `rapina doctor --fix-agents` outside a project). The generated
    // AGENTS.md will include only the always-on fragments; conditional ones are omitted.
    let flags = super::verify_rapina_project()
        .map(|cargo| detect_flags(&cargo))
        .unwrap_or(AgentsFlags {
            with_db: false,
            with_websocket: false,
            with_jobs: false,
        });

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

    #[test]
    fn test_fix_agents_refuses_user_edits_without_force() {
        let dir = tempfile::tempdir().unwrap();
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
        // No Cargo.toml in the temp dir → verify_rapina_project fails → empty AgentsFlags
        // (always-on fragments only). This tests the fresh-creation path, not flag selection.
        let dir = tempfile::tempdir().unwrap();
        fix_agents(dir.path(), false).unwrap();
        assert!(dir.path().join("AGENTS.md").exists());
    }

    #[test]
    fn test_fix_agents_creates_claude_md_when_absent() {
        // No Cargo.toml → empty AgentsFlags (always-on fragments only).
        let dir = tempfile::tempdir().unwrap();
        fix_agents(dir.path(), false).unwrap();
        assert!(dir.path().join("CLAUDE.md").exists());
        let content = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert!(content.contains("@AGENTS.md"));
    }

    #[test]
    fn test_fix_agents_does_not_overwrite_existing_claude_md() {
        let dir = tempfile::tempdir().unwrap();
        let custom = "# My custom claude rules\n";
        std::fs::write(dir.path().join("CLAUDE.md"), custom).unwrap();

        fix_agents(dir.path(), false).unwrap();

        let content = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert_eq!(content, custom);
    }

    #[test]
    fn test_fix_agents_populates_rapina_docs() {
        let dir = tempfile::tempdir().unwrap();
        fix_agents(dir.path(), false).unwrap();
        assert!(dir.path().join(".rapina-docs/core.md").exists());
        assert!(dir.path().join(".rapina-docs/extractors.md").exists());
        assert!(dir.path().join(".rapina-docs/errors.md").exists());
        assert!(dir.path().join(".rapina-docs/testing.md").exists());
    }

    #[test]
    fn test_fix_agents_stale_preserves_surrounding_content() {
        let dir = tempfile::tempdir().unwrap();
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
}
