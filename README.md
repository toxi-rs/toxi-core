# toxi-core

HTTP server, router, and request extractors. Everything else in Toxi
builds on this crate.

```toml
[dependencies]
toxi-core = "3"
```

```rust
use toxi_core::{Application, Request, Response, Result};

async fn hello(_req: Request) -> Result<Response> {
    Ok(Response::text("Hello, Toxi!"))
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut app = Application::new(toxi_config::Config::default());
    app.router_mut().get("/", hello);
    app.run().await
}
```

`HEAD` falls back to `GET` routes. A path that exists under another
method returns `MethodNotAllowed` instead of `NotFound`.
