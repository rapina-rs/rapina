use crate::route::RouteBuilder;
use http::Method;
use rapina::handler::Handler;
use rapina::prelude::*;

/// The main application structure for the Rapina DX API.
/// 
/// Provides a fluent interface for configuring routes and starting the server.
pub struct App {
    pub(crate) inner: Rapina,
    pub(crate) prefix: String,
}

/// Creates a new Rapina DX application.
pub fn app() -> App {
    App::new()
}

impl App {
    /// Creates a new Rapina DX application.
    pub fn new() -> Self {
        Self {
            inner: Rapina::new(),
            prefix: String::new(),
        }
    }

    /// Registers a GET route handler at the given path.
    pub fn get<H: Handler>(self, path: impl Into<String>, handler: H) -> RouteBuilder {
        let full_path = self.join_path(path.into());
        RouteBuilder::new(self, Method::GET, full_path, handler)
    }

    /// Registers a POST route handler at the given path.
    pub fn post<H: Handler>(self, path: impl Into<String>, handler: H) -> RouteBuilder {
        let full_path = self.join_path(path.into());
        RouteBuilder::new(self, Method::POST, full_path, handler)
    }

    /// Registers a PUT route handler at the given path.
    pub fn put<H: Handler>(self, path: impl Into<String>, handler: H) -> RouteBuilder {
        let full_path = self.join_path(path.into());
        RouteBuilder::new(self, Method::PUT, full_path, handler)
    }

    /// Registers a DELETE route handler at the given path.
    pub fn delete<H: Handler>(self, path: impl Into<String>, handler: H) -> RouteBuilder {
        let full_path = self.join_path(path.into());
        RouteBuilder::new(self, Method::DELETE, full_path, handler)
    }

    /// Adds a middleware to the application.
    pub fn middleware(mut self, middleware: impl Middleware) -> Self {
        self.inner.middlewares.add(middleware);
        self
    }

    /// Groups routes under a common path prefix.
    /// 
    /// The callback receives a new `App` instance configured with the prefix.
    pub fn group<F, R>(mut self, prefix: &str, callback: F) -> App
    where
        F: FnOnce(App) -> R,
        R: IntoApp,
    {
        let original_prefix = self.prefix.clone();
        self.prefix = self.join_path(prefix.to_string());
        let res = callback(self);
        let mut app = res.into_app();
        app.prefix = original_prefix;
        app
    }

    /// Starts the HTTP server on the given address or port.
    /// 
    /// Supports `u16` (port), `&str`, or `String` as the address.
    pub async fn listen(self, addr: impl IntoAddr) -> std::io::Result<()> {
        self.inner.listen(&addr.into_addr()).await
    }

    fn join_path(&self, path: String) -> String {
        let prefix = self.prefix.trim_end_matches('/');
        let path = path.trim_start_matches('/');

        if prefix.is_empty() {
            if path.is_empty() {
                "/".to_string()
            } else {
                format!("/{}", path)
            }
        } else if path.is_empty() {
            prefix.to_string()
        } else {
            format!("{}/{}", prefix, path)
        }
    }
}

/// Trait for types that can be converted into an `App`.
/// 
/// Used to allow `RouteBuilder` to be used in `.group()` callbacks.
pub trait IntoApp {
    fn into_app(self) -> App;
}

impl IntoApp for App {
    fn into_app(self) -> App {
        self
    }
}

/// Trait for types that can be converted into a server address string.
pub trait IntoAddr {
    fn into_addr(self) -> String;
}

impl IntoAddr for u16 {
    fn into_addr(self) -> String {
        format!("127.0.0.1:{}", self)
    }
}

impl IntoAddr for &str {
    fn into_addr(self) -> String {
        self.to_string()
    }
}

impl IntoAddr for String {
    fn into_addr(self) -> String {
        self
    }
}
