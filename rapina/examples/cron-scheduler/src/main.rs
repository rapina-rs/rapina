use rapina::prelude::*;

#[get("/")]
async fn hello() -> &'static str {
    "Hello, Rapina!"
}

async fn first_cronjob() -> std::io::Result<()> {
    tracing::info!("Doing some work (every 5 seconds)");
    Ok(())
}

async fn second_cronjob() -> std::io::Result<()> {
    tracing::info!("Doing some work (every 10 seconds)");
    Ok(())
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt().init();

    Rapina::new()
        .discover()
        .cron("1/5 * * * * *", first_cronjob)
        .cron("1/10 * * * * *", second_cronjob)
        .listen("127.0.0.1:3000")
        .await
}
