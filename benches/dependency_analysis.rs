//! Benchmarks for PR dependency analysis.
//!
//! This benchmark suite measures the performance of the bitmap-optimized
//! dependency analysis across various scenarios.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use mergers::core::operations::{ChangeType, DependencyAnalyzer, FileChange, LineRange, PRInfo};
use std::collections::HashMap;

/// Generates synthetic test data for benchmarking.
///
/// # Arguments
///
/// * `num_prs` - Number of PRs to generate
/// * `files_per_pr` - Average number of files per PR
/// * `overlap_rate` - Fraction of files that overlap between PRs (0.0 to 1.0)
/// * `lines_per_file` - Number of line ranges per file
fn generate_test_data(
    num_prs: usize,
    files_per_pr: usize,
    overlap_rate: f64,
    lines_per_file: usize,
) -> (Vec<PRInfo>, HashMap<i32, Vec<FileChange>>) {
    let mut prs = Vec::with_capacity(num_prs);
    let mut pr_changes = HashMap::with_capacity(num_prs);

    // Total unique files in the pool
    let total_unique_files =
        (num_prs as f64 * files_per_pr as f64 * (1.0 - overlap_rate * 0.5)).ceil() as usize;
    let shared_files = (total_unique_files as f64 * overlap_rate) as usize;

    for i in 0..num_prs {
        let pr_id = (i + 1) as i32;

        prs.push(PRInfo {
            id: pr_id,
            title: format!("PR #{}", pr_id),
            is_selected: i % 3 == 0, // Every 3rd PR is selected
            commit_id: Some(format!("abc{:04x}", i)),
        });

        let mut changes = Vec::with_capacity(files_per_pr);

        for j in 0..files_per_pr {
            // Some files are shared (from the shared pool), others are unique
            let file_idx = if j < (files_per_pr as f64 * overlap_rate) as usize {
                // Shared file - pick from shared pool based on PR index
                (i + j) % shared_files.max(1)
            } else {
                // Unique file for this PR
                shared_files + i * files_per_pr + j
            };

            let path = format!("src/module{}/file{}.rs", file_idx / 10, file_idx % 100);

            let mut line_ranges = Vec::with_capacity(lines_per_file);
            for k in 0..lines_per_file {
                // Generate line ranges that may overlap
                let start = ((i * 50 + k * 20) % 1000) as u32 + 1;
                let end = start + 10 + (k as u32 % 5);
                line_ranges.push(LineRange::new(start, end));
            }

            changes.push(FileChange::with_ranges(
                path,
                ChangeType::Modify,
                line_ranges,
            ));
        }

        pr_changes.insert(pr_id, changes);
    }

    (prs, pr_changes)
}

/// Benchmark the full dependency analysis pipeline.
fn bench_dependency_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("dependency_analysis");

    // Scenarios: (name, num_prs, files_per_pr, overlap_rate, lines_per_file)
    let scenarios = [
        // Small scenarios - typical day-to-day usage
        ("small_sparse", 30, 8, 0.1, 3),
        ("small_medium", 30, 8, 0.3, 3),
        ("small_dense", 30, 8, 0.7, 3),
        // Medium scenarios - weekly release batch
        ("medium_sparse", 100, 12, 0.15, 4),
        ("medium_medium", 100, 12, 0.35, 4),
        ("medium_dense", 100, 12, 0.6, 4),
        // Large scenarios - major release batch
        ("large_sparse", 300, 15, 0.1, 5),
        ("large_medium", 300, 15, 0.25, 5),
        ("large_dense", 300, 15, 0.5, 5),
        // Stress test scenarios
        ("stress_sparse", 500, 20, 0.1, 6),
        ("stress_medium", 500, 20, 0.3, 6),
        // Worst case - all PRs touch same files
        ("worst_case", 100, 5, 1.0, 3),
    ];

    for (name, num_prs, files_per_pr, overlap_rate, lines_per_file) in scenarios {
        let (prs, changes) =
            generate_test_data(num_prs, files_per_pr, overlap_rate, lines_per_file);
        let analyzer = DependencyAnalyzer::new();

        // Calculate number of pairwise comparisons for throughput
        let num_comparisons = (num_prs * (num_prs - 1)) / 2;

        group.throughput(Throughput::Elements(num_comparisons as u64));

        group.bench_with_input(
            BenchmarkId::new("bitmap", name),
            &(&prs, &changes),
            |b, (prs, changes)| {
                b.iter(|| analyzer.analyze_parallel(prs, changes));
            },
        );
    }

    group.finish();
}

/// Benchmark just the bitmap index building phase.
fn bench_bitmap_index_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("bitmap_index_build");

    let scenarios = [
        ("small", 50, 10, 0.2, 3),
        ("medium", 200, 15, 0.25, 4),
        ("large", 500, 20, 0.2, 5),
    ];

    for (name, num_prs, files_per_pr, overlap_rate, lines_per_file) in scenarios {
        let (_, changes) = generate_test_data(num_prs, files_per_pr, overlap_rate, lines_per_file);

        group.bench_with_input(BenchmarkId::new("build", name), &changes, |b, changes| {
            b.iter(|| mergers::core::operations::PRBitmapIndex::build(changes));
        });
    }

    group.finish();
}

/// Benchmark scaling behavior - how performance changes with PR count.
fn bench_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("scaling");
    group.sample_size(20); // Reduce sample size for slower benchmarks

    // Test scaling from 50 to 400 PRs
    for num_prs in [50, 100, 150, 200, 300, 400] {
        let (prs, changes) = generate_test_data(num_prs, 12, 0.2, 4);
        let analyzer = DependencyAnalyzer::new();

        group.throughput(Throughput::Elements((num_prs * (num_prs - 1) / 2) as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(num_prs),
            &(&prs, &changes),
            |b, (prs, changes)| {
                b.iter(|| analyzer.analyze_parallel(prs, changes));
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_dependency_analysis,
    bench_bitmap_index_build,
    bench_scaling,
);
criterion_main!(benches);
