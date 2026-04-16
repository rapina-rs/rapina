//! Tower compatibility adapters for Rapina middleware.
//!
//! Provides two adapters for interop between Rapina's middleware system
//! and the Tower ecosystem:
//!
//! - [`TowerLayerMiddleware`] wraps a `tower::Layer` for use as a Rapina
//!   [`Middleware`](super::Middleware).
//! - [`RapinaService`] wraps Rapina's middleware + router stack as a
//!   `tower::Service`.

use std::convert::Infallible;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};

use hyper::body::Incoming;
use hyper::{Request, Response};
use tower_layer::Layer;
use tower_service::Service;

use crate::context::RequestContext;
use crate::middleware::{BoxFuture, Middleware, MiddlewareStack, Next};
use crate::response::{BoxBody, IntoResponse};
use crate::router::Router;
use crate::state::AppState;

// ─── Direction A: Tower Layer → Rapina Middleware ────────────────────────────

/// A [`tower::Service`] that delegates to the Rapina middleware chain.
///
/// Owns all data needed to execute the chain, with no lifetime parameter.
/// This makes it compatible with any Tower layer — including those requiring
/// `Clone` (e.g. tower-resilience, retry, circuit breaker).
#[derive(Clone)]
pub struct NextService {
    middlewares: Arc<[Arc<dyn Middleware>]>,
    router: Arc<Router>,
    state: Arc<AppState>,
}

impl Service<Request<Incoming>> for NextService {
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response<BoxBody>, Infallible>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Incoming>) -> Self::Future {
        let middlewares = self.middlewares.clone();
        let router = self.router.clone();
        let state = self.state.clone();

        Box::pin(async move {
            let ctx = req
                .extensions()
                .get::<RequestContext>()
                .cloned()
                .unwrap_or_default();
            let next = Next::new(&middlewares, router, state, &ctx);
            Ok(next.run(req).await)
        })
    }
}

/// Wraps a [`tower::Layer`] so it can be added to a Rapina middleware stack.
///
/// The tower service is built once on the first request and cached. Each
/// subsequent request clones the cached service — since tower layers store
/// shared state behind `Arc` internally, all requests share the same state.
/// This means stateful layers like rate limiters, circuit breakers, and
/// bulkheads work correctly across concurrent requests.
///
/// # Body type
///
/// Tower layers that preserve the response body type (`Response<BoxBody>`)
/// work directly. Layers that change the body type (e.g. tower-http
/// compression) are not compatible without an additional body adapter.
///
/// # Example
///
/// ```ignore
/// use rapina::middleware::TowerLayerMiddleware;
///
/// // Any tower Layer that accepts NextService works:
/// Rapina::new()
///     .middleware(TowerLayerMiddleware::new(my_tower_layer))
///     .listen("127.0.0.1:3000")
///     .await
/// ```
pub struct TowerLayerMiddleware<L: Layer<NextService>> {
    layer: L,
    service: OnceLock<L::Service>,
}

impl<L: Layer<NextService>> TowerLayerMiddleware<L> {
    pub fn new(layer: L) -> Self {
        Self {
            layer,
            service: OnceLock::new(),
        }
    }
}

impl<L> fmt::Debug for TowerLayerMiddleware<L>
where
    L: Layer<NextService> + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TowerLayerMiddleware")
            .field("layer", &self.layer)
            .finish()
    }
}

impl<L> Middleware for TowerLayerMiddleware<L>
where
    L: Layer<NextService> + Send + Sync + 'static,
    <L as Layer<NextService>>::Service:
        Service<Request<Incoming>, Response = Response<BoxBody>> + Clone + Send + Sync + 'static,
    <<L as Layer<NextService>>::Service as Service<Request<Incoming>>>::Error:
        fmt::Display + Send + 'static,
    <<L as Layer<NextService>>::Service as Service<Request<Incoming>>>::Future: Send + 'static,
{
    fn handle<'a>(
        &'a self,
        mut req: Request<Incoming>,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        Box::pin(async move {
            req.extensions_mut().insert(ctx.clone());
            let template = self.service.get_or_init(|| {
                let next_svc = NextService {
                    middlewares: next.middlewares.iter().cloned().collect(),
                    router: next.router.clone(),
                    state: next.state.clone(),
                };
                self.layer.layer(next_svc)
            });
            let mut svc = template.clone();

            if let Err(e) = std::future::poll_fn(|cx| svc.poll_ready(cx)).await {
                tracing::error!("tower service not ready: {}", e);
                return crate::error::Error::service_unavailable("service unavailable")
                    .into_response();
            }

            match svc.call(req).await {
                Ok(response) => response,
                Err(e) => {
                    tracing::error!("tower service error: {}", e);
                    crate::error::Error::internal("internal server error").into_response()
                }
            }
        })
    }
}

