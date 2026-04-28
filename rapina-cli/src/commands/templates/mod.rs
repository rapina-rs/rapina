pub mod auth;
pub mod crud;
pub mod rest_api;

use colored::Colorize;
use std::fs;
use std::path::Path;

/// Database types supported for project generation.
#[derive(Clone, PartialEq)]
pub enum DatabaseType {
    Sqlite,
    Postgres,
    Mysql,
}

impl std::str::FromStr for DatabaseType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sqlite" => Ok(DatabaseType::Sqlite),
            "postgres" | "postgresql" => Ok(DatabaseType::Postgres),
            "mysql" => Ok(DatabaseType::Mysql),
            _ => Err(format!(
                "Unknown database type '{}'. Available: sqlite, postgres, mysql",
                s
            )),
        }
    }
}

impl std::fmt::Display for DatabaseType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DatabaseType::Sqlite => write!(f, "sqlite"),
            DatabaseType::Postgres => write!(f, "postgres"),
            DatabaseType::Mysql => write!(f, "mysql"),
        }
    }
}

/// Write `content` to `path`, printing a confirmation line on success.
pub fn write_file(path: &Path, content: &str, display_name: &str) -> Result<(), String> {
    fs::write(path, content).map_err(|e| format!("Failed to write {display_name}: {e}"))?;
    println!("  {} Created {}", "✓".green(), display_name.cyan());
    Ok(())
}

/// Generate a `Cargo.toml` for a new Rapina project.
///
/// `rapina_dep` is the full right-hand side of the `rapina = …` entry, e.g.:
/// - `"\"0.1.0\""` for the default dependency
/// - `"{ version = \"0.1.0\", features = [\"sqlite\"] }"` for a feature-gated one
pub fn generate_cargo_toml(name: &str, rapina_dep: &str) -> String {
    format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"

[dependencies]
rapina = {rapina_dep}
tokio = {{ version = "1", features = ["full"] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
validator = {{ version = "0.20", features = ["derive"] }}
"#
    )
}

/// Generate a `.gitignore` with the standard Rust entries plus any `extras`.
pub fn generate_gitignore(extras: &[&str]) -> String {
    let mut content = "/target\nCargo.lock\n".to_string();
    for entry in extras {
        content.push_str(entry);
        content.push('\n');
    }
    content
}

/// Generate the rapina dependency line for Cargo.toml.
///
/// Returns the full right-hand side of the `rapina = …` entry.
/// - Without database: `"\"0.1.0\""`
/// - With database: `"{ version = \"0.1.0\", features = [\"sqlite\"] }"`
pub fn generate_rapina_dep(version: &str, db_type: Option<&DatabaseType>) -> String {
    if let Some(db) = db_type {
        let feature = match db {
            DatabaseType::Sqlite => "sqlite",
            DatabaseType::Postgres => "postgres",
            DatabaseType::Mysql => "mysql",
        };
        format!(
            "{{ version = \"{}\", features = [\"{}\"] }}",
            version, feature
        )
    } else {
        format!("\"{}\"", version)
    }
}

/// Generate the `use rapina::database::DatabaseConfig;` import line.
/// Returns an empty string if no database is configured.
pub fn generate_db_import(db_type: Option<&DatabaseType>) -> &'static str {
    db_type.map_or("", |_| "use rapina::database::DatabaseConfig;\n")
}

/// Generate the `let db_config = ...` line for database configuration.
/// Uses `DatabaseConfig::from_env()` for all database types since `.env` is auto-generated.
/// Returns an empty string if no database is configured.
pub fn generate_database_config(db_type: Option<&DatabaseType>) -> &'static str {
    db_type.map_or("", |_| "let db_config = DatabaseConfig::from_env().expect(\"Failed to configure database\");\n")
}

/// Generate the `.with_database(db_config)` builder line.
/// Returns an empty string if no database is configured.
pub fn generate_with_database_line(db_type: Option<&DatabaseType>) -> &'static str {
    db_type.map_or(
        "",
        |_| "        .with_database(db_config)\n        .await?\n",
    )
}

/// Generate `.env` file content with optional extra variables.
/// Includes comments with instructions to replace with real values.
///
/// `db_type` is `None` when no database is configured (e.g., auth-only projects).
/// `extra_vars` can be used to add additional environment variables (e.g., JWT_SECRET for auth).
pub fn generate_env_content(db_type: Option<&DatabaseType>, extra_vars: Option<&str>) -> String {
    let db_section = match db_type {
        Some(DatabaseType::Sqlite) => "DATABASE_URL=sqlite://app.db?mode=rwc",
        Some(DatabaseType::Postgres) => {
            "DATABASE_URL=postgres://username:password@localhost:5432/myapp"
        }
        Some(DatabaseType::Mysql) => "DATABASE_URL=mysql://username:password@localhost:3306/myapp",
        None => "",
    };

    let extra_section = extra_vars
        .filter(|v| !v.is_empty())
        .map(|v| format!("\n{}\n", v))
        .unwrap_or_default();

    format!(
        "# ⚠️  Replace the values below with your actual configuration.\n# Do not commit this file with real credentials in production!\n\n{}{}",
        db_section, extra_section
    )
}

/// Generate the content for `src/bin/rapina_migrate.rs`.
pub fn generate_migrate_bin_rs() -> String {
    crate::commands::migrate::generate_migrate_bin_rs()
}

