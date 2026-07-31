use crate::error::{Error, Result};
use crate::types::{ToxiRequest, ToxiResponse};
use hyper::Method;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower_service::Service;

use regex::Regex;

/// Trait for type-erased handlers stored in the router
pub trait Endpoint: Send + Sync + 'static {
    fn call(&self, req: ToxiRequest) -> Pin<Box<dyn Future<Output = Result<ToxiResponse>> + Send>>;
}

impl Endpoint for Arc<dyn Endpoint> {
    fn call(&self, req: ToxiRequest) -> Pin<Box<dyn Future<Output = Result<ToxiResponse>> + Send>> {
        (**self).call(req)
    }
}

/// A `tower::Service` wrapper around a type-erased [`Endpoint`].
///
/// Bridges the `Endpoint` trait with tower's `Service` trait so that
/// handlers can be used with tower middleware and hyper's server stack.
pub struct EndpointService(pub Arc<dyn Endpoint>);

impl Clone for EndpointService {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Service<ToxiRequest> for EndpointService {
    type Response = ToxiResponse;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<ToxiResponse>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: ToxiRequest) -> Self::Future {
        self.0.call(req)
    }
}

impl Endpoint for EndpointService {
    fn call(&self, req: ToxiRequest) -> Pin<Box<dyn Future<Output = Result<ToxiResponse>> + Send>> {
        self.0.call(req)
    }
}

/// CORS configuration for the Router
#[derive(Clone, Debug)]
pub struct CorsConfig {
    /// Allowed origins (e.g., "http://localhost:3000")
    /// Use "*" to allow all origins
    pub allowed_origins: Vec<String>,
    /// Allowed HTTP methods (e.g., "GET", "POST", "PUT", "DELETE")
    /// Empty means allow all methods
    pub allowed_methods: Vec<String>,
    /// Allowed HTTP headers (e.g., "Content-Type", "Authorization")
    /// Empty means allow all headers
    pub allowed_headers: Vec<String>,
    /// Whether to allow credentials (cookies, authorization headers)
    pub allow_credentials: bool,
    /// Max age for CORS preflight cache (in seconds)
    pub max_age: u32,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec!["GET".to_string(), "POST".to_string(), "PUT".to_string(), "DELETE".to_string(), "OPTIONS".to_string(), "PATCH".to_string()],
            allowed_headers: vec!["*".to_string()],
            allow_credentials: false,
            max_age: 3600,
        }
    }
}

impl CorsConfig {
    /// Create a new CORS config that allows everything (useful for development)
    pub fn permissive() -> Self {
        Self::default()
    }

    /// Create a new CORS config with no allowed origins (restrictive default)
    pub fn restrictive() -> Self {
        Self {
            allowed_origins: Vec::new(),
            allowed_methods: vec!["GET".to_string(), "POST".to_string()],
            allowed_headers: vec!["Content-Type".to_string()],
            allow_credentials: false,
            max_age: 3600,
        }
    }

    /// Builder: add an allowed origin.
    pub fn allow_origin(mut self, origin: &str) -> Self {
        self.allowed_origins.push(origin.to_string());
        self
    }

    /// Builder: add an allowed HTTP method.
    pub fn allow_method(mut self, method: &str) -> Self {
        self.allowed_methods.push(method.to_string());
        self
    }

    /// Builder: add an allowed HTTP header.
    pub fn allow_header(mut self, header: &str) -> Self {
        self.allowed_headers.push(header.to_string());
        self
    }

    /// Builder: enable credentials (cookies, authorization headers).
    pub fn credentials(mut self) -> Self {
        self.allow_credentials = true;
        self
    }

    /// Builder: set the max age for CORS preflight cache (in seconds).
    pub fn max_age(mut self, seconds: u32) -> Self {
        self.max_age = seconds;
        self
    }
}

// Note: tower-http middleware like Cors and Compression change the response body type,
// so they cannot be used as Endpoint-level middleware. They should be applied at the
// Server level instead, after body type conversion.
//
// For CORS at the router level, use custom middleware that only modifies headers
// without changing the body type.


