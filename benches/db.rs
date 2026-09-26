//! Database-backed handler benches (divan): sqlite through toxi-db,
//! single-row fetch with full dispatch through a stateful handler.

use divan::Bencher;
use http::Method;
use std::sync::Arc;
use toxi_core::extract::State;
use toxi_core::types::BoxBody;
use toxi_core::{Result, Router, ToxiRequest, ToxiResponse};
use toxi_db::{DbPool, Model};

fn main() {
    divan::main();
}

#[derive(Model)]
#[model(table = "users")]
struct User {
    id: i64,
    name: String,
    email: String,
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt")
}

async fn setup_db(rows: i64) -> DbPool {
    use toxi_db::Database;
    let db = DbPool::connect("sqlite::memory:").await.unwrap();
    db.execute(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, email TEXT)",
    )
    .await
    .unwrap();
    for i in 0..rows {
        db.execute(&format!(
            "INSERT INTO users (id, name, email) VALUES ({i}, 'user-{i}', 'user-{i}@example.com')"
        ))
        .await
        .unwrap();
    }
    db
}

async fn get_user(State(db): State<Arc<DbPool>>) -> Result<ToxiResponse> {
    let user = User::find_by_id(&db, 1).await.map_err(|e| {
        toxi_core::Error::InternalServerError(e.to_string())
    })?;
    Ok(ToxiResponse::json(serde_json::json!({ "id": user.id })))
}

#[divan::bench]
fn db_find_by_id(bencher: Bencher) {
    let rt = rt();
    let db = rt.block_on(setup_db(100));
    bencher.bench(|| {
        divan::black_box(rt.block_on(User::find_by_id(&db, 42)))
    });
}

#[divan::bench]
fn db_handler_through_router(bencher: Bencher) {
    let rt = rt();
    let db = rt.block_on(setup_db(100));
    let mut router = Router::new();
    router.with_state(Arc::new(db));
    router.get("/users", get_user);
    bencher
        .with_inputs(|| {
            http::Request::builder()
                .method(Method::GET)
                .uri("/users")
                .body(BoxBody::default())
                .expect("bench request")
        })
        .bench_values(|r| divan::black_box(rt.block_on(router.handle(r))));
}
