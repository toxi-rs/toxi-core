//! Response construction benches (divan). All constructors are
//! synchronous; serialization dominates the JSON cases.

use divan::Bencher;
use serde::Serialize;
use toxi_core::{Error, ToxiResponse};

fn main() {
    divan::main();
}

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

fn large_rows() -> Vec<Row> {
    (0..1000)
        .map(|i| Row {
            id: i,
            name: format!("user-{i}"),
            email: format!("user-{i}@example.com"),
            active: i % 2 == 0,
        })
        .collect()
}

#[divan::bench]
fn json_small(bencher: Bencher) {
    let small = Small {
        status: "ok",
        count: 42,
    };
    bencher.bench(|| divan::black_box(ToxiResponse::json(&small)));
}

#[divan::bench]
fn json_medium(bencher: Bencher) {
    let medium = serde_json::json!({
        "user": {"id": 7, "name": "alice", "roles": ["admin", "editor"]},
        "page": {"number": 3, "size": 50, "total": 1200},
        "items": [1, 2, 3, 4, 5],
    });
    bencher.bench(|| divan::black_box(ToxiResponse::json(&medium)));
}

#[divan::bench]
fn json_large_1000_rows(bencher: Bencher) {
    let large = large_rows();
    bencher.bench(|| divan::black_box(ToxiResponse::json(&large)));
}

#[divan::bench]
fn text_1k(bencher: Bencher) {
    let body = "x".repeat(1024);
    bencher.bench(|| divan::black_box(ToxiResponse::text(body.clone())));
}

#[divan::bench]
fn html_page(bencher: Bencher) {
    let page = "<html><body><h1>Hello</h1><p>toxi</p></body></html>".repeat(20);
    bencher.bench(|| divan::black_box(ToxiResponse::html(page.clone())));
}

#[divan::bench]
fn ok_empty(bencher: Bencher) {
    bencher.bench(|| divan::black_box(ToxiResponse::ok()));
}

#[divan::bench]
fn error_to_404(bencher: Bencher) {
    bencher.bench(|| {
        let r: ToxiResponse = divan::black_box(Error::NotFound("missing".to_string())).into();
        divan::black_box(r)
    });
}