/// Trait for async functions that can be used as handlers
pub trait Handler<Args>: Clone + Send + Sync + 'static {
    fn call(&self, req: ToxiRequest) -> Pin<Box<dyn Future<Output = Result<ToxiResponse>> + Send>>;
}

// Wrapper to convert Handler<Args> into Endpoint
struct HandlerService<H, Args> {
    handler: H,
    _marker: std::marker::PhantomData<Args>,
}

impl<H, Args> Endpoint for HandlerService<H, Args>
where
    H: Handler<Args>,
    Args: Send + Sync + 'static,
{
    fn call(&self, req: ToxiRequest) -> Pin<Box<dyn Future<Output = Result<ToxiResponse>> + Send>> {
        self.handler.call(req)
    }
}

// Handler for Fn() -> Fut (no extractors)
impl<F, Fut> Handler<()> for F
where
    F: Fn() -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ToxiResponse>> + Send + 'static,
{
    fn call(&self, _req: ToxiRequest) -> Pin<Box<dyn Future<Output = Result<ToxiResponse>> + Send>> {
        let fut = self();
        Box::pin(async move { fut.await })
    }
}

// Handler for Fn(ToxiRequest) -> Fut (raw request)
impl<F, Fut> Handler<ToxiRequest> for F
where
    F: Fn(ToxiRequest) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<ToxiResponse>> + Send + 'static,
{
    fn call(&self, req: ToxiRequest) -> Pin<Box<dyn Future<Output = Result<ToxiResponse>> + Send>> {
        let fut = self(req);
        Box::pin(async move { fut.await })
    }
}

/// Generate [`Handler`] implementations for functions with the given number
/// of extractors. The built-in set covers arities 1 through 12. Users can
/// extend beyond 12:
///
/// ```rust,ignore
/// toxi::impl_handler_for_fn!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14);
/// ```
#[macro_export]
macro_rules! impl_handler_for_fn {
    // impl_handler_for_fn!(T1, T2, T3) — user-supplied type idents
    ($($T:ident),+) => {
        #[allow(non_snake_case)]
        impl<F, Fut, $($T,)*> Handler<($($T,)*)> for F
        where
            F: Fn($($T,)*) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Result<ToxiResponse>> + Send + 'static,
            $($T: $crate::extract::FromRequest + Send + 'static,)*
        {
            fn call(&self, mut req: ToxiRequest) -> Pin<Box<dyn Future<Output = Result<ToxiResponse>> + Send>> {
                let handler = self.clone();
                Box::pin(async move {
                    $(let $T = $T::from_request(&mut req).await?;)*
                    handler($($T,)*).await
                })
            }
        }
    };
}

// Built-in handler arities 1 through 12.
// To add more: impl_handler_for_fn!(T1, T2, ..., T13, T14);
impl_handler_for_fn!(T1);
impl_handler_for_fn!(T1, T2);
impl_handler_for_fn!(T1, T2, T3);
impl_handler_for_fn!(T1, T2, T3, T4);
impl_handler_for_fn!(T1, T2, T3, T4, T5);
impl_handler_for_fn!(T1, T2, T3, T4, T5, T6);
impl_handler_for_fn!(T1, T2, T3, T4, T5, T6, T7);
impl_handler_for_fn!(T1, T2, T3, T4, T5, T6, T7, T8);
impl_handler_for_fn!(T1, T2, T3, T4, T5, T6, T7, T8, T9);
impl_handler_for_fn!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10);
impl_handler_for_fn!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11);
impl_handler_for_fn!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);

struct Route {
    pattern: Regex,
    param_names: Vec<String>,
    handler: Arc<dyn Endpoint>,
}

/// The central request router.
///
/// Maintains a map of HTTP methods to registered route patterns and their
/// associated handler endpoints. Supports path parameters (`/users/:id`),
/// wildcard segments, middleware layers, and CORS configuration.
///
/// Implements `tower::Service` so it can be used directly with hyper's
/// server or wrapped with tower middleware.
#[derive(Clone)]
pub struct Router {
    routes: HashMap<Method, Vec<Arc<Route>>>,
    extensions: Arc<std::sync::RwLock<http::Extensions>>,
    middleware: Vec<Arc<dyn Fn(Arc<dyn Endpoint>) -> Arc<dyn Endpoint> + Send + Sync>>,
    cors_config: Option<CorsConfig>,
}

