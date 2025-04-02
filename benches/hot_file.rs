use bytes::Bytes;
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use falcon_transfer::hot_file::{FileMultiRange, HotFile};
use rand::{Rng, rng};
use std::fs::File;
use std::io::Write;
use std::sync::{Arc, OnceLock};
use tempfile::NamedTempFile;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::runtime::Runtime;
use tokio::sync::Mutex;

const KB: usize = 1024;
const MB: usize = 1024 * KB;

static RT: OnceLock<Runtime> = OnceLock::new();

fn rt() -> &'static Runtime {
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
    })
}

fn random_data(size: usize) -> Bytes {
    let mut rng = rng();
    Bytes::from((0..size).map(|_| rng.random()).collect::<Vec<u8>>())
}

fn prepare_file_sync(size: usize, sync: bool) -> (NamedTempFile, Bytes) {
    let file = NamedTempFile::new().unwrap();
    let data = random_data(size);
    let mut f = File::create(&file).unwrap();
    f.write_all(&data).unwrap();
    if sync {
        f.sync_all().unwrap();
    }
    (file, data)
}

fn bench_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("write");
    group.sample_size(10);

    for size in [4 * KB, 256 * KB, 4 * MB].into_iter() {
        group.bench_with_input(format!("hotfile_{}KB", size / KB), &size, |b, &size| {
            b.to_async(rt()).iter_batched(
                || prepare_file_sync(size, false),
                |(file, data)| async move {
                    let hot_file = HotFile::open_existed(&file).await.unwrap();
                    hot_file.write(data.clone(), 0).await.unwrap();
                    hot_file.sync().await.unwrap();
                },
                BatchSize::SmallInput,
            )
        });

        group.bench_with_input(format!("tokio_{}KB", size / KB), &size, |b, &size| {
            b.to_async(rt()).iter_batched(
                || prepare_file_sync(size, false),
                |(file, data)| async move {
                    let mut handle = tokio::fs::File::create(&file).await.unwrap();
                    handle.write_all(&data).await.unwrap();
                    handle.sync_all().await.unwrap();
                },
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("read");
    group.sample_size(10);

    let scenarios = [
        ("fs-cached", false, 4 * KB),
        ("disk", true, 4 * KB),
        ("mixed", false, 4 * MB),
    ];

    for (name, sync, size) in scenarios.iter() {
        let size = *size;

        group.bench_with_input(
            format!("hotfile_{}_{}KB", name, size / KB),
            &size,
            |b, &size| {
                b.to_async(rt()).iter_batched(
                    || prepare_file_sync(size, *sync),
                    |(file, expected)| async move {
                        let hot_file = HotFile::open_existed(&file).await.unwrap();
                        let mask = FileMultiRange::try_from([0..size].as_slice()).unwrap();
                        let result = hot_file.read(mask).await.unwrap();
                        let received: Vec<u8> =
                            result.iter().flat_map(|b| b.iter().cloned()).collect();
                        assert_eq!(received, expected.as_ref());
                    },
                    BatchSize::SmallInput,
                )
            },
        );

        group.bench_with_input(
            format!("tokio_{}_{}KB", name, size / KB),
            &size,
            |b, &size| {
                b.to_async(rt()).iter_batched(
                    || prepare_file_sync(size, *sync),
                    |(file, expected)| async move {
                        let mut buf = vec![0u8; size];
                        let mut handle = tokio::fs::File::open(&file).await.unwrap();
                        handle.read_exact(&mut buf).await.unwrap();
                        assert_eq!(buf, expected.as_ref());
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }
    group.finish();
}

fn bench_concurrent(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent");
    group.sample_size(10);

    for concurrency in [4, 16, 64, 128].into_iter() {
        group.bench_with_input(
            format!("hotfile_{}_writers", concurrency),
            &concurrency,
            |b, &concurrency| {
                b.to_async(rt()).iter_batched(
                    || NamedTempFile::new().unwrap(),
                    |file| async move {
                        let hot_file = Arc::new(HotFile::open_existed(file).await.unwrap());
                        let chunk_size = 4 * KB;

                        let mut handles = Vec::with_capacity(concurrency);
                        for i in 0..concurrency {
                            let hf = hot_file.clone();
                            let data = random_data(chunk_size);
                            handles.push(tokio::spawn(async move {
                                hf.write(data, i * chunk_size).await.unwrap();
                            }));
                        }

                        futures::future::join_all(handles).await;
                        hot_file.sync().await.unwrap();
                    },
                    BatchSize::SmallInput,
                )
            },
        );

        group.bench_with_input(
            format!("tokio_{}_writers", concurrency),
            &concurrency,
            |b, &concurrency| {
                b.to_async(rt()).iter_batched(
                    || async {
                        let f = NamedTempFile::new().unwrap();
                        Arc::new(Mutex::new(tokio::fs::File::create(&f).await.unwrap()))
                    },
                    |file| async move {
                        let file = file.await;
                        let mut handles = Vec::with_capacity(concurrency);
                        for _ in 0..concurrency {
                            let data = random_data(4 * KB);
                            let file = file.clone();
                            handles.push(tokio::spawn(async move {
                                file.clone().lock().await.write_all(&data).await.unwrap();
                            }));
                        }
                        futures::future::join_all(handles).await;
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_write, bench_read, bench_concurrent);
criterion_main!(benches);
