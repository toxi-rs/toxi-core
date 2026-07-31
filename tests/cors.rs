use std::net::SocketAddr;
use tokio::sync::oneshot;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use toxi_core::router::{CorsConfig, Router};
use toxi_core::server::BodyAdapter;
use toxi_core::types::ToxiResponse;

fn build_router(cors: CorsConfig) -> Router {
    let mut router = Router::new().with_cors(cors.clone());
    router.get("/test", || async { Ok(ToxiResponse::text("ok")) });
    router
}

fn toxi_service(router: Router, cors: CorsConfig) -> BodyAdapter<Router> {
    BodyAdapter::new(router).with_cors(Some(cors))
}

async fn start_server(service: BodyAdapter<Router>) -> (SocketAddr, oneshot::Sender<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bound_addr = listener.local_addr().unwrap();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                result = listener.accept() => {
                    let (stream, _) = result.unwrap();
                    let io = TokioIo::new(stream);
                    let service = service.clone();

                    tokio::spawn(async move {
                        let hyper_service = TowerToHyperService::new(service);
                        let _ = http1::Builder::new()
                            .serve_connection(io, hyper_service)
                            .with_upgrades()
                            .await;
                    });
                }
                _ = &mut shutdown_rx => break,
            }
        }
    });

    (bound_addr, shutdown_tx)
}

#[tokio::test]
async fn test_cors_success_response_headers() {
    let _ = env_logger::try_init();
    let cors = CorsConfig::permissive();
    let service = toxi_service(build_router(cors.clone()), cors);
    let (addr, _shutdown) = start_server(service).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/test", addr))
        .header("Origin", "https://example.com")
        .send()
        .await
        .expect("GET failed");

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    assert_eq!(
        resp.headers().get("Access-Control-Allow-Origin").unwrap(),
        "*"
    );
}

#[tokio::test]
async fn test_cors_preflight_headers() {
    let _ = env_logger::try_init();
    let cors = CorsConfig {
        allowed_origins: vec!["http://localhost:3000".to_string()],
        allowed_methods: vec!["GET".to_string(), "POST".to_string()],
        allowed_headers: vec!["Content-Type".to_string()],
        allow_credentials: true,
        max_age: 7200,
    };
    let service = toxi_service(build_router(cors.clone()), cors);
    let (addr, _shutdown) = start_server(service).await;

    let client = reqwest::Client::new();
    let resp = client
        .request(reqwest::Method::OPTIONS, format!("http://{}/test", addr))
        .header("Origin", "http://localhost:3000")
        .header("Access-Control-Request-Method", "GET")
        .send()
        .await
        .expect("OPTIONS failed");

    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);
    assert_eq!(
        resp.headers().get("Access-Control-Allow-Origin").unwrap(),
        "http://localhost:3000"
    );
    assert_eq!(
        resp.headers().get("Access-Control-Allow-Methods").unwrap(),
        "GET, POST"
    );
    assert_eq!(
        resp.headers().get("Access-Control-Allow-Headers").unwrap(),
        "Content-Type"
    );
    assert_eq!(
        resp.headers().get("Access-Control-Allow-Credentials").unwrap(),
        "true"
    );
    assert_eq!(
        resp.headers().get("Access-Control-Max-Age").unwrap(),
        "7200"
    );
}

#[tokio::test]
async fn test_no_cors_when_not_configured() {
    let _ = env_logger::try_init();
    let mut router = Router::new();
    router.get("/test", || async { Ok(ToxiResponse::text("ok")) });
    let service = BodyAdapter::new(router);
    let (addr, _shutdown) = start_server(service).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/test", addr))
        .header("Origin", "https://example.com")
        .send()
        .await
        .expect("GET failed");

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    assert!(resp.headers().get("Access-Control-Allow-Origin").is_none());
}
