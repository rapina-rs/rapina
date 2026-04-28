use std::sync::Arc;

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::Full;
use hyper::body::Incoming;
use prometheus::{
    CounterVec, Encoder, HistogramOpts, HistogramVec, IntGauge, Opts, Registry, TextEncoder,
    core::Collector,
};

use crate::extract::PathParams;
use crate::response::{BoxBody, PROMETHEUS_TEXT_FORMAT};
use crate::state::AppState;
use http::header::CONTENT_TYPE;

#[derive(Clone)]
pub struct MetricsRegistry {
    pub(crate) registry: Arc<Registry>,
    pub(crate) http_requests_total: CounterVec,
    pub(crate) http_request_duration_seconds: HistogramVec,
    pub(crate) http_requests_in_flight: IntGauge,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        Self::new_with_collectors(vec![])
    }

    pub(crate) fn new_with_collectors(collectors: Vec<Box<dyn Collector>>) -> Self {
        let registry = Registry::new();

        let http_requests_total = CounterVec::new(
            Opts::new("http_requests_total", "Total number of HTTP requests"),
            &["method", "path", "status"],
        )
        .expect("failed to create http_requests_total metric");

        registry
            .register(Box::new(http_requests_total.clone()))
            .expect("failed to register http_requests_total");

        let http_request_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "http_request_duration_seconds",
                "HTTP request duration in seconds",
            ),
            &["method", "path"],
        )
        .expect("failed to create http_request_duration_seconds metric");

        registry
            .register(Box::new(http_request_duration_seconds.clone()))
            .expect("failed to register http_request_duration_seconds");

        let http_requests_in_flight = IntGauge::new(
            "http_requests_in_flight",
            "Number of HTTP requests currently being processed",
        )
        .expect("failed to create http_requests_in_flight metric");

        registry
            .register(Box::new(http_requests_in_flight.clone()))
            .expect("failed to register http_requests_in_flight");

        for collector in collectors {
            registry
                .register(collector)
                .expect("failed to register custom metric");
        }

        Self {
            registry: Arc::new(registry),
            http_requests_total,
            http_request_duration_seconds,
            http_requests_in_flight,
        }
    }

    /// Encodes all metrics in the Prometheus text exposition format.
    pub fn encode(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .unwrap_or_default();
        String::from_utf8(buffer).unwrap_or_default()
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Handler for the `GET /metrics` endpoint.
///
/// Returns all collected metrics in Prometheus text format.
pub async fn metrics_handler(
    _req: Request<Incoming>,
    _params: PathParams,
    state: Arc<AppState>,
) -> Response<BoxBody> {
    match state.get::<MetricsRegistry>() {
        Some(registry) => {
            let body = registry.encode();
            Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, PROMETHEUS_TEXT_FORMAT)
                .body(Full::new(Bytes::from(body)))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body(Full::new(Bytes::new()))
            .unwrap(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_registry_new() {
        let _registry = MetricsRegistry::new();
    }

    #[test]
    fn test_metrics_registry_default() {
        let _registry = MetricsRegistry::default();
    }

    #[test]
    fn test_metrics_registry_encode_empty_contains_metric_names() {
        let registry = MetricsRegistry::new();
        let output = registry.encode();

        // It only loads `http_requests_in_flight` because its from IntGauge type
        assert!(output.contains("http_requests_in_flight"));
    }

    #[test]
    fn test_metrics_registry_encode_prometheus_format() {
        let registry = MetricsRegistry::new();
        let output = registry.encode();
        assert!(output.contains("# HELP"));
        assert!(output.contains("# TYPE"));
    }

    #[test]
    fn test_metrics_registry_counter_increments() {
        let registry = MetricsRegistry::new();
        registry
            .http_requests_total
            .with_label_values(&["GET", "/health", "200"])
            .inc();

        let output = registry.encode();
        assert!(output.contains("http_requests_total"));
        assert!(output.contains(r#"method="GET""#));
        assert!(output.contains(r#"path="/health""#));
        assert!(output.contains(r#"status="200""#));
        assert!(output.contains("} 1"));
    }

    #[test]
    fn test_metrics_registry_in_flight_gauge() {
        let registry = MetricsRegistry::new();
        assert_eq!(registry.http_requests_in_flight.get(), 0);

        registry.http_requests_in_flight.inc();
        registry.http_requests_in_flight.inc();
        assert_eq!(registry.http_requests_in_flight.get(), 2);

        registry.http_requests_in_flight.dec();
        assert_eq!(registry.http_requests_in_flight.get(), 1);
    }

    #[test]
    fn test_metrics_registry_histogram_observe() {
        let registry = MetricsRegistry::new();
        registry
            .http_request_duration_seconds
            .with_label_values(&["POST", "/users"])
            .observe(0.042);

        let output = registry.encode();
        assert!(output.contains("http_request_duration_seconds"));
        assert!(output.contains(r#"method="POST""#));
    }

    #[test]
    fn test_metrics_registry_clone_shares_state() {
        let registry = MetricsRegistry::new();
        let clone = registry.clone();

        registry
            .http_requests_total
            .with_label_values(&["POST", "/", "200"])
            .inc();

        // The clone wraps the same Arc<Registry>, so its encode reflects the increment
        let output = clone.encode();
        assert!(output.contains("} 1"));
    }

    #[test]
    fn test_custom_collector_appears_in_output() {
        use prometheus::IntCounter;

        let counter = IntCounter::new("my_custom_total", "A custom counter").unwrap();
        counter.inc();

        let registry = MetricsRegistry::new_with_collectors(vec![Box::new(counter)]);
        let output = registry.encode();

        assert!(output.contains("my_custom_total"));
        assert!(output.contains("A custom counter"));
    }

    #[test]
    fn test_multiple_custom_collectors() {
        use prometheus::{IntCounter, IntGauge};

        let counter = IntCounter::new("custom_requests", "Custom request counter").unwrap();
        let gauge = IntGauge::new("custom_queue_depth", "Custom queue depth").unwrap();
        gauge.set(42);

        let registry =
            MetricsRegistry::new_with_collectors(vec![Box::new(counter), Box::new(gauge)]);
        let output = registry.encode();

        assert!(output.contains("custom_requests"));
        assert!(output.contains("custom_queue_depth"));
        assert!(output.contains("42"));
    }

    #[test]
    fn test_custom_collector_with_labels() {
        use prometheus::{IntCounterVec, Opts};

        let counter =
            IntCounterVec::new(Opts::new("orders_total", "Total orders"), &["status"]).unwrap();
        counter.with_label_values(&["placed"]).inc();
        counter.with_label_values(&["placed"]).inc();
        counter.with_label_values(&["cancelled"]).inc();

        let registry = MetricsRegistry::new_with_collectors(vec![Box::new(counter)]);
        let output = registry.encode();

        assert!(output.contains("orders_total"));
        assert!(output.contains(r#"status="placed""#));
        assert!(output.contains(r#"status="cancelled""#));
    }

    #[test]
    fn test_empty_collectors_vec_is_same_as_new() {
        let r1 = MetricsRegistry::new();
        let r2 = MetricsRegistry::new_with_collectors(vec![]);

        let o1 = r1.encode();
        let o2 = r2.encode();
        assert_eq!(o1, o2);
    }
}
