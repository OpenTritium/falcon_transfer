use std::{
    sync::{Arc, atomic::AtomicUsize},
    time::Duration,
};

use falcon_transfer::{
    inbound::{Inbound, Msg, split_group},
    link::Uid,
};
use futures::SinkExt;
use tokio::time::sleep;

#[derive(Default)]
struct BenchMetrics {
    sent: AtomicUsize,
    received: AtomicUsize,
    cached: AtomicUsize,
}

#[tokio::main]
async fn main() {
    let metrics = Arc::new(BenchMetrics::default());
    let (tx, rx) = split_group().await.unwrap();
    let (_inbound, mut rx) = Inbound::receiving(rx).await;
    tokio::spawn({
        let metrics = metrics.clone();
        async move {
            // 请确保至少有一个接口
            while let Some((msg, _)) = rx.recv().await {
                metrics
                    .cached
                    .fetch_update(
                        std::sync::atomic::Ordering::Relaxed,
                        std::sync::atomic::Ordering::Relaxed,
                        |_| Some(rx.len()),
                    )
                    .unwrap();
                drop(msg);
                metrics
                    .received
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    });
    for (addr, mut sink) in tx.into_iter() {
        let metrics = metrics.clone();
        tokio::spawn(async move {
            loop {
                let msg = Msg::Discovery {
                    host: Uid::random(),
                    remote: addr.clone(),
                };
                sink.send((msg, addr.into())).await.unwrap();
                metrics
                    .sent
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        });
    }
    sleep(Duration::from_secs(8)).await;
    println!(
        "Sent: {}",
        metrics.sent.load(std::sync::atomic::Ordering::Relaxed)
    );
    println!(
        "Received: {}",
        metrics.received.load(std::sync::atomic::Ordering::Relaxed)
    );
    println!(
        "Cached: {}",
        metrics.cached.load(std::sync::atomic::Ordering::Relaxed)
    );
}
