use rapina::prelude::*;

#[get("/")]
async fn hello() -> &'static str {
    "Hello, Rapina!"
}

async fn first_cronjob() -> std::io::Result<()> {
    tracing::info!(
        "Doing some work (every 5 seconds and specifically starting at 1 second past the minute, i.e. executes at 00:01, 00:06, 00:11, ...)"
    );
    Ok(())
}

async fn second_cronjob() -> std::io::Result<()> {
    tracing::info!(
        "Doing some work (every 10 seconds, starting at 0 second past the minute, i.e. executes at 00:00, 00:10, 00:20, ...)"
    );
    Ok(())
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt().init();

    Rapina::new()
        .discover()
        .cron("1/5 * * * * *", first_cronjob)
        .cron("*/10 * * * * *", second_cronjob)
        .listen("127.0.0.1:3000")
        .await
}
