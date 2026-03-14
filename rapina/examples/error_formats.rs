use rapina::prelude::*;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // This example demonstrates the RFC 7807 (Problem Details for HTTP APIs) format.
    // It is enabled globally using `.enable_rfc7807_errors()` in the builder.

    println!("Starting server with RFC 7807 errors enabled...");
    println!(
        "Try visiting http://localhost:3000/not-found and http://localhost:3000/validation-error"
    );

    Rapina::new()
        .router(
            Router::new()
                .route(http::Method::GET, "/not-found", |_, _, _| async {
                    Error::not_found("The requested resource was not found")
                        .with_instance("/users/123")
                })
                .route(http::Method::GET, "/validation-error", |_, _, _| async {
                    Error::validation("Invalid input provided").with_details(serde_json::json!({
                        "field": "email",
                        "reason": "must be a valid email address"
                    }))
                }),
        )
        // Enable RFC 7807 format globally for this server
        .enable_rfc7807_errors()
        .rfc7807_base_uri("https://userapina.com/errors/")
        .listen("0.0.0.0:3000")
        .await?;

    Ok(())
}
