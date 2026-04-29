+++
title = "llms.txt"
description = "Serve a machine-readable route summary at /__rapina/llms.txt for AI agents"
weight = 11
date = 2026-04-23
+++

Rapina can serve a [`llms.txt`](https://llmstxt.org/) document at `/__rapina/llms.txt`. The document is a single Markdown file that lists every route your app exposes — methods, paths, request schemas, response schemas, and error codes — in a format AI agents can read in one fetch without scraping HTML or consulting a verbose OpenAPI spec.

The document is generated once at startup from the same route metadata that powers OpenAPI, so it is always in sync with your handlers.

## Enabling llms.txt

llms.txt is **on by default in debug builds** and off in release builds. No configuration needed for local development.

To control it explicitly:

```rust
use rapina::prelude::*;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .enable_llms_txt()   // force on (e.g. in a staging build)
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```

To disable it:

```rust
Rapina::new()
    .disable_llms_txt()
    .discover()
    .listen("127.0.0.1:3000")
    .await
```

Or use `.with_llms_txt(bool)` for conditional logic:

```rust
Rapina::new()
    .with_llms_txt(std::env::var("EXPOSE_LLMS_TXT").is_ok())
    .discover()
    .listen("127.0.0.1:3000")
    .await
```

---

## The Endpoint

`GET /__rapina/llms.txt` returns `text/plain; charset=utf-8`. The endpoint is public — it does not require authentication even when auth middleware is enabled.

If llms.txt was not enabled, the endpoint is not registered and requests return 404.

Internal `/__rapina/*` routes are excluded from the document. Only user-defined routes appear.

---

## Output Format

The document follows the [llms.txt convention](https://llmstxt.org/). Everything is inlined in one file — no index/detail split. Example output for an app with a single route:

```markdown
# API

Built with [Rapina](https://rapina.rs) v0.11.0.

## Routes

### POST /v1/users

Request (application/json):
{
"type": "object",
"properties": {
"email": { "type": "string" },
"name": { "type": "string" }
}
}

Response:
{
"type": "object",
"properties": {
"id": { "type": "number" },
"email": { "type": "string" }
}
}

Errors:

- 409 CONFLICT: email is already registered
- 422 VALIDATION_ERROR: request body failed validation
```

Request and response schemas are rendered verbatim from the same JSON Schema that powers the OpenAPI spec. Handlers that don't return `Json<T>` or don't accept a typed request body produce no schema block for that section.

---

## CLI

The `rapina llms export` command fetches the document from your running development server and writes it to stdout or a file.

```sh
# Print to stdout
rapina llms export

# Write to file
rapina llms export -o llms.txt
```

See [rapina llms export](/docs/cli/commands/#rapina-llms-export) for the full option reference.
