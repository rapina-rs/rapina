use colored::Colorize;
use std::fs;
use std::path::Path;

use super::verify_rapina_project;

#[cfg(feature = "jobs")]
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};

/// Set up the background jobs migration in the current project.
///
/// Creates or updates `src/migrations/mod.rs` to include the framework's
/// `create_rapina_jobs` migration. Safe to run multiple times — skips
/// if the migration is already referenced.
pub fn init() -> Result<(), String> {
    verify_rapina_project()?;

    let migrations_dir = Path::new("src/migrations");

    if !migrations_dir.exists() {
        fs::create_dir_all(migrations_dir)
            .map_err(|e| format!("Failed to create migrations directory: {e}"))?;
        println!("  {} Created {}", "✓".green(), "src/migrations/".cyan());
    }

    let mod_path = migrations_dir.join("mod.rs");

    if mod_path.exists() {
        let content =
            fs::read_to_string(&mod_path).map_err(|e| format!("Failed to read mod.rs: {e}"))?;

        if is_already_configured(&content) {
            println!(
                "  {} Background jobs migration already configured",
                "✓".green()
            );
            return Ok(());
        }

        let updated = inject_into_existing(&content);
        fs::write(&mod_path, updated).map_err(|e| format!("Failed to update mod.rs: {e}"))?;
    } else {
        fs::write(&mod_path, fresh_mod_rs())
            .map_err(|e| format!("Failed to create mod.rs: {e}"))?;
    }

    println!(
        "  {} Added background jobs migration to {}",
        "✓".green(),
        "src/migrations/mod.rs".cyan()
    );
    println!();
    println!("  Run {} to apply the migration.", "rapina migrate".cyan());

    Ok(())
}

/// Whether `create_rapina_jobs` is already referenced in the mod.rs content.
fn is_already_configured(content: &str) -> bool {
    content.contains("create_rapina_jobs")
}

/// Prepend the `use` import and insert into the `migrations!` macro.
fn inject_into_existing(content: &str) -> String {
    let with_use = format!("use rapina::jobs::create_rapina_jobs;\n{content}");
    super::migrate::add_to_migrations_macro(&with_use, "create_rapina_jobs")
}

/// Content for a brand-new `src/migrations/mod.rs`.
fn fresh_mod_rs() -> &'static str {
    "use rapina::jobs::create_rapina_jobs;\n\
     \n\
     rapina::migrations! {\n\
     \x20   create_rapina_jobs,\n\
     }\n"
}

