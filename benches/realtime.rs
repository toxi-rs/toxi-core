//! Realtime benches: broadcast fan-out with pub/sub publish.

use divan::Bencher;
use std::sync::Arc;
use toxi_realtime::{Event, Message, PubSub, WebSocketConnection, WebSocketManager};

fn main() {
    divan::main();
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt")
}

async fn manager_with(n: usize) -> WebSocketManager {
    let manager = WebSocketManager::new();
    for _ in 0..n {
        let (conn, _rx) = WebSocketConnection::new(None);
        manager.add_connection(Arc::new(conn)).await;
    }
    manager
}

#[divan::bench(args = [1, 10, 100])]
fn broadcast_fanout(bencher: Bencher, n: usize) {
    let rt = rt();
    let manager = rt.block_on(manager_with(n));
    bencher.bench(|| {
        divan::black_box(
            rt.block_on(manager.broadcast(Message::text("hello subscribers"))),
        )
    });
}

#[divan::bench]
fn pubsub_publish(bencher: Bencher) {
    let rt = rt();
    let pubsub = PubSub::new();
    bencher.bench(|| {
        divan::black_box(rt.block_on(
            pubsub.publish("news", Event::message("news", serde_json::json!({"n": 1}))),
        ))
    });
}
