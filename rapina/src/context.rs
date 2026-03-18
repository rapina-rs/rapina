use std::sync::OnceLock;
use std::time::Instant;

/// Per-request context passed through the middleware stack and into handlers.
///
/// Created automatically for each incoming connection. Available via
/// `req.extensions().get::<RequestContext>()` inside handlers and middleware.
///
/// The trace ID is generated lazily on first access, so requests that never
/// read the trace ID (e.g. when no tracing middleware is registered) avoid
/// the UUID v4 allocation entirely.
#[derive(Debug, Clone)]
pub struct RequestContext {
    trace_id: OnceLock<String>,
    /// Timestamp recorded when the request context was created, used to
    /// calculate request duration.
    pub start_time: Instant,
}

impl RequestContext {
    pub fn new() -> Self {
        Self {
            trace_id: OnceLock::new(),
            start_time: Instant::now(),
        }
    }

    pub fn with_trace_id(trace_id: String) -> Self {
        let cell = OnceLock::new();
        let _ = cell.set(trace_id);
        Self {
            trace_id: cell,
            start_time: Instant::now(),
        }
    }

    /// Returns the trace ID for this request, generating a UUID v4 on first access.
    pub fn trace_id(&self) -> &str {
        self.trace_id
            .get_or_init(|| uuid::Uuid::new_v4().to_string())
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_new_generates_uuid() {
        let ctx = RequestContext::new();
        // UUID v4 format: 8-4-4-4-12 hex chars
        assert_eq!(ctx.trace_id().len(), 36);
        assert!(ctx.trace_id().chars().filter(|c| *c == '-').count() == 4);
    }

    #[test]
    fn test_new_generates_unique_ids() {
        let ctx1 = RequestContext::new();
        let ctx2 = RequestContext::new();
        assert_ne!(ctx1.trace_id(), ctx2.trace_id());
    }

    #[test]
    fn test_with_trace_id() {
        let custom_id = "custom-trace-123".to_string();
        let ctx = RequestContext::with_trace_id(custom_id.clone());
        assert_eq!(ctx.trace_id(), custom_id);
    }

    #[test]
    fn test_elapsed_increases() {
        let ctx = RequestContext::new();
        let elapsed1 = ctx.elapsed();
        thread::sleep(Duration::from_millis(10));
        let elapsed2 = ctx.elapsed();
        assert!(elapsed2 > elapsed1);
    }

    #[test]
    fn test_default_is_new() {
        let ctx = RequestContext::default();
        assert_eq!(ctx.trace_id().len(), 36);
    }

    #[test]
    fn test_clone() {
        let ctx1 = RequestContext::new();
        // Access trace_id to initialize it before cloning
        let _ = ctx1.trace_id();
        let ctx2 = ctx1.clone();
        assert_eq!(ctx1.trace_id(), ctx2.trace_id());
    }

    #[test]
    fn test_trace_id_is_lazy() {
        let ctx = RequestContext::new();
        // The internal OnceLock should not be initialized until trace_id() is called
        assert!(
            ctx.trace_id.get().is_none(),
            "trace_id should not be initialized before first access"
        );
        let id = ctx.trace_id();
        assert_eq!(id.len(), 36, "trace_id should be a valid UUID after access");
        assert!(
            ctx.trace_id.get().is_some(),
            "trace_id should be initialized after first access"
        );
    }

    #[test]
    fn test_trace_id_stable_across_calls() {
        let ctx = RequestContext::new();
        let id1 = ctx.trace_id();
        let id2 = ctx.trace_id();
        assert_eq!(
            id1, id2,
            "trace_id should return the same value on repeated calls"
        );
    }

    #[test]
    fn test_with_trace_id_is_eager() {
        let ctx = RequestContext::with_trace_id("pre-set".to_string());
        assert!(
            ctx.trace_id.get().is_some(),
            "with_trace_id should eagerly set the value"
        );
        assert_eq!(ctx.trace_id(), "pre-set");
    }

    #[test]
    fn test_debug() {
        let ctx = RequestContext::with_trace_id("test-id".to_string());
        let debug_str = format!("{:?}", ctx);
        assert!(debug_str.contains("test-id"));
    }
}
