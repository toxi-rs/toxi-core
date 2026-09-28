//! Auth benches: JWT creation with verification roundtrip.

use divan::Bencher;

fn main() {
    divan::main();
}

const SECRET: &str = "bench-secret-key-that-is-long-enough";

#[divan::bench]
fn jwt_create(bencher: Bencher) {
    bencher.bench(|| {
        divan::black_box(
            toxi_auth::create_token("user-42".to_string(), SECRET, 3600).unwrap(),
        )
    });
}

#[divan::bench]
fn jwt_roundtrip(bencher: Bencher) {
    bencher.bench(|| {
        let token =
            toxi_auth::create_token("user-42".to_string(), SECRET, 3600).unwrap();
        divan::black_box(toxi_auth::verify_token(&token, SECRET).unwrap())
    });
}
