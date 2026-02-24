use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use verter_core::css::{
    modules::apply_css_modules, prepass::prepass, scoped::apply_scoped, scoped::apply_scoped_raw,
    ProcessStyleOptions,
};

// =============================================================================
// Data generators
// =============================================================================

/// Generate CSS with N simple class rules.
fn generate_class_rules(n: usize) -> String {
    (0..n)
        .map(|i| format!(".class-{} {{ color: red; padding: {}px; }}", i, i))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with descendant selectors.
fn generate_descendant_selectors(n: usize) -> String {
    (0..n)
        .map(|i| format!(".parent-{} .child-{} {{ color: blue; }}", i, i))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with pseudo-classes.
fn generate_pseudo_selectors(n: usize) -> String {
    let pseudos = [":hover", ":focus", ":active", ":first-child", ":last-child"];
    (0..n)
        .map(|i| {
            let pseudo = pseudos[i % pseudos.len()];
            format!(".btn-{}{} {{ color: red; }}", i, pseudo)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with comma-separated selector lists.
fn generate_selector_lists(n: usize) -> String {
    (0..n)
        .map(|i| {
            let selectors = (0..3)
                .map(|j| format!(".sel-{}-{}", i, j))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} {{ margin: {}px; }}", selectors, i)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with v-bind() expressions.
fn generate_v_bind_rules(n: usize) -> String {
    (0..n)
        .map(|i| format!(".item-{} {{ color: v-bind(color{}); }}", i, i))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with quoted v-bind() and dot notation.
fn generate_v_bind_dotted(n: usize) -> String {
    (0..n)
        .map(|i| {
            format!(
                ".item-{} {{ color: v-bind('theme.colors.primary{}'); }}",
                i, i
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with :deep() selectors.
fn generate_deep_rules(n: usize) -> String {
    (0..n)
        .map(|i| format!(":deep(.inner-{}) {{ color: red; }}", i))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with :slotted() selectors.
fn generate_slotted_rules(n: usize) -> String {
    (0..n)
        .map(|i| format!(":slotted(.slot-{}) {{ color: red; }}", i))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with mixed Vue syntax.
fn generate_mixed_vue(n: usize) -> String {
    (0..n)
        .map(|i| match i % 3 {
            0 => format!(".item-{} {{ color: v-bind(color{}); }}", i, i),
            1 => format!(":deep(.inner-{}) {{ padding: {}px; }}", i, i),
            _ => format!(":slotted(.slot-{}) {{ margin: {}px; }}", i, i),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with :global() selectors.
fn generate_global_rules(n: usize) -> String {
    (0..n)
        .map(|i| format!(":global(.reset-{}) {{ margin: 0; }}", i))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with repeated class names (for modules cache hit testing).
fn generate_repeated_classes(unique: usize, repeats: usize) -> String {
    let mut rules = Vec::new();
    for r in 0..repeats {
        for i in 0..unique {
            rules.push(format!(".btn-{} {{ padding: {}px; }}", i, r));
        }
    }
    rules.join("\n")
}

// =============================================================================
// Benchmark: process_style (full pipeline)
// =============================================================================

fn bench_process_style(c: &mut Criterion) {
    let mut group = c.benchmark_group("process_style");

    // Scoped — simple classes
    for n in [5, 20, 50] {
        let css = generate_class_rules(n);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("scoped/classes", n), &css, |b, css| {
            let options = ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,
                module_name: None,
                filename: None,
                sourcemap: false,
            };
            b.iter(|| {
                let result =
                    verter_core::css::process_style(black_box(css), black_box(&options)).unwrap();
                black_box(&result.code);
            });
        });
    }

    // Scoped — pseudo-classes
    {
        let css = generate_pseudo_selectors(20);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("scoped/pseudo", 20), &css, |b, css| {
            let options = ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,
                module_name: None,
                filename: None,
                sourcemap: false,
            };
            b.iter(|| {
                let result =
                    verter_core::css::process_style(black_box(css), black_box(&options)).unwrap();
                black_box(&result.code);
            });
        });
    }

    // Modules — few classes
    for n in [5, 20, 50] {
        let css = generate_class_rules(n);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("modules/classes", n), &css, |b, css| {
            let options = ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: false,
                is_module: true,
                module_name: None,
                filename: None,
                sourcemap: false,
            };
            b.iter(|| {
                let result =
                    verter_core::css::process_style(black_box(css), black_box(&options)).unwrap();
                black_box(&result.code);
                black_box(&result.module_classes);
            });
        });
    }

    // Scoped + modules combined
    {
        let css = generate_class_rules(20);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("scoped+modules", 20), &css, |b, css| {
            let options = ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: true,
                module_name: None,
                filename: None,
                sourcemap: false,
            };
            b.iter(|| {
                let result =
                    verter_core::css::process_style(black_box(css), black_box(&options)).unwrap();
                black_box(&result.code);
            });
        });
    }

    // v-bind replacement
    for n in [1, 5, 20] {
        let css = generate_v_bind_rules(n);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("v-bind/simple", n), &css, |b, css| {
            let options = ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,
                module_name: None,
                filename: None,
                sourcemap: false,
            };
            b.iter(|| {
                let result =
                    verter_core::css::process_style(black_box(css), black_box(&options)).unwrap();
                black_box(&result.v_bind_vars);
            });
        });
    }

    // No-transform passthrough
    {
        let css = generate_class_rules(20);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("passthrough", 20), &css, |b, css| {
            let options = ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: false,
                is_module: false,
                module_name: None,
                filename: None,
                sourcemap: false,
            };
            b.iter(|| {
                let result =
                    verter_core::css::process_style(black_box(css), black_box(&options)).unwrap();
                black_box(&result.code);
            });
        });
    }

    group.finish();
}

// =============================================================================
// Benchmark: prepass (isolated)
// =============================================================================

fn bench_prepass(c: &mut Criterion) {
    let mut group = c.benchmark_group("prepass");

    // Plain CSS passthrough (no Vue syntax)
    for n in [5, 20, 50] {
        let css = generate_class_rules(n);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("passthrough", n), &css, |b, css| {
            b.iter(|| {
                let result = prepass(black_box(css), black_box("a4f2eed6"));
                black_box(&result.css);
            });
        });
    }

    // v-bind simple
    for n in [1, 5, 20] {
        let css = generate_v_bind_rules(n);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("v-bind/simple", n), &css, |b, css| {
            b.iter(|| {
                let result = prepass(black_box(css), black_box("a4f2eed6"));
                black_box(&result.css);
                black_box(&result.v_bind_vars);
            });
        });
    }

    // v-bind dotted/quoted
    for n in [1, 5, 20] {
        let css = generate_v_bind_dotted(n);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("v-bind/dotted", n), &css, |b, css| {
            b.iter(|| {
                let result = prepass(black_box(css), black_box("a4f2eed6"));
                black_box(&result.css);
            });
        });
    }

    // :deep
    for n in [5, 20] {
        let css = generate_deep_rules(n);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("deep", n), &css, |b, css| {
            b.iter(|| {
                let result = prepass(black_box(css), black_box("a4f2eed6"));
                black_box(&result.css);
            });
        });
    }

    // :slotted
    for n in [5, 20] {
        let css = generate_slotted_rules(n);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("slotted", n), &css, |b, css| {
            b.iter(|| {
                let result = prepass(black_box(css), black_box("a4f2eed6"));
                black_box(&result.css);
            });
        });
    }

    // Mixed Vue syntax
    for n in [6, 30] {
        let css = generate_mixed_vue(n);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("mixed", n), &css, |b, css| {
            b.iter(|| {
                let result = prepass(black_box(css), black_box("a4f2eed6"));
                black_box(&result.css);
            });
        });
    }

    group.finish();
}

