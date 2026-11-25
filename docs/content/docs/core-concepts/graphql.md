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

## Request context

`RapinaGraphQLContext` carries Rapina's request-scoped data into resolvers
(the authenticated user and the trace id):

```rust
pub struct RapinaGraphQLContext {
    pub current_user: Option<CurrentUser>,
    pub trace_id: String,
}
```

async-graphql's `Context` is a type-map, so resolvers retrieve it with
`ctx.data::<RapinaGraphQLContext>()`:

```rust
use rapina::prelude::*;

async fn me(&self, ctx: &Context<'_>) -> async_graphql::Result<User> {
    let rapina_ctx = ctx.data::<RapinaGraphQLContext>()?;
    let user = rapina_ctx.current_user.as_ref().ok_or_else(|| {
        graphql_error("UNAUTHORIZED", "Login required", &rapina_ctx.trace_id)
    })?;
    // ...
}
```

Automatic per-request injection by pulling `CurrentUser` and `trace_id` from the
request and calling `request.data(ctx)`, it will arrive with the `with_graphql` builder method (Next PR).
So until then, inject it yourself before executing:

```rust
GraphQLResponse(schema.execute(req.into_inner().data(rapina_ctx)).await)
```

## Errors

Resolver errors can carry Rapina's error vocabulary  the same `code` and
`trace_id`, the REST error envelope guarantees  into the GraphQL response's
`errors[].extensions`. There are two entry points.

**Bridging an existing `rapina::Error`.** It implements async-graphql's
`ErrorExtensions`, so `.extend()` turns one into a GraphQL error that keeps its
`code` (and `trace_id`, when the error carries one):

```rust
let post = find_post(id).await.extend()?;
```

> A bare `?` on a `Result<_, rapina::Error>` also compiles  async-graphql ships a
> blanket `From<T: Display>`  but that path carries only the *message* and drops
> `code`/`trace_id`. Use `.extend()` (or `map_err(|e| e.extend())`) when you want
> the extensions.

**Constructing an error inline.** When the error originates in the resolver and
you have no `rapina::Error` to bridge, `graphql_error` builds one with both `code`
and the current `trace_id` set:

```rust
let post = find_post(id).await.ok_or_else(|| {
    graphql_error("NOT_FOUND", "Post not found", &rapina_ctx.trace_id)
})?;
```

## Status

Available today: the `GraphQLRequest`/`GraphQLResponse` extractor and responder,
the `RapinaGraphQLContext` type, and the error bridge.

Still to come:

- **Automatic context injection** and **schema management** via a `with_graphql`
  builder method  for now you wire the schema and inject the context manually.
- **GraphiQL** playground.
