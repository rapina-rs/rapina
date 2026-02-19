//! The main application builder for Rapina.

use std::net::SocketAddr;

use crate::auth::{AuthConfig, AuthMiddleware, PublicRoutes};
use crate::introspection::{RouteRegistry, list_routes};
#[cfg(feature = "metrics")]
use crate::metrics::{MetricsMiddleware, MetricsRegistry, metrics_handler};
use crate::middleware::{
    CompressionConfig, CompressionMiddleware, CorsConfig, CorsMiddleware, Middleware,
    MiddlewareStack, RateLimitConfig, RateLimitMiddleware,
};
use crate::observability::TracingConfig;
use crate::openapi::{OpenApiRegistry, build_openapi_spec, openapi_spec};
use crate::router::Router;
use crate::server::serve;
use crate::state::AppState;

/// The main application type for building Rapina servers.
///
/// Use the builder pattern to configure routing, state, middleware,
/// and observability before starting the server.
///
/// # Examples
///
/// ```rust,no_run
/// use rapina::prelude::*;
///
/// #[get("/")]
/// async fn hello() -> &'static str {
///     "Hello!"
/// }
///
/// #[tokio::main]
/// async fn main() -> std::io::Result<()> {
///     let router = Router::new()
///         .get("/", hello);
///
///     Rapina::new()
///         .router(router)
///         .listen("127.0.0.1:3000")
///         .await
/// }
/// ```
pub struct Rapina {
    /// The router for this application.
    pub(crate) router: Router,
    /// The application state.
    pub(crate) state: AppState,
    /// The middleware stack.
    pub(crate) middlewares: MiddlewareStack,
    /// Whether introspection is enabled.
    pub(crate) introspection: bool,
    /// Whether metrics is enabled.
    pub(crate) metrics: bool,
    /// Whether OpenAPI is enabled
    pub(crate) openapi: bool,
    pub(crate) openapi_title: String,
    pub(crate) openapi_version: String,
    /// Authentication configuration (if enabled)
    pub(crate) auth_config: Option<AuthConfig>,
    /// Public routes registry
    pub(crate) public_routes: PublicRoutes,
}

impl Rapina {
    /// Creates a new Rapina application builder.
    ///
    /// Introspection is enabled by default in debug builds.
    pub fn new() -> Self {
        Self {
            router: Router::new(),
            state: AppState::new(),
            middlewares: MiddlewareStack::new(),
            introspection: cfg!(debug_assertions),
            metrics: false,
            openapi: false,
            openapi_title: "API".to_string(),
            openapi_version: "1.0.0".to_string(),
            auth_config: None,
            public_routes: PublicRoutes::new(),
        }
    }

    /// Sets the router for the application.
    pub fn router(mut self, router: Router) -> Self {
        self.router = router;
        self
    }

