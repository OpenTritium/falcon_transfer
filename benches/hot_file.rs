use bytes::Bytes;
use criterion::{Criterion, SamplingMode, criterion_group, criterion_main};
use falcon_transfer::hot_file::{HotFile, MultiInterval};
use falcon_transfer::interval;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::sync::Arc;
use tempfile::tempdir;
use tokio::runtime::Runtime;

fn generate_test_data(rng: &mut StdRng, size: usize) -> Bytes {
    let mut data = vec![0u8; size];
    rng.fill(&mut data[..]);
    Bytes::from(data)
}

fn sync_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("sync_operations");
    group.sample_size(1000);
    group.sampling_mode(SamplingMode::Flat);

    let mut rng = StdRng::seed_from_u64(42);
    let rt = Runtime::new().unwrap();

    // 测试不同数据块大小
    for size in [4 * 1024, 16 * 1024, 64 * 1024, 256 * 1024].iter() {
        group.bench_with_input(format!("{}KB_sync", size / 1024), size, |b, &size| {
            let dir = tempdir().unwrap();
            let path = dir.path().join("sync_bench");
            let hot_file = rt.block_on(HotFile::open(&path)).unwrap();
            let data = generate_test_data(&mut rng, size);

            b.to_async(&rt).iter(|| async {
                hot_file.write(data.clone(), 0);
                hot_file.sync().await.unwrap();
            })
        });
    }
}

fn concurrent_write_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_writes");
    group.measurement_time(std::time::Duration::from_secs(30));

    let rt = Runtime::new().unwrap();
    let mut rng = StdRng::seed_from_u64(42);

    group.bench_function("4_threads_256kb", |b| {
        let dir = tempdir().unwrap();
        let path = dir.path().join("concurrent_bench");
        let hot_file = Arc::new(rt.block_on(HotFile::open(&path)).unwrap());
        let data = generate_test_data(&mut rng, 256 * 1024);

        b.to_async(&rt).iter_custom(|iters| {
            let hot_clone = hot_file.clone();
            let data = data.clone();
            async move {
                let start = std::time::Instant::now();

                let mut tasks = vec![];
                for _ in 0..4 {
                    let hot_cclone = hot_clone.clone();
                    let data_clone = data.clone();
                    tasks.push(tokio::spawn(async move {
                        for i in 0..iters / 4 {
                            let offset = i as usize * data_clone.len();
                            hot_cclone.clone().write(data_clone.clone(), offset);
                        }
                        hot_cclone.sync().await.unwrap();
                    }));
                }

                for t in tasks {
                    t.await.unwrap();
                }
                start.elapsed()
            }
        })
    });
}

fn read_mixed_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_performance");
    group.confidence_level(0.99);

    let rt = Runtime::new().unwrap();
    let mut rng = StdRng::seed_from_u64(42);

    // 准备测试数据
    let mut prepare = || {
        let dir = tempdir().unwrap();
        let path = dir.path().join("read_bench");
        let hot_file = rt.block_on(HotFile::open(&path)).unwrap();

        // 写入基础数据
        let base_data = generate_test_data(&mut rng, 1024 * 1024);
        hot_file.write(base_data.clone(), 0);
        rt.block_on(hot_file.sync()).unwrap();

        // 添加脏数据
        for _ in 0..100 {
            let offset = rng.random_range(0..1024 * 1024 - 4096);
            let data = generate_test_data(&mut rng, 4096);
            hot_file.write(data, offset);
        }
        Arc::new(hot_file)
    };

    group.bench_function("mixed_read_4k", |b| {
        let hot_file = prepare();
        let mut offset = 0;

        b.iter(|| {
            let hot_file = hot_file.clone();
            let read_size = 4096;
            let mask = MultiInterval::new(&[interval!(offset..offset + read_size).unwrap()]);
            offset = (offset + read_size) % (1024 * 1024 - read_size);

            hot_file.read(mask);
        })
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(3))
        .measurement_time(std::time::Duration::from_secs(10));
    targets = sync_bench, concurrent_write_bench, read_mixed_bench
}
criterion_main!(benches);
