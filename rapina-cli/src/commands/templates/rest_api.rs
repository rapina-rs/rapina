use std::path::Path;

use super::{generate_cargo_toml, generate_gitignore, write_file};

pub fn generate(name: &str, project_path: &Path, src_path: &Path) -> Result<(), String> {
    let version = env!("CARGO_PKG_VERSION");
    let rapina_dep = format!("\"{}\"", version);

    write_file(
        &project_path.join("Cargo.toml"),
        &generate_cargo_toml(name, &rapina_dep),
        "Cargo.toml",
    )?;
    write_file(
        &src_path.join("main.rs"),
        &generate_main_rs(),
        "src/main.rs",
    )?;
    write_file(
        &project_path.join(".gitignore"),
        &generate_gitignore(&[]),
        ".gitignore",
    )?;

    Ok(())
}

fn generate_main_rs() -> String {
    r#"use rapina::prelude::*;
use rapina::middleware::RequestLogMiddleware;
use rapina::schemars;

#[derive(Serialize, JsonSchema)]
struct MessageResponse {
    message: String,
}

#[derive(Serialize, JsonSchema)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_main_rs_has_hello_route() {
        let content = generate_main_rs();
        assert!(content.contains("#[get(\"/\")]"));
        assert!(content.contains("#[get(\"/health\")]"));
        assert!(content.contains("async fn hello()"));
        assert!(content.contains("async fn health()"));
        assert!(content.contains("Rapina::new()"));
    }
}
