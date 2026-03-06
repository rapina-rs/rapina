+++
title = "Protected Routes"
template = "tutorial.html"
weight = 2

[extra]
chapter = 2
prev = "/tutorial/01-your-first-route/"
next = "/tutorial/03-validation/"
doc = "/docs/core-concepts/authentication/"

code = """use rapina::prelude::*;

#[derive(Serialize, JsonSchema)]
struct ProfileResponse {
    message: String,
}

#[public]
#[get("/profile")]
async fn profile() -> Json<ProfileResponse> {
    Json(ProfileResponse {
        message: "Hello!".into(),
    })
}"""

testcases = """[
  {
    "title": "Remove #[public]",
    "description": "Protected routes don't use the #[public] attribute",
    "pattern": "^(?!.*#\\\\[public\\\\]).*#\\\\[get"
  },
  {
    "title": "Add CurrentUser extractor",
    "description": "Use CurrentUser as a function parameter to access the authenticated user",
    "pattern": "fn\\\\s+\\\\w+\\\\s*\\\\([^)]*CurrentUser"
  },
  {
    "title": "Use the user's name in the response",
    "description": "Access user.name to personalize the message",
    "pattern": "user\\\\.name",
    "response": {
      "method": "GET",
      "path": "/profile",
      "status": 200,
      "body": { "message": "Hello, Alice!" }
    }
  }
]"""
+++

# Protected Routes

In Rapina, all routes are protected by default. The `#[public]` attribute is an opt-out — without it, the framework requires a valid JWT token in the `Authorization` header.

To access information about the authenticated user, use the `CurrentUser` extractor:

```rust
async fn handler(user: CurrentUser) -> Json<T> {
    // user.id, user.name, user.email are available
}
```

If no valid token is provided, Rapina automatically returns a `401 Unauthorized` response before your handler ever runs.

## Assignment

1. Remove the `#[public]` attribute to make the route protected
2. Add `CurrentUser` as a parameter to the handler
3. Use `user.name` to return a personalized greeting like `"Hello, Alice!"`

{% answer() %}
```rust
use rapina::prelude::*;

#[derive(Serialize, JsonSchema)]
struct ProfileResponse {
    message: String,
}

#[get("/profile")]
async fn profile(user: CurrentUser) -> Json<ProfileResponse> {
    Json(ProfileResponse {
        message: format!("Hello, {}!", user.name),
    })
}
```
{% end %}
