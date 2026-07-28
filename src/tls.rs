//! HTTPS and HTTP/2 support for Toxi

use std::net::SocketAddr;
use tokio::net::TcpListener;
use hyper::server::conn::{http1, http2};
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::TlsAcceptor;
use std::sync::Arc;
use std::fs::File;
use std::io::BufReader;
use crate::error::{Error, Result};
use crate::types::{ToxiRequest, ToxiResponse};
use crate::server::{BodyAdapter, HttpVersion};
use tower_service::Service;

/// TLS configuration for HTTPS
pub struct TlsConfig {
    pub cert_path: String,
    pub key_path: String,
}

impl TlsConfig {
    /// Create a new TLS configuration referencing the given certificate and key file paths.
    pub fn new(cert_path: impl Into<String>, key_path: impl Into<String>) -> Self {
        Self {
            cert_path: cert_path.into(),
            key_path: key_path.into(),
        }
    }
    
    /// Load certificates and private key, advertising both HTTP/1.1 and HTTP/2 via ALPN.
    pub fn load_config(&self) -> Result<ServerConfig> {
        self.load_config_with_alpn(true)
    }

    /// Load certificates and private key with optional ALPN negotiation.
    ///
    /// When `enable_alpn` is true, the server advertises both `h2` and `http/1.1`
    /// so clients (and proxies) can negotiate HTTP/2 via TLS ALPN.
    pub fn load_config_with_alpn(&self, enable_alpn: bool) -> Result<ServerConfig> {
        let certs = load_certs(&self.cert_path)?;
        let key = load_private_key(&self.key_path)?;

        let mut config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| Error::InternalServerError(e.to_string()))?;

        if enable_alpn {
            config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        }

        Ok(config)
    }
}

fn load_certs(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let file = File::open(path).map_err(|e| Error::InternalServerError(format!("Failed to open cert file: {}", e)))?;
    let mut reader = BufReader::new(file);
    rustls_pemfile::certs(&mut reader)
        .map(|res| res.map_err(|e| Error::InternalServerError(format!("Failed to parse cert: {}", e))))
        .collect::<Result<Vec<_>>>()
}

fn load_private_key(path: &str) -> Result<PrivateKeyDer<'static>> {
    let file = File::open(path).map_err(|e| Error::InternalServerError(format!("Failed to open key file: {}", e)))?;
    let mut reader = BufReader::new(file);

    // Try to read the first private key
    loop {
        match rustls_pemfile::read_one(&mut reader).map_err(|e| Error::InternalServerError(format!("Failed to parse key: {}", e)))? {
            Some(rustls_pemfile::Item::Pkcs1Key(key)) => return Ok(key.into()),
            Some(rustls_pemfile::Item::Pkcs8Key(key)) => return Ok(key.into()),
            Some(rustls_pemfile::Item::Sec1Key(key)) => return Ok(key.into()),
            None => break,
            _ => {} // Ignore other items like certificates
        }
    }

    Err(Error::InternalServerError("No supported private key found".to_string()))
}

/// Server builder with HTTPS support
pub struct SecureServer<S> {
    service: S,
    tls_config: Option<TlsConfig>,
    http_version: HttpVersion,
}

