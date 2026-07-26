# toxi-core

Core HTTP server, router, request/response types, and extractors for Toxi.

## Installation

```toml
[dependencies]
toxi-core = "2.3.4"
```

## Key Components

- `Router`: method/path routing with path params and wildcard support.
- `Server`: Hyper-based async server integration for Toxi services.
- `ToxiRequest` / `ToxiResponse`: request/response core types.
- Extractors: `Path`, `Query`, `Json`, `Form`, `State`, `Cookies`, `Body`, `WebSocketUpgrade`.

## Basic Example

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

> **Alternative**: The manual approach using `Router::new()` + `Server::new(router).listen(addr)` works identically under the hood.

## Notes

- `HEAD` requests automatically fall back to matching `GET` routes.
- If a path exists for another method, the router returns `MethodNotAllowed`.
