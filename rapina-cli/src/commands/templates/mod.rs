pub mod auth;
pub mod crud;
pub mod rest_api;

use colored::Colorize;
use std::fs;
use std::path::Path;

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
hyper = "1"
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
