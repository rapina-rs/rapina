+++
title = "GraphQL"
description = "GraphQL request/response extractor and responder behind the graphql feature flag"
weight = 13
date = 2026-06-16
+++

Rapina ships a thin integration with [async-graphql](https://docs.rs/async-graphql)
so handlers can receive and return GraphQL payloads without boilerplate. Gated
behind the `graphql` feature flag.

## Setup

Add the feature to your `Cargo.toml`:

```toml
[dependencies]
rapina = { version = "0.13.1", features = ["graphql"] }
async-graphql = "7.0"
```

## The extractor and responder

Two newtypes wrap `async_graphql::Request` and `async_graphql::Response`:

```rust
use rapina::prelude::*;
use async_graphql::{Schema, EmptyMutation, EmptySubscription};

#[post("/graphql")]
async fn graphql_handler(req: GraphQLRequest) -> GraphQLResponse {
    let schema: Schema<Query, EmptyMutation, EmptySubscription> = /* ... */;
    GraphQLResponse(schema.execute(req.0).await)
}
```

`GraphQLRequest` accepts both POST (JSON body) and GET (query string with
`query`, `variables`, `operationName`). Malformed input returns 400.

## HTTP semantics

Per the [GraphQL-over-HTTP spec](https://graphql.github.io/graphql-over-http/),
`GraphQLResponse` always returns HTTP 200 with `Content-Type: application/json`.
Field-level resolver errors live in the response body's `errors` array, never
in the HTTP status.

The 400 status is reserved for *transport-level* failures: malformed JSON in
the POST body or an invalid query string in GET. Unsupported HTTP methods return 405.

 ### Limitations

This pr just delivers the extractors, Schema management and the `with_graphql` builder method are still to be done!