impl Router {
    /// Create an empty router with no routes, middleware, or CORS configuration.
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
            extensions: Arc::new(std::sync::RwLock::new(http::Extensions::new())),
            middleware: Vec::new(),
            cors_config: None,
        }
    }

    /// Add a shared state to the router that will be available in all handlers
    pub fn with_state<T: Clone + Send + Sync + 'static>(&mut self, state: T) {
        if let Ok(mut exts) = self.extensions.write() {
            exts.insert(state);
        }
    }

    /// Register a handler for `GET` requests at the given path.
    pub fn get<H, Args>(&mut self, path: &str, handler: H)
    where
        H: Handler<Args>,
        Args: Send + Sync + 'static,
    {
        self.add_route(Method::GET, path, handler);
    }
    
    /// Register a handler for `POST` requests at the given path.
    pub fn post<H, Args>(&mut self, path: &str, handler: H)
    where
        H: Handler<Args>,
        Args: Send + Sync + 'static,
    {
        self.add_route(Method::POST, path, handler);
    }

    /// Register a handler for `PUT` requests at the given path.
    pub fn put<H, Args>(&mut self, path: &str, handler: H)
    where
        H: Handler<Args>,
        Args: Send + Sync + 'static,
    {
        self.add_route(Method::PUT, path, handler);
    }

    /// Register a handler for `DELETE` requests at the given path.
    pub fn delete<H, Args>(&mut self, path: &str, handler: H)
    where
        H: Handler<Args>,
        Args: Send + Sync + 'static,
    {
        self.add_route(Method::DELETE, path, handler);
    }

    /// Register a handler for `PATCH` requests at the given path.
    pub fn patch<H, Args>(&mut self, path: &str, handler: H)
    where
        H: Handler<Args>,
        Args: Send + Sync + 'static,
    {
        self.add_route(Method::PATCH, path, handler);
    }

    /// Add a middleware layer to all routes in the router.
    ///
    /// The layer must implement `tower::Layer<EndpointService>` and return a new `Endpoint`.
    ///
    /// # Limitations
    ///
    /// Body-type-changing middleware (like `CorsLayer` and `CompressionLayer` from tower-http)
    /// **cannot** be used with this method. These middleware change the HTTP response body type,
    /// which is incompatible with the `Endpoint` trait that expects `ToxiResponse`.
    ///
    /// For such middleware, use `ServiceBuilder` instead:
    /// ```rust,ignore
    /// let service = ServiceBuilder::new()
    ///     .layer(CorsLayer::permissive())
    ///     .layer(CompressionLayer::new())
    ///     .service(router);
    /// ```
    pub fn layer<L>(mut self, layer: L) -> Self
    where
        L: tower::Layer<EndpointService> + Send + Sync + 'static,
        L::Service: Endpoint,
    {
        let layer = Arc::new(layer);
        self.middleware.push(Arc::new(move |endpoint| {
            Arc::new(layer.layer(EndpointService(endpoint)))
        }));
        self
    }

    /// Alias for `.layer()`.
    pub fn with_layer<L>(self, layer: L) -> Self
    where
        L: tower::Layer<EndpointService> + Send + Sync + 'static,
        L::Service: Endpoint,
    {
        self.layer(layer)
    }

    /// Configure CORS for this router.
    ///
    /// This adds CORS headers to all responses, including preflight OPTIONS requests.
    /// This is a framework-level CORS implementation that doesn't require tower-http.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use toxi::prelude::*;
    ///
    /// let router = Router::new()
    ///     .with_cors(CorsConfig {
    ///         allowed_origins: vec!["http://localhost:3000".to_string()],
    ///         allowed_methods: vec!["GET".to_string(), "POST".to_string()],
    ///         allowed_headers: vec!["Content-Type".to_string(), "Authorization".to_string()],
    ///         allow_credentials: true,
    ///         max_age: 3600,
    ///     });
    /// ```
    ///
    /// For development, you can use the permissive config:
    ///
    /// ```rust,ignore
    /// let router = Router::new()
    ///     .with_cors(CorsConfig::permissive());
    /// ```
    pub fn with_cors(mut self, config: CorsConfig) -> Self {
        self.cors_config = Some(config);
        self
    }

    /// Internal: compile the path pattern, wrap the handler, apply router-level
    /// middleware, and store the resulting route.
    fn add_route<H, Args>(&mut self, method: Method, path: &str, handler: H)
    where
        H: Handler<Args>,
        Args: Send + Sync + 'static,
    {
        let (pattern, param_names) = compile_path(path);
        let mut endpoint: Arc<dyn Endpoint> = Arc::new(HandlerService {
            handler,
            _marker: std::marker::PhantomData,
        });

        // Apply router-level middleware in reverse order (so the first added is the outermost)
        for mw in self.middleware.iter().rev() {
            endpoint = mw(endpoint);
        }
        
        let route = Arc::new(Route {
            pattern,
            param_names,
            handler: endpoint,
        });
        
        self.routes
            .entry(method)
            .or_insert_with(Vec::new)
            .push(route);
    }

    /// Match the incoming request against registered routes and dispatch
    /// to the corresponding handler.
    ///
    /// Handles route matching with path parameter extraction, CORS preflight
    /// (OPTIONS), HEAD-to-GET fallback, and produces `405 Method Not Allowed`
    /// or `404 Not Found` errors as appropriate.
    pub async fn handle(&self, mut req: ToxiRequest) -> Result<ToxiResponse> {
        req.extensions_mut().insert(self.extensions.clone());
        let method = req.method().clone();
        let path = req.uri().path().to_string();

        // Helper to try matching routes for a specific method
        let try_match = |target_method: &Method, req: &mut ToxiRequest| -> Option<Arc<Route>> {
            if let Some(routes) = self.routes.get(target_method) {
                for route in routes {
                    if let Some(captures) = route.pattern.captures(&path) {
                        // Extract path parameters
                        let mut params = serde_json::Map::new();
                        for (i, name) in route.param_names.iter().enumerate() {
                            if let Some(value) = captures.get(i + 1) {
                                let raw = value.as_str();
                                let val = if let Ok(n) = raw.parse::<i64>() {
                                    serde_json::Value::Number(n.into())
                                } else if let Ok(n) = raw.parse::<f64>() {
                                    serde_json::Number::from_f64(n)
                                        .map(serde_json::Value::Number)
                                        .unwrap_or(serde_json::Value::String(raw.to_string()))
                                } else {
                                    serde_json::Value::String(raw.to_string())
                                };
                                params.insert(name.clone(), val);
                            }
                        }

                        // Store params in request extensions
                        if !params.is_empty() {
                            req.extensions_mut().insert(crate::extract::PathParams(
                                serde_json::Value::Object(params),
                            ));
                        }
                        
                        return Some(route.clone());
                    }
                }
            }
            None
        };

        // 1. Try exact method match
        if let Some(route) = try_match(&method, &mut req) {
            // Add router extensions to request so State extractor can find global state
            req.extensions_mut().insert(self.extensions.clone());
            // CORS headers are applied by BodyAdapter at the server level;
            // do NOT add them here to avoid doubling.
            return route.handler.call(req).await;
        }

        // 2. If OPTIONS, return empty success response for CORS if no explicit handler
        // CORS headers are added by BodyAdapter at the server level.
        if method == Method::OPTIONS {
            if let Some(_route) = try_match(&Method::OPTIONS, &mut req) {
                // Explicit handler exists, will be handled by step 1
            } else {
                // Return 204 No Content for CORS preflight
                return Ok(ToxiResponse::new(
                    http::Response::builder()
                        .status(http::StatusCode::NO_CONTENT)
                        .body(crate::types::BoxBody::default())
                        .unwrap(),
                ));
            }
        }

        // 3. If HEAD, try GET
        if method == Method::HEAD {
            if let Some(route) = try_match(&Method::GET, &mut req) {
                // Add router extensions to request so State extractor can find global state
                req.extensions_mut().insert(self.extensions.clone());
                // For HEAD requests, we execute the GET handler but the server/hyper 
                // will strip the body automatically since it's a HEAD response.
                return route.handler.call(req).await;
            }
        }

        // 3. Path exists for other methods => method not allowed
        let allowed_methods: Vec<String> = self
            .routes
            .iter()
            .filter(|(route_method, _)| **route_method != method)
            .filter_map(|(route_method, routes)| {
                if routes.iter().any(|route| route.pattern.is_match(&path)) {
                    Some(route_method.as_str().to_string())
                } else {
                    None
                }
            })
            .collect();
        if !allowed_methods.is_empty() {
            return Err(Error::MethodNotAllowed(format!(
                "{} {} (allowed: {})",
                method,
                path,
                allowed_methods.join(", ")
            )));
        }

        Err(Error::NotFound("Route not found".to_string()))
    }
}

