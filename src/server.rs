use std::net::SocketAddr;
use tokio::net::TcpListener;
use hyper::server::conn::{http1, http2};
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use crate::error::{Error, Result};
use crate::types::{ToxiRequest, ToxiResponse};
use crate::router::CorsConfig;
use tower_service::Service;
use std::error::Error as StdError;
use http::HeaderValue;

use http_body_util::BodyExt;

use std::task::{Context, Poll};

// WebSocket upgrades are handled by route handlers via the WebSocketUpgrade extractor.
// No default WS interception here — route handlers (agent, sandbox terminal, etc.)
// receive the OnUpgrade future from hyper's request extensions.

/// HTTP protocol version for the server.
#[derive(Debug, Clone, Copy, Default)]
pub enum HttpVersion {
    /// Serve only HTTP/1.1 (default).
    #[default]
    Http1,
    /// Serve only HTTP/2.
    Http2,
    /// Auto-detect: try HTTP/2 first, fall back to HTTP/1.1.
    Auto,
}

/// Executor for HTTP/2 connections.
#[derive(Clone)]
struct TokioExecutor;

impl<F> hyper::rt::Executor<F> for TokioExecutor
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    fn execute(&self, fut: F) {
        tokio::task::spawn(fut);
    }
}

/// HTTP/3 (QUIC) server module — enabled with the `http3` feature flag.
#[cfg(feature = "http3")]
pub mod http3_server;

#[cfg(feature = "http3")]
pub use http3_server::Http3Server;

/// Adapter to convert `hyper::Request<Incoming>` to ToxiRequest
#[derive(Clone)]
pub struct BodyAdapter<S> {
    inner: S,
    cors_config: Option<CorsConfig>,
}

impl<S> BodyAdapter<S> {
    /// Create a new `BodyAdapter` wrapping the given inner service.
    pub fn new(service: S) -> Self {
        Self {
            inner: service,
            cors_config: None,
        }
    }

    /// Attach a CORS configuration to this adapter.
    pub fn with_cors(mut self, cors_config: Option<CorsConfig>) -> Self {
        self.cors_config = cors_config;
        self
    }

    /// Add CORS headers to a hyper response
    fn add_cors_to_response(&self, res: &mut hyper::Response<crate::types::BoxBody>) {
        if let Some(cors) = &self.cors_config {
            let headers = res.headers_mut();
            
            if let Some(origin) = cors.allowed_origins.first() {
                if let Ok(val) = HeaderValue::from_str(origin) {
                    headers.insert(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, val);
                }
            }
            
            // Add Access-Control-Allow-Methods (join all methods with ", ")
            if !cors.allowed_methods.is_empty() {
                let methods = cors.allowed_methods.join(", ");
                if let Ok(val) = HeaderValue::from_str(&methods) {
                    headers.insert(http::header::ACCESS_CONTROL_ALLOW_METHODS, val);
                }
            }
            
            // Add Access-Control-Allow-Headers (join all headers with ", ")
            if !cors.allowed_headers.is_empty() {
                let headers_list = cors.allowed_headers.join(", ");
                if let Ok(val) = HeaderValue::from_str(&headers_list) {
                    headers.insert(http::header::ACCESS_CONTROL_ALLOW_HEADERS, val);
                }
            }
            
            if cors.allow_credentials {
                headers.insert(http::header::ACCESS_CONTROL_ALLOW_CREDENTIALS, HeaderValue::from_static("true"));
            }
            
            if let Ok(val) = HeaderValue::from_str(&cors.max_age.to_string()) {
                headers.insert(http::header::ACCESS_CONTROL_MAX_AGE, val);
            }
        }
    }
}



use std::pin::Pin;

impl<S> Service<hyper::Request<hyper::body::Incoming>> for BodyAdapter<S>
where
    S: Service<ToxiRequest, Response = ToxiResponse, Error = Error> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = hyper::Response<crate::types::BoxBody>;
    type Error = Error;
    type Future = Pin<Box<dyn std::future::Future<Output = std::result::Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: hyper::Request<hyper::body::Incoming>) -> Self::Future {
        let is_ws_upgrade = req.headers().get(hyper::header::UPGRADE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_lowercase() == "websocket")
            .unwrap_or(false);
        let accepts_html = req.headers().get(hyper::header::ACCEPT)
            .map(|h| h.to_str().unwrap_or("").contains("text/html"))
            .unwrap_or(false);

        if is_ws_upgrade {
            log::info!("WebSocket upgrade: {} {}", req.method(), req.uri().path());
        }
            
        let req = req.map(|b| b.map_err(|e| e.into()).boxed());
        let fut = self.inner.call(req);
        let cors = self.cors_config.clone();
        
        Box::pin(async move {
            match fut.await {
                Ok(response) => {
                    let mut hyper_response: hyper::Response<crate::types::BoxBody> = response.into();
                    // Add CORS headers to successful responses
                    let adapter = BodyAdapter { inner: (), cors_config: cors };
                    adapter.add_cors_to_response(&mut hyper_response);
                    Ok(hyper_response)
                },
                Err(error) => {
                    let env = std::env::var("TOXI_ENV").unwrap_or_else(|_| "development".to_string());
                    
                    if env == "development" && accepts_html && error.is_server_error() {
                        use bytes::Bytes;
                        use http_body_util::Full;
                        use hyper::header::{CONTENT_TYPE, SERVER};
                        
                        let html = crate::error::render_ignition_error(&error);
                        
                        let mut res = hyper::Response::builder()
                            .status(error.status_code())
                            .header(CONTENT_TYPE, "text/html; charset=utf-8")
                            .header(SERVER, crate::response::SERVER_HEADER_VALUE)
                            .body(Full::new(Bytes::from(html)).map_err(|e| match e {}).boxed())
                            .unwrap();
                        
                        // Add CORS headers to error responses too
                        let adapter = BodyAdapter { inner: (), cors_config: cors };
                        adapter.add_cors_to_response(&mut res);
                        Ok(res)
                    } else {
                        let mut error_response: hyper::Response<crate::types::BoxBody> = ToxiResponse::from(error).into();
                        // Add CORS headers to error responses
                        let adapter = BodyAdapter { inner: (), cors_config: cors };
                        adapter.add_cors_to_response(&mut error_response);
                        Ok(error_response)
                    }
                }
            }
        })
    }
}



