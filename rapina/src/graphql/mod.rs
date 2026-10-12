//! GraphQL integration via [`async-graphql`].
//!
//! Gated behind the `graphql` feature flag. Provides two newtype wrappers that
//! bridge `async-graphql` request/response types into Rapina's extractor and
//! responder system, so handlers can receive and return GraphQL payloads
//! without dealing with HTTP plumbing.
//!
//! - [`GraphQLRequest`] — `FromRequest` extractor that handles both **POST**
//!   (JSON body) and **GET** (query string) per the [GraphQL-over-HTTP spec].
//!   Malformed input returns 400.
//! - [`GraphQLResponse`] — `IntoResponse` responder that always returns HTTP
//!   200 with `Content-Type: application/json`. Field-level resolver errors
//!   live in the response body's `errors` array, never in the HTTP status.
//!
//! [`async-graphql`]: https://docs.rs/async-graphql
//! [GraphQL-over-HTTP spec]: https://graphql.github.io/graphql-over-http/
//!
//! # Example
//!
//! ```rust,ignore
//! use rapina::prelude::*;
//! use rapina::graphql::{GraphQLRequest, GraphQLResponse};
//! use async_graphql::{EmptyMutation, EmptySubscription, Object, Schema};
//!
//! struct Query;
//!
//! #[Object]
//! impl Query {
//!     async fn hello(&self) -> &str {
//!         "world"
//!     }
//! }
//!
//! type AppSchema = Schema<Query, EmptyMutation, EmptySubscription>;
//!
//! #[post("/graphql")]
//! #[public]
//! async fn graphql_handler(
//!     State(schema): State<AppSchema>,
//!     req: GraphQLRequest,
//! ) -> GraphQLResponse {
//!     GraphQLResponse(schema.execute(req.into_inner()).await)
//! }
//!
//! #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     let schema = Schema::build(Query, EmptyMutation, EmptySubscription).finish();
//!
//!     Rapina::new()
//!         .state(schema)
//!         .router(Router::new())
//!         .discover()
//!         .listen("127.0.0.1:3000")
//!         .await
//! }
//! ```

mod context;
mod error;
mod request;
mod response;

pub use context::RapinaGraphQLContext;
pub use error::graphql_error;
pub use request::GraphQLRequest;
pub use response::GraphQLResponse;
