use colored::Colorize;
use std::fs;
use std::path::Path;

/// Generate a new migration file.
pub fn new_migration(name: &str) -> Result<(), String> {
    validate_name(name)?;

    let migrations_dir = Path::new("src/migrations");

    if !migrations_dir.exists() {
        fs::create_dir_all(migrations_dir)
            .map_err(|e| format!("Failed to create migrations directory: {}", e))?;
        println!("  {} Created {}", "✓".green(), "src/migrations/".cyan());
    }

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let module_name = format!("m{}_{}", timestamp, name);
    let filename = format!("{}.rs", module_name);
    let filepath = migrations_dir.join(&filename);

    if filepath.exists() {
        return Err(format!("Migration file already exists: {}", filename));
    }

    let template = generate_template(name);
    fs::write(&filepath, template).map_err(|e| format!("Failed to write migration file: {}", e))?;
    println!(
        "  {} Created {}",
        "✓".green(),
        format!("src/migrations/{}", filename).cyan()
    );

    update_mod_rs(migrations_dir, &module_name)?;

    println!();
    println!(
        "  Migration created. Add your schema changes to the {} and {} methods.",
        "up".cyan(),
        "down".cyan()
    );
    println!();

    Ok(())
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Migration name cannot be empty".to_string());
    }

    for c in name.chars() {
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '_' {
            return Err(format!(
                "Migration name must be lowercase alphanumeric with underscores, got '{}'",
                c
            ));
        }
    }

    if name.starts_with('_') || name.ends_with('_') {
        return Err("Migration name cannot start or end with underscore".to_string());
    }

    Ok(())
}

fn generate_template(name: &str) -> String {
    let readable_name = name.replace('_', " ");

    format!(
        r#"//! Migration: {readable_name}

use rapina::sea_orm_migration;
use rapina::migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait]
impl MigrationTrait for Migration {{
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {{
        todo!("Write your migration here")
    }}

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {{
        todo!("Write your rollback here")
    }}
}}
"#
    )
}

pub(crate) fn update_mod_rs(migrations_dir: &Path, module_name: &str) -> Result<(), String> {
    let mod_path = migrations_dir.join("mod.rs");

    if mod_path.exists() {
        let content =
            fs::read_to_string(&mod_path).map_err(|e| format!("Failed to read mod.rs: {}", e))?;

        if content.contains("rapina::migrations!") {
            let new_mod = format!("mod {};\n\n", module_name);
            let updated = format!("{}{}", new_mod, content);
            let updated = add_to_migrations_macro(&updated, module_name);
            fs::write(&mod_path, updated).map_err(|e| format!("Failed to update mod.rs: {}", e))?;
        } else {
            let updated = format!("{}mod {};\n", content, module_name);
            fs::write(&mod_path, updated).map_err(|e| format!("Failed to update mod.rs: {}", e))?;
        }
    } else {
        let content = format!(
            r#"mod {module_name};

rapina::migrations! {{
    {module_name},
}}
"#
        );
        fs::write(&mod_path, &content).map_err(|e| format!("Failed to create mod.rs: {}", e))?;
    }

    println!(
        "  {} Updated {}",
        "✓".green(),
        "src/migrations/mod.rs".cyan()
    );

    Ok(())
}

pub(crate) fn add_to_migrations_macro(content: &str, module_name: &str) -> String {
    if let Some(macro_start) = content.find("rapina::migrations! {") {
        let after_macro = &content[macro_start..];
        if let Some(close_brace) = after_macro.rfind('}') {
            let insertion_point = macro_start + close_brace;
            let mut result = String::new();
            result.push_str(&content[..insertion_point]);
            result.push_str(&format!("    {},\n", module_name));
            result.push_str(&content[insertion_point..]);
            return result;
        }
    }
    content.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_name_valid() {
        assert!(validate_name("create_users").is_ok());
        assert!(validate_name("add_email_to_users").is_ok());
        assert!(validate_name("create_posts_table").is_ok());
    }

    #[test]
    fn test_validate_name_invalid() {
        assert!(validate_name("").is_err());
        assert!(validate_name("CreateUsers").is_err());
        assert!(validate_name("create-users").is_err());
        assert!(validate_name("_create_users").is_err());
        assert!(validate_name("create_users_").is_err());
    }

    #[test]
    fn test_generate_template() {
        let template = generate_template("create_users");
        assert!(template.contains("MigrationTrait for Migration"));
        assert!(template.contains("async fn up"));
        assert!(template.contains("async fn down"));
        assert!(template.contains("use rapina::migration::prelude::*"));
    }
}
