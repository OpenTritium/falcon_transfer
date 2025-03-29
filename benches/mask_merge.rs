use criterion::{Criterion, black_box, criterion_group, criterion_main};
use falcon_transfer::hot_file::{FileMultiRange, FileRange};
use rand::Rng;

fn generate_test_data(n: usize, max_gap: usize) -> Vec<FileRange> {
    let mut rng = rand::rng();
    let mut current = 0;

    (0..n)
        .map(|_| {
            let start = current + rng.random_range(0..=max_gap);
            let end = start + rng.random_range(1..=max_gap);
            current = end + rng.random_range(0..=max_gap);
            FileRange { start, end }
        })
        .collect()
}

fn bench_merge(c: &mut Criterion) {
    let mut group = c.benchmark_group("Mask Merge");

    for &size in &[5, 10, 50, 100, 1_000, 10_000, 100_000] {
        group.bench_with_input(format!("Sequential {}", size), &size, |b, &size| {
            let data = generate_test_data(size, 100);
            b.iter(|| {
                let mask = FileMultiRange::try_from(data.as_slice()).unwrap();
                black_box(mask);
            })
        });
    }

    group.finish();
}

criterion_group!(benches, bench_merge);
criterion_main!(benches);
