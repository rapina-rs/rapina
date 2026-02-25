use std::net::SocketAddr;
use std::sync::Arc;

use hyper::Request;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

use crate::context::RequestContext;
use crate::middleware::MiddlewareStack;
use crate::router::Router;
use crate::state::AppState;

pub async fn serve(
    router: Router,
    state: AppState,
    middlewares: MiddlewareStack,
    addr: SocketAddr,
) -> std::io::Result<()> {
    let router = Arc::new(router);
    let state = Arc::new(state);
    let middlewares = Arc::new(middlewares);
    let listener = TcpListener::bind(addr).await?;

    tracing::info!("Rapina listening on http://{}", addr);

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let router = router.clone();
        let state = state.clone();
        let middlewares = middlewares.clone();

        tokio::spawn(async move {
            let service = service_fn(move |mut req: Request<Incoming>| {
                let router = router.clone();
                let state = state.clone();
                let middlewares = middlewares.clone();

                // Create and inject RequestContext at request start
                let ctx = RequestContext::new();
                req.extensions_mut().insert(ctx.clone());

                async move {
                    let response = middlewares.execute(req, &router, &state, &ctx).await;
                    Ok::<_, std::convert::Infallible>(response)
                }
            });

            if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                tracing::error!("connection error: {}", e);
            }
        });
    }
}
