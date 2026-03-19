//! Built-in health check endpoints for Rapina.
//!
//! When enabled via [`.with_health_check(true)`](crate::app::Rapina::with_health_check),
//! three endpoints are registered:
//!
//! | Endpoint | Handler | Purpose |
//! |---|---|---|
//! | `GET /__rapina/health` | [`health_check`] | Alias for `/ready` — simple setups and load balancers |
//! | `GET /__rapina/health/live` | [`liveness_check`] | Kubernetes liveness probe — always `200` |
//! | `GET /__rapina/health/ready` | [`readiness_check`] | Kubernetes readiness probe — runs all checks |
//!
//! # Response format
//!
//! All endpoints return JSON. When all checks pass:
//!
//! ```json
//! { "status": "ok" }
//! ```
//!
//! When checks are configured (database or custom), they appear under `"checks"`:
//!
//! ```json
//! {
//!   "status": "ok",
//!   "checks": {
//!     "db": "ok",
//!     "redis": "ok"
//!   }
//! }
//! ```
//!
//! If any check fails, `"status"` is `"error"` and the HTTP status code is `503`:
//!
//! ```json
//! {
//!   "status": "error",
//!   "checks": {
//!     "db": "error",
//!     "redis": "ok"
//!   }
//! }
//! ```
//!
//! # Built-in checks
//!
//! - **`db`** — when the `database` feature is enabled and a [`DatabaseConnection`](sea_orm::DatabaseConnection)
//!   is in state, a `SELECT 1` query is executed to verify connectivity.
//!   Only runs on `/ready` and `/__rapina/health`, never on `/live`.
//!
//! # Custom checks
//!
//! Register additional checks via [`Rapina::add_health_check`](crate::app::Rapina::add_health_check).
//! All custom checks run on `/ready` (and its `/health` alias):
//!
//! ```ignore
//! Rapina::new()
//!     .with_health_check(true)
//!     .add_health_check("redis", || async {
//!         redis_ping().await.is_ok()
//!     })
//!     .add_health_check("stripe", || async {
//!         stripe_ping().await.is_ok()
//!     })
//!     .listen("127.0.0.1:3000")
//!     .await
//! ```

pub mod config;
mod endpoint;

pub use config::HealthRegistry;
pub use endpoint::{health_check, liveness_check, readiness_check};
