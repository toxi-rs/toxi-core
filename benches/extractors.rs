//! Extractor benchmarks, which quantify the cost of obtaining typed arguments
//! from a `ToxiRequest`.
//!
//! Two levels are measured, since either level alone would present an
//! incomplete account. Micro measurements invoke `FromRequest::from_request`
//! directly, which isolates parsing cost. Integrated measurements invoke
//! `Router::handle` with a handler that requires the extractor, which
//! records the cost that a deployed handler incurs, namely routing with
//! extraction jointly.

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use http::Method;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use toxi_core::extract::{Cookies, FromRequest, Json, Path, Query, State};
use toxi_core::types::BoxBody;
use toxi_core::{Result, Router, ToxiRequest, ToxiResponse};

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

fn body_req(method: &Method, uri: &str, content_type: &str, bytes: &[u8]) -> ToxiRequest {
    use http_body_util::{BodyExt, Full};
    // The infallible error of `Full` is erased through exhaustive matching,
    // since `BoxBody` requires `hyper::Error` as its error type.
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

fn cookie_req() -> ToxiRequest {
    http::Request::builder()
        .method(Method::GET)
        .uri("/")
        .header(
            "cookie",
            "session=abc123; theme=dark; lang=en; tz=UTC; pref=compact",
        )
        .body(BoxBody::default())
        .expect("bench request")
}

// ---- integrated handlers ----
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

fn bench_extractors(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt");

    let small_json = serde_json::to_vec(&SmallBody {
        name: "alice".into(),
        email: "alice@example.com".into(),
    })
    .unwrap();
    // ~10KB payload: 200 users
    let big_json = serde_json::to_vec(&(0..200u32).map(|i| SmallBody {
        name: format!("user-{i}"),
        email: format!("user-{i}@example.com"),
    }).collect::<Vec<_>>())
    .unwrap();

    // ===== micro: pure FromRequest =====
    {
        let mut g = c.benchmark_group("extract/micro");
        g.throughput(Throughput::Bytes(small_json.len() as u64));

        g.bench_function("json-small", |b| {
            b.iter_batched(
                || {
                    body_req(
                        &Method::POST,
                        "/",
                        "application/json",
                        black_box(&small_json),
                    )
                },
                |mut r| rt.block_on(Json::<SmallBody>::from_request(&mut r)),
                BatchSize::SmallInput,
            );
        });
        g.bench_function("json-10kb", |b| {
            b.iter_batched(
                || {
                    body_req(
                        &Method::POST,
                        "/",
                        "application/json",
                        black_box(&big_json),
                    )
                },
                |mut r| {
                    rt.block_on(Json::<Vec<SmallBody>>::from_request(&mut r))
                },
                BatchSize::SmallInput,
            );
        });
        g.bench_function("query", |b| {
            b.iter_batched(
                || empty_req(&Method::GET, "/?page=3&limit=50"),
                |mut r| rt.block_on(Query::<Pagination>::from_request(&mut r)),
                BatchSize::SmallInput,
            );
        });
        g.bench_function("cookies-5", |b| {
            b.iter_batched(
                cookie_req,
                |mut r| rt.block_on(Cookies::from_request(&mut r)),
                BatchSize::SmallInput,
            );
        });
        g.finish();
    }

    // ===== integrated: router + extractor =====
    {
        let mut router = Router::new();
        router.post("/json", json_handler);
        router.get("/query", query_handler);
        router.get("/users/:id", path_handler);
        router.with_state(AppState {
            db_url: Arc::new("postgres://localhost/toxi".to_string()),
        });
        router.get("/state", state_handler);
        let router = Arc::new(router);

        let mut g = c.benchmark_group("extract/integrated");
        g.bench_function("router+json-small", |b| {
            b.iter_batched(
                || {
                    body_req(
                        &Method::POST,
                        "/json",
                        "application/json",
                        black_box(&small_json),
                    )
                },
                |r| rt.block_on(router.handle(r)),
                BatchSize::SmallInput,
            );
        });
        g.bench_function("router+query", |b| {
            b.iter_batched(
                || empty_req(&Method::GET, "/query?page=3&limit=50"),
                |r| rt.block_on(router.handle(r)),
                BatchSize::SmallInput,
            );
        });
        g.bench_function("router+path", |b| {
            b.iter_batched(
                || empty_req(&Method::GET, "/users/42"),
                |r| rt.block_on(router.handle(r)),
                BatchSize::SmallInput,
            );
        });
        g.bench_function("router+state", |b| {
            b.iter_batched(
                || empty_req(&Method::GET, "/state"),
                |r| rt.block_on(router.handle(r)),
                BatchSize::SmallInput,
            );
        });
        g.finish();
    }
}

criterion_group!(benches, bench_extractors);
criterion_main!(benches);
