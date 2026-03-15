use url_shortner::build_app;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let (app, addr) = build_app().await?;
    app.listen(&addr).await
}
