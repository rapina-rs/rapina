//! Todo API example with JWT authentication and in-memory storage.
//!
//! Run with: `JWT_SECRET=your-secret-key cargo run --example todo_api`
//!
//! Endpoints:
//! - POST /login       (public)  — Get JWT token (username: admin, password: password)
//! - GET  /todos       (auth)    — List current user's todos
//! - POST /todos       (auth)    — Create a todo
//! - PUT  /todos/:id   (auth)    — Update a todo
//! - DELETE /todos/:id (auth)    — Delete a todo

use rapina::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

// =============================================================================
// Configuration
// =============================================================================

/// App config loaded from environment (host, port).
#[derive(Clone, Config)]
struct AppConfig {
    #[env = "HOST"]
    #[default = "127.0.0.1"]
    host: String,

    #[env = "PORT"]
    #[default = "3000"]
    port: u16,
}

// =============================================================================
// In-memory storage
// =============================================================================

/// A single todo item. Stored per-user; `user_id` identifies the owner.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
struct Todo {
    id: String,
    user_id: String,
    title: String,
    completed: bool,
}

/// In-memory store: map from todo id → Todo.
/// Wrapped in Arc<RwLock<>> so all handlers can share and mutate it.
#[derive(Clone)]
struct TodoStore(Arc<RwLock<HashMap<String, Todo>>>);

impl TodoStore {
    fn new() -> Self {
        Self(Arc::new(RwLock::new(HashMap::new())))
    }

    /// List all todos for a user.
    fn list_by_user(&self, user_id: &str) -> Vec<Todo> {
        let guard = self.0.read().expect("lock poisoned");
        guard
            .values()
            .filter(|t| t.user_id == user_id)
            .cloned()
            .collect()
    }

    /// Create a todo. Returns the created todo or error if id collision.
    fn create(&self, todo: Todo) -> Result<Todo> {
        let id = todo.id.clone();
        let mut guard = self.0.write().expect("lock poisoned");
        if guard.contains_key(&id) {
            return Err(Error::conflict("todo id already exists"));
        }
        guard.insert(id.clone(), todo.clone());
        Ok(todo)
    }

    /// Update a todo. Returns 404 if not found, 403 if not owned by user.
    fn update(
        &self,
        id: &str,
        user_id: &str,
        title: Option<String>,
        completed: Option<bool>,
    ) -> Result<Todo> {
        let mut guard = self.0.write().expect("lock poisoned");
        let todo = guard
            .get_mut(id)
            .ok_or_else(|| Error::not_found("todo not found"))?;
        if todo.user_id != user_id {
            return Err(Error::forbidden("you can only update your own todos"));
        }
        if let Some(t) = title {
            todo.title = t;
        }
        if let Some(c) = completed {
            todo.completed = c;
        }
        Ok(todo.clone())
    }

    /// Delete a todo. Returns 404 if not found, 403 if not owned by user.
    fn delete(&self, id: &str, user_id: &str) -> Result<()> {
        let mut guard = self.0.write().expect("lock poisoned");
        let todo = guard
            .get(id)
            .ok_or_else(|| Error::not_found("todo not found"))?;
        if todo.user_id != user_id {
            return Err(Error::forbidden("you can only delete your own todos"));
        }
        guard.remove(id);
        Ok(())
    }
}

// =============================================================================
// DTOs
// =============================================================================

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct CreateTodoRequest {
    title: String,
}

#[derive(Deserialize)]
struct UpdateTodoRequest {
    title: Option<String>,
    completed: Option<bool>,
}

// =============================================================================
// Handlers
// =============================================================================

/// POST /login — Public. Validate credentials and return a JWT.
/// In a real app you would check a database or auth service.
/// Marked #[public] so it bypasses the auth middleware.
#[public]
#[post("/login")]
async fn login(body: Json<LoginRequest>, auth: State<AuthConfig>) -> Result<Json<TokenResponse>> {
    if body.username == "admin" && body.password == "password" {
        let token = auth.create_token(&body.username)?;
        Ok(Json(TokenResponse::new(token, auth.expiration())))
    } else {
        Err(Error::unauthorized("invalid credentials"))
    }
}

/// GET /todos — List all todos for the authenticated user.
/// CurrentUser is injected by the auth middleware from the JWT.
#[get("/todos")]
async fn list_todos(user: CurrentUser, store: State<TodoStore>) -> Json<Vec<Todo>> {
    let todos = store.list_by_user(&user.id);
    Json(todos)
}

/// POST /todos — Create a new todo for the authenticated user.
/// Returns 201 CREATED with the created todo.
#[post("/todos")]
async fn create_todo(
    user: CurrentUser,
    body: Json<CreateTodoRequest>,
    store: State<TodoStore>,
) -> Result<(StatusCode, Json<Todo>)> {
    let id = uuid::Uuid::new_v4().to_string();
    let todo = Todo {
        id: id.clone(),
        user_id: user.id.clone(),
        title: body.title.clone(),
        completed: false,
    };
    let created = store.create(todo)?;
    Ok::<_, Error>((StatusCode::CREATED, Json(created)))
}

/// PUT /todos/:id — Update a todo by id. Only the owner can update.
#[put("/todos/:id")]
async fn update_todo(
    id: Path<String>,
    user: CurrentUser,
    body: Json<UpdateTodoRequest>,
    store: State<TodoStore>,
) -> Result<Json<Todo>> {
    let updated = store.update(&id, &user.id, body.title.clone(), body.completed)?;
    Ok::<_, Error>(Json(updated))
}

/// DELETE /todos/:id — Delete a todo by id. Only the owner can delete.
#[delete("/todos/:id")]
async fn delete_todo(
    id: Path<String>,
    user: CurrentUser,
    store: State<TodoStore>,
) -> Result<StatusCode> {
    let id = id.to_string();
    store.delete(&id, &user.id)?;
    Ok::<_, Error>(StatusCode::NO_CONTENT)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    load_dotenv();

    let config = AppConfig::from_env().expect("Failed to load config");
    let auth_config = AuthConfig::from_env().unwrap_or_else(|_| {
        eprintln!(
            "  Warning: JWT_SECRET not set, using dev default. Set JWT_SECRET for production."
        );
        AuthConfig::new("dev-secret", 3600)
    });
    let todo_store = TodoStore::new();

    let addr = format!("{}:{}", config.host, config.port);

    println!();
    println!("  Rapina Todo API");
    println!("  --------------");
    println!();
    println!("  Server: http://{}", addr);
    println!();
    println!("  Public:");
    println!("    POST /login  — get JWT");
    println!("      Body: {{\"username\":\"admin\",\"password\":\"password\"}}");
    println!();
    println!("  Protected (Authorization: Bearer <token>):");
    println!("    GET    /todos       — list todos");
    println!("      (no body)");
    println!("    POST   /todos       — create todo");
    println!("      Body: {{\"title\":\"My todo\"}}");
    println!("    PUT    /todos/:id   — update todo");
    println!("      Body: {{\"title\":\"Updated\",\"completed\":true}} (both optional)");
    println!("    DELETE /todos/:id   — delete todo");
    println!("      (no body)");
    println!();

    Rapina::new()
        .with_auth(auth_config.clone())
        .state(auth_config)
        .state(todo_store)
        .discover()
        .listen(&addr)
        .await
}
