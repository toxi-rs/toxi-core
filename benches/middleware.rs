//! Middleware benchmarks, which quantify per-layer composition cost.
//!
//! `Router::layer` wraps each endpoint in a tower layer, with the
//! consequence that depth contributes dispatch overhead independently of
//! substantive middleware logic such as logging or authentication. The
//! present benches employ a minimal passthrough layer at depths of zero,
//! one, and five, which isolates composition cost and thereby establishes
//! the budget within which substantive layers must operate.

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use http::Method;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use toxi_core::router::Endpoint;
use toxi_core::types::BoxBody;
use toxi_core::{Result, Router, ToxiRequest, ToxiResponse};

async fn ok_handler() -> Result<ToxiResponse> {
    Ok(ToxiResponse::text("ok"))
}

fn req() -> ToxiRequest {
    http::Request::builder()
        .method(Method::GET)
        .uri("/")
        .body(BoxBody::default())
        .expect("bench request")
}

// Minimal passthrough: does nothing but forward, so we measure
// the wrapping/dispatch cost, not middleware logic.
struct Pass<S>(S);
impl<S: Endpoint> Endpoint for Pass<S> {
    fn call(&self, r: ToxiRequest) -> Pin<Box<dyn Future<Output = Result<ToxiResponse>> + Send>> {
        self.0.call(r)
    }
}

#[derive(Clone, Copy)]
struct PassLayer;
impl<S> tower::Layer<S> for PassLayer {
    type Service = Pass<S>;
    fn layer(&self, inner: S) -> Self::Service {
        Pass(inner)
    }
}

fn router_with_layers(depth: usize) -> Arc<Router> {
    // Layers must be added BEFORE routes: add_route snapshots the middleware vec.
    let mut builder = Router::new();
    for _ in 0..depth {
        builder = builder.layer(PassLayer);
    }
    builder.get("/", ok_handler);
    Arc::new(builder)
}

fn bench_middleware(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt");

    let mut g = c.benchmark_group("middleware/passthrough");
    for depth in [0usize, 1, 5] {
        let router = router_with_layers(depth);
        // warm the "middleware_called" path once so first-iter effects don't skew
        let _ = rt.block_on(router.handle(req()));
        g.bench_function(format!("{depth}-layers"), |b| {
            b.iter_batched(
                || black_box(req()),
                |r| rt.block_on(router.handle(r)),
                BatchSize::SmallInput,
            );
        });
    }
    g.finish();
}

criterion_group!(benches, bench_middleware);
criterion_main!(benches);
