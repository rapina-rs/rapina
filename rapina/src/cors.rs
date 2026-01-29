//! Cors

#[derive(Debug, Clone)]
pub struct CorsConfig {
    pub allowed_origins: AllowedOrigins,
    pub allowed_methods: AllowedMethods,
    pub allowed_headers: AllowedHeaders,
}

impl CorsConfig {
    pub fn permissive() -> Self {
        // Allow everything for dev
        Self {
            allowed_origins: AllowedOrigins::Any,
            allowed_methods: AllowedMethods::Any,
            allowed_headers: AllowedHeaders::Any,
        }
    }

    pub fn with_origins(origins: Vec<String>) -> Self {
        // Specific origins
        Self {
            allowed_origins: AllowedOrigins::Exact(vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
                "PATCH".to_string(),
                "OPTIONS".to_string(),
            ]),
            allowed_methods: AllowedMethods::Any,
            allowed_headers: AllowedHeaders::Any,
        }
    }
}

#[derive(Debug, Clone)]
pub enum AllowedHeaders {
    Any,
    List(Vec<String>),
}

#[derive(Debug, Clone)]
pub enum AllowedMethods {
    Any,
    List(Vec<String>),
}

#[derive(Debug, Clone)]
pub enum AllowedOrigins {
    Any,
    Exact(Vec<String>),
}
