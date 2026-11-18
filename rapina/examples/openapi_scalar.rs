//! Run `cargo run -p rapina --features openapi-scalar --example openapi_scalar`, then open
//! http://127.0.0.1:3000/docs and try GET /greetings/{name}.

use rapina::prelude::*;

#[derive(Serialize, JsonSchema)]
struct Greeting {
    message: String,
}

#[get("/greetings/:name")]
async fn greet(name: Path<String>) -> Json<Greeting> {
    Json(Greeting {
        message: format!("Hello, {}!", *name),
    })
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .openapi("Greetings API", "1.0.0")
        .openapi_ui(OpenApiUi::Scalar)
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
