# Benches

Divan suites in this directory. One file per area; results are
collected in `benchmarks/` at the repository root.

- `router.rs` — dispatch: statics, params, wildcards, miss paths,
  route-count scaling.
- `extractors.rs` — `FromRequest` micro cost with integrated dispatch.
- `responses.rs` — response construction and serialization.
- `middleware.rs` — passthrough layer depth.
- `throughput.rs` — concurrent dispatch fan-out.
