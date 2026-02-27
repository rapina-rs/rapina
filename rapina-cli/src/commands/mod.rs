//! CLI command implementations.

pub mod add;
pub(crate) mod codegen;
pub mod dev;
pub mod doctor;
#[cfg(feature = "import")]
pub mod import;
pub mod migrate;
pub mod new;
pub mod openapi;
pub mod routes;
pub mod test;

#[cfg(feature = "import-openapi")]
pub mod import_openapi;

/// Verify that we're in a valid Rapina project directory.
pub fn verify_rapina_project() -> Result<toml::Value, String> {
    let cargo_toml = std::path::Path::new("Cargo.toml");
    if !cargo_toml.exists() {
        return Err("No Cargo.toml found. Are you in a Rust project directory?".to_string());
    }

    let content = std::fs::read_to_string(cargo_toml)
        .map_err(|e| format!("Failed to read Cargo.toml: {}", e))?;

    let parsed: toml::Value =
        toml::from_str(&content).map_err(|e| format!("Failed to parse Cargo.toml: {}", e))?;

    // Check for rapina in dependencies
    let has_rapina = parsed
        .get("dependencies")
        .and_then(|deps| deps.get("rapina"))
        .is_some();

    if !has_rapina {
        return Err(
            "This doesn't appear to be a Rapina project (no rapina dependency found)".to_string(),
        );
    }

    Ok(parsed)
}
