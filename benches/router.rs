//! Router benchmarks, which exercise the dispatch path of `toxi-core`.
//!
//! The router maintains one compiled regular expression per route within a
//! per-method vector, and it matches each incoming request through a linear
//! scan over that vector. The present benches render the cost of that scan
//! observable, since the cost cannot be inferred reliably from inspection
//! without measurement under controlled route counts.
//!
//! The cases are formulated as follows. Static hits are measured at the
//! first route and at the last route, which correspond to best-case and
//! worst-case scan positions. Parameterised paths of the form `/users/:id`,
//! multi-parameter paths, and wildcard paths are measured separately, since
//! capture extraction contributes cost beyond pattern matching. Miss paths
//! are measured for 404 responses, which require a full scan with a
//! subsequent allowed-methods scan, and for 405 responses, which require
//! cross-method matching. Scaling is measured at 10, 100, and 500 static
//! routes with dispatch to the final route, in order to establish whether
//! growth remains approximately linear.

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use http::Method;
use std::sync::Arc;
use toxi_core::types::BoxBody;
use toxi_core::{Result, Router, ToxiRequest, ToxiResponse};

async fn ok_handler() -> Result<ToxiResponse> {
    Ok(ToxiResponse::text("ok"))
}

async fn param_handler() -> Result<ToxiResponse> {
    Ok(ToxiResponse::text("param-ok"))
}

fn req(method: &Method, path: &str) -> ToxiRequest {
    http::Request::builder()
        .method(method.clone())
        .uri(path)
        .body(BoxBody::default())
        .expect("bench request")
}

fn router_with(n_static: usize) -> Arc<Router> {
    let mut r = Router::new();
    for i in 0..n_static {
        // leak per-route paths into owned Strings; Router::get copies the pattern
        let p = format!("/static-{i}");
        // need a distinct fn per route? No — same handler is fine.
        r.get(&p, ok_handler);
    }
    // dynamic routes always present at the end (worst-case scan position)
    r.get("/users/:id", param_handler);
    r.get("/users/:user_id/posts/:post_id", param_handler);
    r.get("/files/*", param_handler);
    r.post("/users", ok_handler);
    Arc::new(r)
}

fn bench_router(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt");

    // --- hit position: first route vs last route (10 routes) ---
    {
        let router = router_with(10);
        let mut g = c.benchmark_group("router/static-hit");
        g.bench_function("first-of-10", |b| {
            b.iter_batched(
                || req(&Method::GET, "/static-0"),
                |r| rt.block_on(router.handle(r)),
                BatchSize::SmallInput,
            );
        });
        g.bench_function("last-of-10", |b| {
            b.iter_batched(
                || req(&Method::GET, "/files/some/deep/path"),
                |r| rt.block_on(router.handle(r)),
                BatchSize::SmallInput,
            );
        });
        g.finish();
    }

    // --- dynamic segments ---
    {
        let router = router_with(10);
        let mut g = c.benchmark_group("router/dynamic");
        for (name, path) in [
            ("param", "/users/42"),
            ("multi-param", "/users/7/posts/99"),
            ("wildcard", "/files/a/b/c"),
        ] {
            g.bench_function(name, |b| {
                b.iter_batched(
                    || req(&Method::GET, black_box(path)),
                    |r| rt.block_on(router.handle(r)),
                    BatchSize::SmallInput,
                );
            });
        }
        g.finish();
    }

    // --- miss / wrong-method: the expensive paths ---
    {
        let router = router_with(100);
        let mut g = c.benchmark_group("router/miss");
        g.bench_function("404-miss-100-routes", |b| {
            b.iter_batched(
                || req(&Method::GET, "/nope/not-here"),
                |r| rt.block_on(router.handle(r)),
                BatchSize::SmallInput,
            );
        });
        g.bench_function("405-wrong-method", |b| {
            // path exists for GET, we send DELETE -> MethodNotAllowed scan
            b.iter_batched(
                || req(&Method::DELETE, "/static-50"),
                |r| rt.block_on(router.handle(r)),
                BatchSize::SmallInput,
            );
        });
        g.bench_function("options-preflight", |b| {
            b.iter_batched(
                || req(&Method::OPTIONS, "/static-1"),
                |r| rt.block_on(router.handle(r)),
                BatchSize::SmallInput,
            );
        });
        g.finish();
    }

    // --- scaling: hit the LAST route with N statics ---
    {
        let mut g = c.benchmark_group("router/scale-last-hit");
        for n in [10usize, 100, 500] {
            let router = router_with(n);
            g.bench_function(format!("{n}-routes"), |b| {
                b.iter_batched(
                    || req(&Method::GET, "/files/x"),
                    |r| rt.block_on(router.handle(r)),
                    BatchSize::SmallInput,
                );
            });
        }
        g.finish();
    }
}

criterion_group!(benches, bench_router);
criterion_main!(benches);
