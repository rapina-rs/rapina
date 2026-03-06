+++
title = "State and Config"
template = "tutorial.html"
weight = 5

[extra]
chapter = 5
prev = "/tutorial/04-error-handling/"
next = "/tutorial/06-database-basics/"
doc = "/docs/getting-started/configuration/"

code = """use rapina::prelude::*;

#[derive(Serialize, JsonSchema)]
struct HealthResponse {
    status: String,
}

#[public]
#[get("/health")]
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
    })
}"""

testcases = """[
  {
    "title": "Define a Config struct",
    "description": "Create a struct with #[derive(Config)] and typed fields",
    "pattern": "#\\\\[derive\\\\([^)]*Config[^)]*\\\\)\\\\]\\\\s*struct\\\\s+\\\\w+Config"
  },
  {
    "title": "Add State<T> extractor",
    "description": "Use State<AppConfig> as a handler parameter",
    "pattern": "fn\\\\s+\\\\w+\\\\s*\\\\([^)]*State<"
  },
  {
    "title": "Use config in the response",
    "description": "Access a field from the config in your response",
    "pattern": "config\\\\.app_name",
    "response": {
      "method": "GET",
      "path": "/health",
      "status": 200,
      "body": { "status": "ok", "app_name": "my-api", "version": "1.0.0" }
    }
  }
]"""
+++

# State and Config

Rapina's `#[derive(Config)]` macro loads configuration from environment variables and `.env` files. The `State<T>` extractor gives handlers access to shared application state, including your config.

```rust
#[derive(Config)]
struct AppConfig {
    app_name: String,    // reads APP_NAME env var
    version: String,     // reads VERSION env var
}
```

Config values are loaded at startup — if a required variable is missing, the app crashes immediately instead of failing at runtime. This is the "fail fast" principle.

Access it in handlers via `State<T>`:

```rust
async fn handler(State(config): State<AppConfig>) -> Json<T> {
    // config.app_name, config.version
}
```

## Assignment

1. Create an `AppConfig` struct with `#[derive(Config)]` and fields `app_name: String` and `version: String`
2. Add `State(config): State<AppConfig>` as a handler parameter
3. Include `config.app_name` and the version in the health response

{% answer() %}
```rust
use rapina::prelude::*;

#[derive(Config)]
struct AppConfig {
    app_name: String,
    version: String,
}

#[derive(Serialize, JsonSchema)]
struct HealthResponse {
    status: String,
    app_name: String,
    version: String,
}

#[public]
#[get("/health")]
async fn health(State(config): State<AppConfig>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        app_name: config.app_name.clone(),
        version: config.version.clone(),
    })
}
```
{% end %}