/// Query `rapina_jobs` and display counts by status.
///
/// With `--failed`, also lists individual failed jobs with their error details.
#[cfg(feature = "jobs")]
pub async fn list(failed: bool) -> Result<(), String> {
    let conn = connect_to_db().await?;

    println!();
    println!("  {} Querying job counts...", "→".cyan());

    let rows = conn
        .query_all(Statement::from_string(
            conn.get_database_backend(),
            "SELECT status, CAST(COUNT(*) AS bigint) AS count \
             FROM rapina_jobs GROUP BY status ORDER BY status"
                .to_string(),
        ))
        .await
        .map_err(|e| format!("Failed to query rapina_jobs: {e}"))?;

    let mut counts = std::collections::HashMap::<String, i64>::new();
    for row in &rows {
        let status: String = row
            .try_get("", "status")
            .map_err(|e| format!("Failed to read status: {e}"))?;
        let count: i64 = row
            .try_get("", "count")
            .map_err(|e| format!("Failed to read count: {e}"))?;
        counts.insert(status, count);
    }

    println!();
    println!("  {}  {}", "STATUS      ".bold(), "COUNT".bold());
    println!("  ────────────  ─────");

    let known_statuses = ["pending", "running", "completed", "failed"];
    let mut total = 0i64;
    for status in known_statuses {
        let count = *counts.get(status).unwrap_or(&0);
        total += count;
        // Pad the raw label first so ANSI codes don't break alignment.
        let padding = " ".repeat(12usize.saturating_sub(status.len()));
        let label = match status {
            "pending" => status.yellow().to_string(),
            "running" => status.cyan().to_string(),
            "completed" => status.green().to_string(),
            "failed" => status.red().to_string(),
            _ => status.to_string(),
        };
        println!("  {}{}  {}", label, padding, count);
    }

    // Show any unexpected statuses so nothing is silently hidden.
    for (status, &count) in &counts {
        if !known_statuses.contains(&status.as_str()) {
            total += count;
            let padding = " ".repeat(12usize.saturating_sub(status.len()));
            println!("  {}{}  {}", status.magenta(), padding, count);
        }
    }

    println!();
    println!("  {} {} total job(s)", "✓".green(), total);

    if failed {
        let fail_rows = conn
            .query_all(Statement::from_string(
                conn.get_database_backend(),
                "SELECT CAST(id AS text) AS id, queue, job_type, attempts, max_retries, last_error \
                 FROM rapina_jobs WHERE status = 'failed' \
                 ORDER BY created_at DESC"
                    .to_string(),
            ))
            .await
            .map_err(|e| format!("Failed to query failed jobs: {e}"))?;

        println!();

        if fail_rows.is_empty() {
            println!("  {} No failed jobs", "✓".green());
        } else {
            println!(
                "  {:<36}  {:<12}  {:<20}  {:<6}  {}",
                "ID".bold(),
                "QUEUE".bold(),
                "JOB TYPE".bold(),
                "ATT.".bold(),
                "LAST ERROR".bold(),
            );
            println!(
                "  {}  {}  {}  {}  {}",
                "─".repeat(36),
                "─".repeat(12),
                "─".repeat(20),
                "─".repeat(6),
                "─".repeat(40),
            );

            for row in &fail_rows {
                let id: String = row
                    .try_get("", "id")
                    .map_err(|e| format!("Failed to read id: {e}"))?;
                let queue: String = row
                    .try_get("", "queue")
                    .map_err(|e| format!("Failed to read queue: {e}"))?;
                let job_type: String = row
                    .try_get("", "job_type")
                    .map_err(|e| format!("Failed to read job_type: {e}"))?;
                let attempts: i32 = row
                    .try_get("", "attempts")
                    .map_err(|e| format!("Failed to read attempts: {e}"))?;
                let max_retries: i32 = row
                    .try_get("", "max_retries")
                    .map_err(|e| format!("Failed to read max_retries: {e}"))?;
                let last_error: Option<String> = row
                    .try_get("", "last_error")
                    .map_err(|e| format!("Failed to read last_error: {e}"))?;

                let error_display = last_error.as_deref().unwrap_or("—");
                let truncated_error = truncate_chars(error_display, 40);
                let short_job_type = truncate_chars(&job_type, 20);

                let id_pad = " ".repeat(36usize.saturating_sub(id.len()));
                println!(
                    "  {}{}  {:<12}  {:<20}  {:<6}  {}",
                    id.red(),
                    id_pad,
                    queue,
                    short_job_type,
                    format!("{}/{}", attempts, max_retries),
                    truncated_error,
                );
            }

            println!();
            println!("  {} {} failed job(s)", "!".red().bold(), fail_rows.len());
        }
    }

    println!();
    Ok(())
}

#[cfg(feature = "jobs")]
async fn connect_to_db() -> Result<DatabaseConnection, String> {
    dotenvy::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL environment variable is not set".to_string())?;

    sea_orm::Database::connect(&database_url)
        .await
        .map_err(|e| format!("Failed to connect to database: {e}"))
}

/// Truncate a string to `max` visible characters, appending "..." if it exceeds
/// the limit. Safe for multi-byte UTF-8 — never splits a codepoint.
#[cfg(feature = "jobs")]
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    if max < 4 {
        return ".".repeat(max);
    }
    let end = s
        .char_indices()
        .nth(max.saturating_sub(3))
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    format!("{}...", &s[..end])
}

#[cfg(all(test, feature = "jobs"))]
mod tests {
    use super::*;

