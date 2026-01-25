+++
title = "Getting Started"
description = "Install Rapina and create your first API"
weight = 1
+++

## Installation

### Using the CLI (Recommended)

The fastest way to get started is with the Rapina CLI:

```bash
# Install the CLI
cargo install rapina-cli

# Create a new project
rapina new my-app
cd my-app

# Start the development server
rapina dev
```

Your API is now running at `http://127.0.0.1:3000`.

### Manual Setup

Add Rapina to your `Cargo.toml`:

```toml
[dependencies]
rapina = "0.1.0-alpha.4"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
```

## Your First API

Create a simple API with a few endpoints:

```rust
use rapina::prelude::*;

#[get("/")]
async fn hello() -> &'static str {
    "Hello, Rapina!"
}

#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> Result<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({
        "id": id.into_inner(),
        "name": "Alice"
    })))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let router = Router::new()
        .get("/", hello)
        .get("/users/:id", get_user);

    Rapina::new()
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
```

## What's Next?

- [Configuration](/guide/configuration/) - Set up environment variables and type-safe config
- [Routing](/guide/routing/) - Define routes and handle parameters
- [Extractors](/guide/extractors/) - Parse request data with type safety
- [Authentication](/guide/authentication/) - Add JWT authentication
