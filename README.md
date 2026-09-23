# `toxi-core`

## 1. Background

`toxi-core` constitutes the HTTP kernel of the Toxi framework. It provides
routing with path parameters and wildcard segments, an asynchronous server
that is integrated with hyper, strongly typed request and response
abstractions, and a set of extractors through which handlers obtain typed
data from incoming requests. Although the broader Toxi workspace distributes
persistence, authentication, caching, and related concerns across companion
crates, `toxi-core` is sufficient for a minimal API server, and it is the
component upon which all such extensions depend.

## 2. Installation

The crate is declared as a dependency in the conventional manner, with
version 3.1.0 constituting the current baseline for the examples that
follow.

```toml
[dependencies]
toxi-core = "3.1.0"
```

## 3. Components

The principal components, each of which is documented at its definition
site with executable examples where behaviour admits concise illustration,
are as follows.

- `Router`, which maps HTTP methods with path patterns to handler endpoints,
  and which supports parameter segments of the form `/users/:id` together
  with wildcard segments.
- `Server`, which binds a TCP listener, adapts incoming hyper requests, and
  dispatches them to the configured service over HTTP/1.1 or HTTP/2.
- `ToxiRequest` with `ToxiResponse`, which constitute the request and
  response types that handlers exchange.
- Extractors, namely `Path`, `Query`, `Json`, `Form`, `State`, `Cookies`,
  `Body`, and `WebSocketUpgrade`, each of which implements `FromRequest`
  in order to render its extraction logic explicit and testable.

## 4. Illustrative usage

The following example, which responds with plain text on the root path,
is minimal in the sense that it exercises routing and response
construction without extraction or middleware.

```rust
use toxi_core::{Application, Request, Response, Result};

async fn hello(_req: Request) -> Result<Response> {
    Ok(Response::text("Hello, Toxi!"))
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = toxi_config::Config::default();
    let mut app = Application::new(config);
    app.router_mut().get("/", hello);
    app.run().await
}
```

An alternative construction through `Router::new` with
`Server::new(router).listen(addr)` remains available, and it operates
identically under the stated configuration, since `Application` coordinates
the same primitives.

## 5. Behavioural notes

`HEAD` requests fall back to matching `GET` routes, because hyper strips
the body for head responses, with the consequence that separate head
handlers are not required for standard retrieval endpoints. Where a path
exists for methods other than the requested one, the router returns
`MethodNotAllowed` with an enumeration of permitted methods, rather than
`NotFound`, in order to distinguish misconfiguration of the method from
absence of the resource.

## 6. Scope and limitations

The documentation confines itself to dispatch, extraction, and response
behaviour as implemented in this crate. Performance characteristics,
which depend upon route count and payload size in ways that require
measurement rather than assertion, are addressed in the benchmark suite
under `benches`, to which the reader is referred for quantified baselines
with graphical reporting.
