//! Authentication middleware for Rapina.

use hyper::body::Incoming;
use hyper::{Request, Response};

use crate::auth::{AuthConfig, CurrentUser, PublicRoutes};
use crate::context::RequestContext;
use crate::error::Error;
use crate::middleware::{BoxFuture, Middleware, Next};
use crate::response::{BoxBody, IntoResponse};

/// Middleware that enforces JWT authentication on all routes.
///
/// Routes marked with `#[public]` or starting with `/__rapina` bypass authentication.
/// All other routes require a valid `Authorization: Bearer <token>` header.
///
/// # Example
///
/// ```ignore
/// use rapina::prelude::*;
/// use rapina::auth::{AuthConfig, AuthMiddleware};
///
/// let auth_config = AuthConfig::from_env().expect("JWT_SECRET required");
///
/// Rapina::new()
///     .middleware(AuthMiddleware::new(auth_config))
///     .router(router)
///     .listen("127.0.0.1:3000")
///     .await
/// ```
pub struct AuthMiddleware {
    config: AuthConfig,
    public_routes: PublicRoutes,
}

impl AuthMiddleware {
    /// Creates a new auth middleware with the given configuration.
    pub fn new(config: AuthConfig) -> Self {
        Self {
            config,
            public_routes: PublicRoutes::new(),
        }
    }

    /// Creates a new auth middleware with explicit public routes.
    pub fn with_public_routes(config: AuthConfig, public_routes: PublicRoutes) -> Self {
        Self {
            config,
            public_routes,
        }
    }

    /// Extracts the bearer token from the Authorization header.
    fn extract_bearer_token(req: &Request<Incoming>) -> Option<&str> {
        req.headers()
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
    }
}

impl Middleware for AuthMiddleware {
    fn handle<'a>(
        &'a self,
        mut req: Request<Incoming>,
        _ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        Box::pin(async move {
            let method = req.method().as_str();
            let path = req.uri().path();

            // Check if this route is public
            if self.public_routes.is_public(method, path) {
                return next.run(req).await;
            }

            // Extract and validate the bearer token
            let token = match Self::extract_bearer_token(&req) {
                Some(t) => t,
                None => {
                    return Error::unauthorized("missing authorization header").into_response();
                }
            };

            // Decode and validate the JWT
            let claims = match self.config.decode(token) {
                Ok(c) => c,
                Err(e) => {
                    return e.into_response();
                }
            };

            // Create CurrentUser and inject it into request extensions
            let current_user = CurrentUser {
                id: claims.sub.clone(),
                claims,
            };

            req.extensions_mut().insert(current_user);

            next.run(req).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_middleware_new() {
        let config = AuthConfig::new("secret", 3600);
        let _middleware = AuthMiddleware::new(config);
    }

    #[test]
    fn test_auth_middleware_with_public_routes() {
        let config = AuthConfig::new("secret", 3600);
        let mut public = PublicRoutes::new();
        public.add("GET", "/health");

        let middleware = AuthMiddleware::with_public_routes(config, public);
        assert!(middleware.public_routes.is_public("GET", "/health"));
    }
}
