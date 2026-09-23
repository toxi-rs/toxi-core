//! Response benchmarks, which quantify construction with serialization cost.
//!
//! All constructors are synchronous, with the consequence that plain
//! iteration without asynchronous execution is appropriate. The JSON cases
//! dominate through `serde_json::to_vec`, whereas text and HTML responses
//! record principally allocation with header preparation, which the suite
//! retains in order to prevent regression in paths that are assumed to be
//! negligible without evidence.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use serde::Serialize;
use toxi_core::{Error, ToxiResponse};

#[derive(Serialize)]
struct Small {
    status: &'static str,
    count: u32,
}

#[derive(Serialize)]
struct Row {
    id: u64,
    name: String,
    email: String,
    active: bool,
}

fn bench_response(c: &mut Criterion) {
    let small = Small {
        status: "ok",
        count: 42,
    };
    let medium = serde_json::json!({
        "user": {"id": 7, "name": "alice", "roles": ["admin", "editor"]},
        "page": {"number": 3, "size": 50, "total": 1200},
        "items": [1, 2, 3, 4, 5],
    });
    // ~large: 1000 rows, representative of a list endpoint
    let large: Vec<Row> = (0..1000)
        .map(|i| Row {
            id: i,
            name: format!("user-{i}"),
            email: format!("user-{i}@example.com"),
            active: i % 2 == 0,
        })
        .collect();
    let large_json_len = serde_json::to_vec(&large).unwrap().len() as u64;

    let text_1k = "x".repeat(1024);
    let html_page = "<html><body><h1>Hello</h1><p>toxi</p></body></html>".repeat(20);

    {
        let mut g = c.benchmark_group("response/json");
        g.bench_function("small-struct", |b| {
            b.iter(|| ToxiResponse::json(black_box(&small)))
        });
        g.bench_function("medium-value", |b| {
            b.iter(|| ToxiResponse::json(black_box(&medium)))
        });
        g.throughput(Throughput::Bytes(large_json_len));
        g.bench_function("large-1000-rows", |b| {
            b.iter(|| ToxiResponse::json(black_box(&large)))
        });
        g.finish();
    }

    {
        let mut g = c.benchmark_group("response/body");
        g.bench_function("text-1k", |b| {
            b.iter(|| ToxiResponse::text(black_box(text_1k.clone())))
        });
        g.bench_function("html-page", |b| {
            b.iter(|| ToxiResponse::html(black_box(html_page.clone())))
        });
        g.bench_function("ok-empty", |b| b.iter(ToxiResponse::ok));
        g.bench_function("error-to-response-404", |b| {
            b.iter(|| {
                let r: ToxiResponse =
                    black_box(Error::NotFound("missing".to_string())).into();
                r
            })
        });
        g.finish();
    }
}

criterion_group!(benches, bench_response);
criterion_main!(benches);
