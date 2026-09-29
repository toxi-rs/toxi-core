# Changelog — `toxi-core`

Per-crate history extracted from the monolith changelog
([meshackbahati/toxi](https://github.com/meshackbahati/toxi/blob/main/CHANGELOG.md)),
which remains the full documentation hub.

## 3.1.9

- **toxi-core** (`3.1.9`): idle keep-alive connections reap after a
  75-second header read timeout. Without it, connection counts
  accumulate without bound across load generations and memory grows
  with them. No public API change.

## [3.1.10] - 2026-09-28

- **toxi-core** (`3.1.10`): router dispatch borrows the request path
  instead of copying it per request and answers misses without a
  second method scan; CORS headers resolve once at layer build and
  share across responses; `Path` extraction deserializes from a borrow
  instead of cloning the parameter map; cookie map pre-sizes from the
  separator count. No public API change.

## 3.1.8

- **toxi-core** (`3.1.8`): route table shared by clone instead of
  reallocated per request. `Service::call` cloned the entire routes map
  on every request; the table now lives behind `Arc` with copy-on-write
  mutation, so per-request clones cost one atomic increment. No public
  API change.

## 3.1.7

- **toxi-core** (`3.1.7`): server I/O path tuning. Explicit 1024 listen
  backlog via `TcpSocket` instead of the default bind backlog,
  `TCP_NODELAY` on accepted connections, 32 KB hyper read-buffer cap,
  and allocation-free WebSocket upgrade detection. No public API change.

## 3.1.6

- **toxi-core** (`3.1.6`): TLS configuration installs the ring crypto
  provider explicitly. rustls 0.23 selects no provider by default, with
  the consequence that every HTTPS handshake panicked at runtime. All
  `SecureServer` users are covered by the single install.

## 3.1.5

- **toxi-core** (`3.1.5`): router dispatch uses a boolean `is_match` fast
  path for routes without parameters instead of full capture extraction.
  Measured improvements: −45% at 100 routes, −38% at 500 routes, −68% on
  the 404 path, with per-route marginal cost restored from 0.24 µs to
  0.09 µs (linear scaling). Behavior is unchanged: the skipped branch
  stored no parameters for such routes.
- **toxi-core** (`3.1.5`): `Json` extractor parses from a contiguous buffer
  with `from_slice` instead of byte-wise `from_reader`. Measured −79% on
  10 KB payloads; direction confirmed with p = 0.00, exact magnitude
  pending quiet-box rerun.

## 3.1.4

- **toxi-core** `3.1.2` and `3.1.3` — these versions contained a broken BodyAdapter that
  intercepted all WebSocket upgrades with a no-op handler, silently dropping all WS
  connections. Users should upgrade directly to `3.1.4`.

## 3.1.4

- **toxi-core**: BodyAdapter no longer intercepts WebSocket upgrade requests with a default no-op
  handler. WS upgrades now pass through to the router, allowing route handlers (agent,
  sandbox terminal) to receive the `OnUpgrade` future from hyper's request extensions and
  manage their own connection lifecycle. Previously, all WS connections were silently dropped.
- **toxi-core**: Removed `hyper_tungstenite` default WS upgrade path from BodyAdapter — route
  handlers using `WebSocketUpgrade` extractor now correctly receive the upgrade future.
- **toxi-core**: Added structured logging via the `log` crate — BodyAdapter now emits
  `info!`/`warn!`/`error!` messages for WebSocket upgrade attempts, route dispatch, and
  connection lifecycle events.

## 3.1.4

- **toxi-core**: WebSocket integration tests — `tests/websocket.rs` covers:
  - WS upgrade handshake via `WebSocketUpgrade` extractor
  - Message round-trip (echo server)
  - Connection close handling
  - Multiple concurrent WS connections
  - Route-based WS dispatch
- **toxi-core**: `tracing` optional dependency for structured log output (default-off feature).

## 3.1.4

- **toxi-core**: Diagnostic messages now use `log` crate macros (`info!`, `warn!`, `error!`)
  instead of raw `println!`/`eprintln!`.

## 3.1.0

- **toxi-core** serves HTTP/1.1 connections using the low-level hyper 1.x library. In hyper 1.x, http1::Builder::new().serve_connection(io, service) does not support or honor WebSocket/HTTP upgrades (yielding Error(User(ManualUpgrade)) when an upgrade is attempted).

## [3.1.11] - 2026-09-28

### Added
- `RequestExt::json::<T>()` parses the body as JSON, so handlers map
  request bodies without importing a JSON library.
