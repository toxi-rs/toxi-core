//! Queue benches: enqueue with drain a batch of jobs.

use divan::Bencher;
use serde::{Deserialize, Serialize};
use toxi_queue::{job::JobWrapper, Job, Queue};

fn main() {
    divan::main();
}

#[derive(Serialize, Deserialize)]
struct BenchJob {
    n: u32,
}

#[async_trait::async_trait]
impl Job for BenchJob {
    async fn perform(&self) -> toxi_queue::Result<()> {
        Ok(())
    }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt")
}

#[divan::bench]
fn queue_enqueue_drain_50(bencher: Bencher) {
    let rt = rt();
    bencher.bench(|| {
        divan::black_box(rt.block_on(async {
            let queue = Queue::memory();
            for n in 0..50u32 {
                queue
                    .enqueue(JobWrapper::new(&BenchJob { n }).unwrap())
                    .await
                    .unwrap();
            }
            let mut drained = 0u32;
            while queue.dequeue().await.unwrap().is_some() {
                drained += 1;
            }
            drained
        }))
    });
}