// =============================================================================
// Benchmark: scoped (isolated)
// =============================================================================

fn bench_scoped(c: &mut Criterion) {
    let mut group = c.benchmark_group("scoped");

    // Single class selector
    {
        let css = ".box { color: red; }";
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_function("single_class", |b| {
            b.iter(|| {
                let result = apply_scoped(black_box(css), black_box("a4f2eed6")).unwrap();
                black_box(result);
            });
        });
    }

    // Descendant selectors
    for n in [5, 20] {
        let css = generate_descendant_selectors(n);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("descendant", n), &css, |b, css| {
            b.iter(|| {
                let result = apply_scoped(black_box(css), black_box("a4f2eed6")).unwrap();
                black_box(result);
            });
        });
    }

    // Selector lists (comma-separated)
    for n in [5, 20] {
        let css = generate_selector_lists(n);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("selector_list", n), &css, |b, css| {
            b.iter(|| {
                let result = apply_scoped(black_box(css), black_box("a4f2eed6")).unwrap();
                black_box(result);
            });
        });
    }

    // Pseudo-classes
    for n in [5, 20] {
        let css = generate_pseudo_selectors(n);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("pseudo", n), &css, |b, css| {
            b.iter(|| {
                let result = apply_scoped(black_box(css), black_box("a4f2eed6")).unwrap();
                black_box(result);
            });
        });
    }

    // :global() passthrough
    for n in [5, 20] {
        let css = generate_global_rules(n);
        // After prepass, :global is left as-is
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("global", n), &css, |b, css| {
            b.iter(|| {
                let result = apply_scoped(black_box(css), black_box("a4f2eed6")).unwrap();
                black_box(result);
            });
        });
    }

    group.finish();
}

