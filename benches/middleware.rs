//! Middleware composition cost (divan): a passthrough tower layer at
//! depths 0, 1, and 5 around an identical route.

use divan::Bencher;
use http::Method;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use toxi_core::router::Endpoint;
use toxi_core::types::BoxBody;
use toxi_core::{Result, Router, ToxiRequest, ToxiResponse};

fn main() {
    divan::main();
}

async fn ok_handler() -> Result<ToxiResponse> {
    Ok(ToxiResponse::text("ok"))
}

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
    let mut builder = Router::new();
    for _ in 0..depth {
        builder = builder.layer(PassLayer);
    }
    builder.get("/", ok_handler);
    Arc::new(builder)
}

#[divan::bench(args = [0, 1, 5])]
fn passthrough_depth(bencher: Bencher, depth: usize) {
    let router = router_with_layers(depth);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt");
    bencher
        .with_inputs(|| {
            http::Request::builder()
                .method(Method::GET)
                .uri("/")
                .body(BoxBody::default())
                .expect("bench request")
        })
        .bench_values(|r| divan::black_box(rt.block_on(router.handle(r))));
}
