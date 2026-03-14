use hyper::body::Incoming;
use hyper::{Request, Response};
use tracing::{Instrument, info, info_span};

use crate::context::RequestContext;
use crate::response::BoxBody;

use super::{BoxFuture, Middleware, Next};

/// Configuration for [`RequestLogMiddleware`] verbosity and redaction.
///
/// By default, all extra logging is disabled — the middleware only logs
/// method, path, status, and duration (same as the previous behavior).
///
/// Use [`verbose()`](Self::verbose) to enable all fields with sensible
/// redaction defaults, or toggle individual flags with the builder methods.
#[derive(Debug, Clone, Default)]
pub struct RequestLogConfig {
    /// Log request and response headers.
    pub log_headers: bool,
    /// Log the URI query string.
    pub log_query_params: bool,
    /// Log `content-length` for request and response.
    pub log_body_size: bool,
    /// Header names whose values are replaced with `[REDACTED]`.
    /// Matching is case-insensitive.
    pub redacted_headers: Vec<String>,
}

impl RequestLogConfig {
    /// All flags on with default redacted headers.
    pub fn verbose() -> Self {
        Self {
            log_headers: true,
            log_query_params: true,
            log_body_size: true,
            redacted_headers: default_redacted_headers(),
        }
    }

    pub fn log_headers(mut self, enabled: bool) -> Self {
        self.log_headers = enabled;
        self
    }

    pub fn log_query_params(mut self, enabled: bool) -> Self {
        self.log_query_params = enabled;
        self
    }

    pub fn log_body_size(mut self, enabled: bool) -> Self {
        self.log_body_size = enabled;
        self
    }

    /// Append a header name to the redaction list.
    pub fn redact_header(mut self, name: impl Into<String>) -> Self {
        self.redacted_headers.push(name.into());
        self
    }
}

fn default_redacted_headers() -> Vec<String> {
    vec![
        "authorization".to_string(),
        "proxy-authorization".to_string(),
        "cookie".to_string(),
        "set-cookie".to_string(),
        "x-api-key".to_string(),
    ]
}

fn format_headers(headers: &hyper::HeaderMap, redacted: &[String]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (name, value) in headers.iter() {
        let name_lower = name.as_str().to_lowercase();
        let val = if redacted.iter().any(|r| r.to_lowercase() == name_lower) {
            "[REDACTED]".to_string()
        } else {
            value.to_str().unwrap_or("[non-utf8]").to_string()
        };
        parts.push(format!("{}={}", name.as_str(), val));
    }
    parts.join("; ")
}

/// Structured request/response logging middleware.
///
/// With default configuration this logs method, path, status, and duration
/// at INFO level — identical to the previous zero-config behavior.
///
/// For richer output, use [`RequestLogMiddleware::verbose()`] or pass a
/// custom [`RequestLogConfig`] via [`RequestLogMiddleware::with_config()`].
#[derive(Debug, Clone)]
pub struct RequestLogMiddleware {
    config: RequestLogConfig,
}

impl RequestLogMiddleware {
    /// Default config — no extra logging.
    pub fn new() -> Self {
        Self {
            config: RequestLogConfig::default(),
        }
    }

    /// All extra logging enabled with default redaction.
    pub fn verbose() -> Self {
        Self {
            config: RequestLogConfig::verbose(),
        }
    }

    /// Full control over what gets logged.
    pub fn with_config(config: RequestLogConfig) -> Self {
        Self { config }
    }
}

impl Default for RequestLogMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl Middleware for RequestLogMiddleware {
    fn handle<'a>(
        &'a self,
        req: Request<Incoming>,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        let trace_id = ctx.trace_id.clone();
        let verbose =
            self.config.log_headers || self.config.log_query_params || self.config.log_body_size;

        let req_headers = if self.config.log_headers {
            Some(format_headers(req.headers(), &self.config.redacted_headers))
        } else {
            None
        };

        let query = if self.config.log_query_params {
            Some(req.uri().query().unwrap_or("").to_string())
        } else {
            None
        };

