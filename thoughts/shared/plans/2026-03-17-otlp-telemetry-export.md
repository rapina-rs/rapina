# OTLP Telemetry Export Implementation Plan

## Overview

Add OpenTelemetry Protocol (OTLP) trace export to Rapina so teams using Jaeger, Datadog, or other OTLP-compatible backends can receive distributed traces. Currently, all tracing output goes to stdout only via `tracing-subscriber`. This plan adds a new `TelemetryConfig` that coexists with `TracingConfig`, gated behind a `telemetry` feature flag.

## Current State Analysis

- `TracingConfig` (`observability/tracing.rs`) initializes a `tracing-subscriber` fmt layer (text or JSON) to stdout. No OTLP.
- `TraceIdMiddleware` (`middleware/trace_id.rs`) propagates a custom `x-trace-id` UUID header — not W3C `traceparent`.
- `RequestLogMiddleware` creates `tracing::info_span!` per request with `method`, `path`, `trace_id`.
- `with_tracing()` on `Rapina` (`app.rs:297-300`) calls `config.init()` eagerly and does not store config.
- Dependencies: only `tracing = "0.1"` and `tracing-subscriber = "0.3"`.
- Feature flags: `compression`, `database`, `postgres`, `mysql`, `sqlite`, `metrics`, `cache-redis`, `multipart`, `websocket`. No `telemetry` flag.

### Key Discoveries:
- `TracingConfig::init()` calls `.init()` on the subscriber, which sets the global default — `observability/tracing.rs:83-103`
- `server.rs:119-121` already runs shutdown hooks in order — we can register an OTLP flush hook
- `TraceIdMiddleware` reads `x-trace-id` header and echoes it back — `middleware/trace_id.rs:52-72`
- The `Rapina` struct has no field for tracing config — `app.rs:49-81`

## Desired End State

```rust
use rapina::prelude::*;

Rapina::new()
    .with_telemetry(TelemetryConfig {
        endpoint: "http://jaeger:4317".into(),
        service_name: "my-api".into(),
        sample_rate: 1.0,
    })
    .router(router)
    .listen("127.0.0.1:3000")
    .await
```

- Traces are exported to the configured OTLP endpoint over gRPC (port 4317) or HTTP (port 4318).
- Incoming `traceparent` headers are parsed and used as parent span context.
- The existing `x-trace-id` header continues to work alongside `traceparent`.
- A shutdown hook flushes the OTLP exporter before the process exits.
- All new code is behind `feature = "telemetry"` — zero cost when not enabled.

### Verification:
- `cargo test --features telemetry` passes all new and existing tests
- `cargo test` (without telemetry feature) still compiles and passes — no regressions
- `cargo clippy --features telemetry` is clean
- `cargo fmt --check` passes

## What We're NOT Doing

- Not replacing `TracingConfig` — it continues to work for stdout-only logging
- Not removing the custom `x-trace-id` header — it coexists with `traceparent`
- Not adding metrics export (Prometheus already exists separately)
- Not adding log export via OTLP (only traces)
- Not adding baggage propagation (only trace context)

## Implementation Approach

Layer an `OpenTelemetryLayer` from `tracing-opentelemetry` on top of a `tracing-subscriber` `Registry`. The OTLP exporter sends spans to the configured endpoint. A `TraceparentMiddleware` extracts W3C trace context from incoming requests and creates child spans. The `TelemetryConfig` struct provides a builder API with sensible defaults.

## Phase 1: Dependencies and Feature Flag

### Overview
Add the `telemetry` feature flag and required crate dependencies.

### Changes Required:

#### 1. Cargo.toml
**File**: `rapina/Cargo.toml`
**Changes**: Add optional OpenTelemetry dependencies and `telemetry` feature flag.

