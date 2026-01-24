//! Authentication system for Rapina applications.
//!
//! Provides JWT-based authentication with a "protected by default" approach.
//! All routes require authentication unless explicitly marked with `#[public]`.
//!
//! # Quick Start
//!
//! ```ignore
//! use rapina::prelude::*;
//! use rapina::auth::{AuthConfig, CurrentUser};
//!
//! #[public]
//! #[get("/health")]
//! async fn health() -> &'static str {
//!     "ok"
//! }
//!
//! #[get("/me")]
//! async fn me(user: CurrentUser) -> Json<serde_json::Value> {
//!     Json(serde_json::json!({ "id": user.id }))
//! }
//!
//! #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     let auth_config = AuthConfig::from_env().expect("Missing JWT_SECRET");
//!
//!     Rapina::new()
//!         .with_auth(auth_config)
//!         .router(router)
//!         .listen("127.0.0.1:3000")
//!         .await
//! }
//! ```

mod middleware;

pub use middleware::AuthMiddleware;

use crate::error::Error;
use crate::extract::{FromRequestParts, PathParams};
use crate::state::AppState;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// JWT claims structure.
///
/// Contains the standard JWT claims plus any custom data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject - typically the user ID
    pub sub: String,
    /// Expiration time (Unix timestamp)
    pub exp: u64,
    /// Issued at time (Unix timestamp)
    pub iat: u64,
}

impl Claims {
    /// Creates new claims for the given subject with specified expiration.
    pub fn new(sub: impl Into<String>, expires_in_secs: u64) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            sub: sub.into(),
            exp: now + expires_in_secs,
            iat: now,
        }
    }

    /// Checks if the token has expired.
    pub fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.exp < now
    }
}

/// Standard token response for login endpoints.
///
/// Provides a consistent response format for token generation.
///
/// # Example
///
/// ```ignore
/// #[post("/login")]
/// async fn login(auth: State<AuthConfig>) -> Result<Json<TokenResponse>> {
///     let token = auth.create_token("user123")?;
///     Ok(Json(TokenResponse::new(token, auth.expiration())))
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TokenResponse {
    /// The JWT token
    pub token: String,
    /// Token expiration time in seconds
    pub expires_in: u64,
}

impl TokenResponse {
    /// Creates a new token response.
    pub fn new(token: String, expires_in: u64) -> Self {
        Self { token, expires_in }
    }
}

/// The authenticated user extracted from a valid JWT token.
///
/// This extractor is automatically populated by the auth middleware
/// for protected routes. Use it to access the current user's information.
///
/// # Example
///
/// ```ignore
/// #[get("/me")]
/// async fn me(user: CurrentUser) -> Json<serde_json::Value> {
///     Json(serde_json::json!({
///         "id": user.id,
///         "claims": user.claims
///     }))
/// }
/// ```
#[derive(Debug, Clone)]
pub struct CurrentUser {
    /// The user ID (from JWT `sub` claim)
    pub id: String,
    /// The full JWT claims
    pub claims: Claims,
}

impl FromRequestParts for CurrentUser {
    async fn from_request_parts(
        parts: &http::request::Parts,
        _params: &PathParams,
        _state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        parts
            .extensions
            .get::<CurrentUser>()
            .cloned()
            .ok_or_else(|| Error::unauthorized("authentication required"))
    }
}

/// Configuration for JWT authentication.
///
/// Use environment variables to configure:
/// - `JWT_SECRET` - The secret key for signing/verifying tokens (required)
/// - `JWT_EXPIRATION` - Token expiration in seconds (default: 3600)
///
/// # Example
///
/// ```ignore
/// let config = AuthConfig::from_env().expect("Missing JWT_SECRET");
/// // or with explicit values:
/// let config = AuthConfig::new("my-secret-key", 7200);
/// ```
#[derive(Clone)]
pub struct AuthConfig {
    /// The secret key for signing and verifying JWT tokens
    secret: String,
    /// Token expiration time in seconds
    expiration: u64,
}

impl AuthConfig {
    /// Creates a new auth configuration with the given secret and expiration.
    pub fn new(secret: impl Into<String>, expiration: u64) -> Self {
        Self {
            secret: secret.into(),
            expiration,
        }
    }

    /// Loads configuration from environment variables.
    ///
    /// Required: `JWT_SECRET`
    /// Optional: `JWT_EXPIRATION` (default: 3600 seconds)
    pub fn from_env() -> Result<Self, crate::config::ConfigError> {
        let secret = crate::config::get_env("JWT_SECRET")?;
        let expiration = crate::config::get_env_parsed_or("JWT_EXPIRATION", 3600);
        Ok(Self { secret, expiration })
    }

    /// Returns the configured expiration time in seconds.
    pub fn expiration(&self) -> u64 {
        self.expiration
    }

    /// Encodes claims into a JWT token.
    pub fn encode(&self, claims: &Claims) -> Result<String, Error> {
        encode(
            &Header::default(),
            claims,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )
        .map_err(|e| Error::internal(format!("failed to encode token: {}", e)))
    }

