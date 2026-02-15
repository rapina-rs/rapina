+++
title = "Configuration"
description = "Type-safe configuration from environment variables"
weight = 2
date = 2025-02-13
+++

## Environment Variables

Rapina uses environment variables for configuration. Create a `.env` file for local development:

```bash
DATABASE_URL=postgres://localhost/myapp
PORT=3000
JWT_SECRET=your-secret-key
```

Load the `.env` file at startup:

```rust
use rapina::prelude::*;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    load_dotenv();  // Load .env file

    // ...
}
```

## Config Derive Macro

Use the `#[derive(Config)]` macro for type-safe configuration:

```rust
use rapina::prelude::*;

#[derive(Config, Clone)]
struct AppConfig {
    #[env = "DATABASE_URL"]
    database_url: String,

    #[env = "PORT"]
    #[default = "3000"]
    port: u16,

    #[env = "HOST"]
    #[default = "127.0.0.1"]
    host: String,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    load_dotenv();

    let config = AppConfig::from_env().expect("Missing config");

    Rapina::new()
        .state(config.clone())
        .router(router)
        .listen(format!("{}:{}", config.host, config.port))
        .await
}
```

## Attributes

| Attribute | Description |
|-----------|-------------|
| `#[env = "VAR_NAME"]` | Environment variable name |
| `#[default = "value"]` | Default value if not set |

## Fail-Fast Validation

If required variables are missing, `from_env()` returns an error listing **all** missing variables at once:

```
Error: Missing environment variables: DATABASE_URL, JWT_SECRET
```

This prevents the frustrating cycle of fixing one error at a time.

## Accessing Config in Handlers

Use the `State` extractor to access configuration in handlers:

```rust
#[get("/config")]
async fn show_config(config: State<AppConfig>) -> String {
    format!("Running on port {}", config.into_inner().port)
}
```
