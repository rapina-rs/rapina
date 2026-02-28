use rapina::extract::{FromRequestParts, State};
use rapina::prelude::*;
use std::sync::Arc;

#[derive(Clone, Config)]
struct AppConfig {
    #[env = "APP_NAME"]
    #[default = "Rapina Demo"]
    app_name: String,

    #[env = "PORT"]
    #[default = "3000"]
    port: u16,

    #[env = "HOST"]
    #[default = "127.0.0.1"]
    host: String,
}

#[derive(Deserialize)]
struct CreateUser {
    name: String,
    email: String,
}

#[derive(Serialize, Clone, JsonSchema)]
struct User {
    id: u64,
    name: String,
    email: String,
}

struct CurrentUser {
    user_id: u64,
}

impl FromRequestParts for CurrentUser {
    async fn from_request_parts(
        parts: &http::request::Parts,
        _params: &rapina::extract::PathParams,
        _state: &Arc<rapina::state::AppState>,
    ) -> rapina::error::Result<Self> {
        let user_id = parts
            .headers
            .get("x-user-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| {
                rapina::error::Error::unauthorized("missing or invalid x-user-id header")
            })?;

        Ok(CurrentUser { user_id })
    }
}

#[get("/")]
async fn hello(config: State<AppConfig>) -> String {
    format!("Hello from {}!", config.app_name)
}

#[get("/health")]
async fn health() -> StatusCode {
    StatusCode::OK
}

#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> Result<Json<User>> {
    let id = *id;
    if id == 0 {
        return Err(Error::bad_request("id cannot be zero"));
    }

    if id == 999 {
        return Err(Error::not_found("user not found"));
    }

    Ok(Json(User {
        id,
        name: "Antonio".to_string(),
        email: "antonio@tier3.dev".to_string(),
    }))
}

#[get("/me")]
async fn get_me(user: CurrentUser) -> Json<User> {
    Json(User {
        id: user.user_id,
        name: "Current User".to_string(),
        email: "me@example.com".to_string(),
    })
}

#[post("/users")]
async fn create_user(body: Json<CreateUser>) -> Json<User> {
    Json(User {
        id: 1,
        name: body.name.clone(),
        email: body.email.clone(),
    })
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Load .env file if present
    load_dotenv();
    let config = AppConfig::from_env().expect("Failed to load config");
    let addr = format!("{}:{}", config.host, config.port);

    let router = Router::new()
        .get("/", hello)
        .get("/health", health)
        .get("/users/:id", get_user)
        .get("/me", get_me)
        .post("/users", create_user);

    Rapina::new()
        .openapi("Rapina Test", "1.0.0")
        .state(config)
        .router(router)
        .listen(&addr)
        .await
}