/// HTTP server that listens for incoming connections and dispatches
/// requests to a tower service.
///
/// Binds a TCP listener, adapts incoming hyper requests via [`BodyAdapter`],
/// and serves them over HTTP/1.1 or HTTP/2. Supports optional CORS configuration
/// and graceful shutdown.
pub struct Server<S> {
    service: S,
    addr: Option<SocketAddr>,
    cors_config: Option<CorsConfig>,
    http_version: HttpVersion,
    max_body_size: usize,
}

impl<S> Server<S>
where
    S: Service<ToxiRequest, Response = ToxiResponse, Error = Error> + Clone + Send + Sync + 'static,
    S::Future: Send + 'static,
{
    /// Create a new `Server` wrapping the given service.
    ///
    /// Default settings:
    /// - HTTP version: HTTP/1.1
    /// - Max body size: 10 MB
    pub fn new(service: S) -> Self {
        Self {
            service,
            addr: None,
            cors_config: None,
            http_version: HttpVersion::default(),
            max_body_size: 10 * 1024 * 1024, // 10 MB
        }
    }

    /// Set the socket address to bind to and return the server.
    pub fn bind(mut self, addr: SocketAddr) -> Self {
        self.addr = Some(addr);
        self
    }

    /// Configure CORS for the server (applies to all responses including errors)
    pub fn with_cors(mut self, cors_config: CorsConfig) -> Self {
        self.cors_config = Some(cors_config);
        self
    }

    /// Set the maximum request body size in bytes.
    ///
    /// Requests exceeding this limit receive a `413 Payload Too Large` response.
    /// Default is 10 MB.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Server::new(router)
    ///     .with_body_limit(5 * 1024 * 1024) // 5 MB
    ///     .listen(addr)
    ///     .await
    /// ```
    pub fn with_body_limit(mut self, max_bytes: usize) -> Self {
        self.max_body_size = max_bytes;
        self
    }

    /// Set the HTTP version to serve.
    ///
    /// - `HttpVersion::Http1` — HTTP/1.1 only (default)
    /// - `HttpVersion::Http2` — HTTP/2 only (useful when behind a TLS-terminating proxy)
    /// - `HttpVersion::Auto` — try HTTP/2 first, fall back to HTTP/1.1
    pub fn with_http_version(mut self, version: HttpVersion) -> Self {
        self.http_version = version;
        self
    }

    /// Start the server using the stored address or fall back to `127.0.0.1:3000`.
    pub async fn run(self) -> Result<()> {
        let addr = self.addr.unwrap_or_else(|| "127.0.0.1:3000".parse().unwrap());
        self.listen(addr).await
    }

    /// Bind a TCP listener to the given address and serve requests.
    ///
    /// Handles SIGTERM/SIGINT for graceful shutdown: stops accepting new
    /// connections and waits for in-flight requests to complete.
    pub async fn listen(self, addr: SocketAddr) -> Result<()> {
        let listener = TcpListener::bind(addr).await?;
        let version_str = match self.http_version {
            HttpVersion::Http1 => "HTTP/1.1",
            HttpVersion::Http2 => "HTTP/2",
            HttpVersion::Auto => "HTTP/1.1 + HTTP/2",
        };
        log::info!("Listening on http://{} ({})", addr, version_str);

        let cors_config = self.cors_config.clone();
        let http_version = self.http_version;

        // Track in-flight connections for graceful shutdown
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Spawn signal handler for graceful shutdown
        tokio::spawn(async move {
            let ctrl_c = tokio::signal::ctrl_c();
            #[cfg(unix)]
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("failed to install SIGTERM handler");

            #[cfg(unix)]
            tokio::select! {
                _ = ctrl_c => {}
                _ = sigterm.recv() => {}
            }
            #[cfg(not(unix))]
            ctrl_c.await.ok();

            log::info!("Received shutdown signal, shutting down gracefully...");
            let _ = shutdown_tx.send(true);
        });

        loop {
            tokio::select! {
                result = listener.accept() => {
                    let (stream, _) = result?;
                    let io = TokioIo::new(stream);
                    let service = self.service.clone();
                    let cors = cors_config.clone();

                    tokio::task::spawn(async move {
                        let service = BodyAdapter::new(service).with_cors(cors);
                        let hyper_service = TowerToHyperService::new(service);

                        let result: std::result::Result<(), Box<dyn StdError + Send + Sync>> = match http_version {
                            HttpVersion::Http1 => {
                                http1::Builder::new()
                                    .serve_connection(io, hyper_service)
                                    .with_upgrades()
                                    .await
                                    .map_err(|e| Box::new(e) as Box<dyn StdError + Send + Sync>)
                            }
                            HttpVersion::Http2 => {
                                http2::Builder::new(TokioExecutor)
                                    .serve_connection(io, hyper_service)
                                    .await
                                    .map_err(|e| Box::new(e) as Box<dyn StdError + Send + Sync>)
                            }
                            HttpVersion::Auto => {
                                hyper_util::server::conn::auto::Builder::new(TokioExecutor)
                                    .serve_connection_with_upgrades(io, hyper_service)
                                    .await
                            }
                        };

                        if let Err(err) = result {
                            let err_msg = err.to_string();
                            if err_msg.contains("invalid HTTP method")
                                || err_msg.contains("invalid HTTP version")
                            {
                                log::warn!(
                                    "Protocol mismatch — client or reverse proxy may be sending \
                                     HTTP/2 traffic to an HTTP/1.1 listener ({:?}). Original error: {}",
                                    http_version, err
                                );
                                return;
                            }

                            if let Some(service_err) = err.source().and_then(|e: &(dyn StdError + 'static)| e.downcast_ref::<Error>()) {
                                if service_err.is_server_error() {
                                    log::error!("Service error: {}", service_err);
                                }
                            } else if !err_msg.contains("connection closed")
                                && !err_msg.contains("broken pipe")
                            {
                                log::warn!("Connection error: {}", err);
                            }
                        }
                    });
                }
                _ = async {
                    let mut rx = shutdown_rx.clone();
                    rx.changed().await.ok();
                } => {
                    log::info!("No longer accepting new connections. Waiting for in-flight requests...");
                    // Give in-flight connections time to drain
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    log::info!("Shutdown complete.");
                    return Ok(());
                }
            }
        }
    }

    /// Listen with both HTTP/1.1 and HTTP/3 support
    #[cfg(feature = "http3")]
    pub async fn listen_h3(self, addr: SocketAddr, cert_pem: &str, key_pem: &str) -> Result<()> {
        use rustls::ServerConfig;  
        use rustls_pemfile::{certs, pkcs8_private_keys};
        use std::io::Cursor;
        
        let cors_config = self.cors_config.clone();
        
        // Setup HTTP/1.1 server in background
        let http1_addr = addr;
        let http1_service = self.service.clone();
        let http1_cors = cors_config.clone();
        
        tokio::spawn(async move {
            let listener = TcpListener::bind(http1_addr).await.unwrap();
            log::info!("HTTP/1.1 server listening on http://{}", http1_addr);
            
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let io = TokioIo::new(stream);
                let service = http1_service.clone();
                let cors = http1_cors.clone();
                
                tokio::task::spawn(async move {
                    let service = BodyAdapter::new(service).with_cors(cors);
                    let hyper_service = TowerToHyperService::new(service);
                    
                    if let Err(err) = http1::Builder::new()
                        .serve_connection(io, hyper_service)
                        .with_upgrades()
                        .await
                    {
                        // Only log server errors, not client errors
                        if let Some(service_err) = err.source().and_then(|e: &(dyn StdError + 'static)| e.downcast_ref::<Error>()) {
                            if service_err.is_server_error() {
                                log::error!("HTTP/1.1 server error: {}", service_err);
                            }
                        } else {
                            let err_msg = err.to_string();
                            if !err_msg.contains("NotFound") && !err_msg.contains("connection closed") {
                                log::warn!("HTTP/1.1 connection error: {}", err);
                            }
                        }
                    }
                });
            }
        });
        
        // Setup HTTP/3 server
        let cert_chain = certs(&mut Cursor::new(cert_pem))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| crate::error::Error::InternalServerError(e.to_string()))?;
        
        let mut keys = pkcs8_private_keys(&mut Cursor::new(key_pem))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        
        if keys.is_empty() {
            return Err(crate::error::Error::InternalServerError("No private keys found".to_string()));
        }
        
        let tls_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert_chain, rustls::pki_types::PrivateKeyDer::Pkcs8(keys.remove(0)))
            .map_err(|e| crate::error::Error::InternalServerError(e.to_string()))?;
        
        let http3_server = Http3Server::new(self.service);
        http3_server.listen(addr, tls_config).await?;
        
        Ok(())
    }
}