    /// Adds shared state that can be accessed by handlers via [`State`](crate::extract::State).
    pub fn state<T: Send + Sync + 'static>(mut self, value: T) -> Self {
        self.state = self.state.with(value);
        self
    }

    /// Adds a middleware to the application.
    pub fn middleware<M: Middleware>(mut self, middleware: M) -> Self {
        self.middlewares.add(middleware);
        self
    }

    /// Enables CORS for the application.
    ///
    /// Use `CorsConfig::permisive()` for development (it allows all origins),
    /// or `CorsConfig::with_origins()` for production with specific origins.
    ///
    /// # Example
    ///
    /// ```ignore
    ///  Rapina::new()
    ///  .with_cors(CorsConfig::permisive())
    ///  .router(router)
    ///  .listen("127.0.0.1:3000")
    ///  .await
    pub fn with_cors(mut self, config: CorsConfig) -> Self {
        self.middlewares.add(CorsMiddleware::new(config));
        self
    }

    /// Enables rate limiting for the application.
    ///
    /// Uses a token bucket algorithm to limit requests per client.
    /// By default, clients are identified by IP address.
    ///
    /// # Example
    ///
    /// ```ignore
    /// Rapina::new()
    ///     .with_rate_limit(RateLimitConfig::per_minute(100))
    ///     .router(router)
    ///     .listen("127.0.0.1:3000")
    ///     .await
    /// ```
    pub fn with_rate_limit(mut self, config: RateLimitConfig) -> Self {
        self.middlewares.add(RateLimitMiddleware::new(config));
        self
    }

    /// Enables response compression (gzip, deflate).
    pub fn with_compression(mut self, config: CompressionConfig) -> Self {
        self.middlewares.add(CompressionMiddleware::new(config));
        self
    }

    /// Enables JWT authentication with the given configuration.
    ///
    /// When enabled, all routes require a valid `Authorization: Bearer <token>` header
    /// unless marked with `#[public]` or registered via [`public_route`](Self::public_route).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let auth_config = AuthConfig::from_env().expect("JWT_SECRET required");
    ///
    /// Rapina::new()
    ///     .with_auth(auth_config)
    ///     .router(router)
    ///     .listen("127.0.0.1:3000")
    ///     .await
    /// ```
    pub fn with_auth(mut self, config: AuthConfig) -> Self {
        self.auth_config = Some(config);
        self
    }

    /// Registers a route as public (no authentication required).
    ///
    /// Use this for routes that should be accessible without a JWT token.
    /// Routes starting with `/__rapina` are automatically public.
    ///
    /// # Example
    ///
    /// ```ignore
    /// Rapina::new()
    ///     .with_auth(auth_config)
    ///     .public_route("GET", "/health")
    ///     .public_route("POST", "/login")
    ///     .router(router)
    ///     .listen("127.0.0.1:3000")
    ///     .await
    /// ```
    pub fn public_route(mut self, method: &str, path: &str) -> Self {
        self.public_routes.add(method, path);
        self
    }

    /// Configures tracing/logging for the application.
    pub fn with_tracing(self, config: TracingConfig) -> Self {
        config.init();
        self
    }

    /// Enables or disables the introspection endpoint.
    ///
    /// When enabled, a `GET /.__rapina/routes` endpoint is registered
    /// that returns all routes as JSON.
    ///
    /// Introspection is enabled by default in debug builds.
    pub fn with_introspection(mut self, enabled: bool) -> Self {
        self.introspection = enabled;
        self
    }

    /// Enables or disables the metrics endpoint.
    ///
    /// When enabled, a `GET /metrics` endpoint is registered
    /// that returns all metrics to Prometheus.
    ///
    /// Metrics is disabled by default unless you call `with_metrics(true)`.
    pub fn with_metrics(mut self, enabled: bool) -> Self {
        self.metrics = enabled;
        self
    }

    /// Enables or disables openapi endpoint
    ///
    /// When enabled, a get `/__rapina/openapi.json` endpoint is registered
    /// that returns all routes as OpenAPI specification
    /// OpenAPI is disabled by default
    pub fn openapi(mut self, title: impl Into<String>, version: impl Into<String>) -> Self {
        self.openapi = true;
        self.openapi_title = title.into();
        self.openapi_version = version.into();
        self
    }

    /// Configures database connection with the given configuration.
    ///
    /// This method connects to the database and registers the connection
    /// in the application state. Use the [`Db`](crate::database::Db) extractor
    /// in your handlers to access the connection.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use rapina::prelude::*;
    /// use rapina::database::{DatabaseConfig, Db};
    ///
    /// #[get("/users")]
    /// async fn list_users(db: Db) -> Result<Json<Vec<User>>> {
    ///     let users = UserEntity::find().all(db.conn()).await?;
    ///     Ok(Json(users))
    /// }
    ///
    /// #[tokio::main]
    /// async fn main() -> std::io::Result<()> {
    ///     let db_config = DatabaseConfig::from_env()?;
    ///
    ///     Rapina::new()
    ///         .with_database(db_config).await?
    ///         .router(router)
    ///         .listen("127.0.0.1:3000")
    ///         .await
    /// }
    /// ```
    #[cfg(feature = "database")]
    pub async fn with_database(
        mut self,
        config: crate::database::DatabaseConfig,
    ) -> Result<Self, std::io::Error> {
        let conn = config
            .connect()
            .await
            .map_err(|e| std::io::Error::other(format!("Database connection failed: {}", e)))?;
        self.state = self.state.with(conn);
        Ok(self)
    }

    /// Runs all pending database migrations at startup.
    ///
    /// Call this after `with_database()` to apply migrations before serving requests.
    ///
    /// # Example
    ///
    /// ```ignore
    /// mod migrations;
    ///
    /// Rapina::new()
    ///     .with_database(DatabaseConfig::from_env()?).await?
    ///     .run_migrations::<migrations::Migrator>().await?
    ///     .router(router)
    ///     .listen("127.0.0.1:3000")
    ///     .await
    /// ```
    #[cfg(feature = "database")]
    pub async fn run_migrations<M: crate::migration::MigratorTrait>(
        self,
    ) -> Result<Self, std::io::Error> {
        let conn = self
            .state
            .get::<sea_orm::DatabaseConnection>()
            .ok_or_else(|| {
                std::io::Error::other(
                    "Database not configured. Call .with_database() before
  .run_migrations()",
                )
            })?;

        crate::migration::run_pending::<M>(conn)
            .await
            .map_err(|e| std::io::Error::other(format!("Migration failed: {}", e)))?;

        Ok(self)
    }

    /// Applies all deferred setup (auth middleware, introspection, metrics, openapi).
    ///
    /// Both [`listen`](Self::listen) and [`TestClient::new`](crate::testing::TestClient::new)
    /// call this so the app behaves identically in tests and production.
    pub(crate) fn prepare(mut self) -> Self {
        // Add auth middleware if configured
        if let Some(auth_config) = self.auth_config.take() {
            let auth_middleware =
                AuthMiddleware::with_public_routes(auth_config, self.public_routes.clone());
            self.middlewares.add(auth_middleware);
        }

        if self.introspection {
            let routes = self.router.routes();
            self.state = self.state.with(RouteRegistry::with_routes(routes));
            self.router = self
                .router
                .get_named("/__rapina/routes", "list_routes", list_routes);
        }

        #[cfg(feature = "metrics")]
        if self.metrics {
            let registry = MetricsRegistry::new();
            self.state = self.state.with(registry.clone());
            self.middlewares.add(MetricsMiddleware::new(registry));
            self.router = self
                .router
                .get_named("/metrics", "metrics", metrics_handler);
        }

        if self.openapi {
            let routes = self.router.routes();
            let spec = build_openapi_spec(&self.openapi_title, &self.openapi_version, &routes);
            self.state = self.state.with(OpenApiRegistry::new(spec));
            self.router =
                self.router
                    .get_named("/__rapina/openapi.json", "openapi_spec", openapi_spec);
        }

        self
    }

    /// Starts the HTTP server on the given address.
    ///
    /// # Panics
    ///
    /// Panics if the address cannot be parsed.
    pub async fn listen(self, addr: &str) -> std::io::Result<()> {
        let addr: SocketAddr = addr.parse().expect("invalid address");
        let app = self.prepare();
        serve(app.router, app.state, app.middlewares, addr).await
    }
}