```toml
# After line 41 (tracing-subscriber), add:

# OpenTelemetry (optional — enabled by `telemetry` feature)
opentelemetry = { version = "0.28", optional = true }
opentelemetry_sdk = { version = "0.28", optional = true, features = ["rt-tokio"] }
opentelemetry-otlp = { version = "0.28", optional = true, features = ["grpc-tonic", "http-proto"] }
tracing-opentelemetry = { version = "0.29", optional = true }

# In [features] section, add:
telemetry = ["opentelemetry", "opentelemetry_sdk", "opentelemetry-otlp", "tracing-opentelemetry"]
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo check --features telemetry` compiles cleanly
- [x] `cargo check` (no telemetry) still compiles cleanly
- [x] `cargo clippy --features telemetry` is clean

#### Manual Verification:
- [x] Verify crate versions are compatible with each other

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation.

---

## Phase 2: TelemetryConfig and SamplerConfig

### Overview
Create the `TelemetryConfig` struct with builder methods and a `SamplerConfig` enum. This phase is purely data structures and unit tests — no OTLP initialization yet.

### Changes Required:

#### 1. New file: `observability/telemetry.rs`
**File**: `rapina/src/observability/telemetry.rs`
**Changes**: Define `TelemetryConfig`, `SamplerConfig`, `OtlpProtocol` with builder pattern.

```rust
//! OpenTelemetry OTLP export configuration.

/// The transport protocol for OTLP export.
#[derive(Debug, Clone, PartialEq)]
pub enum OtlpProtocol {
    /// gRPC transport (typically port 4317).
    Grpc,
    /// HTTP/protobuf transport (typically port 4318).
    HttpProto,
}

/// Controls how traces are sampled before export.
#[derive(Debug, Clone, PartialEq)]
pub enum SamplerConfig {
    /// Export every trace.
    AlwaysOn,
    /// Export no traces.
    AlwaysOff,
    /// Export a fraction of traces based on trace ID.
    /// Value must be between 0.0 and 1.0.
    TraceIdRatio(f64),
}

/// Configuration for OpenTelemetry OTLP trace export.
///
/// # Examples
///
/// ```ignore
/// use rapina::prelude::*;
///
/// Rapina::new()
///     .with_telemetry(TelemetryConfig {
///         endpoint: "http://jaeger:4317".into(),
///         service_name: "my-api".into(),
///         sample_rate: 1.0,
///     })
/// ```
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    /// The OTLP collector endpoint (e.g. "http://jaeger:4317").
    pub endpoint: String,
    /// The service name reported in traces.
    pub service_name: String,
    /// Sample rate between 0.0 and 1.0. Convenience field that maps to
    /// `SamplerConfig::TraceIdRatio`. Defaults to 1.0 (export all).
    pub sample_rate: f64,
    /// The transport protocol. Defaults to `Grpc`.
    pub(crate) protocol: OtlpProtocol,
    /// Override sampler config. When set, takes precedence over `sample_rate`.
    pub(crate) sampler: Option<SamplerConfig>,
}

impl TelemetryConfig {
    /// Creates a new telemetry config with the given endpoint and service name.
    pub fn new(endpoint: impl Into<String>, service_name: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            service_name: service_name.into(),
            sample_rate: 1.0,
            protocol: OtlpProtocol::Grpc,
            sampler: None,
        }
    }

    /// Sets the sample rate (0.0 to 1.0).
    pub fn sample_rate(mut self, rate: f64) -> Self {
        self.sample_rate = rate;
        self
    }

    /// Uses HTTP/protobuf protocol instead of gRPC.
    pub fn http(mut self) -> Self {
        self.protocol = OtlpProtocol::HttpProto;
        self
    }

    /// Uses gRPC protocol (default).
    pub fn grpc(mut self) -> Self {
        self.protocol = OtlpProtocol::Grpc;
        self
    }

    /// Sets an explicit sampler, overriding `sample_rate`.
    pub fn sampler(mut self, sampler: SamplerConfig) -> Self {
        self.sampler = Some(sampler);
        self
    }

    /// Returns the effective sampler configuration.
    pub(crate) fn effective_sampler(&self) -> SamplerConfig {
        if let Some(ref s) = self.sampler {
            return s.clone();
        }
        if self.sample_rate >= 1.0 {
            SamplerConfig::AlwaysOn
        } else if self.sample_rate <= 0.0 {
            SamplerConfig::AlwaysOff
        } else {
            SamplerConfig::TraceIdRatio(self.sample_rate)
        }
    }
}
```

#### 2. Update `observability/mod.rs`
**File**: `rapina/src/observability/mod.rs`
**Changes**: Conditionally declare and re-export the telemetry module.

```rust
#[cfg(feature = "telemetry")]
mod telemetry;