impl<S> SecureServer<S>
where
    S: Service<ToxiRequest, Response = ToxiResponse, Error = Error> + Clone + Send + Sync + 'static,
    S::Future: Send + 'static,
{
    /// Create a new `SecureServer` wrapping the given service.
    ///
    /// By default no TLS is configured and HTTP/1.1 is used.
    pub fn new(service: S) -> Self {
        Self {
            service,
            tls_config: None,
            http_version: HttpVersion::Auto,
        }
    }
    
    /// Enable HTTPS with TLS certificates
    pub fn with_tls(mut self, tls_config: TlsConfig) -> Self {
        self.tls_config = Some(tls_config);
        self
    }
    
    /// Set HTTP version
    pub fn with_http_version(mut self, version: HttpVersion) -> Self {
        self.http_version = version;
        self
    }
    
    /// Start the server
    pub async fn listen(self, addr: SocketAddr) -> Result<()> {
        if let Some(tls_config) = self.tls_config {
            Self::listen_https(addr, self.service, tls_config, self.http_version).await
        } else {
            Self::listen_http(addr, self.service).await
        }
    }
    
    /// Listen on HTTP
    async fn listen_http(addr: SocketAddr, service: S) -> Result<()> {
        let listener = TcpListener::bind(addr).await?;
        println!("Listening on http://{}", addr);

        loop {
            let (stream, _) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let service = service.clone();

            tokio::task::spawn(async move {
                let service = BodyAdapter::new(service);
                let hyper_service = TowerToHyperService::new(service);
                
                if let Err(err) = http1::Builder::new()
                    .serve_connection_with_upgrades(io, hyper_service)
                    .await
                {
                    eprintln!("Error serving connection: {:?}", err);
                }
            });
        }
    }
    
    /// Listen on HTTPS with ALPN-based protocol negotiation.
    ///
    /// When `HttpVersion::Auto` is set, the TLS configuration advertises both
    /// `h2` and `http/1.1` via ALPN. The client (or reverse proxy) picks the
    /// highest mutually-supported version. The server then dispatches to the
    /// matching hyper connection builder.
    async fn listen_https(addr: SocketAddr, service: S, tls_config: TlsConfig, http_version: HttpVersion) -> Result<()> {
        let enable_alpn = matches!(http_version, HttpVersion::Auto);
        let server_config = tls_config.load_config_with_alpn(enable_alpn)?;
        let acceptor = TlsAcceptor::from(Arc::new(server_config));

        let listener = TcpListener::bind(addr).await?;
        let version_str = match http_version {
            HttpVersion::Http1 => "HTTP/1.1",
            HttpVersion::Http2 => "HTTP/2",
            HttpVersion::Auto => "HTTP/1.1 + HTTP/2 (ALPN)",
        };
        println!("Listening on https://{} ({})", addr, version_str);

        loop {
            let (stream, _) = listener.accept().await?;
            let acceptor = acceptor.clone();
            let service = service.clone();

            tokio::task::spawn(async move {
                match acceptor.accept(stream).await {
                    Ok(tls_stream) => {
                        // Extract ALPN protocol before moving tls_stream into TokioIo
                        let negotiated = if matches!(http_version, HttpVersion::Auto) {
                            match tls_stream.get_ref().1.alpn_protocol() {
                                Some(b"h2") => HttpVersion::Http2,
                                _ => HttpVersion::Http1,
                            }
                        } else {
                            http_version
                        };

                        let io = TokioIo::new(tls_stream);
                        let service = BodyAdapter::new(service);
                        let hyper_service = TowerToHyperService::new(service);

                        let result = match negotiated {
                            HttpVersion::Http1 => {
                                http1::Builder::new()
                                    .serve_connection_with_upgrades(io, hyper_service)
                                    .await
                            }
                            HttpVersion::Http2 => {
                                http2::Builder::new(TokioExecutor)
                                    .serve_connection(io, hyper_service)
                                    .await
                            }
                            HttpVersion::Auto => unreachable!(),
                        };

                        if let Err(err) = result {
                            let err_msg = err.to_string();
                            if err_msg.contains("invalid HTTP method")
                                || err_msg.contains("invalid HTTP version")
                            {
                                eprintln!(
                                    "TLS connection error: protocol mismatch — the client or proxy \
                                     sent incompatible traffic. Original error: {}",
                                    err
                                );
                            } else {
                                eprintln!("TLS connection error: {}", err);
                            }
                        }
                    }
                    Err(err) => {
                        eprintln!("TLS accept error: {:?}", err);
                    }
                }
            });
        }
    }
}

// Executor for HTTP/2
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