        let req_body_size = if self.config.log_body_size {
            req.headers()
                .get(hyper::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        } else {
            None
        };

        let span = info_span!(
            "request",
            method = %method,
            path = %path,
            trace_id = %trace_id,
        );

        Box::pin(
            async move {
                let response = next.run(req).await;
                let duration = ctx.elapsed();
                let status = response.status().as_u16();

                if verbose {
                    let res_headers = if self.config.log_headers {
                        Some(format_headers(
                            response.headers(),
                            &self.config.redacted_headers,
                        ))
                    } else {
                        None
                    };

                    let res_body_size = if self.config.log_body_size {
                        response
                            .headers()
                            .get(hyper::header::CONTENT_LENGTH)
                            .and_then(|v| v.to_str().ok())
                            .map(|s| s.to_string())
                    } else {
                        None
                    };

                    info!(
                        status = status,
                        duration_ms = duration.as_millis() as u64,
                        request_headers = req_headers.as_deref().unwrap_or_default(),
                        response_headers = res_headers.as_deref().unwrap_or_default(),
                        query = query.as_deref().unwrap_or_default(),
                        request_body_size = req_body_size.as_deref().unwrap_or_default(),
                        response_body_size = res_body_size.as_deref().unwrap_or_default(),
                        "request completed"
                    );
                } else {
                    info!(
                        status = status,
                        duration_ms = duration.as_millis() as u64,
                        "request completed"
                    );
                }

                response
            }
            .instrument(span),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_all_flags_off() {
        let config = RequestLogConfig::default();
        assert!(!config.log_headers);
        assert!(!config.log_query_params);
        assert!(!config.log_body_size);
        assert!(config.redacted_headers.is_empty());
    }

    #[test]
    fn test_verbose_config_all_flags_on() {
        let config = RequestLogConfig::verbose();
        assert!(config.log_headers);
        assert!(config.log_query_params);
        assert!(config.log_body_size);
        assert_eq!(config.redacted_headers.len(), 5);
        assert!(
            config
                .redacted_headers
                .contains(&"authorization".to_string())
        );
        assert!(
            config
                .redacted_headers
                .contains(&"proxy-authorization".to_string())
        );
        assert!(config.redacted_headers.contains(&"cookie".to_string()));
        assert!(config.redacted_headers.contains(&"set-cookie".to_string()));
        assert!(config.redacted_headers.contains(&"x-api-key".to_string()));
    }

    #[test]
    fn test_builder_toggles_individual_flags() {
        let config = RequestLogConfig::default()
            .log_headers(true)
            .log_body_size(true);
        assert!(config.log_headers);
        assert!(!config.log_query_params);
        assert!(config.log_body_size);
    }

    #[test]
    fn test_redact_header_appends() {
        let config = RequestLogConfig::verbose().redact_header("x-custom-secret");
        assert_eq!(config.redacted_headers.len(), 6);
        assert!(
            config
                .redacted_headers
                .contains(&"x-custom-secret".to_string())
        );
    }

    #[test]
    fn test_format_headers_redacts_case_insensitive() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert("authorization", "Bearer secret".parse().unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());

        let redacted = vec!["Authorization".to_string()];
        let formatted = format_headers(&headers, &redacted);
        assert!(formatted.contains("authorization=[REDACTED]"));
        assert!(formatted.contains("content-type=application/json"));
        assert!(!formatted.contains("secret"));
    }

    #[test]
    fn test_format_headers_no_redaction() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert("content-type", "text/plain".parse().unwrap());

        let formatted = format_headers(&headers, &[]);
        assert!(formatted.contains("content-type=text/plain"));
    }

    #[test]
    fn test_middleware_new_uses_default_config() {
        let mw = RequestLogMiddleware::new();
        assert!(!mw.config.log_headers);
    }

    #[test]
    fn test_middleware_verbose_uses_verbose_config() {
        let mw = RequestLogMiddleware::verbose();
        assert!(mw.config.log_headers);
        assert_eq!(mw.config.redacted_headers.len(), 5);
    }

    #[test]
    fn test_middleware_with_config() {
        let config = RequestLogConfig::default().log_query_params(true);
        let mw = RequestLogMiddleware::with_config(config);
        assert!(mw.config.log_query_params);
        assert!(!mw.config.log_headers);
    }

    #[test]
    fn test_middleware_default() {
        let mw: RequestLogMiddleware = Default::default();
        assert!(!mw.config.log_headers);
    }
}