/// Write `src/bin/rapina_migrate.rs` into `src_path/bin/`.
/// `src_path` is the `src/` directory of the generated project.
pub fn write_migrate_bin(src_path: &Path) -> Result<(), String> {
    let bin_dir = src_path.join("bin");
    fs::create_dir_all(&bin_dir).map_err(|e| format!("Failed to create src/bin/: {e}"))?;
    write_file(
        &bin_dir.join("rapina_migrate.rs"),
        &generate_migrate_bin_rs(),
        "src/bin/rapina_migrate.rs",
    )
}

/// Generate the `.gitignore` extras for a given database type.
/// Always includes `.env`. Adds `*.db` for SQLite.
pub fn generate_gitignore_extras(db_type: Option<&DatabaseType>) -> Vec<&'static str> {
    let mut extra = vec![".env"];
    if let Some(DatabaseType::Sqlite) = db_type {
        extra.push("*.db");
    }
    extra
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_migrate_bin_rs_content() {
        let content = generate_migrate_bin_rs();
        assert!(content.contains("#[path = \"../migrations/mod.rs\"]"));
        assert!(content.contains("mod migrations"));
        assert!(content.contains("run_cli::<migrations::Migrator>()"));
        assert!(content.contains("#[tokio::main]"));
    }

    #[test]
    fn test_generate_rapina_dep_without_db() {
        let dep = generate_rapina_dep("0.1.0", None);
        assert_eq!(dep, "\"0.1.0\"");
    }

    #[test]
    fn test_generate_rapina_dep_with_sqlite() {
        let dep = generate_rapina_dep("0.1.0", Some(&DatabaseType::Sqlite));
        assert!(dep.contains("sqlite"));
        assert!(dep.contains("0.1.0"));
    }

    #[test]
    fn test_generate_rapina_dep_with_postgres() {
        let dep = generate_rapina_dep("0.1.0", Some(&DatabaseType::Postgres));
        assert!(dep.contains("postgres"));
        assert!(dep.contains("0.1.0"));
    }

    #[test]
    fn test_generate_rapina_dep_with_mysql() {
        let dep = generate_rapina_dep("0.1.0", Some(&DatabaseType::Mysql));
        assert!(dep.contains("mysql"));
        assert!(dep.contains("0.1.0"));
    }

    #[test]
    fn test_generate_db_import_with_db() {
        let import = generate_db_import(Some(&DatabaseType::Sqlite));
        assert!(import.contains("use rapina::database::DatabaseConfig"));
    }

    #[test]
    fn test_generate_db_import_without_db() {
        let import = generate_db_import(None);
        assert!(import.is_empty());
    }

    #[test]
    fn test_generate_database_config_with_db() {
        let config = generate_database_config(Some(&DatabaseType::Postgres));
        assert!(config.contains("let db_config ="));
        assert!(
            config.contains("DatabaseConfig::from_env().expect(\"Failed to configure database\");")
        );
    }

    #[test]
    fn test_generate_database_config_without_db() {
        let config = generate_database_config(None);
        assert!(config.is_empty());
    }

    #[test]
    fn test_generate_env_content_sqlite() {
        let content = generate_env_content(Some(&DatabaseType::Sqlite), None);
        assert!(content.contains("sqlite://"));
        assert!(content.contains("Replace"));
    }

    #[test]
    fn test_generate_env_content_postgres() {
        let content = generate_env_content(Some(&DatabaseType::Postgres), None);
        assert!(content.contains("postgres://"));
        assert!(content.contains("Replace"));
    }

    #[test]
    fn test_generate_env_content_mysql() {
        let content = generate_env_content(Some(&DatabaseType::Mysql), None);
        assert!(content.contains("mysql://"));
        assert!(content.contains("Replace"));
    }

    #[test]
    fn test_generate_env_content_with_extra_vars() {
        let content = generate_env_content(Some(&DatabaseType::Sqlite), Some("EXTRA_VAR=test"));
        assert!(content.contains("sqlite://"));
        assert!(content.contains("EXTRA_VAR=test"));
    }

    #[test]
    fn test_generate_env_content_no_database() {
        let content = generate_env_content(None, Some("JWT_SECRET=test"));
        assert!(!content.contains("DATABASE_URL"));
        assert!(content.contains("JWT_SECRET=test"));
        assert!(content.contains("Replace"));
    }

    #[test]
    fn test_generate_gitignore_extras_sqlite() {
        let extras = generate_gitignore_extras(Some(&DatabaseType::Sqlite));
        assert!(extras.contains(&"*.db"));
        assert!(extras.contains(&".env"));
    }

    #[test]
    fn test_generate_gitignore_extras_postgres() {
        let extras = generate_gitignore_extras(Some(&DatabaseType::Postgres));
        assert!(extras.contains(&".env"));
        assert!(!extras.contains(&"*.db"));
    }

    #[test]
    fn test_generate_gitignore_extras_mysql() {
        let extras = generate_gitignore_extras(Some(&DatabaseType::Mysql));
        assert!(extras.contains(&".env"));
        assert!(!extras.contains(&"*.db"));
    }

    #[test]
    fn test_generate_gitignore_extras_none() {
        let extras = generate_gitignore_extras(None);
        assert!(extras.contains(&".env"));
        assert!(!extras.contains(&"*.db"));
    }

    #[test]
    fn test_generate_with_database_line_with_db() {
        let line = generate_with_database_line(Some(&DatabaseType::Sqlite));
        assert!(line.contains(".with_database(db_config)"));
    }

    #[test]
    fn test_generate_with_database_line_without_db() {
        let line = generate_with_database_line(None);
        assert!(line.is_empty());
    }
}
