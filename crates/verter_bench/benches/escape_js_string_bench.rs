use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;

use verter_compiler::template::code_gen::shared::helpers::{
    escape_js_string, escape_js_string_into,
};

fn datasets() -> Vec<(&'static str, String)> {
    let no_escape = "plain_text_segment_".repeat(256);

    let mut sparse_escape = String::with_capacity(4096);
    for i in 0..4096 {
        if i % 97 == 0 {
            sparse_escape.push('"');
        } else if i % 211 == 0 {
            sparse_escape.push('\\');
        } else {
            sparse_escape.push('a');
        }
    }

    let dense_escape = "\\\"\n\r\t\0\u{2028}\u{2029}".repeat(384);
    let mixed_comment_like = "hello \"world\" \\\\ path\nnext line\tindent".repeat(180);

    vec![
        ("no_escape", no_escape),
        ("sparse_escape", sparse_escape),
        ("dense_escape", dense_escape),
        ("mixed_comment_like", mixed_comment_like),
    ]
}

fn bench_escape_js_string(c: &mut Criterion) {
    let mut group = c.benchmark_group("escape_js_string");

    for (name, input) in datasets() {
        group.throughput(Throughput::Bytes(input.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("escape_js_string", name),
            &input,
            |b, s| {
                b.iter(|| {
                    black_box(escape_js_string(black_box(s.as_str())));
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("escape_js_string_into_reuse", name),
            &input,
            |b, s| {
                let mut buf = String::with_capacity(s.len() + 16);
                b.iter(|| {
                    buf.clear();
                    escape_js_string_into(&mut buf, black_box(s.as_str()));
                    black_box(&buf);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_escape_js_string);
criterion_main!(benches);
