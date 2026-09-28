//! Cache benches: in-memory set, get, and exists.

use divan::Bencher;
use std::time::Duration;
use toxi_cache::{Cache, MemoryCache};

fn main() {
    divan::main();
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt")
}

#[divan::bench]
fn cache_set(bencher: Bencher) {
    use std::sync::atomic::{AtomicU64, Ordering};
    let rt = rt();
    let cache = MemoryCache::new();
    let i = AtomicU64::new(0);
    bencher.bench(|| {
        let n = i.fetch_add(1, Ordering::Relaxed);
        divan::black_box(rt.block_on(cache.set(
            &format!("k{n}"),
            &"some-value",
            Some(Duration::from_secs(60)),
        )))
    });
}

#[divan::bench]
fn cache_get_hit(bencher: Bencher) {
    let rt = rt();
    let cache = MemoryCache::new();
    rt.block_on(cache.set("hot", &"some-value", Some(Duration::from_secs(60))))
        .unwrap();
    bencher.bench(|| {
        divan::black_box(rt.block_on(cache.get::<String>("hot")))
    });
}

#[divan::bench]
fn cache_exists(bencher: Bencher) {
    let rt = rt();
    let cache = MemoryCache::new();
    rt.block_on(cache.set("hot", &"some-value", Some(Duration::from_secs(60))))
        .unwrap();
    bencher.bench(|| divan::black_box(rt.block_on(cache.exists("hot"))));
}