// ─── Direction B: Rapina Stack → Tower Service ──────────────────────────────

/// Wraps Rapina's middleware + router stack as a [`tower::Service`].
///
/// This allows embedding a fully-configured Rapina application inside
/// tower-based infrastructure or using tower testing utilities.
///
/// The service is always ready (`poll_ready` returns `Poll::Ready(Ok(()))`)
/// and never errors (`Error = Infallible`), matching Rapina's infallible
/// response model.
///
/// # Example
///
/// ```ignore
/// use rapina::middleware::RapinaService;
///
/// let service = RapinaService::new(router, state, middlewares);
/// // `service` implements tower::Service<Request<Incoming>>
/// ```
#[derive(Clone)]
pub struct RapinaService {
    router: Arc<Router>,
    state: Arc<AppState>,
    middlewares: Arc<MiddlewareStack>,
}

impl RapinaService {
    pub fn new(mut router: Router, state: AppState, middlewares: MiddlewareStack) -> Self {
        router.freeze();
        Self {
            router: Arc::new(router),
            state: Arc::new(state),
            middlewares: Arc::new(middlewares),
        }
    }
}

impl Service<Request<Incoming>> for RapinaService {
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response<BoxBody>, Infallible>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: Request<Incoming>) -> Self::Future {
        let router = self.router.clone();
        let state = self.state.clone();
        let middlewares = self.middlewares.clone();

        Box::pin(async move {
            let ctx = match req.extensions().get::<RequestContext>() {
                Some(existing) => existing.clone(),
                None => {
                    let new_ctx = RequestContext::default();
                    req.extensions_mut().insert(new_ctx.clone());
                    new_ctx
                }
            };

            let response = middlewares.execute(req, router, state, &ctx).await;
            Ok(response)
        })
    }
}

impl fmt::Debug for RapinaService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RapinaService").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::MiddlewareStack;
    use crate::router::Router;
    use crate::state::AppState;

    #[test]
    fn test_rapina_service_is_clone() {
        let svc = RapinaService::new(Router::new(), AppState::new(), MiddlewareStack::new());
        let _clone = svc.clone();
    }

    #[test]
    fn test_rapina_service_debug() {
        let svc = RapinaService::new(Router::new(), AppState::new(), MiddlewareStack::new());
        let debug = format!("{:?}", svc);
        assert!(debug.contains("RapinaService"));
    }

    #[tokio::test]
    async fn test_rapina_service_poll_ready() {
        let mut svc = RapinaService::new(Router::new(), AppState::new(), MiddlewareStack::new());
        let ready = std::future::poll_fn(|cx| svc.poll_ready(cx)).await;
        assert!(ready.is_ok());
    }

    #[test]
    fn test_tower_layer_middleware_debug() {
        let mw = TowerLayerMiddleware::new(tower_layer::Identity::new());
        let debug = format!("{:?}", mw);
        assert!(debug.contains("TowerLayerMiddleware"));
    }

    #[tokio::test]
    async fn test_identity_layer_passes_through() {
        use crate::app::Rapina;
        use crate::testing::TestClient;

        let app = Rapina::new()
            .with_introspection(false)
            .middleware(TowerLayerMiddleware::new(tower_layer::Identity::new()))
            .router(Router::new().route(http::Method::GET, "/", |_, _, _| async { "hello tower" }));

        let client = TestClient::new(app).await;
        let response = client.get("/").send().await;

        assert_eq!(response.status(), http::StatusCode::OK);
        assert_eq!(response.text(), "hello tower");
    }

    #[tokio::test]
    async fn test_concurrency_limit_shared_state() {
        use crate::app::Rapina;
        use crate::testing::TestClient;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tower::limit::ConcurrencyLimitLayer;

        let max_concurrent = Arc::new(AtomicUsize::new(0));
        let current = Arc::new(AtomicUsize::new(0));
        let max_c = max_concurrent.clone();
        let cur = current.clone();

        let app = Rapina::new()
            .with_introspection(false)
            .layer(ConcurrencyLimitLayer::new(1))
            .router(
                Router::new().route(http::Method::GET, "/slow", move |_, _, _| {
                    let max_c = max_c.clone();
                    let cur = cur.clone();
                    async move {
                        let c = cur.fetch_add(1, Ordering::SeqCst) + 1;
                        max_c.fetch_max(c, Ordering::SeqCst);
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        cur.fetch_sub(1, Ordering::SeqCst);
                        "done"
                    }
                }),
            );

        let client = TestClient::new(app).await;

        let (r1, r2) = tokio::join!(client.get("/slow").send(), client.get("/slow").send());

        assert_eq!(r1.status(), http::StatusCode::OK);
        assert_eq!(r2.status(), http::StatusCode::OK);
        assert_eq!(max_concurrent.load(Ordering::SeqCst), 1);
    }
}
