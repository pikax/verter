//! Benchmarks for script and CSS analysis in verter_analysis.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use oxc_allocator::Allocator;
use oxc_span::SourceType;
use verter_analysis::{
    build_css_style_analysis, build_export_signatures, build_script_analysis, classify_vue_api,
    extract_import_sources, VueStyleInput,
};

/// Generate a script with N imports and N bindings.
fn gen_script(n_imports: usize, n_bindings: usize) -> String {
    let mut code = String::new();
    // Vue imports
    code.push_str("import { ref, computed, watch, onMounted, provide, inject } from 'vue';\n");
    // User imports
    for i in 0..n_imports {
        code.push_str(&format!("import type {{ Type{i} }} from './module{i}';\n"));
    }
    // defineProps with type ref
    code.push_str("const props = defineProps<{foo: string}>();\n");
    code.push_str("const emit = defineEmits<{(e: 'click'): void}>();\n");
    // Bindings
    for i in 0..n_bindings {
        if i % 3 == 0 {
            code.push_str(&format!("const val{i} = ref({i});\n"));
        } else if i % 3 == 1 {
            code.push_str(&format!(
                "const comp{i} = computed(() => val0.value + {i});\n"
            ));
        } else {
            code.push_str(&format!("const str{i} = 'hello {i}';\n"));
        }
    }
    code
}

/// Generate a script with N exports.
fn gen_exports(n: usize) -> String {
    let mut code = String::new();
    for i in 0..n {
        if i % 3 == 0 {
            code.push_str(&format!("export interface Type{i} {{ val: number }}\n"));
        } else if i % 3 == 1 {
            code.push_str(&format!("export const CONST_{i} = {i};\n"));
        } else {
            code.push_str(&format!("export function fn{i}() {{ return {i}; }}\n"));
        }
    }
    code
}

/// Generate CSS with N rules.
fn gen_css(n_rules: usize) -> String {
    let mut css = String::new();
    for i in 0..n_rules {
        match i % 5 {
            0 => css.push_str(&format!(".cls-{i} {{ color: red; }}\n")),
            1 => css.push_str(&format!("#id-{i} {{ display: flex; }}\n")),
            2 => css.push_str(&format!(":root {{ --var-{i}: {i}px; }}\n")),
            3 => css.push_str(&format!(
                "@media (max-width: {i}px) {{ .m-{i} {{ display: none; }} }}\n"
            )),
            _ => css.push_str(&format!(
                ".parent-{i} > .child-{i} {{ font-size: {i}px; }}\n"
            )),
        }
    }
    css
}

fn bench_build_script_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("build_script_analysis");
    for n in [5, 20, 50] {
        let code = gen_script(n, n);
        group.throughput(Throughput::Bytes(code.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &code, |b, code| {
            b.iter(|| {
                let alloc = Allocator::new();
                black_box(build_script_analysis(
                    black_box(code),
                    SourceType::ts(),
                    &alloc,
                ))
            });
        });
    }
    group.finish();
}

fn bench_extract_import_sources(c: &mut Criterion) {
    let mut group = c.benchmark_group("extract_import_sources");
    for n in [5, 20, 50] {
        let code = gen_script(n, 0);
        group.throughput(Throughput::Bytes(code.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &code, |b, code| {
            b.iter(|| {
                let alloc = Allocator::new();
                black_box(extract_import_sources(
                    black_box(code),
                    SourceType::ts(),
                    &alloc,
                ))
            });
        });
    }
    group.finish();
}

fn bench_build_export_signatures(c: &mut Criterion) {
    let mut group = c.benchmark_group("build_export_signatures");
    for n in [5, 20, 50] {
        let code = gen_exports(n);
        group.throughput(Throughput::Bytes(code.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &code, |b, code| {
            b.iter(|| {
                let alloc = Allocator::new();
                black_box(build_export_signatures(
                    black_box(code),
                    SourceType::ts(),
                    &alloc,
                ))
            });
        });
    }
    group.finish();
}

fn bench_build_css_style_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("build_css_style_analysis");
    for n in [5, 20, 50, 100] {
        let css = gen_css(n);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &css, |b, css| {
            b.iter(|| {
                black_box(build_css_style_analysis(
                    black_box(css),
                    VueStyleInput::default(),
                    false,
                    false,
                    None,
                    0,
                ))
            });
        });
    }
    group.finish();
}

fn bench_classify_vue_api(c: &mut Criterion) {
    let api_names = [
        "ref",
        "computed",
        "watch",
        "onMounted",
        "defineProps",
        "provide",
        "inject",
        "unknownApi",
    ];
    c.bench_function("classify_vue_api", |b| {
        b.iter(|| {
            for name in &api_names {
                black_box(classify_vue_api(black_box(name)));
            }
        });
    });
}

criterion_group!(
    benches,
    bench_build_script_analysis,
    bench_extract_import_sources,
    bench_build_export_signatures,
    bench_build_css_style_analysis,
    bench_classify_vue_api,
);
criterion_main!(benches);