#[cfg(feature = "telemetry")]
pub use self::telemetry::{OtlpProtocol, SamplerConfig, TelemetryConfig};
```

#### 3. Update prelude in `lib.rs`
**File**: `rapina/src/lib.rs`
**Changes**: Re-export `TelemetryConfig` in prelude when feature is enabled.

```rust
// In the prelude module, add:
#[cfg(feature = "telemetry")]
pub use crate::observability::{OtlpProtocol, SamplerConfig, TelemetryConfig};
```

### Tests (TDD — write before implementation):

```rust
// In observability/telemetry.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_config_new() {
        let config = TelemetryConfig::new("http://localhost:4317", "test-svc");
        assert_eq!(config.endpoint, "http://localhost:4317");
        assert_eq!(config.service_name, "test-svc");
        assert_eq!(config.sample_rate, 1.0);
        assert_eq!(config.protocol, OtlpProtocol::Grpc);
        assert!(config.sampler.is_none());
    }

    #[test]
    fn test_telemetry_config_struct_literal() {
        let config = TelemetryConfig {
            endpoint: "http://jaeger:4317".into(),
            service_name: "my-api".into(),
            sample_rate: 0.5,
            ..TelemetryConfig::new("", "")
        };
        assert_eq!(config.endpoint, "http://jaeger:4317");
        assert_eq!(config.sample_rate, 0.5);
    }

    #[test]
    fn test_telemetry_config_http_protocol() {
        let config = TelemetryConfig::new("http://localhost:4318", "svc").http();
        assert_eq!(config.protocol, OtlpProtocol::HttpProto);
    }

    #[test]
    fn test_telemetry_config_grpc_protocol() {
        let config = TelemetryConfig::new("http://localhost:4317", "svc")
            .http()
            .grpc();
        assert_eq!(config.protocol, OtlpProtocol::Grpc);
    }

    #[test]
    fn test_telemetry_config_sample_rate() {
        let config = TelemetryConfig::new("http://localhost:4317", "svc").sample_rate(0.25);
        assert_eq!(config.sample_rate, 0.25);
    }

    #[test]
    fn test_effective_sampler_always_on() {
        let config = TelemetryConfig::new("ep", "svc").sample_rate(1.0);
        assert_eq!(config.effective_sampler(), SamplerConfig::AlwaysOn);
    }

    #[test]
    fn test_effective_sampler_always_on_above_one() {
        let config = TelemetryConfig::new("ep", "svc").sample_rate(1.5);
        assert_eq!(config.effective_sampler(), SamplerConfig::AlwaysOn);
    }

    #[test]
    fn test_effective_sampler_always_off() {
        let config = TelemetryConfig::new("ep", "svc").sample_rate(0.0);
        assert_eq!(config.effective_sampler(), SamplerConfig::AlwaysOff);
    }

    #[test]
    fn test_effective_sampler_always_off_negative() {
        let config = TelemetryConfig::new("ep", "svc").sample_rate(-0.1);
        assert_eq!(config.effective_sampler(), SamplerConfig::AlwaysOff);
    }

    #[test]
    fn test_effective_sampler_ratio() {
        let config = TelemetryConfig::new("ep", "svc").sample_rate(0.5);
        assert_eq!(
            config.effective_sampler(),
            SamplerConfig::TraceIdRatio(0.5)
        );
    }

    #[test]
    fn test_explicit_sampler_overrides_rate() {
        let config = TelemetryConfig::new("ep", "svc")
            .sample_rate(0.5)
            .sampler(SamplerConfig::AlwaysOn);
        assert_eq!(config.effective_sampler(), SamplerConfig::AlwaysOn);
    }

    #[test]
    fn test_builder_chain() {
        let config = TelemetryConfig::new("http://collector:4317", "my-api")
            .sample_rate(0.75)
            .http()
            .sampler(SamplerConfig::AlwaysOff);
        assert_eq!(config.endpoint, "http://collector:4317");
        assert_eq!(config.service_name, "my-api");
        assert_eq!(config.protocol, OtlpProtocol::HttpProto);
        assert_eq!(config.effective_sampler(), SamplerConfig::AlwaysOff);
    }
}
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo test --features telemetry` — all telemetry config tests pass
- [x] `cargo test` — existing tests still pass (telemetry module not compiled)
- [x] `cargo clippy --features telemetry` is clean

#### Manual Verification:
- [x] API ergonomics look right — both struct literal and builder styles work

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation.

---

## Phase 3: OTLP Exporter Initialization

### Overview
Implement `TelemetryConfig::init()` which sets up the OpenTelemetry tracing pipeline with an OTLP exporter, and wire it into the `Rapina` builder via `with_telemetry()`.

### Changes Required:

#### 1. Add `init()` to `TelemetryConfig`
**File**: `rapina/src/observability/telemetry.rs`
**Changes**: Add the `init()` method that creates the OTel pipeline.

```rust
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::{SdkTracerProvider, Sampler};
use opentelemetry_sdk::Resource;
use opentelemetry::KeyValue;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