// =============================================================================
// Benchmark: modules (isolated)
// =============================================================================

fn bench_modules(c: &mut Criterion) {
    let mut group = c.benchmark_group("modules");

    // Few unique classes
    for n in [3, 10, 30] {
        let css = generate_class_rules(n);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("unique_classes", n), &css, |b, css| {
            b.iter(|| {
                let (out, mapping) =
                    apply_css_modules(black_box(css), black_box("a4f2eed6")).unwrap();
                black_box(out);
                black_box(mapping);
            });
        });
    }

    // Repeated class names (cache hit ratio)
    for repeats in [2, 5, 10] {
        let css = generate_repeated_classes(5, repeats);
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_with_input(BenchmarkId::new("repeated_5x", repeats), &css, |b, css| {
            b.iter(|| {
                let (out, mapping) =
                    apply_css_modules(black_box(css), black_box("a4f2eed6")).unwrap();
                black_box(out);
                black_box(mapping);
            });
        });
    }

    group.finish();
}

// =============================================================================
// Benchmark: lightningcss (process_style) vs fast (process_style_fast)
// =============================================================================

/// Generate CSS with @media blocks (exercises the walker fix).
fn generate_media_rules(n: usize) -> String {
    let mut css = String::new();
    for i in 0..n {
        css.push_str(&format!(
            ".class-{} {{ color: red; }}\n\
             @media (max-width: {}px) {{ .inner-{} {{ display: flex; }} }}\n",
            i,
            600 + i * 10,
            i
        ));
    }
    css
}

