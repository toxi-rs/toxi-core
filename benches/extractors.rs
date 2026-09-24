//! Extractor benches (divan): micro `FromRequest` cost with integrated
//! `Router::handle` cost for the same extractors.

use divan::Bencher;
use http::Method;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use toxi_core::extract::{Cookies, FromRequest, Json, Path, Query, State};
use toxi_core::types::BoxBody;
use toxi_core::{Result, Router, ToxiRequest, ToxiResponse};

fn main() {
    divan::main();
}

#[derive(Debug, Serialize, Deserialize)]
struct SmallBody {
    name: String,
    email: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Pagination {
    page: u32,
    limit: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserPath {
    id: u64,
}

#[derive(Clone)]
struct AppState {
    db_url: Arc<String>,
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt")
}

fn body_req(method: &Method, uri: &str, content_type: &str, bytes: &[u8]) -> ToxiRequest {
    use http_body_util::{BodyExt, Full};
    let body = Full::new(bytes::Bytes::copy_from_slice(bytes))
        .map_err(|e| match e {})
        .boxed();
    http::Request::builder()
        .method(method.clone())
        .uri(uri)
        .header("content-type", content_type)
        .body(body)
        .expect("bench request")
}

fn empty_req(method: &Method, uri: &str) -> ToxiRequest {
    http::Request::builder()
        .method(method.clone())
        .uri(uri)
        .body(BoxBody::default())
        .expect("bench request")
}

async fn json_handler(Json(b): Json<SmallBody>) -> Result<ToxiResponse> {
    Ok(ToxiResponse::text(format!("hi {}", b.name)))
}

async fn query_handler(Query(p): Query<Pagination>) -> Result<ToxiResponse> {
    Ok(ToxiResponse::text(format!("p{}:{}", p.page, p.limit)))
}

async fn path_handler(Path(p): Path<UserPath>) -> Result<ToxiResponse> {
    Ok(ToxiResponse::text(format!("u{}", p.id)))
}

async fn state_handler(State(s): State<AppState>) -> Result<ToxiResponse> {
    Ok(ToxiResponse::text(s.db_url.as_str().to_string()))
}

fn small_json() -> Vec<u8> {
    serde_json::to_vec(&SmallBody {
        name: "alice".into(),
        email: "alice@example.com".into(),
    })
    .unwrap()
}

fn big_json() -> Vec<u8> {
    serde_json::to_vec(
        &(0..200u32)
            .map(|i| SmallBody {
                name: format!("user-{i}"),
                email: format!("user-{i}@example.com"),
            })
            .collect::<Vec<_>>(),
    )
    .unwrap()
}

#[divan::bench]
fn micro_json_small(bencher: Bencher) {
    let rt = rt();
    let payload = small_json();
    bencher
        .with_inputs(|| body_req(&Method::POST, "/", "application/json", &payload))
        .bench_values(|mut r| {
            divan::black_box(rt.block_on(Json::<SmallBody>::from_request(&mut r)))
        });
}

#[divan::bench]
fn micro_json_10kb(bencher: Bencher) {
    let rt = rt();
    let payload = big_json();
    bencher
        .with_inputs(|| body_req(&Method::POST, "/", "application/json", &payload))
        .bench_values(|mut r| {
            divan::black_box(rt.block_on(Json::<Vec<SmallBody>>::from_request(&mut r)))
        });
}

#[divan::bench]
fn micro_query(bencher: Bencher) {
    let rt = rt();
    bencher
        .with_inputs(|| empty_req(&Method::GET, "/?page=3&limit=50"))
        .bench_values(|mut r| {
            divan::black_box(rt.block_on(Query::<Pagination>::from_request(&mut r)))
        });
}

#[divan::bench]
fn micro_cookies(bencher: Bencher) {
    let rt = rt();
    bencher
        .with_inputs(|| {
            http::Request::builder()
                .method(Method::GET)
                .uri("/")
                .header("cookie", "session=abc123; theme=dark; lang=en; tz=UTC; pref=compact")
                .body(BoxBody::default())
                .expect("bench request")
        })
        .bench_values(|mut r| divan::black_box(rt.block_on(Cookies::from_request(&mut r))));
}

fn integrated_router() -> Arc<Router> {
    let mut router = Router::new();
    router.post("/json", json_handler);
    router.get("/query", query_handler);
    router.get("/users/:id", path_handler);
    router.with_state(AppState {
        db_url: Arc::new("postgres://localhost/toxi".to_string()),
    });
    router.get("/state", state_handler);
    Arc::new(router)
}

#[divan::bench]
fn integrated_json(bencher: Bencher) {
    let router = integrated_router();
    let rt = rt();
    let payload = small_json();
    bencher
        .with_inputs(|| body_req(&Method::POST, "/json", "application/json", &payload))
        .bench_values(|r| divan::black_box(rt.block_on(router.handle(r))));
}

#[divan::bench]
fn integrated_query(bencher: Bencher) {
    let router = integrated_router();
    let rt = rt();
    bencher
        .with_inputs(|| empty_req(&Method::GET, "/query?page=3&limit=50"))
        .bench_values(|r| divan::black_box(rt.block_on(router.handle(r))));
}

#[divan::bench]
fn integrated_path(bencher: Bencher) {
    let router = integrated_router();
    let rt = rt();
    bencher
        .with_inputs(|| empty_req(&Method::GET, "/users/42"))
        .bench_values(|r| divan::black_box(rt.block_on(router.handle(r))));
}

#[divan::bench]
fn integrated_state(bencher: Bencher) {
    let router = integrated_router();
    let rt = rt();
    bencher
        .with_inputs(|| empty_req(&Method::GET, "/state"))
        .bench_values(|r| divan::black_box(rt.block_on(router.handle(r))));
}
