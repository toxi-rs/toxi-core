//! # Toxi Core
//!
//! The foundational HTTP kernel and routing engine for the **Toxi** framework.
//!
//! `toxi-core` provides everything needed to build high-performance HTTP services:
//! a regex-based router with typed extractors, an HTTP/1.1 and HTTP/2 server,
//! TLS/HTTPS support with ALPN negotiation, middleware composition via Tower,
//! and a rich set of response helpers.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐     ┌──────────────┐     ┌──────────────┐
//! │   Client     │────▶│    Server     │────▶│   Router     │
//! │  (HTTP/WS)   │     │  (hyper)      │     │  (routes)    │
//! └─────────────┘     └──────────────┘     └──────┬───────┘
//!                                                  │
//!                                          ┌───────▼───────┐
//!                                          │   Handler     │
//!                                          │  (extractors) │
//!                                          └───────────────┘
//! ```
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use toxi_core::*;
//!
//! async fn hello(_req: Request) -> Result<Response> {
//!     Ok(Response::text("Hello from Toxi!"))
//! }
//!
//! # async fn run() -> Result<()> {
//! let mut app = Application::new(Config::default());
//! app.router_mut().get("/", hello);
//! app.run().await
//! # }
//! ```
//!
//! ## Key Concepts
//!
//! - **Extractors**: Types implementing [`FromRequest`] that pull typed data from requests
//!   (`Json<T>`, `Path<T>`, `Query<T>`, `State<T>`, etc.)
//! - **Handlers**: Async functions whose parameters are extractors, up to 12 by default
//!   (extendable via [`impl_handler_for_fn!`](macro@crate::impl_handler_for_fn))
//! - **Middleware**: Tower-compatible layers applied via [`Router::layer`] or
//!   [`Server::with_cors`]
//! - **HTTP/2**: Server supports HTTP/2 via ALPN when TLS is enabled, or via
//!   [`Server::with_http_version`]

// ── Module declarations ──────────────────────────────────────────────

/// Application boot coordinator and configuration.
///
/// [`Application`] orchestrates the startup sequence: Config → Router → Middleware → Server.
pub mod app;

/// Error types and the `Result` alias.
///
/// [`Error`] covers common HTTP error conditions (400, 401, 403, 404, 405, 409, 422, 429, 500, 503)
/// as well as low-level errors from hyper, serde, and I/O.
pub mod error;

/// Request extractors for pulling typed data from incoming requests.
///
/// Built-in extractors: [`Json<T>`](extract::Json), [`Path<T>`](extract::Path),
/// [`Query<T>`](extract::Query), [`State<T>`](extract::State),
/// [`Form<T>`](extract::Form), [`Cookies`](extract::Cookies),
/// [`Body<T>`](extract::Body), [`WebSocketUpgrade`](extract::WebSocketUpgrade).
///
/// Custom extractors implement the [`FromRequest`](extract::FromRequest) trait.
pub mod extract;

/// Extension trait for `Request` with body-reading helpers.
pub mod request;

/// Response constructors and helpers.
///
/// Provides [`ToxiResponse::json`], [`ToxiResponse::text`], [`ToxiResponse::html`],
/// and convenience functions in the [`helpers`] module.
pub mod response;

/// HTTP router with path parameters, middleware, and CORS support.
///
/// The [`Router`] maps HTTP methods + path patterns to handler endpoints.
/// Supports `:param` segments, `*` wildcards, Tower middleware layers,
/// and CORS preflight handling.
pub mod router;

/// HTTP/1.1 and HTTP/2 server loop and body adapter.
///
/// The [`Server`] binds a TCP listener, adapts incoming hyper requests via
/// [`BodyAdapter`](server::BodyAdapter), and dispatches to the router.
/// Supports HTTP/2 via [`HttpVersion`].
pub mod server;

/// TLS/HTTPS support and `SecureServer`.
///
/// Provides [`TlsConfig`](tls::TlsConfig) for loading certificates and
/// [`SecureServer`](tls::SecureServer) for serving over HTTPS with ALPN negotiation.
pub mod tls;

/// Type aliases for requests, responses, and boxed bodies.
///
/// - [`ToxiRequest`] — the framework's request type (`http::Request<BoxBody>`)
/// - [`ToxiResponse`] — the framework's response type
/// - [`BoxBody`] — the erased body type used internally
pub mod types;

/// API versioning with [`ApiVersion`] and [`VersionedRouter`].
pub mod versioning;

/// Backwards-compatible re-exports for cookies and form data.
pub mod cookie;

// ── Re-exports ───────────────────────────────────────────────────────

/// Core error and result types.
pub use error::{Error, Result};

/// Common request extractors.
pub use extract::{FromRequest, Json, Path, Query, State, Form, Cookies, Body, WebSocketUpgrade, PathParams};

/// Re-exports from the `http` crate.
pub use http::{StatusCode, Method, HeaderMap, HeaderValue};

/// Re-export of `tokio::sync::mpsc` for channel-based communication.
pub use tokio::sync::mpsc;

/// Re-export of `http_body_util::BodyExt` for body-reading utilities.
pub use http_body_util::BodyExt;

/// The application boot coordinator.
pub use app::Application;

/// Router and handler types.
pub use router::{Handler, Router, IntoHandler, handler_fn, CorsConfig};

/// The HTTP server.
pub use server::{Server, HttpVersion};

/// Core request and response types.
pub use types::{ToxiRequest, ToxiResponse};

/// Convenience alias: `ToxiResponse` as `Response`.
pub use types::ToxiResponse as Response;

/// Convenience alias: `ToxiRequest` as `Request`.
pub use types::ToxiRequest as Request;

/// API versioning types.
pub use versioning::{ApiVersion, VersionedRouter};

/// Re-export response helpers for convenient access.
///
/// These provide cleaner syntax: `json!({...})`, `text("...")`, `html("...")`
pub use response::helpers::{json, text, html};
