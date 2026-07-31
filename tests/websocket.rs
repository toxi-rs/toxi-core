use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::oneshot;

/// Build an echo-style WebSocket handler that reflects incoming messages.
fn build_echo_router() -> toxi_core::router::Router {
    use toxi_core::router::Router;
    use toxi_core::extract::WebSocketUpgrade;

    let mut router = Router::new();

    router.get("/echo", |ws: WebSocketUpgrade| async move {
        Ok(ws.on_upgrade(|upgraded, _extensions| async move {
            let ws_stream = tokio_tungstenite::WebSocketStream::from_raw_socket(
                hyper_util::rt::TokioIo::new(upgraded),
                tokio_tungstenite::tungstenite::protocol::Role::Server,
                None,
            )
            .await;

            let (mut write, mut read) = ws_stream.split();
            while let Some(msg) = read.next().await {
                let msg = msg.expect("ws read error");
                let is_close = msg.is_close();
                if msg.is_text() || msg.is_binary() {
                    if write.send(msg).await.is_err() {
                        break;
                    }
                }
                if is_close {
                    break;
                }
            }
        }))
    });

    router
}

async fn start_server_handle(service: toxi_core::router::Router) -> (SocketAddr, oneshot::Sender<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bound_addr = listener.local_addr().unwrap();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                result = listener.accept() => {
                    let (stream, _) = result.unwrap();
                    let io = hyper_util::rt::TokioIo::new(stream);
                    let service = service.clone();
                    let adapter = toxi_core::server::BodyAdapter::new(service);

                    tokio::spawn(async move {
                        let hyper_service = hyper_util::service::TowerToHyperService::new(adapter);
                        let _ = hyper::server::conn::http1::Builder::new()
                            .serve_connection(io, hyper_service)
                            .with_upgrades()
                            .await;
                    });
                }
                _ = &mut shutdown_rx => break,
            }
        }
    });

    (bound_addr, shutdown_tx)
}

#[tokio::test]
async fn test_ws_echo_roundtrip() {
    let _ = env_logger::try_init();

    let router = build_echo_router();
    let (addr, _shutdown) = start_server_handle(router).await;

    let ws_url = format!("ws://{}/echo", addr);
    let (mut ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS connect failed");

    let msg = "hello websocket";
    ws_stream
        .send(tokio_tungstenite::tungstenite::Message::Text(msg.into()))
        .await
        .expect("send failed");

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let response = ws_stream
        .next()
        .await
        .expect("no response")
        .expect("response error");

    assert_eq!(response.to_string(), msg);

    ws_stream
        .close(None)
        .await
        .expect("close failed");
}

#[tokio::test]
async fn test_ws_concurrent_connections() {
    let _ = env_logger::try_init();

    let router = build_echo_router();
    let (addr, _shutdown) = start_server_handle(router).await;

    let ws_url = format!("ws://{}/echo", addr);
    let mut handles = vec![];

    let counter = Arc::new(AtomicU16::new(0));

    for i in 0..5u16 {
        let url = ws_url.clone();
        let c = counter.clone();
        handles.push(tokio::spawn(async move {
            let (mut ws, _) = tokio_tungstenite::connect_async(&url)
                .await
                .expect("connect failed");

            let msg = format!("concurrent_{}", i);
            ws.send(tokio_tungstenite::tungstenite::Message::Text(msg.clone().into()))
                .await
                .expect("send failed");

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            let resp = ws
                .next()
                .await
                .expect("no response")
                .expect("response error");

            assert_eq!(resp.to_string(), msg);
            c.fetch_add(1, Ordering::SeqCst);

            ws.close(None).await.expect("close failed");
        }));
    }

    for h in handles {
        h.await.expect("task panicked");
    }

    assert_eq!(counter.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn test_ws_binary_message() {
    let _ = env_logger::try_init();

    let router = build_echo_router();
    let (addr, _shutdown) = start_server_handle(router).await;

    let ws_url = format!("ws://{}/echo", addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("connect failed");

    let binary = vec![0u8, 1, 2, 3, 255];
    ws.send(tokio_tungstenite::tungstenite::Message::Binary(binary.clone().into()))
        .await
        .expect("send failed");

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let resp = ws
        .next()
        .await
        .expect("no response")
        .expect("response error");

    match resp {
        tokio_tungstenite::tungstenite::Message::Binary(data) => {
            assert_eq!(data.to_vec(), binary);
        }
        other => panic!("expected Binary, got {:?}", other),
    }

    ws.close(None).await.expect("close failed");
}

#[tokio::test]
async fn test_ws_close_frame() {
    let _ = env_logger::try_init();

    let router = build_echo_router();
    let (addr, _shutdown) = start_server_handle(router).await;

    let ws_url = format!("ws://{}/echo", addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("connect failed");

    ws.close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
        code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal,
        reason: "bye".into(),
    }))
    .await
    .expect("close failed");

    let close_received = ws
        .next()
        .await
        .expect("expected close frame");

    assert!(close_received.is_err() || close_received.unwrap().is_close());
}

#[tokio::test]
async fn test_ws_rejects_non_upgrade_requests() {
    let _ = env_logger::try_init();

    let router = build_echo_router();
    let (addr, _shutdown) = start_server_handle(router).await;

    let http_url = format!("http://{}/echo", addr);
    let client = reqwest::Client::new();
    let resp = client.get(&http_url).send().await.expect("http get failed");

    assert_eq!(resp.status(), 400);
}
