//! Throughput benchmarks, which record in-process load without TCP transport.
//!
//! Loopback TCP introduces scheduler and loopback variation that obscures
//! router comparison on limited hardware, with the consequence that these
//! benches invoke `Router::handle` concurrently from tokio tasks and report
//! requests per second through criterion throughput. Socket-level figures
//! are prescribed as a complement in the accompanying methodology document,
//! although they are excluded from continuous measurement on account of
//! their sensitivity to machine load.
//!
//! The cases comprise sequential invocation, which establishes the latency
//! ceiling, concurrent fan-out at several task counts, which exercises
//! contention over shared references with compiled patterns, and JSON
//! handling under concurrency, which records serialization cost under load.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use http::Method;
use std::sync::Arc;
use toxi_core::extract::Json;
use toxi_core::types::BoxBody;
use toxi_core::{Result, Router, ToxiRequest, ToxiResponse};

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
    // The infallible error of `Full` is erased through exhaustive matching,
    // since `BoxBody` requires `hyper::Error` as its error type.
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

/// Drive `reqs_per_task * tasks` handles concurrently, return total served.
async fn drive(router: Arc<Router>, tasks: usize, reqs_per_task: usize, path: &str) -> usize {
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..tasks {
        let r = router.clone();
        let p = path.to_string();
        set.spawn(async move {
            let mut n = 0usize;
            for _ in 0..reqs_per_task {
                let req = empty_req(&p);
                if r.handle(req).await.is_ok() {
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

fn bench_throughput(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio rt");
    let router = test_router();

    // sequential baseline
    {
        let mut g = c.benchmark_group("throughput/sequential");
        g.throughput(Throughput::Elements(1));
        g.bench_function("text-1-req", |b| {
            b.iter(|| rt.block_on(router.handle(black_box(empty_req("/text")))));
        });
        g.bench_function("json-1-req", |b| {
            b.iter(|| rt.block_on(router.handle(black_box(empty_req("/json")))));
        });
        g.finish();
    }

    // concurrent fan-out: each iter serves tasks*50 requests
    {
        let mut g = c.benchmark_group("throughput/concurrent-text");
        for tasks in [4usize, 16, 50] {
            const PER_TASK: usize = 50;
            g.throughput(Throughput::Elements((tasks * PER_TASK) as u64));
            g.bench_function(format!("tasks-{tasks}x{PER_TASK}"), |b| {
                b.iter(|| {
                    rt.block_on(drive(
                        black_box(router.clone()),
                        black_box(tasks),
                        black_box(PER_TASK),
                        "/text",
                    ))
                });
            });
        }
        g.finish();
    }

    // echo under load: includes JSON parse + serialize per request
    {
        let mut g = c.benchmark_group("throughput/concurrent-echo");
        let payload = serde_json::to_vec(&serde_json::json!({"hello": "world", "n": 1})).unwrap();
        const TASKS: usize = 16;
        const PER_TASK: usize = 25;
        g.throughput(Throughput::Elements((TASKS * PER_TASK) as u64));
        g.bench_function(format!("tasks-{TASKS}x{PER_TASK}"), |b| {
            b.iter(|| {
                rt.block_on(async {
                    let mut set = tokio::task::JoinSet::new();
                    for _ in 0..TASKS {
                        let r = router.clone();
                        let p = payload.clone();
                        set.spawn(async move {
                            let mut n = 0usize;
                            for _ in 0..PER_TASK {
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
                })
            });
        });
        g.finish();
    }
}

criterion_group!(benches, bench_throughput);
criterion_main!(benches);