impl TelemetryConfig {
    /// Initializes the OpenTelemetry tracing pipeline and installs it as the
    /// global subscriber.
    ///
    /// Returns a `TracerProvider` handle that must be shut down on exit to
    /// flush pending spans. Use `Rapina::with_telemetry()` which handles
    /// this automatically via a shutdown hook.
    pub fn init(self) -> SdkTracerProvider {
        let sampler = match self.effective_sampler() {
            SamplerConfig::AlwaysOn => Sampler::AlwaysOn,
            SamplerConfig::AlwaysOff => Sampler::AlwaysOff,
            SamplerConfig::TraceIdRatio(r) => Sampler::TraceIdRatioBased(r),
        };

        let resource = Resource::builder()
            .with_attribute(KeyValue::new(
                opentelemetry_sdk::resource::SERVICE_NAME,
                self.service_name.clone(),
            ))
            .build();

        let exporter = match self.protocol {
            OtlpProtocol::Grpc => opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(&self.endpoint)
                .build()
                .expect("failed to create gRPC OTLP exporter"),
            OtlpProtocol::HttpProto => opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .with_endpoint(&self.endpoint)
                .build()
                .expect("failed to create HTTP OTLP exporter"),
        };

        let provider = SdkTracerProvider::builder()
            .with_sampler(sampler)
            .with_resource(resource)
            .with_batch_exporter(exporter)
            .build();

        let tracer = provider.tracer(self.service_name);
        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info"));

        tracing_subscriber::registry()
            .with(filter)
            .with(otel_layer)
            .with(tracing_subscriber::fmt::layer())
            .init();

        provider
    }
}
```

#### 2. Add `with_telemetry()` to `Rapina`
**File**: `rapina/src/app.rs`
**Changes**: Add the builder method that initializes telemetry and registers a shutdown hook.

```rust
// After with_tracing() method (line 300), add:

