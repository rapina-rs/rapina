+++
title = "Routing"
description = "Define routes and handle parameters"
weight = 2
+++

Routes in Rapina map HTTP methods and URL patterns to handler functions. The router matches incoming requests and extracts path parameters automatically.

## Basic Routing

Use the `Router` to define your API endpoints:

```rust
use rapina::prelude::*;

let router = Router::new()
    .get("/", home)
    .get("/about", about)
    .post("/users", create_user)
    .put("/users/:id", update_user)
    .delete("/users/:id", delete_user);
```

### HTTP Methods

Rapina provides convenience methods for common HTTP verbs:

| Method | Description |
|--------|-------------|
| `.get(pattern, handler)` | GET requests (read) |
| `.post(pattern, handler)` | POST requests (create) |
| `.put(pattern, handler)` | PUT requests (update) |
| `.delete(pattern, handler)` | DELETE requests (remove) |
| `.route(Method, pattern, handler)` | Any HTTP method |

### Using Macros

For cleaner syntax, use the route macros:

```rust
use rapina::prelude::*;

#[get("/")]
async fn home() -> &'static str {
    "Welcome to Rapina!"
}

#[post("/users")]
async fn create_user(body: Json<CreateUser>) -> Result<Json<User>> {
    // Create user...
    Ok(Json(user))
}

#[put("/users/:id")]
async fn update_user(id: Path<u64>, body: Json<UpdateUser>) -> Result<Json<User>> {
    // Update user...
    Ok(Json(user))
}

#[delete("/users/:id")]
async fn delete_user(id: Path<u64>) -> StatusCode {
    // Delete user...
    StatusCode::NO_CONTENT
}
```

## Path Parameters

Extract dynamic values from URL segments using the `:param` syntax:

```rust
#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> String {
    format!("User ID: {}", id.into_inner())
}
```

### Parameter Types

Path parameters are automatically parsed to their target type:

```rust
#[get("/items/:id")]
async fn get_item(id: Path<u64>) -> Result<Json<Item>> {
    // id is parsed as u64
    let item = find_item(id.into_inner()).await?;
    Ok(Json(item))
}

#[get("/products/:slug")]
async fn get_product(slug: Path<String>) -> Result<Json<Product>> {
    // slug is kept as String
    let product = find_by_slug(&slug.into_inner()).await?;
    Ok(Json(product))
}
```

If parsing fails (e.g., non-numeric value for `u64`), Rapina returns a `400 Bad Request` with error details.

## Route Matching

Routes are matched in the order they are added. More specific routes should be defined before generic ones:

```rust
let router = Router::new()
    // Specific route first
    .get("/users/me", get_current_user)
    // Generic route second
    .get("/users/:id", get_user);
```

### Trailing Slashes

Trailing slashes are treated as different routes:

- `/users` and `/users/` are **not** equivalent
- Define both if you want to handle both patterns

```rust
let router = Router::new()
    .get("/users", list_users)
    .get("/users/", list_users); // Optional: handle trailing slash
```

## Named Routes

For better introspection and documentation, use named routes:

```rust
let router = Router::new()
    .get_named("/users", "list_users", list_users)
    .post_named("/users", "create_user", create_user)
    .get_named("/users/:id", "get_user", get_user);
```

Named routes appear in the introspection endpoint at `/__rapina/routes`.

## Route Introspection

Enable introspection to expose your API structure:

```rust
let app = Rapina::new()
    .with_introspection(true)
    .router(router);
```

Then access `GET /__rapina/routes` to see all registered routes:

```json
[
  {
    "method": "GET",
    "path": "/users",
    "name": "list_users"
  },
  {
    "method": "POST",
    "path": "/users",
    "name": "create_user"
  }
]
```

## Complete Example

```rust
use rapina::prelude::*;

#[derive(Deserialize)]
struct CreateUser {
    name: String,
    email: String,
}

#[derive(Serialize)]
struct User {
    id: u64,
    name: String,
    email: String,
}

#[get("/")]
async fn home() -> &'static str {
    "Welcome to the User API"
}

#[get("/users")]
async fn list_users() -> Json<Vec<User>> {
    Json(vec![
        User { id: 1, name: "Alice".into(), email: "alice@example.com".into() },
        User { id: 2, name: "Bob".into(), email: "bob@example.com".into() },
    ])
}

#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> Result<Json<User>> {
    let user = User {
        id: id.into_inner(),
        name: "Alice".into(),
        email: "alice@example.com".into(),
    };
    Ok(Json(user))
}

#[post("/users")]
async fn create_user(body: Json<CreateUser>) -> (StatusCode, Json<User>) {
    let input = body.into_inner();
    let user = User {
        id: 3,
        name: input.name,
        email: input.email,
    };
    (StatusCode::CREATED, Json(user))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let router = Router::new()
        .get("/", home)
        .get("/users", list_users)
        .get("/users/:id", get_user)
        .post("/users", create_user);

    Rapina::new()
        .with_introspection(true)
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
```

## What's Next?

- [Extractors](/guide/extractors/) - Parse request data with type safety
- [Authentication](/guide/authentication/) - Add JWT authentication
- [Errors](/guide/errors/) - Handle errors consistently
