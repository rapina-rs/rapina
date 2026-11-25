//! Context for GraphQL
//! Auth with `CurrentUser` and Tracing with `trace_id`
//! Inside the `RapinaGraphQLContext` structure

use crate::auth::CurrentUser;

/// Request-scoped data injected into every async_graphql::Request before execution.
pub struct RapinaGraphQLContext {
    pub current_user: Option<CurrentUser>,
    pub trace_id: String,
}