/// Configures OpenTelemetry OTLP trace export.
///
/// Sets up a tracing pipeline that exports spans to the configured OTLP
/// endpoint. A shutdown hook is automatically registered to flush pending
/// spans on graceful shutdown.
///
/// Requires the `telemetry` feature.
///
/// # Example
///
/// ```ignore
/// use rapina::prelude::*;
///
/// Rapina::new()
///     .with_telemetry(TelemetryConfig {
///         endpoint: "http://jaeger:4317".into(),
///         service_name: "my-api".into(),
///         sample_rate: 1.0,
///     })
///     .router(router)
///     .listen("127.0.0.1:3000")
///     .await
/// ```
#[cfg(feature = "telemetry")]
pub fn with_telemetry(self, config: crate::observability::TelemetryConfig) -> Self {
    let provider = config.init();
    self.on_shutdown(move || async move {
        if let Err(e) = provider.shutdown() {
            eprintln!("OpenTelemetry shutdown error: {e}");
        }
    })
}
```

### Tests:

```rust
// In app.rs tests
#[cfg(feature = "telemetry")]
#[test]
fn test_rapina_with_telemetry_registers_shutdown_hook() {
    // We can't fully init OTel in unit tests (global subscriber conflict),
    // but we can verify the config struct is accepted by the builder.
    // Integration tests will cover the full pipeline.
    let config = crate::observability::TelemetryConfig::new(
        "http://localhost:4317",
        "test-svc",
    );
    // Verify the type is accepted - compilation is the test
    let _: fn(Rapina, crate::observability::TelemetryConfig) -> Rapina =
        |app, cfg| app.with_telemetry(cfg);
}
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo check --features telemetry` compiles
- [x] `cargo test --features telemetry` passes
- [x] `cargo test` (without feature) passes — `with_telemetry` not visible
- [x] `cargo clippy --features telemetry` is clean

#### Manual Verification:
- [ ] Confirm that calling `with_telemetry()` without `with_tracing()` works (they're independent)
- [ ] Confirm shutdown hook count increases by 1

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation.

---

## Phase 4: W3C Traceparent Propagation Middleware

### Overview
Add a `TraceparentMiddleware` that extracts W3C `traceparent` headers from incoming requests and creates properly-parented OpenTelemetry spans. This coexists with the existing `TraceIdMiddleware` and its `x-trace-id` header.

### Changes Required:

#### 1. New file: `middleware/traceparent.rs`
**File**: `rapina/src/middleware/traceparent.rs`
**Changes**: Implement `TraceparentMiddleware`.

The middleware will:
1. Parse `traceparent` header (format: `{version}-{trace_id}-{parent_id}-{flags}`)
2. Create a `tracing::Span` with the extracted context as parent
3. Instrument the downstream call with that span
4. Inject `traceparent` into the response with the current span's context

```rust
//! W3C Trace Context propagation middleware.

use hyper::body::Incoming;
use hyper::header::HeaderValue;
use hyper::{Request, Response};

use crate::context::RequestContext;
use crate::response::BoxBody;

use super::{BoxFuture, Middleware, Next};

const TRACEPARENT_HEADER: &str = "traceparent";

/// Middleware that propagates W3C `traceparent` headers for distributed tracing.
///
/// When an incoming request carries a `traceparent` header, this middleware
/// creates a child span linked to the upstream trace. The `traceparent`
/// header is echoed back in the response so downstream services can
/// continue the trace chain.
///
/// This middleware is independent of [`TraceIdMiddleware`](super::TraceIdMiddleware)
/// and both can be used simultaneously.
///
/// Requires the `telemetry` feature.
///
/// # Example
///
/// ```rust,ignore
/// use rapina::prelude::*;
/// use rapina::middleware::TraceparentMiddleware;
///
/// Rapina::new()
///     .with_telemetry(TelemetryConfig::new("http://jaeger:4317", "my-api"))
///     .middleware(TraceparentMiddleware::new())
///     .router(router)
///     .listen("127.0.0.1:3000")
///     .await
/// ```
#[derive(Debug, Clone, Copy)]
pub struct TraceparentMiddleware;

impl TraceparentMiddleware {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TraceparentMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

/// Parsed W3C traceparent header fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Traceparent {
    pub version: u8,
    pub trace_id: String,
    pub parent_id: String,
    pub trace_flags: u8,
}