impl Service<ToxiRequest> for Router {
    type Response = ToxiResponse;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: ToxiRequest) -> Self::Future {
        let router = self.clone();
        Box::pin(async move {
            router.handle(req).await
        })
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

/// Compile a route path pattern into a regex
/// Converts `/users/:id` to `^/users/([^/]+)$` and returns param names
fn compile_path(path: &str) -> (Regex, Vec<String>) {
    let mut pattern = String::from("^");
    let mut param_names = Vec::new();
    let mut chars = path.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            ':' => {
                // Extract parameter name
                let mut param_name = String::new();
                while let Some(&next_ch) = chars.peek() {
                    if next_ch.is_alphanumeric() || next_ch == '_' {
                        param_name.push(next_ch);
                        chars.next();
                    } else {
                        break;
                    }
                }
                param_names.push(param_name);
                pattern.push_str("([^/]+)");
            }
            '*' => {
                // Wildcard
                pattern.push_str("(.*)");
            }
            '.' | '+' | '?' | '^' | '$' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\' => {
                // Escape regex special characters
                pattern.push('\\');
                pattern.push(ch);
            }
            _ => {
                pattern.push(ch);
            }
        }
    }

    pattern.push('$');
    let regex = Regex::new(&pattern).expect("Invalid route pattern");
    (regex, param_names)
}

