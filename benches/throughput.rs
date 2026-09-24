//! In-process concurrent load (divan): `Router::handle` fanned out
//! across Tokio tasks. No TCP; loopback figures belong to the online kit.

use divan::Bencher;
use http::Method;
use std::sync::Arc;
use toxi_core::extract::Json;
use toxi_core::types::BoxBody;
use toxi_core::{Result, Router, ToxiRequest, ToxiResponse};

fn main() {
    divan::main();
}

async fn text_handler() -> Result<ToxiResponse> {
    Ok(ToxiResponse::text("ok"))
}

async fn json_handler() -> Result<ToxiResponse> {
    Ok(ToxiResponse::json(serde_json::json!({"status": "ok", "n": 42})))
}

async fn echo_handler(Json(v): Json<serde_json::Value>) -> Result<ToxiResponse> {
    Ok(ToxiResponse::json(v))
}

fn empty_req(path: &str) -> ToxiRequest {
    http::Request::builder()
        .method(Method::GET)
        .uri(path)
        .body(BoxBody::default())
        .expect("bench request")
}

fn json_post_req(payload: &[u8]) -> ToxiRequest {
    use http_body_util::{BodyExt, Full};
    let body = Full::new(bytes::Bytes::copy_from_slice(payload))
        .map_err(|e| match e {})
        .boxed();
    http::Request::builder()
        .method(Method::POST)
        .uri("/echo")
        .header("content-type", "application/json")
        .body(body)
        .expect("bench request")
}

fn test_router() -> Arc<Router> {
    let mut r = Router::new();
    r.get("/text", text_handler);
    r.get("/json", json_handler);
    r.post("/echo", echo_handler);
    Arc::new(r)
}

fn multi_rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio rt")
}

async fn drive(router: Arc<Router>, tasks: usize, reqs_per_task: usize, path: &str) -> usize {
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..tasks {
        let r = router.clone();
        let p = path.to_string();
        set.spawn(async move {
            let mut n = 0usize;
            for _ in 0..reqs_per_task {
                if r.handle(empty_req(&p)).await.is_ok() {
                    n += 1;
                }
            }
            n
        });
    }
    let mut total = 0;
    while let Some(res) = set.join_next().await {
        total += res.unwrap_or(0);
    }
    total
}

#[divan::bench]
fn sequential_text(bencher: Bencher) {
    let router = test_router();
    let rt = multi_rt();
    bencher.bench(|| divan::black_box(rt.block_on(router.handle(empty_req("/text")))));
}

#[divan::bench]
fn sequential_json(bencher: Bencher) {
    let router = test_router();
    let rt = multi_rt();
    bencher.bench(|| divan::black_box(rt.block_on(router.handle(empty_req("/json")))));
}

#[divan::bench(args = [4, 16, 50])]
fn concurrent_text(bencher: Bencher, tasks: usize) {
    let router = test_router();
    let rt = multi_rt();
    bencher.bench(|| divan::black_box(rt.block_on(drive(router.clone(), tasks, 50, "/text"))));
}

#[divan::bench]
fn concurrent_echo(bencher: Bencher) {
    let router = test_router();
    let rt = multi_rt();
    let payload = serde_json::to_vec(&serde_json::json!({"hello": "world", "n": 1})).unwrap();
    bencher.bench(|| {
        divan::black_box(rt.block_on(async {
            let mut set = tokio::task::JoinSet::new();
            for _ in 0..16 {
                let r = router.clone();
                let p = payload.clone();
                set.spawn(async move {
                    let mut n = 0usize;
                    for _ in 0..25 {
                        if r.handle(json_post_req(&p)).await.is_ok() {
                            n += 1;
                        }
                    }
                    n
                });
            }
            let mut total = 0;
            while let Some(res) = set.join_next().await {
                total += res.unwrap_or(0);
            }
            total
        }))
    });
}
