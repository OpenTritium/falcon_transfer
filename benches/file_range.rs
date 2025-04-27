use benchmarks::benches;
use criterion::criterion_main;

#[cfg(test)]
mod benchmarks {
    use criterion::{BatchSize, Criterion, black_box, criterion_group};
    use falcon_transfer::hot_file::{FileMultiRange, FileRange};
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};
    
    // 生成有序不重叠区间
    fn generate_sorted_ranges(count: usize, size: usize) -> Vec<(usize, usize)> {
        (0..count)
            .map(|i| (i * size * 2, i * size * 2 + size))
            .collect()
    }

    // 生成随机可能重叠的区间
    fn generate_random_ranges(rng: &mut StdRng, count: usize, max: usize) -> Vec<(usize, usize)> {
        (0..count)
            .map(|_| {
                let start = rng.random_range(0..max);
                let end = start + rng.random_range(1..=max - start);
                (start, end)
            })
            .collect()
    }

    // 基准测试组配置
    fn config() -> Criterion {
        Criterion::default()
            .warm_up_time(std::time::Duration::from_millis(500))
            .measurement_time(std::time::Duration::from_secs(2))
            .sample_size(20)
    }

    // FileRange基本操作
    fn filerange_ops(c: &mut Criterion) {
        c.bench_function("FileRange::intersect", |b| {
            let r1 = FileRange::new(100, 200);
            let r2 = FileRange::new(150, 250);
            b.iter(|| r1.intersect(black_box(&r2)))
        });

        c.bench_function("FileRange::union", |b| {
            let r1 = FileRange::new(100, 200);
            let r2 = FileRange::new(150, 250);
            b.iter(|| r1.union(black_box(&r2)))
        });

        c.bench_function("FileRange::subtract", |b| {
            let r1 = FileRange::new(100, 200);
            let r2 = FileRange::new(150, 250);
            b.iter(|| r1.subtract(black_box(&r2)))
        });
    }

    fn parameterized_add_bench(
        group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
        name: &str,
        ranges: Vec<(usize, usize)>,
    ) {
        group.bench_function(name, |b| {
            b.iter_batched(
                || ranges.clone(),
                |ranges| {
                    let mut mr = FileMultiRange::new();
                    for (s, e) in ranges {
                        mr.add_checked(s, e).unwrap();
                    }
                    mr
                },
                BatchSize::LargeInput,
            )
        });
    }

    fn multimap_add(c: &mut Criterion) {
        let mut group = c.benchmark_group("FileMultiRange::add_checked");

        for &size in &[8, 16, 32, 64] {
            let ranges = generate_sorted_ranges(size, 10);
            parameterized_add_bench(&mut group, &format!("sorted_{}", size), ranges);
        }

        let mut rng = StdRng::seed_from_u64(42);
        for &size in &[8, 16, 32, 64] {
            let max = 10_000 * (size / 100).max(1);
            let ranges = generate_random_ranges(&mut rng, size, max);
            parameterized_add_bench(&mut group, &format!("random_{}", size), ranges);
        }

        for &size in &[8, 16, 32, 64] {
            let base = 1000;
            let ranges = (0..size).map(|i| (base + i, base + i + 50)).collect();
            parameterized_add_bench(&mut group, &format!("overlap_{}", size), ranges);
        }
    }

    fn large_scale_ops(c: &mut Criterion) {
        let mut group = c.benchmark_group("large_scale");
        let mut rng = StdRng::seed_from_u64(42);
        let mr1 = FileMultiRange::try_from(&generate_random_ranges(&mut rng, 5000, 1_000_000)[..])
            .unwrap();
        let mr2 = FileMultiRange::try_from(&generate_random_ranges(&mut rng, 5000, 1_000_000)[..])
            .unwrap();

        group.bench_function("intersect", |b| b.iter(|| mr1.intersect(black_box(&mr2))));

        group.bench_function("subtract", |b| b.iter(|| mr1.subtract(black_box(&mr2))));
    }

    criterion_group! {
        name = benches;
        config = config();
        targets =
            filerange_ops,
            multimap_add,
            large_scale_ops,
    }
}

criterion_main!(benches);
