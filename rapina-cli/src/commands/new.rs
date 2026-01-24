//! Implementation of the `rapina new` command.

use colored::Colorize;
use std::fs;
use std::path::Path;

/// Execute the `new` command to create a new Rapina project.
pub fn execute(name: &str) -> Result<(), String> {
    // Validate project name
    validate_project_name(name)?;

    // Check if directory already exists
    let project_path = Path::new(name);
    if project_path.exists() {
        return Err(format!("Directory '{}' already exists", name));
    }

    println!();
    println!(
        "  {} {}",
        "Creating new Rapina project:".bright_cyan(),
        name.bold()
    );
    println!();

    // Create project directory structure
    let src_path = project_path.join("src");
    fs::create_dir_all(&src_path).map_err(|e| format!("Failed to create directory: {}", e))?;

    // Create Cargo.toml
    let cargo_toml = generate_cargo_toml(name);
    let cargo_path = project_path.join("Cargo.toml");
    fs::write(&cargo_path, cargo_toml).map_err(|e| format!("Failed to write Cargo.toml: {}", e))?;
    println!("  {} Created {}", "✓".green(), "Cargo.toml".cyan());

    // Create src/main.rs
    let main_rs = generate_main_rs();
    let main_path = src_path.join("main.rs");
    fs::write(&main_path, main_rs).map_err(|e| format!("Failed to write main.rs: {}", e))?;
    println!("  {} Created {}", "✓".green(), "src/main.rs".cyan());

    // Create .gitignore
    let gitignore = generate_gitignore();
    let gitignore_path = project_path.join(".gitignore");
    fs::write(&gitignore_path, gitignore)
        .map_err(|e| format!("Failed to write .gitignore: {}", e))?;
    println!("  {} Created {}", "✓".green(), ".gitignore".cyan());

    println!();
    println!("  {} Project created successfully!", "🎉".bold());
    println!();
    println!("  {}:", "Next steps".bright_yellow());
    println!("    cd {}", name.cyan());
    println!("    rapina dev");
    println!();

    Ok(())
}

/// Validate that the project name is a valid Rust crate name.
fn validate_project_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Project name cannot be empty".to_string());
    }

    // Check if name starts with a digit
    if name.chars().next().unwrap().is_ascii_digit() {
        return Err("Project name cannot start with a digit".to_string());
    }

    // Check for valid characters (alphanumeric, underscore, hyphen)
    for c in name.chars() {
        if !c.is_alphanumeric() && c != '_' && c != '-' {
            return Err(format!(
                "Project name contains invalid character: '{}'. Only alphanumeric characters, underscores, and hyphens are allowed.",
                c
            ));
        }
    }

    // Check for reserved names
    let reserved = ["test", "self", "super", "crate", "Self"];
    if reserved.contains(&name) {
        return Err(format!("'{}' is a reserved Rust keyword", name));
    }

    Ok(())
}

/// Generate the content for Cargo.toml.
fn generate_cargo_toml(name: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"

[dependencies]
rapina = "{version}"
tokio = {{ version = "1", features = ["full"] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
hyper = "1"
"#
    )
}

/// Generate the content for src/main.rs.
fn generate_main_rs() -> String {
    r#"use rapina::prelude::*;
use rapina::middleware::RequestLogMiddleware;

#[derive(Serialize)]
struct MessageResponse {
    message: String,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
}

#[get("/")]
async fn hello() -> Json<MessageResponse> {
    Json(MessageResponse {
        message: "Hello from Rapina!".to_string(),
    })
}

#[get("/health")]
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let router = Router::new()
        .get("/", hello)
        .get("/health", health);

    Rapina::new()
        .with_tracing(TracingConfig::new())
        .middleware(RequestLogMiddleware::new())
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
"#
    .to_string()
}

/// Generate the content for .gitignore.
fn generate_gitignore() -> String {
    r#"/target
Cargo.lock
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_project_name_valid() {
        assert!(validate_project_name("my-app").is_ok());
        assert!(validate_project_name("my_app").is_ok());
        assert!(validate_project_name("myapp").is_ok());
        assert!(validate_project_name("myapp123").is_ok());
    }

    #[test]
    fn test_validate_project_name_invalid() {
        assert!(validate_project_name("").is_err());
        assert!(validate_project_name("123app").is_err());
        assert!(validate_project_name("my app").is_err());
        assert!(validate_project_name("my.app").is_err());
        assert!(validate_project_name("self").is_err());
    }
}