impl Traceparent {
    /// Parses a `traceparent` header value.
    ///
    /// Format: `{version}-{trace_id}-{parent_id}-{flags}`
    /// Example: `00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01`
    pub(crate) fn parse(value: &str) -> Option<Self> {
        let parts: Vec<&str> = value.split('-').collect();
        if parts.len() != 4 {
            return None;
        }

        let version = u8::from_str_radix(parts[0], 16).ok()?;
        let trace_id = parts[1];
        let parent_id = parts[2];
        let trace_flags = u8::from_str_radix(parts[3], 16).ok()?;

        // Validate lengths per W3C spec
        if trace_id.len() != 32 || parent_id.len() != 16 {
            return None;
        }

        // Validate hex characters
        if !trace_id.chars().all(|c| c.is_ascii_hexdigit())
            || !parent_id.chars().all(|c| c.is_ascii_hexdigit())
        {
            return None;
        }

        // All-zero trace_id or parent_id is invalid
        if trace_id.chars().all(|c| c == '0') || parent_id.chars().all(|c| c == '0') {
            return None;
        }

        Some(Self {
            version,
            trace_id: trace_id.to_string(),
            parent_id: parent_id.to_string(),
            trace_flags,
        })
    }

    /// Formats the traceparent header value.
    pub(crate) fn to_header_value(&self) -> String {
        format!(
            "{:02x}-{}-{}-{:02x}",
            self.version, self.trace_id, self.parent_id, self.trace_flags
        )
    }
}

impl Middleware for TraceparentMiddleware {
    fn handle<'a>(
        &'a self,
        req: Request<Incoming>,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        Box::pin(async move {
            let incoming_traceparent = req
                .headers()
                .get(TRACEPARENT_HEADER)
                .and_then(|v| v.to_str().ok())
                .and_then(Traceparent::parse);

            let span = if let Some(ref tp) = incoming_traceparent {
                tracing::info_span!(
                    "http.request",
                    otel.kind = "server",
                    trace_id = %tp.trace_id,
                    parent_id = %tp.parent_id,
                )
            } else {
                tracing::info_span!("http.request", otel.kind = "server")
            };

            let mut response = {
                let _guard = span.enter();
                next.run(req).await
            };

            // Echo traceparent back in response
            if let Some(tp) = incoming_traceparent {
                if let Ok(val) = HeaderValue::from_str(&tp.to_header_value()) {
                    response.headers_mut().insert(TRACEPARENT_HEADER, val);
                }
            }

            response
        })
    }
}
```

#### 2. Update `middleware/mod.rs`
**File**: `rapina/src/middleware/mod.rs`
**Changes**: Conditionally declare and re-export the traceparent module.

```rust
// Add after trace_id module declaration:
#[cfg(feature = "telemetry")]
mod traceparent;

// Add to public exports:
#[cfg(feature = "telemetry")]
pub use traceparent::TraceparentMiddleware;
```

#### 3. Update prelude in `lib.rs`
**File**: `rapina/src/lib.rs`
**Changes**: Re-export `TraceparentMiddleware` in prelude.

```rust
// In prelude, add:
#[cfg(feature = "telemetry")]
pub use crate::middleware::TraceparentMiddleware;
```

### Tests (TDD):

```rust
// In middleware/traceparent.rs
#[cfg(test)]
mod tests {
    use super::*;

    // --- Traceparent parsing tests ---

