//! `Context`, the per-request context extractor.

use std::ops::Deref;
use std::sync::Arc;

use crate::context::RequestContext;
use crate::error::Error;
use crate::extract::{FromRequestParts, PathParams};
use crate::state::AppState;

/// Provides access to the request context.
///
/// Contains the `trace_id` and request start time for logging and tracing.
///
/// # Examples
///
/// ```ignore
/// use rapina::prelude::*;
///
/// #[get("/trace")]
/// async fn get_trace(ctx: Context) -> String {
///     format!("Trace ID: {}", ctx.trace_id())
/// }
/// ```
#[derive(Debug)]
pub struct Context(pub RequestContext);

impl Context {
    /// Consumes the extractor and returns the inner RequestContext.
    pub fn into_inner(self) -> RequestContext {
        self.0
    }

    /// Returns the trace ID for this request.
    pub fn trace_id(&self) -> &str {
        self.0.trace_id()
    }

    /// Returns the elapsed time since the request started.
    pub fn elapsed(&self) -> std::time::Duration {
        self.0.elapsed()
    }
}

impl Deref for Context {
    type Target = RequestContext;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromRequestParts for Context {
    async fn from_request_parts(
        parts: &http::request::Parts,
        _params: &PathParams,
        _state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        parts
            .extensions
            .get::<RequestContext>()
            .cloned()
            .map(Context)
            .ok_or_else(|| {
                Error::internal(
                    "RequestContext missing from request extensions. \
                     The request pipeline did not initialize the request context.",
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{TestRequest, empty_params, empty_state};

    // Context extractor tests
    #[tokio::test]
    async fn test_context_extractor() {
        let (parts, _) = TestRequest::get("/").into_parts();
        let result = Context::from_request_parts(&parts, &empty_params(), &empty_state()).await;

        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert!(!ctx.trace_id().is_empty());
    }

    #[tokio::test]
    async fn test_context_extractor_with_custom_trace_id() {
        let custom_ctx = crate::context::RequestContext::with_trace_id("custom-123".to_string());
        let (parts, _) = TestRequest::get("/").into_parts_with_context(custom_ctx);

        let result = Context::from_request_parts(&parts, &empty_params(), &empty_state()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().trace_id(), "custom-123");
    }

    #[test]
    fn test_context_into_inner() {
        let ctx = crate::context::RequestContext::with_trace_id("test".to_string());
        let context = Context(ctx);
        assert_eq!(context.into_inner().trace_id(), "test");
    }

    #[test]
    fn test_context_elapsed() {
        let ctx = crate::context::RequestContext::new();
        let context = Context(ctx);
        // Verify elapsed() returns a Duration (compile-time check)
        let _elapsed: std::time::Duration = context.elapsed();
    }

    #[test]
    fn test_context_autoderef() {
        let ctx = Context(crate::context::RequestContext::with_trace_id(
            "test".to_string(),
        ));
        assert_eq!(ctx.trace_id(), "test");
    }
}
