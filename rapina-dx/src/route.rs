use http::Method;
use rapina::prelude::*;
use rapina::handler::Handler;
use crate::app::{App, IntoAddr, IntoApp};

/// A builder for configuring a route and its metadata.
pub struct RouteBuilder {
    app: App,
    method: Method,
    path: String,
    handler_reg: Box<dyn FnOnce(Rapina) -> Rapina>,
    is_public: bool,
    tags: Vec<String>,
    description: Option<String>,
}

impl RouteBuilder {
    pub(crate) fn new<H: Handler>(app: App, method: Method, path: String, handler: H) -> Self {
        let h_clone = handler.clone();
        let path_clone = path.clone();
        let name = H::NAME;
        let method_inner = method.clone();

        let register = Box::new(move |mut r: Rapina| {
            r.router = r.router.route_named(
                method_inner,
                &path_clone,
                name,
                H::response_schema(),
                H::error_responses(),
                move |req, params, state| h_clone.call(req, params, state)
            );
            r
        });

        Self {
            app,
            method,
            path,
            handler_reg: register,
            is_public: false,
            tags: Vec::new(),
            description: None,
        }
    }

    /// Marks the route as public (no authentication required).
    pub fn public(mut self) -> Self {
        self.is_public = true;
        self
    }

    /// Adds a tag to the route for OpenAPI documentation.
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Sets the description for the route in OpenAPI documentation.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Finalizes the route registration and returns the `App` instance.
    pub fn finish(self) -> App {
        let is_public = self.is_public;
        let path = self.path.clone();
        let method = self.method.clone();
        let tags = self.tags.clone();
        let description = self.description.clone();
        
        let mut app = self.app;
        app.inner = (self.handler_reg)(app.inner);
        
        // Find the newly added route and update its metadata
        if let Some((_, route)) = app.inner.router.routes.last_mut() {
            route.tags = tags;
            route.description = description;
        }
        
        if is_public {
            app.inner.public_routes.add(method.as_str(), &path);
        }
        
        app
    }

    /// Registers a GET route and continues the chain.
    pub fn get<H: Handler>(self, path: impl Into<String>, handler: H) -> RouteBuilder {
        self.finish().get(path, handler)
    }

    /// Registers a POST route and continues the chain.
    pub fn post<H: Handler>(self, path: impl Into<String>, handler: H) -> RouteBuilder {
        self.finish().post(path, handler)
    }

    /// Registers a PUT route and continues the chain.
    pub fn put<H: Handler>(self, path: impl Into<String>, handler: H) -> RouteBuilder {
        self.finish().put(path, handler)
    }

    /// Registers a DELETE route and continues the chain.
    pub fn delete<H: Handler>(self, path: impl Into<String>, handler: H) -> RouteBuilder {
        self.finish().delete(path, handler)
    }

    /// Adds a middleware and returns the `App` instance.
    pub fn middleware(self, m: impl Middleware) -> App {
        self.finish().middleware(m)
    }

    /// Groups routes under a common path prefix and returns the `App` instance.
    pub fn group<F, R>(self, prefix: &str, callback: F) -> App
    where
        F: FnOnce(App) -> R,
        R: IntoApp,
    {
        self.finish().group(prefix, callback)
    }

    /// Starts the HTTP server on the given address or port.
    pub async fn listen(self, addr: impl IntoAddr) -> std::io::Result<()> {
        self.finish().listen(addr).await
    }
}

impl IntoApp for RouteBuilder {
    fn into_app(self) -> App {
        self.finish()
    }
}
