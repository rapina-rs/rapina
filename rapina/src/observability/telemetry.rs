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

    /// Initializes the OpenTelemetry tracing pipeline and installs it as the
    /// global subscriber.
    ///
    /// Returns a `TracerProvider` handle that must be shut down on exit to
    /// flush pending spans. Use `Rapina::with_telemetry()` which handles
    /// this automatically via a shutdown hook.
    pub fn init(self) -> opentelemetry_sdk::trace::SdkTracerProvider {
        use opentelemetry::trace::TracerProvider;
        use opentelemetry_otlp::WithExportConfig;
        use opentelemetry_sdk::Resource;
        use opentelemetry_sdk::trace::{Sampler, SdkTracerProvider};
        use tracing_subscriber::EnvFilter;
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        let sampler = match self.effective_sampler() {
            SamplerConfig::AlwaysOn => Sampler::AlwaysOn,
            SamplerConfig::AlwaysOff => Sampler::AlwaysOff,
            SamplerConfig::TraceIdRatio(r) => Sampler::TraceIdRatioBased(r),
        };

        let resource = Resource::builder()
            .with_attribute(opentelemetry::KeyValue::new(
                "service.name",
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

        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

        tracing_subscriber::registry()
            .with(filter)
            .with(otel_layer)
            .with(tracing_subscriber::fmt::layer())
            .init();

        provider
    }
}

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
        assert_eq!(config.effective_sampler(), SamplerConfig::TraceIdRatio(0.5));
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
