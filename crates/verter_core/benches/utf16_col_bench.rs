use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use verter_core::cursor::position::utf16_len;

fn find_vue_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    fn scan(dir: &Path, files: &mut Vec<PathBuf>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    if !name.starts_with('.') && name != "node_modules" && name != "dist" {
                        scan(&path, files);
                    }
                } else if path.extension().map_or(false, |e| e == "vue") {
                    files.push(path);
                }
            }
        }
    }

    scan(dir, &mut files);
    files
}

struct LoadedLines {
    lines: Vec<String>,
    total_bytes: usize,
    file_count: usize,
    min_line_len: usize,
    max_line_len: usize,
    avg_line_len: f64,
    lines_under_64: usize,
    lines_64_to_256: usize,
    lines_over_256: usize,
}

fn load_lines() -> LoadedLines {
    let folders = [
        r"D:\dev\accioresearch\WLS\avava\src",
        r"D:\dev\accioresearch\WLS\sport\src",
        r"D:\dev\accioresearch\WLS\nexus\nexus-ui",
        r"D:\dev\csc-web\csc-web\src",
        r"D:\dev\hypermob\judis-app\packages",
        r"D:\dev\mpreis\storefront\src",
        r"D:\dev\spotqa\frontend\src",
    ];

    let mut lines = Vec::new();
    let mut file_count = 0;

    for folder in &folders {
        let path = Path::new(folder);
        if !path.exists() {
            continue;
        }

        let files = find_vue_files(path);
        for file in files {
            if let Ok(content) = fs::read_to_string(&file) {
                file_count += 1;
                for line in content.lines() {
                    lines.push(line.to_string());
                }
            }
        }
    }

    let total_bytes: usize = lines.iter().map(|l| l.len()).sum();
    let min_line_len = lines.iter().map(|l| l.len()).min().unwrap_or(0);
    let max_line_len = lines.iter().map(|l| l.len()).max().unwrap_or(0);
    let avg_line_len = if lines.is_empty() {
        0.0
    } else {
        total_bytes as f64 / lines.len() as f64
    };

    let lines_under_64 = lines.iter().filter(|l| l.len() < 64).count();
    let lines_64_to_256 = lines
        .iter()
        .filter(|l| l.len() >= 64 && l.len() < 256)
        .count();
    let lines_over_256 = lines.iter().filter(|l| l.len() >= 256).count();

    LoadedLines {
        lines,
        total_bytes,
        file_count,
        min_line_len,
        max_line_len,
        avg_line_len,
        lines_under_64,
        lines_64_to_256,
        lines_over_256,
    }
}

/// Reference implementation
fn utf16_len_reference(s: &str) -> usize {
    s.encode_utf16().count()
}

fn bench_utf16_len(c: &mut Criterion) {
    let loaded = load_lines();

    if loaded.lines.is_empty() {
        println!("No lines found - skipping benchmark");
        return;
    }

    println!("\n=== UTF-16 Length Benchmark (Line-Based) ===");
    println!("Files: {}", loaded.file_count);
    println!("Lines: {}", loaded.lines.len());
    println!(
        "Total: {} ({:.2} KB)",
        loaded.total_bytes,
        loaded.total_bytes as f64 / 1024.0
    );
    println!();
    println!(
        "Line lengths: min={}, max={}, avg={:.1}",
        loaded.min_line_len, loaded.max_line_len, loaded.avg_line_len
    );
    println!();
    println!("Distribution:");
    println!(
        "  < 64 bytes:   {} ({:.1}%)",
        loaded.lines_under_64,
        100.0 * loaded.lines_under_64 as f64 / loaded.lines.len() as f64
    );
    println!(
        "  64-256 bytes: {} ({:.1}%)",
        loaded.lines_64_to_256,
        100.0 * loaded.lines_64_to_256 as f64 / loaded.lines.len() as f64
    );
    println!(
        "  > 256 bytes:  {} ({:.1}%)",
        loaded.lines_over_256,
        100.0 * loaded.lines_over_256 as f64 / loaded.lines.len() as f64
    );
    println!();

    let mut group = c.benchmark_group("utf16_len");
    group.throughput(Throughput::Bytes(loaded.total_bytes as u64));

    let id = format!(
        "{}_lines_{}KB",
        loaded.lines.len(),
        loaded.total_bytes / 1024
    );

    group.bench_function(BenchmarkId::new("utf16_len", &id), |b| {
        b.iter(|| {
            for line in &loaded.lines {
                black_box(utf16_len(black_box(line)));
            }
        });
    });

    group.bench_function(BenchmarkId::new("encode_utf16_reference", &id), |b| {
        b.iter(|| {
            for line in &loaded.lines {
                black_box(utf16_len_reference(black_box(line)));
            }
        });
    });

    group.finish();
}

#[test]
fn verify_implementation() {
    let loaded = load_lines();

    for line in &loaded.lines {
        let expected = utf16_len_reference(line);
        let actual = utf16_len(line);
        assert_eq!(
            actual,
            expected,
            "Mismatch for line (len={}): expected {}, got {}",
            line.len(),
            expected,
            actual
        );
    }
}

criterion_group!(benches, bench_utf16_len);
criterion_main!(benches);
