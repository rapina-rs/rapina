use std::path::Path;

use super::{
    generate_cargo_toml, generate_database_config, generate_db_import, generate_env_content,
    generate_gitignore, generate_gitignore_extras, generate_rapina_dep,
    generate_with_database_line, write_file, write_migrate_bin,
};

pub fn generate(
    name: &str,
    project_path: &Path,
    src_path: &Path,
    db_type: Option<&super::DatabaseType>,
) -> Result<(), String> {
    let version = env!("CARGO_PKG_VERSION");
    let rapina_dep = generate_rapina_dep(version, db_type);

    write_file(
        &project_path.join("Cargo.toml"),
        &generate_cargo_toml(name, &rapina_dep),
        "Cargo.toml",
    )?;
    write_file(
        &src_path.join("main.rs"),
        &generate_main_rs(db_type),
        "src/main.rs",
    )?;
    write_file(
        &project_path.join(".gitignore"),
        &generate_gitignore(&generate_gitignore_extras(db_type)),
        ".gitignore",
    )?;

    // Generate .env file if database is configured
    if let Some(db) = db_type {
        write_file(
            &project_path.join(".env"),
            &generate_env_content(Some(db), None),
            ".env",
        )?;
    }

    if db_type.is_some() {
        write_migrate_bin(src_path)?;
    }

    Ok(())
}

fn generate_main_rs(db_type: Option<&super::DatabaseType>) -> String {
    let db_import = generate_db_import(db_type);
    let db_config = generate_database_config(db_type);
    let with_database_line = generate_with_database_line(db_type);

    // Include load_dotenv() when database is configured
    let load_dotenv_line = db_type.map_or("", |_| "load_dotenv();\n");

    format!(
        r#"use rapina::prelude::*;
use rapina::middleware::RequestLogMiddleware;
use rapina::schemars;
{db_import}
#[derive(Serialize, JsonSchema)]
struct MessageResponse {{
    message: String,
}}

#[get("/")]
async fn hello() -> Json<MessageResponse> {{
    Json(MessageResponse {{
        message: "Hello from Rapina!".to_string(),
    }})
}}

#[tokio::main]
async fn main() -> std::io::Result<()> {{
    {load_dotenv_line}
    let router = Router::new()
        .get("/", hello);
    {db_config}
    Rapina::new()
        .with_tracing(TracingConfig::new())
        .middleware(RequestLogMiddleware::new())
        .with_health_check(true)
{with_database_line}        .router(router)
        .listen("127.0.0.1:3000")
        .await
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_main_rs_has_hello_route() {
        let content = generate_main_rs(None);
        assert!(content.contains("#[get(\"/\")]"));
        assert!(content.contains("async fn hello()"));
        assert!(content.contains("with_health_check(true)"));
        assert!(content.contains("Rapina::new()"));
    }

    #[test]
    fn test_generate_main_rs_without_database() {
        let content = generate_main_rs(None);
        assert!(!content.contains("with_database"));
        assert!(!content.contains("db_config"));
        assert!(!content.contains("DatabaseConfig"));
    }

    #[test]
    fn test_generate_main_rs_with_database() {
        let content = generate_main_rs(Some(&crate::commands::templates::DatabaseType::Sqlite));
        assert!(content.contains("load_dotenv();"));
        assert!(content.contains("with_database(db_config)"));
        assert!(content.contains("let db_config"));
        assert!(content.contains("use rapina::database::DatabaseConfig"));
    }

    #[test]
    fn test_rest_api_with_db_creates_migrate_bin() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        generate(
            "myapp",
            dir.path(),
            &src,
            Some(&crate::commands::templates::DatabaseType::Sqlite),
        )
        .unwrap();
        assert!(src.join("bin").join("rapina_migrate.rs").exists());
    }

    #[test]
    fn test_rest_api_without_db_no_migrate_bin() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        generate("myapp", dir.path(), &src, None).unwrap();
        assert!(!src.join("bin").join("rapina_migrate.rs").exists());
    }
}
