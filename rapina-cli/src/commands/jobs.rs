use colored::Colorize;
use std::fs;
use std::path::Path;

use super::verify_rapina_project;

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

#[cfg(test)]
mod tests {
    use super::*;

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
