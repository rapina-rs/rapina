#![cfg(feature = "multipart")]
use rapina::prelude::*;

/// Handler for multipart form data uploads.
///
/// This example demonstrates how to use the `Multipart` extractor to handle
/// file uploads and form fields in a single request.
#[post("/upload")]
async fn upload(mut form: Multipart) -> Result<String> {
    let mut result = String::new();
    while let Some(field) = form.next_field().await? {
        let name = field.name().unwrap_or("unknown").to_string();
        let data = field.bytes().await?;

        result.push_str(&format!(
            "Field: {}, Data: {}",
            name,
            String::from_utf8_lossy(&data)
        ));
    }

    Ok(result)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let router = Router::new().post("/upload", upload);

    println!("Multipart Example Server running at http://127.0.0.1:3000");
    println!("Try uploading a file with curl:");
    println!("  curl -X POST http://127.0.0.1:3000/upload \\");
    println!("    -F \"title=My File\" \\");
    println!("    -F \"file=@Cargo.toml\"");

    Rapina::new()
        .router(router)
        .listen("127.0.0.1:3000")
        .await?;

    Ok(())
}