fn bench_fast_vs_normal(c: &mut Criterion) {
    let mut group = c.benchmark_group("fast_vs_normal");

    // --- Simple classes ---
    for n in [5, 20, 50] {
        let css = generate_class_rules(n);
        group.throughput(Throughput::Bytes(css.len() as u64));

        group.bench_with_input(BenchmarkId::new("normal/classes", n), &css, |b, css| {
            let options = ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,
                module_name: None,
                filename: None,
                sourcemap: false,
            };
            b.iter(|| {
                let result =
                    verter_core::css::process_style(black_box(css), black_box(&options)).unwrap();
                black_box(&result.code);
            });
        });

        group.bench_with_input(BenchmarkId::new("fast/classes", n), &css, |b, css| {
            let options = ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,
                module_name: None,
                filename: None,
                sourcemap: false,
            };
            b.iter(|| {
                let result =
                    verter_core::css::process_style_fast(black_box(css), black_box(&options))
                        .unwrap();
                black_box(&result.code);
            });
        });
    }

    // --- @media blocks (the critical fix path) ---
    for n in [5, 20] {
        let css = generate_media_rules(n);
        group.throughput(Throughput::Bytes(css.len() as u64));

        group.bench_with_input(BenchmarkId::new("normal/media", n), &css, |b, css| {
            let options = ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,
                module_name: None,
                filename: None,
                sourcemap: false,
            };
            b.iter(|| {
                let result =
                    verter_core::css::process_style(black_box(css), black_box(&options)).unwrap();
                black_box(&result.code);
            });
        });

        group.bench_with_input(BenchmarkId::new("fast/media", n), &css, |b, css| {
            let options = ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,
                module_name: None,
                filename: None,
                sourcemap: false,
            };
            b.iter(|| {
                let result =
                    verter_core::css::process_style_fast(black_box(css), black_box(&options))
                        .unwrap();
                black_box(&result.code);
            });
        });
    }

    // --- Real-world: template-heavy.vue CSS ---
    {
        let sfc_source =
            include_str!("../../../packages/benchmark/src/fixtures/template-heavy.vue");
        let style_start = sfc_source
            .find("<style scoped>")
            .expect("must have <style scoped>");
        let css_start = style_start + "<style scoped>".len();
        let css_end = sfc_source[css_start..]
            .find("</style>")
            .expect("must have </style>");
        let css = &sfc_source[css_start..css_start + css_end];

        group.throughput(Throughput::Bytes(css.len() as u64));

        group.bench_function("normal/template-heavy", |b| {
            let options = ProcessStyleOptions {
                scope_id: "0d04bfeb",
                scoped: true,
                is_module: false,
                module_name: None,
                filename: None,
                sourcemap: false,
            };
            b.iter(|| {
                let result =
                    verter_core::css::process_style(black_box(css), black_box(&options)).unwrap();
                black_box(&result.code);
            });
        });

        group.bench_function("fast/template-heavy", |b| {
            let options = ProcessStyleOptions {
                scope_id: "0d04bfeb",
                scoped: true,
                is_module: false,
                module_name: None,
                filename: None,
                sourcemap: false,
            };
            b.iter(|| {
                let result =
                    verter_core::css::process_style_fast(black_box(css), black_box(&options))
                        .unwrap();
                black_box(&result.code);
            });
        });
    }

    group.finish();
}

// =============================================================================
// Benchmark: scoped isolation — apply_scoped (lightningcss) vs apply_scoped_raw
// =============================================================================

fn bench_scoped_fast_vs_normal(c: &mut Criterion) {
    let mut group = c.benchmark_group("scoped_fast_vs_normal");

    for n in [5, 20, 50] {
        let css = generate_class_rules(n);
        group.throughput(Throughput::Bytes(css.len() as u64));

        group.bench_with_input(BenchmarkId::new("apply_scoped", n), &css, |b, css| {
            b.iter(|| {
                let result = apply_scoped(black_box(css), black_box("a4f2eed6")).unwrap();
                black_box(result);
            });
        });

        group.bench_with_input(BenchmarkId::new("apply_scoped_raw", n), &css, |b, css| {
            b.iter(|| {
                let result = apply_scoped_raw(black_box(css), black_box("a4f2eed6"));
                black_box(result);
            });
        });
    }

    // Media rules
    for n in [5, 20] {
        let css = generate_media_rules(n);
        group.throughput(Throughput::Bytes(css.len() as u64));

        group.bench_with_input(BenchmarkId::new("apply_scoped/media", n), &css, |b, css| {
            b.iter(|| {
                let result = apply_scoped(black_box(css), black_box("a4f2eed6")).unwrap();
                black_box(result);
            });
        });

        group.bench_with_input(
            BenchmarkId::new("apply_scoped_raw/media", n),
            &css,
            |b, css| {
                b.iter(|| {
                    let result = apply_scoped_raw(black_box(css), black_box("a4f2eed6"));
                    black_box(result);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_process_style,
    bench_prepass,
    bench_scoped,
    bench_modules,
    bench_fast_vs_normal,
    bench_scoped_fast_vs_normal,
);
criterion_main!(benches);
