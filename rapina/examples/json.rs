use rapina::prelude::*;

#[derive(Deserialize)]
struct CreateUser {
    name: String,
    email: String,
}

#[derive(Serialize, JsonSchema)]
struct User {
    id: u64,
    name: String,
    email: String,
}

#[post("/users")]
async fn create_user(body: Json<CreateUser>) -> Json<User> {
    Json(User {
        id: 1,
        name: body.name.clone(),
        email: body.email.clone(),
    })
}

#[get("/users")]
async fn list_users() -> Json<Vec<User>> {
    Json(vec![
        User {
            id: 1,
            name: "Alice".to_string(),
            email: "alice@test.com".to_string(),
        },
        User {
            id: 2,
            name: "Bob".to_string(),
            email: "bob@test.com".to_string(),
        },
    ])
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let router = Router::new()
        .get("/users", list_users)
        .post("/users", create_user);

    println!("Endpoints:");
    println!("  GET  /users");
    println!("  POST /users");

    Rapina::new().router(router).listen("127.0.0.1:3000").await
}
