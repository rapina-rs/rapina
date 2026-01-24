use rapina::extract::{FromRequestParts, State};
use rapina::prelude::*;
use std::sync::Arc;

#[derive(Clone)]
struct AppConfig {
    app_name: String,
}

#[derive(Deserialize)]
struct CreateUser {
    name: String,
    email: String,
}

#[derive(Serialize, Clone)]
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
    format!("Hello from {}!", config.into_inner().app_name)
}

#[get("/health")]
async fn health() -> StatusCode {
    StatusCode::OK
}

#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> Result<Json<User>> {
    let id = id.into_inner();

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
    let input = body.into_inner();
    Json(User {
        id: 1,
        name: input.name,
        email: input.email,
    })
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let config = AppConfig {
        app_name: "Rapina Demo".to_string(),
    };

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
        .listen("127.0.0.1:3000")
        .await
}