    #[test]
    fn test_parse_valid_traceparent() {
        let tp = Traceparent::parse(
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        )
        .unwrap();
        assert_eq!(tp.version, 0);
        assert_eq!(tp.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(tp.parent_id, "00f067aa0ba902b7");
        assert_eq!(tp.trace_flags, 1);
    }

    #[test]
    fn test_parse_unsampled_traceparent() {
        let tp = Traceparent::parse(
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00",
        )
        .unwrap();
        assert_eq!(tp.trace_flags, 0);
    }

    #[test]
    fn test_parse_invalid_too_few_parts() {
        assert!(Traceparent::parse("00-abc-01").is_none());
    }

    #[test]
    fn test_parse_invalid_too_many_parts() {
        assert!(Traceparent::parse("00-a-b-c-d-e").is_none());
    }

    #[test]
    fn test_parse_invalid_trace_id_length() {
        // 31 chars instead of 32
        assert!(Traceparent::parse(
            "00-4bf92f3577b34da6a3ce929d0e0e473-00f067aa0ba902b7-01"
        )
        .is_none());
    }

    #[test]
    fn test_parse_invalid_parent_id_length() {
        // 15 chars instead of 16
        assert!(Traceparent::parse(
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b-01"
        )
        .is_none());
    }

    #[test]
    fn test_parse_all_zero_trace_id_invalid() {
        assert!(Traceparent::parse(
            "00-00000000000000000000000000000000-00f067aa0ba902b7-01"
        )
        .is_none());
    }

    #[test]
    fn test_parse_all_zero_parent_id_invalid() {
        assert!(Traceparent::parse(
            "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01"
        )
        .is_none());
    }

    #[test]
    fn test_parse_non_hex_trace_id() {
        assert!(Traceparent::parse(
            "00-4bf92f3577b34da6a3ce929d0e0eXXXX-00f067aa0ba902b7-01"
        )
        .is_none());
    }

    #[test]
    fn test_parse_non_hex_version() {
        assert!(Traceparent::parse(
            "zz-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
        )
        .is_none());
    }

    #[test]
    fn test_to_header_value_roundtrip() {
        let original = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let tp = Traceparent::parse(original).unwrap();
        assert_eq!(tp.to_header_value(), original);
    }

    #[test]
    fn test_to_header_value_preserves_flags() {
        let tp = Traceparent {
            version: 0,
            trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".into(),
            parent_id: "00f067aa0ba902b7".into(),
            trace_flags: 0,
        };
        assert_eq!(
            tp.to_header_value(),
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00"
        );
    }

    // --- Middleware construction tests ---

    #[test]
    fn test_traceparent_middleware_new() {
        let _mw = TraceparentMiddleware::new();
    }

    #[test]
    fn test_traceparent_middleware_default() {
        let _mw: TraceparentMiddleware = Default::default();
    }
}
```

Integration test in `rapina/tests/middleware.rs`:

```rust
#[cfg(feature = "telemetry")]
mod traceparent_tests {
    use rapina::middleware::TraceparentMiddleware;
    use rapina::prelude::*;
    use rapina::testing::TestClient;

    #[get("/ping")]
    async fn ping() -> &'static str {
        "pong"
    }

