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
        &src_path.join("auth.rs"),
        &generate_auth_rs(),
        "src/auth.rs",
    )?;
    write_file(
        &project_path.join(".gitignore"),
        &generate_gitignore(&[".env"]),
        ".gitignore",
    )?;
    write_file(
        &project_path.join(".env.example"),
        &generate_env_example(),
        ".env.example",
    )?;

    Ok(())
}

fn generate_main_rs() -> String {
    r#"mod auth;

use rapina::prelude::*;
use rapina::middleware::RequestLogMiddleware;

#[public]
#[get("/health")]
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "healthy" }))
}

#[get("/me")]
async fn me(user: CurrentUser) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "id": user.id }))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    load_dotenv();

    let auth_config = AuthConfig::from_env().expect("JWT_SECRET is required");

    let router = Router::new()
        .get("/health", health)
        .post("/auth/register", auth::register)
        .post("/auth/login", auth::login)
        .get("/me", me);

    Rapina::new()
        .with_tracing(TracingConfig::new())
        .middleware(RequestLogMiddleware::new())
        .with_auth(auth_config.clone())
        .public_route("GET", "/health")
        .public_route("POST", "/auth/register")
        .public_route("POST", "/auth/login")
        .state(auth_config)
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
"#
    .to_string()
}

fn generate_auth_rs() -> String {
    r#"use rapina::prelude::*;
use rapina::schemars;

#[derive(Deserialize, JsonSchema)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[public]
#[post("/auth/register")]
pub async fn register(body: Json<RegisterRequest>) -> Result<Json<TokenResponse>> {
    // TODO: save user to database and hash password
    Err(Error::internal("not implemented"))
}

#[public]
#[post("/auth/login")]
pub async fn login(
    body: Json<LoginRequest>,
    auth: State<AuthConfig>,
) -> Result<Json<TokenResponse>> {
    // TODO: validate credentials against database
    if body.username == "admin" && body.password == "password" {
        let token = auth.create_token(&body.username)?;
        Ok(Json(TokenResponse::new(token, auth.expiration())))
    } else {
        Err(Error::unauthorized("invalid credentials"))
    }
}
"#
    .to_string()
}

fn generate_env_example() -> String {
    r#"JWT_SECRET=change-me-to-a-long-random-secret
JWT_EXPIRATION=3600
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_main_rs_has_auth_routes() {
        let content = generate_main_rs();
        assert!(content.contains(".post(\"/auth/register\", auth::register)"));
        assert!(content.contains(".post(\"/auth/login\", auth::login)"));
        assert!(content.contains(".get(\"/me\", me)"));
        assert!(content.contains("with_auth(auth_config"));
        assert!(content.contains("AuthConfig::from_env()"));
    }

    #[test]
    fn test_generate_main_rs_marks_public_routes() {
        let content = generate_main_rs();
        assert!(content.contains("#[public]"));
        assert!(content.contains("public_route(\"GET\", \"/health\")"));
        assert!(content.contains("public_route(\"POST\", \"/auth/register\")"));
        assert!(content.contains("public_route(\"POST\", \"/auth/login\")"));
    }

    #[test]
    fn test_generate_auth_rs_has_handlers() {
        let content = generate_auth_rs();
        assert!(content.contains("pub async fn register("));
        assert!(content.contains("pub async fn login("));
        assert!(content.contains("pub struct RegisterRequest"));
        assert!(content.contains("pub struct LoginRequest"));
        assert!(content.contains("TokenResponse"));
        assert!(content.contains("Error::unauthorized"));
    }

    #[test]
    fn test_generate_env_example() {
        let content = generate_env_example();
        assert!(content.contains("JWT_SECRET="));
        assert!(content.contains("JWT_EXPIRATION="));
    }

    #[test]
    fn test_gitignore_excludes_env_file() {
        let content = generate_gitignore(&[".env"]);
        assert!(content.contains("/target"));
        assert!(content.contains("Cargo.lock"));
        assert!(content.contains(".env"));
    }
}