    /// Decodes and validates a JWT token.
    pub fn decode(&self, token: &str) -> Result<Claims, Error> {
        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|e| match e.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => {
                Error::unauthorized("token expired")
            }
            jsonwebtoken::errors::ErrorKind::InvalidToken => Error::unauthorized("invalid token"),
            _ => Error::unauthorized(format!("token validation failed: {}", e)),
        })?;

        Ok(token_data.claims)
    }

    /// Creates a new token for the given user ID.
    pub fn create_token(&self, user_id: impl Into<String>) -> Result<String, Error> {
        let claims = Claims::new(user_id, self.expiration);
        self.encode(&claims)
    }
}

/// Registry of public routes that bypass authentication.
///
/// Used internally by the auth middleware to determine which routes
/// should be accessible without a valid JWT token.
#[derive(Clone, Default)]
pub struct PublicRoutes {
    routes: Vec<(String, String)>, // (method, path)
}

impl PublicRoutes {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }

    /// Adds a public route.
    pub fn add(&mut self, method: &str, path: &str) {
        self.routes.push((method.to_string(), path.to_string()));
    }

    /// Checks if a route is public.
    pub fn is_public(&self, method: &str, path: &str) -> bool {
        // Introspection routes are always public
        if path.starts_with("/__rapina") {
            return true;
        }

        self.routes
            .iter()
            .any(|(m, p)| m == method && Self::matches_pattern(p, path))
    }

    /// Matches a route pattern against a path.
    fn matches_pattern(pattern: &str, path: &str) -> bool {
        let pattern_parts: Vec<&str> = pattern.split('/').collect();
        let path_parts: Vec<&str> = path.split('/').collect();

        if pattern_parts.len() != path_parts.len() {
            return false;
        }

        pattern_parts
            .iter()
            .zip(path_parts.iter())
            .all(|(pattern_part, path_part)| {
                pattern_part.starts_with(':') || pattern_part == path_part
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claims_new() {
        let claims = Claims::new("user123", 3600);
        assert_eq!(claims.sub, "user123");
        assert!(claims.exp > claims.iat);
        assert_eq!(claims.exp - claims.iat, 3600);
    }

    #[test]
    fn test_claims_not_expired() {
        let claims = Claims::new("user123", 3600);
        assert!(!claims.is_expired());
    }

    #[test]
    fn test_claims_expired() {
        let mut claims = Claims::new("user123", 0);
        claims.exp = claims.iat - 1; // Set expiration in the past
        assert!(claims.is_expired());
    }

    #[test]
    fn test_auth_config_new() {
        let config = AuthConfig::new("secret", 7200);
        assert_eq!(config.expiration(), 7200);
    }

    #[test]
    fn test_auth_config_encode_decode() {
        let config = AuthConfig::new("test-secret", 3600);
        let claims = Claims::new("user456", 3600);

        let token = config.encode(&claims).unwrap();
        assert!(!token.is_empty());

        let decoded = config.decode(&token).unwrap();
        assert_eq!(decoded.sub, "user456");
    }

    #[test]
    fn test_auth_config_create_token() {
        let config = AuthConfig::new("test-secret", 3600);
        let token = config.create_token("user789").unwrap();
        assert!(!token.is_empty());

        let decoded = config.decode(&token).unwrap();
        assert_eq!(decoded.sub, "user789");
    }

    #[test]
    fn test_auth_config_invalid_token() {
        let config = AuthConfig::new("test-secret", 3600);
        let result = config.decode("invalid.token.here");
        assert!(result.is_err());
    }

    #[test]
    fn test_auth_config_wrong_secret() {
        let config1 = AuthConfig::new("secret1", 3600);
        let config2 = AuthConfig::new("secret2", 3600);

        let token = config1.create_token("user").unwrap();
        let result = config2.decode(&token);
        assert!(result.is_err());
    }

    #[test]
    fn test_public_routes_empty() {
        let routes = PublicRoutes::new();
        assert!(!routes.is_public("GET", "/protected"));
    }

    #[test]
    fn test_public_routes_exact_match() {
        let mut routes = PublicRoutes::new();
        routes.add("GET", "/health");
        routes.add("POST", "/login");

        assert!(routes.is_public("GET", "/health"));
        assert!(routes.is_public("POST", "/login"));
        assert!(!routes.is_public("GET", "/login"));
        assert!(!routes.is_public("POST", "/health"));
    }

    #[test]
    fn test_public_routes_with_params() {
        let mut routes = PublicRoutes::new();
        routes.add("GET", "/users/:id/public");

        assert!(routes.is_public("GET", "/users/123/public"));
        assert!(routes.is_public("GET", "/users/abc/public"));
        assert!(!routes.is_public("GET", "/users/123/private"));
    }

    #[test]
    fn test_public_routes_introspection_always_public() {
        let routes = PublicRoutes::new();
        assert!(routes.is_public("GET", "/__rapina/routes"));
        assert!(routes.is_public("GET", "/__rapina/openapi.json"));
    }
}
