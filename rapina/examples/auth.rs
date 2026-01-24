//! Example demonstrating JWT authentication in Rapina.
//!
//! Run with: `JWT_SECRET=your-secret-key cargo run --example auth`
//!
//! Test endpoints:
//! - GET /health (public) - Health check, no auth required
//! - POST /login (public) - Get a JWT token
//! - GET /me (protected) - Requires valid JWT token

use rapina::prelude::*;

#[derive(Clone, Config)]
struct AppConfig {
    #[env = "HOST"]
    #[default = "127.0.0.1"]
    host: String,

    #[env = "PORT"]
    #[default = "3000"]
    port: u16,
}

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Serialize, JsonSchema)]
struct UserResponse {
    id: String,
    username: String,
}

// Public route - no authentication required
#[public]
#[get("/health")]
async fn health() -> &'static str {
    "ok"
}

// Public route - login to get a token
#[public]
#[post("/login")]
async fn login(body: Json<LoginRequest>, auth: State<AuthConfig>) -> Result<Json<TokenResponse>> {
    let req = body.into_inner();
    let auth_config = auth.into_inner();

    // In a real app, validate credentials against a database
    if req.username == "admin" && req.password == "password" {
        let token = auth_config.create_token(&req.username)?;
        Ok(Json(TokenResponse::new(token, auth_config.expiration())))
    } else {
        Err(Error::unauthorized("invalid credentials"))
    }
}

// Protected route - requires valid JWT
#[get("/me")]
async fn me(user: CurrentUser) -> Json<UserResponse> {
    Json(UserResponse {
        id: user.id.clone(),
        username: user.id,
    })
}

// Protected route - get user by ID
#[get("/users/:id")]
async fn get_user(id: Path<String>, user: CurrentUser) -> Result<Json<UserResponse>> {
    let user_id = id.into_inner();

    // Only allow users to fetch their own data
    if user.id != user_id {
        return Err(Error::forbidden("you can only access your own data"));
    }

    Ok(Json(UserResponse {
        id: user_id.clone(),
        username: user_id,
    }))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    load_dotenv();

    let config = AppConfig::from_env().expect("Failed to load config");
    let auth_config = AuthConfig::from_env().expect("JWT_SECRET is required");

    let addr = format!("{}:{}", config.host, config.port);

    println!();
    println!("  Rapina Auth Example");
    println!("  -------------------");
    println!();
    println!("  Server running at http://{}", addr);
    println!();
    println!("  Public endpoints:");
    println!("    GET  /health  - Health check");
    println!("    POST /login   - Get JWT token");
    println!();
    println!("  Protected endpoints (require Authorization: Bearer <token>):");
    println!("    GET  /me         - Current user info");
    println!("    GET  /users/:id  - Get user by ID");
    println!();

    let router = Router::new()
        .get("/health", health)
        .post("/login", login)
        .get("/me", me)
        .get("/users/:id", get_user);

    Rapina::new()
        .with_auth(auth_config.clone())
        .public_route("GET", "/health")
        .public_route("POST", "/login")
        .state(auth_config)
        .router(router)
        .listen(&addr)
        .await
}