impl Default for Rapina {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::TimeoutMiddleware;
    use http::StatusCode;
    use std::time::Duration;

    #[test]
    fn test_rapina_new() {
        let app = Rapina::new();
        assert!(app.middlewares.is_empty());
    }

    #[test]
    fn test_rapina_default() {
        let app = Rapina::default();
        assert!(app.middlewares.is_empty());
    }

    #[test]
    fn test_rapina_with_router() {
        let router = Router::new().route(
            http::Method::GET,
            "/health",
            |_req, _params, _state| async { StatusCode::OK },
        );

        let app = Rapina::new().router(router);
        assert!(!app.router.routes.is_empty());
    }

    #[test]
    fn test_rapina_with_state() {
        #[derive(Clone)]
        struct Config {
            name: String,
        }

        let app = Rapina::new().state(Config {
            name: "test".to_string(),
        });

        let config = app.state.get::<Config>().unwrap();
        assert_eq!(config.name, "test");
    }

    #[test]
    fn test_rapina_with_middleware() {
        let app = Rapina::new().middleware(TimeoutMiddleware::new(Duration::from_secs(10)));

        assert!(!app.middlewares.is_empty());
    }

    #[test]
    fn test_rapina_chaining() {
        #[allow(dead_code)]
        #[derive(Clone)]
        struct Config {
            name: String,
        }

        let router = Router::new()
            .route(http::Method::GET, "/", |_req, _params, _state| async {
                StatusCode::OK
            })
            .route(
                http::Method::POST,
                "/users",
                |_req, _params, _state| async { StatusCode::CREATED },
            );

        let app = Rapina::new()
            .router(router)
            .state(Config {
                name: "app".to_string(),
            })
            .middleware(TimeoutMiddleware::default());

        assert!(!app.router.routes.is_empty());
        assert!(app.state.get::<Config>().is_some());
        assert!(!app.middlewares.is_empty());
    }

    #[test]
    fn test_rapina_multiple_states() {
        #[allow(dead_code)]
        #[derive(Clone)]
        struct Config {
            name: String,
        }

        #[allow(dead_code)]
        #[derive(Clone)]
        struct DbPool {
            url: String,
        }

        let app = Rapina::new()
            .state(Config {
                name: "app".to_string(),
            })
            .state(DbPool {
                url: "postgres://localhost".to_string(),
            });

        assert!(app.state.get::<Config>().is_some());
        assert!(app.state.get::<DbPool>().is_some());
    }

    #[test]
    fn test_rapina_multiple_middlewares() {
        use crate::middleware::{BodyLimitMiddleware, TraceIdMiddleware};

        let app = Rapina::new()
            .middleware(TraceIdMiddleware::new())
            .middleware(TimeoutMiddleware::default())
            .middleware(BodyLimitMiddleware::default());

        // MiddlewareStack doesn't expose count, but we can verify it's not empty
        assert!(!app.middlewares.is_empty());
    }

    #[test]
    fn test_rapina_introspection_enabled_in_debug() {
        let app = Rapina::new();
        // In debug builds, introspection should be enabled
        assert_eq!(app.introspection, cfg!(debug_assertions));
    }

    #[test]
    fn test_rapina_with_introspection_enabled() {
        let app = Rapina::new().with_introspection(true);
        assert!(app.introspection);
    }

    #[test]
    fn test_rapina_with_introspection_disabled() {
        let app = Rapina::new().with_introspection(false);
        assert!(!app.introspection);
    }

    #[test]
    fn test_rapina_with_metrics_enabled() {
        let app = Rapina::new().with_metrics(true);
        assert!(app.metrics);
    }

    #[test]
    fn test_rapina_with_metrics_disabled() {
        let app = Rapina::new().with_metrics(false);
        assert!(!app.metrics);
    }
}
