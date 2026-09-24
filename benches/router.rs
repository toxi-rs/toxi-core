//! Router dispatch benches (divan).
//!
//! Input construction runs in `with_inputs` and is excluded from the
//! measurement, so each figure records `Router::handle` with trivial
//! handler execution jointly.

use divan::Bencher;
use http::Method;
use std::sync::Arc;
use toxi_core::types::BoxBody;
use toxi_core::{Result, Router, ToxiRequest, ToxiResponse};

fn main() {
    divan::main();
}

async fn ok_handler() -> Result<ToxiResponse> {
    Ok(ToxiResponse::text("ok"))
}

async fn param_handler() -> Result<ToxiResponse> {
    Ok(ToxiResponse::text("param-ok"))
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt")
}

fn req(method: &Method, path: &str) -> ToxiRequest {
    http::Request::builder()
        .method(method.clone())
        .uri(path)
        .body(BoxBody::default())
        .expect("bench request")
}

fn leaked(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn router_with(n_static: usize) -> Arc<Router> {
    let mut r = Router::new();
    for i in 0..n_static {
        r.get(leaked(format!("/static-{i}")), ok_handler);
    }
    r.get("/users/:id", param_handler);
    r.get("/users/:user_id/posts/:post_id", param_handler);
    r.get("/files/*", param_handler);
    r.post("/users", ok_handler);
    Arc::new(r)
}

#[divan::bench]
fn static_first_of_10(bencher: Bencher) {
    let router = router_with(10);
    let rt = rt();
    bencher.with_inputs(|| req(&Method::GET, "/static-0")).bench_values(|r| {
        divan::black_box(rt.block_on(router.handle(r)))
    });
}

#[divan::bench]
fn static_last_of_10(bencher: Bencher) {
    let router = router_with(10);
    let rt = rt();
    bencher
        .with_inputs(|| req(&Method::GET, "/files/some/deep/path"))
        .bench_values(|r| divan::black_box(rt.block_on(router.handle(r))));
}

#[divan::bench]
fn dynamic_param(bencher: Bencher) {
    let router = router_with(10);
    let rt = rt();
    bencher
        .with_inputs(|| req(&Method::GET, "/users/42"))
        .bench_values(|r| divan::black_box(rt.block_on(router.handle(r))));
}

#[divan::bench]
fn dynamic_multi_param(bencher: Bencher) {
    let router = router_with(10);
    let rt = rt();
    bencher
        .with_inputs(|| req(&Method::GET, "/users/7/posts/99"))
        .bench_values(|r| divan::black_box(rt.block_on(router.handle(r))));
}

#[divan::bench]
fn dynamic_wildcard(bencher: Bencher) {
    let router = router_with(10);
    let rt = rt();
    bencher
        .with_inputs(|| req(&Method::GET, "/files/a/b/c"))
        .bench_values(|r| divan::black_box(rt.block_on(router.handle(r))));
}

#[divan::bench]
fn miss_404_100_routes(bencher: Bencher) {
    let router = router_with(100);
    let rt = rt();
    bencher
        .with_inputs(|| req(&Method::GET, "/nope/not-here"))
        .bench_values(|r| divan::black_box(rt.block_on(router.handle(r))));
}

#[divan::bench]
fn miss_405_wrong_method(bencher: Bencher) {
    let router = router_with(100);
    let rt = rt();
    bencher
        .with_inputs(|| req(&Method::DELETE, "/static-50"))
        .bench_values(|r| divan::black_box(rt.block_on(router.handle(r))));
}

#[divan::bench]
fn miss_options_preflight(bencher: Bencher) {
    let router = router_with(100);
    let rt = rt();
    bencher
        .with_inputs(|| req(&Method::OPTIONS, "/static-1"))
        .bench_values(|r| divan::black_box(rt.block_on(router.handle(r))));
}

#[divan::bench(args = [10, 100, 500])]
fn scale_last_hit(bencher: Bencher, n: usize) {
    let router = router_with(n);
    let rt = rt();
    bencher
        .with_inputs(|| req(&Method::GET, "/files/x"))
        .bench_values(|r| divan::black_box(rt.block_on(router.handle(r))));
}
