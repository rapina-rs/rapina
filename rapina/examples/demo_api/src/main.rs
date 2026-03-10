use rapina::prelude::*;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rapina::schemars")]
struct LoginPayload {
    email: String,
    password: String,
}

#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rapina::schemars")]
struct LoginResult {
    token: String,
}

#[post("/login")]
async fn login(body: Json<LoginPayload>) -> Json<LoginResult> {
    println!(
        "Recebemos o email: {} | password: {}",
        body.email, body.password
    );
    Json(LoginResult {
        token: "fake-jwt-token-123".to_string(),
    })
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Configura o projeto com a nossa integração Scalar
    let app = Rapina::new()
        .router(Router::new().post("/login", login))
        .openapi("Minha Nova API 🔥", "1.0.0")
        .with_scalar("/docs");

    println!("=====================================");
    println!("🚀 API iniciada com sucesso!");
    println!("👉 Acesse a documentação em: http://127.0.0.1:4000/docs");
    println!("=====================================");

    app.listen("127.0.0.1:4000").await
}
