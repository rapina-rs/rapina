//! Route auto-discovery via `inventory`.
//!
//! When handlers are annotated with `#[get]`, `#[post]`, `#[put]`, or `#[delete]`,
//! the macro emits an `inventory::submit!` that registers a [`RouteDescriptor`]
//! at link time. Calling [`Rapina::discover()`](crate::app::Rapina::discover)
//! iterates these descriptors and wires them into the router automatically.
//!
//! The `#[public]` attribute emits a [`PublicMarker`] so the discovery loop
//! can mark routes as public without manual `.public_route()` calls.

use crate::error::ErrorVariant;
use crate::router::Router;

/// Metadata about a route handler, collected at link time via `inventory`.
///
/// Emitted by `#[get]`, `#[post]`, `#[put]`, `#[delete]` macros.
pub struct RouteDescriptor {
    /// HTTP method (GET, POST, PUT, DELETE)
    pub method: &'static str,
    /// Route path pattern (e.g. "/users/:id")
    pub path: &'static str,
    /// Function name of the handler
    pub handler_name: &'static str,
    /// Whether `#[public]` was found below the route macro
    pub is_public: bool,
    /// Returns the JSON Schema for the response type, if available
    pub response_schema: fn() -> Option<serde_json::Value>,
    /// Returns documented error variants for this route
    pub error_responses: fn() -> Vec<ErrorVariant>,
    /// Registers this route on the given Router and returns it
    pub register: fn(Router) -> Router,
}

inventory::collect!(RouteDescriptor);

/// Marker indicating a handler should be treated as public (no auth required).
///
/// Emitted by `#[public]` when placed above a route macro. When `#[public]`
/// is below the route macro, the route macro sets `is_public: true` on the
/// [`RouteDescriptor`] directly instead.
pub struct PublicMarker {
    /// Function name of the handler this marker applies to
    pub handler_name: &'static str,
}

inventory::collect!(PublicMarker);
