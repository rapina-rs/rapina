//! Metrics utilities for Rapina applications.
//!
//! This module provides tools for metrics.

pub mod middleware;
mod prometheus;

pub use self::middleware::MetricsMiddleware;
pub use self::prometheus::{MetricsRegistry, metrics_handler};