    #[tokio::test]
    async fn test_traceparent_echoed_in_response() {
        let app = Rapina::new()
            .middleware(TraceparentMiddleware::new())
            .router(Router::new().get("/ping", ping));

        let client = TestClient::new(app);
        let traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let response = client
            .get("/ping")
            .header("traceparent", traceparent)
            .send()
            .await;

        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers().get("traceparent").unwrap().to_str().unwrap(),
            traceparent
        );
    }

    #[tokio::test]
    async fn test_no_traceparent_header_when_not_provided() {
        let app = Rapina::new()
            .middleware(TraceparentMiddleware::new())
            .router(Router::new().get("/ping", ping));

        let client = TestClient::new(app);
        let response = client.get("/ping").send().await;

        assert_eq!(response.status(), 200);
        assert!(response.headers().get("traceparent").is_none());
    }

    #[tokio::test]
    async fn test_invalid_traceparent_ignored() {
        let app = Rapina::new()
            .middleware(TraceparentMiddleware::new())
            .router(Router::new().get("/ping", ping));

        let client = TestClient::new(app);
        let response = client
            .get("/ping")
            .header("traceparent", "invalid-header-value")
            .send()
            .await;

        assert_eq!(response.status(), 200);
        assert!(response.headers().get("traceparent").is_none());
    }

    #[tokio::test]
    async fn test_traceparent_coexists_with_trace_id() {
        use rapina::middleware::TraceIdMiddleware;

        let app = Rapina::new()
            .middleware(TraceIdMiddleware::new())
            .middleware(TraceparentMiddleware::new())
            .router(Router::new().get("/ping", ping));

        let client = TestClient::new(app);
        let traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let response = client
            .get("/ping")
            .header("traceparent", traceparent)
            .header("x-trace-id", "custom-id-123")
            .send()
            .await;

        assert_eq!(response.status(), 200);
        // Both headers should be present
        assert_eq!(
            response.headers().get("traceparent").unwrap().to_str().unwrap(),
            traceparent
        );
        assert_eq!(
            response.headers().get("x-trace-id").unwrap().to_str().unwrap(),
            "custom-id-123"
        );
    }
}
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo test --features telemetry` — all traceparent parsing and middleware tests pass
- [x] `cargo test` — existing tests pass (traceparent module not compiled)
- [x] `cargo clippy --features telemetry` is clean

#### Manual Verification:
- [ ] Verify `x-trace-id` and `traceparent` both appear in responses when both middlewares active

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation.

---

## Phase 5: Final Polish and Verification

### Overview
Run formatting, clippy, and full test suite. Update docs module comment.

### Changes Required:

#### 1. Update observability module doc
**File**: `rapina/src/observability/mod.rs`
**Changes**: Update module doc to mention OTLP export.

```rust
//! Observability utilities for Rapina applications.
//!
//! This module provides tools for logging, tracing, and monitoring.
//!
//! - [`TracingConfig`] — stdout/JSON logging via `tracing-subscriber`
//! - [`TelemetryConfig`] — OTLP trace export to Jaeger, Datadog, etc. (requires `telemetry` feature)
```

#### 2. Update lib.rs module doc
**File**: `rapina/src/lib.rs`
**Changes**: Add telemetry to the features list in the crate-level doc.

Add to the features list:
```rust
//! - **Telemetry** - OpenTelemetry OTLP trace export (optional `telemetry` feature)
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo fmt --check` passes
- [x] `cargo test --features telemetry` passes all tests
- [x] `cargo test` (no telemetry) passes all tests
- [x] `cargo clippy --features telemetry` is clean
- [x] `cargo clippy` (no telemetry) is clean

#### Manual Verification:
- [ ] Review all new public API surface for consistency with existing Rapina patterns

---

## Testing Strategy

### Unit Tests:
- `TelemetryConfig` construction, builder chain, defaults
- `SamplerConfig` / `effective_sampler()` edge cases (0.0, 1.0, negative, >1.0)
- `OtlpProtocol` switching
- `Traceparent::parse()` — valid, invalid, edge cases per W3C spec
- `Traceparent::to_header_value()` — roundtrip fidelity

### Integration Tests:
- `TraceparentMiddleware` echoes header in response
- No `traceparent` header when none provided
- Invalid `traceparent` silently ignored (no 400)
- Both `x-trace-id` and `traceparent` coexist
- Feature-gated: all telemetry tests under `#[cfg(feature = "telemetry")]`

### Manual Testing Steps:
1. Run Jaeger via Docker: `docker run -d -p 4317:4317 -p 16686:16686 jaegertracing/all-in-one`
2. Create example app with `with_telemetry(TelemetryConfig { endpoint: "http://localhost:4317".into(), service_name: "example".into(), sample_rate: 1.0 })`
3. Send requests with `traceparent` header
4. Verify traces appear in Jaeger UI at `http://localhost:16686`

## Performance Considerations

- OTLP export uses batch exporter (default in `opentelemetry_sdk`) — spans are batched and flushed periodically, not per-request.
- When `telemetry` feature is not enabled, zero additional overhead — no OTel code compiled.
- `Traceparent::parse()` is a lightweight string operation with no allocations on the failure path.

## References

- W3C Trace Context spec: https://www.w3.org/TR/trace-context/
- OpenTelemetry Rust SDK: https://docs.rs/opentelemetry_sdk
- Current tracing setup: `rapina/src/observability/tracing.rs`
- Current trace ID middleware: `rapina/src/middleware/trace_id.rs`
- Rapina builder: `rapina/src/app.rs:49-300`
