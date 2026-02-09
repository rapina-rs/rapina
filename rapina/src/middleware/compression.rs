use std::io::Write;

use bytes::Bytes;
use flate2::Compression;
use flate2::write::{DeflateEncoder, GzEncoder};
use http::{HeaderValue, Response, header};
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper::body::Incoming;

use crate::context::RequestContext;
use crate::response::BoxBody;

use super::{BoxFuture, Middleware, Next};

const DEFAULT_MIN_SIZE: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Algorithm {
    Gzip,
    Deflate,
}

impl Algorithm {
    fn from_accept_encoding(header: &str) -> Option<Self> {
        if header.contains("gzip") {
            Some(Algorithm::Gzip)
        } else if header.contains("deflate") {
            Some(Algorithm::Deflate)
        } else {
            None
        }
    }

    fn content_encoding(&self) -> &'static str {
        match self {
            Algorithm::Gzip => "gzip",
            Algorithm::Deflate => "deflate",
        }
    }

    fn compress(&self, data: &[u8], level: Compression) -> std::io::Result<Vec<u8>> {
        match self {
            Algorithm::Gzip => {
                let mut encoder = GzEncoder::new(Vec::new(), level);
                encoder.write_all(data)?;
                encoder.finish()
            }
            Algorithm::Deflate => {
                let mut encoder = DeflateEncoder::new(Vec::new(), level);
                encoder.write_all(data)?;
                encoder.finish()
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompressionConfig {
    pub min_size: usize,
    pub level: u32,
}

impl CompressionConfig {
    pub fn new(min_size: usize, level: u32) -> Self {
        Self {
            min_size,
            level: level.min(9),
        }
    }
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            min_size: DEFAULT_MIN_SIZE,
            level: 6,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompressionMiddleware {
    config: CompressionConfig,
}

impl CompressionMiddleware {
    pub fn new(config: CompressionConfig) -> Self {
        Self { config }
    }

    fn is_compressible_content_type(content_type: Option<&HeaderValue>) -> bool {
        let Some(ct) = content_type else {
            return true;
        };

        let ct_str = ct.to_str().unwrap_or("");

        ct_str.starts_with("text/")
            || ct_str.starts_with("application/json")
            || ct_str.starts_with("application/xml")
            || ct_str.starts_with("application/javascript")
            || ct_str.contains("+json")
            || ct_str.contains("+xml")
    }

    fn is_already_encoded(response: &Response<BoxBody>) -> bool {
        response.headers().contains_key(header::CONTENT_ENCODING)
    }
}

impl Default for CompressionMiddleware {
    fn default() -> Self {
        Self::new(CompressionConfig::default())
    }
}

impl Middleware for CompressionMiddleware {
    fn handle<'a>(
        &'a self,
        req: Request<Incoming>,
        _ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        Box::pin(async move {
            let accept_encoding = req
                .headers()
                .get(header::ACCEPT_ENCODING)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            let algorithm = Algorithm::from_accept_encoding(accept_encoding);

            let response = next.run(req).await;

            let algorithm = match algorithm {
                Some(alg)
                    if !Self::is_already_encoded(&response)
                        && Self::is_compressible_content_type(
                            response.headers().get(header::CONTENT_TYPE),
                        ) =>
                {
                    alg
                }
                _ => return response,
            };

            let (parts, body) = response.into_parts();
            let body_bytes = match body.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(_) => return Response::from_parts(parts, Full::new(Bytes::new())),
            };

            if body_bytes.len() < self.config.min_size {
                return Response::from_parts(parts, Full::new(body_bytes));
            }

            let level = Compression::new(self.config.level);
            let compressed = match algorithm.compress(&body_bytes, level) {
                Ok(data) => data,
                Err(_) => return Response::from_parts(parts, Full::new(body_bytes)),
            };

            // not worth it
            if compressed.len() >= body_bytes.len() {
                return Response::from_parts(parts, Full::new(body_bytes));
            }

            let mut response = Response::from_parts(parts, Full::new(Bytes::from(compressed)));
            response.headers_mut().insert(
                header::CONTENT_ENCODING,
                HeaderValue::from_static(algorithm.content_encoding()),
            );
            response.headers_mut().remove(header::CONTENT_LENGTH);
            response
                .headers_mut()
                .insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));

            response
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = CompressionConfig::default();
        assert_eq!(config.min_size, 1024);
        assert_eq!(config.level, 6);
    }

    #[test]
    fn test_config_clamps_level() {
        let config = CompressionConfig::new(1024, 15);
        assert_eq!(config.level, 9);
    }

    #[test]
    fn test_algorithm_from_accept_encoding() {
        assert_eq!(
            Algorithm::from_accept_encoding("gzip, deflate"),
            Some(Algorithm::Gzip)
        );
        assert_eq!(
            Algorithm::from_accept_encoding("deflate"),
            Some(Algorithm::Deflate)
        );
        assert_eq!(Algorithm::from_accept_encoding("br"), None);
    }

    #[test]
    fn test_gzip_compression() {
        let data = "hello from rapina ".repeat(100);
        let compressed = Algorithm::Gzip
            .compress(data.as_bytes(), Compression::default())
            .unwrap();
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn test_deflate_compression() {
        let data = "hello from rapina ".repeat(100);
        let compressed = Algorithm::Deflate
            .compress(data.as_bytes(), Compression::default())
            .unwrap();
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn test_is_compressible_content_type() {
        assert!(CompressionMiddleware::is_compressible_content_type(Some(
            &HeaderValue::from_static("text/html")
        )));
        assert!(CompressionMiddleware::is_compressible_content_type(Some(
            &HeaderValue::from_static("application/json")
        )));
        assert!(!CompressionMiddleware::is_compressible_content_type(Some(
            &HeaderValue::from_static("image/png")
        )));
        assert!(CompressionMiddleware::is_compressible_content_type(None));
    }
}
