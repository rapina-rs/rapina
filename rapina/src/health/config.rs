//! Health check configuration types.
//!
//! This module defines the types used to register custom health checks
//! with the [`HealthRegistry`], which is stored in application state and
//! queried on every `GET /__rapina/health` request.

use std::future::Future;
use std::pin::Pin;

/// A boxed async function that returns `true` if the check passes, `false` otherwise.
///
/// This is the internal representation of a registered health check after type-erasure.
pub type HealthCheckFn = Box<dyn Fn() -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;

/// Registry of named custom health checks stored in application state.
///
/// Each check is an async function that returns `true` when healthy.
/// The registry is populated via [`Rapina::add_health_check`](crate::app::Rapina::add_health_check)
/// and automatically placed in state when `.enable_health_check()`/`.with_health_check(true)` is set.
///
/// On each request to `/__rapina/health`, all registered checks are called
/// and their results are included in the response under the `"checks"` key.
#[derive(Default)]
pub struct HealthRegistry {
    /// Named check functions, in registration order.
    pub(crate) checks: Vec<(&'static str, HealthCheckFn)>,
}

impl HealthRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a named async health check.
    ///
    /// The function `f` is called on every health check request.
    /// It should return `true` if the dependency is reachable and healthy,
    /// or `false` if it is not.
    ///
    /// # Example
    ///
    /// ```ignore
    /// registry.add("redis", || async {
    ///     redis_client.ping().await.is_ok()
    /// });
    /// ```
    pub fn add<F, Fut>(&mut self, name: &'static str, f: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        self.checks.push((name, Box::new(move || Box::pin(f()))));
    }
}
