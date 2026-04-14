use std::path::Path;

use super::{
    DatabaseType, generate_cargo_toml, generate_database_config, generate_db_import,
    generate_env_content, generate_gitignore, generate_gitignore_extras, generate_rapina_dep,
    generate_with_database_line, write_file,
};

pub fn generate(
    name: &str,
    project_path: &Path,
    src_path: &Path,
    db_type: Option<&DatabaseType>,
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
        &src_path.join("auth.rs"),
        &generate_auth_rs(),
        "src/auth.rs",
    )?;
    write_file(
        &project_path.join(".gitignore"),
        &generate_gitignore(&generate_gitignore_extras(db_type)),
        ".gitignore",
    )?;
    write_file(
        &project_path.join(".env"),
        &generate_auth_env_content(db_type),
        ".env",
    )?;

    Ok(())
}

fn generate_auth_env_content(db_type: Option<&DatabaseType>) -> String {
    const AUTH_VARS: &str = "# Authentication Configuration\n# Replace JWT_SECRET with a long, random string (at least 32 characters).\nJWT_SECRET=change-me-to-a-long-random-secret-change-me\nJWT_EXPIRATION=3600";
    generate_env_content(db_type, Some(AUTH_VARS))
}

fn generate_main_rs(db_type: Option<&DatabaseType>) -> String {
    let db_import = generate_db_import(db_type);
    let db_setup = generate_database_config(db_type);
    let with_database_line = generate_with_database_line(db_type);

    format!(
        r#"mod auth;

use rapina::prelude::*;
use rapina::middleware::RequestLogMiddleware;
{}
#[get("/me")]
async fn me(user: CurrentUser) -> Json<serde_json::Value> {{
    Json(serde_json::json!({{ "id": user.id }}))
}}

#[tokio::main]
async fn main() -> std::io::Result<()> {{
    load_dotenv();

    let auth_config = AuthConfig::from_env().expect("JWT_SECRET is required");

    let router = Router::new()
        .post("/auth/register", auth::register)
        .post("/auth/login", auth::login)
        .get("/me", me);

    {}
    Rapina::new()
        .with_tracing(TracingConfig::new())
        .middleware(RequestLogMiddleware::new())
        .with_auth(auth_config.clone())
        .with_health_check(true)
        .public_route("POST", "/auth/register")
        .public_route("POST", "/auth/login")
        .state(auth_config)
{}        .router(router)
        .listen("127.0.0.1:3000")
        .await
}}
"#,
        db_import, db_setup, with_database_line
    )
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
    auth: State<AuthConfig>,
    body: Json<LoginRequest>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_main_rs_has_auth_routes() {
        let content = generate_main_rs(None);
        assert!(content.contains(".post(\"/auth/register\", auth::register)"));
        assert!(content.contains(".post(\"/auth/login\", auth::login)"));
        assert!(content.contains(".get(\"/me\", me)"));
        assert!(content.contains("with_auth(auth_config"));
        assert!(content.contains("AuthConfig::from_env()"));
    }

    #[test]
    fn test_generate_main_rs_marks_public_routes() {
        let content = generate_main_rs(None);
        assert!(content.contains("with_health_check(true)"));
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
    fn test_generate_env_file() {
        let content = generate_auth_env_content(None);
        assert!(content.contains("JWT_SECRET="));
        assert!(content.contains("JWT_EXPIRATION="));
        assert!(content.contains("Replace"));
    }

    #[test]
    fn test_generate_env_file_with_database() {
        let content = generate_auth_env_content(Some(&DatabaseType::Sqlite));
        assert!(content.contains("JWT_SECRET="));
        assert!(content.contains("DATABASE_URL="));
        assert!(content.contains("Replace"));
    }

    #[test]
    fn test_gitignore_excludes_env_file() {
        let content = generate_gitignore(&[".env"]);
        assert!(content.contains("/target"));
        assert!(content.contains("Cargo.lock"));
        assert!(content.contains(".env"));
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
        let content = generate_main_rs(Some(&DatabaseType::Sqlite));
        assert!(content.contains("with_database(db_config)"));
        assert!(content.contains("let db_config"));
        assert!(content.contains("use rapina::database::DatabaseConfig"));
    }
}
