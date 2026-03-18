//! Observability utilities for Rapina applications.
//!
//! This module provides tools for logging, tracing, and monitoring.
//!
//! - [`TracingConfig`] — stdout/JSON logging via `tracing-subscriber`
//! - [`TelemetryConfig`] — OTLP trace export to Jaeger, Datadog, etc. (requires `telemetry` feature)

mod tracing;

#[cfg(feature = "telemetry")]
mod telemetry;

pub use self::tracing::TracingConfig;

#[cfg(feature = "telemetry")]
pub use self::telemetry::{OtlpProtocol, SamplerConfig, TelemetryConfig};