    // -- truncate_chars --

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate_chars("hello", 10), "hello");
    }

    #[test]
    fn truncate_at_exact_limit() {
        assert_eq!(truncate_chars("hello", 5), "hello");
    }

    #[test]
    fn truncate_long_ascii() {
        let result = truncate_chars("abcdefghijklmnop", 10);
        assert!(result.ends_with("..."));
        assert!(result.chars().count() <= 10);
    }

    #[test]
    fn truncate_multibyte_safe() {
        // 5 CJK characters = 5 chars but 15 bytes
        let input = "日本語テスト";
        let result = truncate_chars(input, 5);
        assert!(result.ends_with("..."));
        // Should not panic, and result should be valid UTF-8
        assert!(result.chars().count() <= 5);
    }

    #[test]
    fn truncate_empty_string() {
        assert_eq!(truncate_chars("", 5), "");
    }

    #[test]
    fn truncate_with_small_max() {
        assert_eq!(truncate_chars("abcdef", 3), "...");
        assert_eq!(truncate_chars("abcdef", 2), "..");
        assert_eq!(truncate_chars("abcdef", 1), ".");
        assert_eq!(truncate_chars("abcdef", 0), "");
    }

    // -- is_already_configured --

    #[test]
    fn detects_configured_via_use_import() {
        let content = "use rapina::jobs::create_rapina_jobs;\n";
        assert!(is_already_configured(content));
    }

    #[test]
    fn detects_configured_inside_macro() {
        let content = "rapina::migrations! {\n    create_rapina_jobs,\n}\n";
        assert!(is_already_configured(content));
    }

    #[test]
    fn not_configured_when_absent() {
        let content = "mod m20260315_000001_create_users;\n\nrapina::migrations! {\n    m20260315_000001_create_users,\n}\n";
        assert!(!is_already_configured(content));
    }

    // -- fresh_mod_rs --

    #[test]
    fn fresh_mod_rs_has_use_import() {
        let content = fresh_mod_rs();
        assert!(content.contains("use rapina::jobs::create_rapina_jobs;"));
    }

    #[test]
    fn fresh_mod_rs_has_migrations_macro() {
        let content = fresh_mod_rs();
        assert!(content.contains("rapina::migrations!"));
        assert!(content.contains("create_rapina_jobs,"));
    }

    #[test]
    fn fresh_mod_rs_is_valid_structure() {
        let content = fresh_mod_rs();
        // Must start with use, contain macro, end with newline
        assert!(content.starts_with("use "));
        assert!(content.ends_with("}\n"));
    }

    // -- inject_into_existing --

    #[test]
    fn inject_prepends_use_import() {
        let existing = "mod m20260315_000001_create_users;\n\nrapina::migrations! {\n    m20260315_000001_create_users,\n}\n";
        let result = inject_into_existing(existing);
        assert!(result.starts_with("use rapina::jobs::create_rapina_jobs;\n"));
    }

    #[test]
    fn inject_adds_to_migrations_macro() {
        let existing = "mod m20260315_000001_create_users;\n\nrapina::migrations! {\n    m20260315_000001_create_users,\n}\n";
        let result = inject_into_existing(existing);
        assert!(result.contains("    create_rapina_jobs,\n"));
    }

    #[test]
    fn inject_preserves_existing_entries() {
        let existing = "mod m20260315_000001_create_users;\n\nrapina::migrations! {\n    m20260315_000001_create_users,\n}\n";
        let result = inject_into_existing(existing);
        assert!(result.contains("m20260315_000001_create_users,"));
    }

    #[test]
    fn inject_result_is_idempotent_via_check() {
        let existing = "mod m20260315_000001_create_users;\n\nrapina::migrations! {\n    m20260315_000001_create_users,\n}\n";
        let result = inject_into_existing(existing);
        // After injection, is_already_configured should return true
        assert!(is_already_configured(&result));
    }

    // -- integration: filesystem round-trip --

    #[test]
    fn fresh_mod_rs_written_and_detected() {
        let dir = tempfile::tempdir().unwrap();
        let mod_path = dir.path().join("mod.rs");
        fs::write(&mod_path, fresh_mod_rs()).unwrap();

        let content = fs::read_to_string(&mod_path).unwrap();
        assert!(is_already_configured(&content));
    }

    #[test]
    fn inject_then_write_then_detect() {
        let dir = tempfile::tempdir().unwrap();
        let mod_path = dir.path().join("mod.rs");

        // Start with an existing mod.rs that has a user migration
        let original = "mod m20260315_000001_create_users;\n\nrapina::migrations! {\n    m20260315_000001_create_users,\n}\n";
        fs::write(&mod_path, original).unwrap();

        // Simulate what init() does
        let content = fs::read_to_string(&mod_path).unwrap();
        assert!(!is_already_configured(&content));

        let updated = inject_into_existing(&content);
        fs::write(&mod_path, &updated).unwrap();

        // Second run should detect it's already configured
        let reread = fs::read_to_string(&mod_path).unwrap();
        assert!(is_already_configured(&reread));
    }
}