/// Trait that provides compile-time verification that a function is a valid handler.
///
/// This is used by the [`handler_fn`] function to give clear, readable error messages
/// when a function doesn't satisfy the handler requirements, rather than the cryptic
/// trait-bound errors that would otherwise surface from the router.
///
/// # Example
///
/// ```rust,ignore
/// use toxi::prelude::*;
///
/// // This compiles because the function matches Handler<(State<Arc<AppState>>,)>
/// let h = handler_fn(my_handler);
/// router.get("/users", h);
///
/// // This would fail at compile time with a clear error if the function
/// // has extractors that don't implement FromRequest.
/// ```
pub trait IntoHandler<Args>: Handler<Args> + Sized {
    fn into_handler(self) -> Self {
        self
    }
}

impl<H, Args> IntoHandler<Args> for H where H: Handler<Args> {}

/// Compile-time handler verification helper.
///
/// Wraps a handler function and ensures at compile time that all its extractors
/// implement `FromRequest`. Provides clearer error messages than raw trait bounds.
///
/// # Example
///
/// ```rust,ignore
/// use toxi::prelude::*;
///
/// async fn index(State(s): State<Arc<AppState>>) -> Result<ToxiResponse> {
///     Ok(response::json(serde_json::json!({"ok": true})))
/// }
///
/// // Verified at compile time:
/// router.get("/", handler_fn(index));
/// ```
pub fn handler_fn<H, Args>(handler: H) -> H
where
    H: IntoHandler<Args>,
    Args: Send + Sync + 'static,
{
    handler
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::BoxBody;

    #[test]
    fn test_compile_path() {
        let (regex, params) = compile_path("/users/:id");
        assert_eq!(params, vec!["id"]);
        assert!(regex.is_match("/users/123"));
        assert!(!regex.is_match("/users/123/posts"));

        let (regex, params) = compile_path("/users/:user_id/posts/:post_id");
        assert_eq!(params, vec!["user_id", "post_id"]);
        assert!(regex.is_match("/users/1/posts/2"));
    }

    #[test]
    fn test_exact_match() {
        let (regex, params) = compile_path("/users");
        assert_eq!(params.len(), 0);
        assert!(regex.is_match("/users"));
        assert!(!regex.is_match("/users/123"));
    }

    #[tokio::test]
    async fn test_handler_with_12_extractors() {
        use crate::extract::State;

        #[derive(Clone)]
        struct AppState;

        async fn h12(
            _s1: State<AppState>,
            _s2: State<AppState>,
            _s3: State<AppState>,
            _s4: State<AppState>,
            _s5: State<AppState>,
            _s6: State<AppState>,
            _s7: State<AppState>,
            _s8: State<AppState>,
            _s9: State<AppState>,
            _s10: State<AppState>,
            _s11: State<AppState>,
            _s12: State<AppState>,
        ) -> Result<ToxiResponse> {
            Ok(ToxiResponse::text("ok"))
        }

        let mut router = Router::new();
        router.with_state(AppState);
        router.get("/", h12);

        let req = http::Request::builder()
            .method(Method::GET)
            .uri("/")
            .body(BoxBody::default())
            .expect("request");

        let result = router.handle(req).await.expect("handle");
        assert_eq!(result.status(), http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_method_not_allowed_when_path_exists() {
        let mut router = Router::new();
        router.get("/users", || async { Ok(crate::ToxiResponse::text("ok")) });
        let req = http::Request::builder()
            .method(Method::POST)
            .uri("/users")
            .body(BoxBody::default())
            .expect("request");

        let result = router.handle(req).await;
        assert!(matches!(result, Err(Error::MethodNotAllowed(_))));
    }

    #[tokio::test]
    async fn test_not_found_when_path_missing() {
        let router = Router::new();
        let req = http::Request::builder()
            .method(Method::GET)
            .uri("/missing")
            .body(BoxBody::default())
            .expect("request");

        let result = router.handle(req).await;
        assert!(matches!(result, Err(Error::NotFound(_))));
    }

    #[tokio::test]
    async fn test_handler_with_8_extractors() {
        use crate::extract::State;

        #[derive(Clone)]
        struct AppState;

        async fn h8(
            _s1: State<AppState>,
            _s2: State<AppState>,
            _s3: State<AppState>,
            _s4: State<AppState>,
            _s5: State<AppState>,
            _s6: State<AppState>,
            _s7: State<AppState>,
            _s8: State<AppState>,
        ) -> Result<ToxiResponse> {
            Ok(ToxiResponse::text("ok"))
        }

        let mut router = Router::new();
        router.with_state(AppState);
        router.get("/", h8);

        let req = http::Request::builder()
            .method(Method::GET)
            .uri("/")
            .body(BoxBody::default())
            .expect("request");

        let result = router.handle(req).await.expect("handle");
        assert_eq!(result.status(), http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_router_layer() {
        use tower::Layer;

        struct MyMiddleware<S>(S);
        impl<S: Endpoint> Endpoint for MyMiddleware<S> {
            fn call(&self, mut req: ToxiRequest) -> Pin<Box<dyn Future<Output = Result<ToxiResponse>> + Send>> {
                req.extensions_mut().insert("middleware_called".to_string());
                self.0.call(req)
            }
        }

        struct MyLayer;
        impl<S> Layer<S> for MyLayer {
            type Service = MyMiddleware<S>;
            fn layer(&self, inner: S) -> Self::Service {
                MyMiddleware(inner)
            }
        }

        // We need to check if extensions were modified.
        // But the handler in this test doesn't check it.
        // Let's modify the handler to check.

        let mut router = Router::new()
            .layer(MyLayer);
        router.get("/", |req: ToxiRequest| async move {
            if req.extensions().get::<String>().map(|s| s == "middleware_called").unwrap_or(false) {
                Ok(ToxiResponse::text("middleware_ok"))
            } else {
                Ok(ToxiResponse::text("middleware_fail"))
            }
        });

        let req = http::Request::builder()
            .method(Method::GET)
            .uri("/")
            .body(BoxBody::default())
            .expect("request");

        let res = router.handle(req).await.expect("handle");
        let hyper_res: hyper::Response<crate::types::BoxBody> = res.into();
        let body = hyper_res.into_body();
        use http_body_util::BodyExt;
        let bytes = body.collect().await.unwrap().to_bytes();
        assert_eq!(bytes, "middleware_ok");
    }

    #[tokio::test]
    async fn test_cors_config_default() {
        let config = CorsConfig::default();
        assert_eq!(config.allowed_origins, vec!["*"]);
        assert!(!config.allowed_methods.is_empty());
        assert_eq!(config.allowed_headers, vec!["*"]);
        assert!(!config.allow_credentials);
        assert_eq!(config.max_age, 3600);
    }

    #[tokio::test]
    async fn test_cors_config_permissive() {
        let config = CorsConfig::permissive();
        assert_eq!(config.allowed_origins, vec!["*"]);
        assert_eq!(config.allowed_headers, vec!["*"]);
    }

    #[tokio::test]
    async fn test_cors_config_restrictive() {
        let config = CorsConfig::restrictive();
        assert!(config.allowed_origins.is_empty());
        assert_eq!(config.allowed_methods, vec!["GET", "POST"]);
        assert_eq!(config.allowed_headers, vec!["Content-Type"]);
    }

    #[tokio::test]
    async fn test_cors_preflight_response() {
        let mut router = Router::new()
            .with_cors(CorsConfig {
                allowed_origins: vec!["http://localhost:3000".to_string()],
                allowed_methods: vec!["GET".to_string(), "POST".to_string()],
                allowed_headers: vec!["Content-Type".to_string()],
                allow_credentials: true,
                max_age: 7200,
            });
        router.get("/test", || async { Ok(ToxiResponse::text("ok")) });

        let req = http::Request::builder()
            .method(Method::OPTIONS)
            .uri("/test")
            .body(BoxBody::default())
            .expect("request");

        let res = router.handle(req).await.expect("handle");
        let hyper_res: hyper::Response<crate::types::BoxBody> = res.into();

        // Router-level: OPTIONS with no explicit handler returns 204 No Content.
        // CORS response headers are appended by BodyAdapter at the server layer,
        // covered by the integration test in tests/cors.rs.
        assert_eq!(hyper_res.status(), http::StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_cors_config_applied_at_server_layer() {
        // CORS headers are attached by BodyAdapter (server layer), not by Router::handle.
        // Verify the router still dispatches normally when CORS is configured.
        let mut router = Router::new()
            .with_cors(CorsConfig::permissive());
        router.get("/test", || async { Ok(ToxiResponse::text("ok")) });

        let req = http::Request::builder()
            .method(Method::GET)
            .uri("/test")
            .body(BoxBody::default())
            .expect("request");

        let res = router.handle(req).await.expect("handle");
        let hyper_res: hyper::Response<crate::types::BoxBody> = res.into();

        assert_eq!(hyper_res.status(), http::StatusCode::OK);
        // CORS headers must NOT be present at the router level (avoid doubling).
        assert!(hyper_res.headers().get("Access-Control-Allow-Origin").is_none());
    }

    #[tokio::test]
    async fn test_no_cors_when_not_configured() {
        let mut router = Router::new();
        router.get("/test", || async { Ok(ToxiResponse::text("ok")) });

        let req = http::Request::builder()
            .method(Method::GET)
            .uri("/test")
            .body(BoxBody::default())
            .expect("request");

        let res = router.handle(req).await.expect("handle");
        let hyper_res: hyper::Response<crate::types::BoxBody> = res.into();
        
        // Should NOT have CORS headers
        let headers = hyper_res.headers();
        assert!(headers.get("Access-Control-Allow-Origin").is_none());
        assert!(headers.get("Access-Control-Allow-Methods").is_none());
    }
}
