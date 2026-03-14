//! Benchmark: New pipeline template expression parsing.
//!
//! Measures the full new pipeline:
//!   tokenize → NewSyntax → TemplateAst → parse_template_expressions
//!
//! Tests across real fixtures and synthetic templates of varying size and
//! expression density. Includes a phase breakdown to identify bottlenecks.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;

use oxc_allocator::Allocator;
use oxc_span::SourceType;

use verter_core::diagnostics::{SyntaxPluginContext, SyntaxPluginOptions};
use verter_core::parser::Syntax as NewSyntax;
use verter_core::template::oxc::parse_template_expressions;
use verter_core::tokenizer::byte::tokenize;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_fixture(name: &str) -> String {
    let path = format!(
        "{}/benches/fixtures/{}.vue",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e))
}

/// New pipeline: tokenize → NewSyntax → TemplateAst → parse_template_expressions.
fn run_new_pipeline(source: &str) {
    let bytes = source.as_bytes();
    let opts = SyntaxPluginOptions::default();
    let ctx = SyntaxPluginContext {
        input: source,
        bytes,
        options: &opts,
        diagnostics: Vec::new(),
    };

    let mut syntax = NewSyntax::new(false);
    tokenize(bytes, |e| syntax.handle(&e, &ctx));
    let ast = syntax.take_template_ast();

    if let Some(ast) = &ast {
        let alloc = Allocator::default();
        let result = parse_template_expressions(ast, source, &alloc, SourceType::tsx());
        black_box(result);
    }

    black_box(ast);
}

/// Generate a synthetic template with N elements, each having dynamic bindings.
fn generate_dynamic_template(n: usize) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(n * 80 + 30);
    s.push_str("<template>\n");
    for i in 0..n {
        writeln!(
            s,
            "  <div :class=\"cls{i}\" :style=\"{{ color: color{i} }}\" @click=\"handle{i}\">{{{{ msg{i} }}}}</div>",
        )
        .unwrap();
    }
    s.push_str("</template>\n");
    s
}

/// Generate a synthetic template with N plain elements (no expressions).
fn generate_static_template(n: usize) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(n * 50 + 30);
    s.push_str("<template>\n");
    for i in 0..n {
        writeln!(s, "  <div class=\"item-{i}\">Static text {i}</div>",).unwrap();
    }
    s.push_str("</template>\n");
    s
}

/// Generate a template with nested v-for scopes.
fn generate_scoped_template(n: usize) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(n * 120 + 30);
    s.push_str("<template>\n");
    for i in 0..n {
        write!(
            s,
            "  <div v-for=\"item{i} of list{i}\" :key=\"item{i}.id\">\n    <span>{{{{ item{i}.name }}}}</span>\n  </div>\n",
        )
        .unwrap();
    }
    s.push_str("</template>\n");
    s
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

struct Fixture {
    name: String,
    source: String,
}

fn real_fixtures() -> Vec<Fixture> {
    [
        "simple",
        "medium",
        "large",
        "kitchen-sink",
        "template-heavy",
        "composition-heavy",
    ]
    .into_iter()
    .map(|name| Fixture {
        name: name.to_string(),
        source: load_fixture(name),
    })
    .collect()
}

fn synthetic_fixtures() -> Vec<Fixture> {
    vec![
        Fixture {
            name: "dynamic-10".into(),
            source: generate_dynamic_template(10),
        },
        Fixture {
            name: "dynamic-100".into(),
            source: generate_dynamic_template(100),
        },
        Fixture {
            name: "dynamic-500".into(),
            source: generate_dynamic_template(500),
        },
        Fixture {
            name: "static-10".into(),
            source: generate_static_template(10),
        },
        Fixture {
            name: "static-100".into(),
            source: generate_static_template(100),
        },
        Fixture {
            name: "static-500".into(),
            source: generate_static_template(500),
        },
        Fixture {
            name: "scoped-10".into(),
            source: generate_scoped_template(10),
        },
        Fixture {
            name: "scoped-100".into(),
            source: generate_scoped_template(100),
        },
        Fixture {
            name: "scoped-500".into(),
            source: generate_scoped_template(500),
        },
    ]
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

/// Full new pipeline on real Vue SFC fixtures.
fn bench_real_fixtures(c: &mut Criterion) {
    let fixtures = real_fixtures();
    let mut group = c.benchmark_group("new_pipeline/real");

    for fixture in &fixtures {
        let source = &fixture.source;
        let bytes = source.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("full", &fixture.name),
            source,
            |b, source| {
                b.iter(|| run_new_pipeline(source));
            },
        );
    }

    group.finish();
}

/// Full new pipeline on synthetic templates.
fn bench_synthetic_fixtures(c: &mut Criterion) {
    let fixtures = synthetic_fixtures();
    let mut group = c.benchmark_group("new_pipeline/synthetic");

    for fixture in &fixtures {
        let source = &fixture.source;
        let bytes = source.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("full", &fixture.name),
            source,
            |b, source| {
                b.iter(|| run_new_pipeline(source));
            },
        );
    }

    group.finish();
}

/// Phase breakdown for template-heavy (the optimization target).
fn bench_template_heavy_breakdown(c: &mut Criterion) {
    let source = load_fixture("template-heavy");
    let mut group = c.benchmark_group("template_heavy_breakdown");

    let bytes = source.as_bytes();
    group.throughput(Throughput::Bytes(bytes.len() as u64));

    // Phase 1: tokenize + AST only
    group.bench_function("tokenize_plus_ast", |b| {
        b.iter(|| {
            let bytes = source.as_bytes();
            let opts = SyntaxPluginOptions::default();
            let ctx = SyntaxPluginContext {
                input: &source,
                bytes,
                options: &opts,
                diagnostics: Vec::new(),
            };
            let mut syntax = NewSyntax::new(false);
            tokenize(bytes, |e| syntax.handle(&e, &ctx));
            black_box(syntax.take_template_ast());
        });
    });

    // Phase 2: OXC expressions only (pre-built AST)
    {
        let opts = SyntaxPluginOptions::default();
        let ctx = SyntaxPluginContext {
            input: &source,
            bytes,
            options: &opts,
            diagnostics: Vec::new(),
        };
        let mut syntax = NewSyntax::new(false);
        tokenize(bytes, |e| syntax.handle(&e, &ctx));
        let pre_ast = syntax.take_template_ast().unwrap();

        group.bench_function("oxc_expressions_only", |b| {
            b.iter(|| {
                let alloc = Allocator::default();
                let result =
                    parse_template_expressions(&pre_ast, &source, &alloc, SourceType::tsx());
                black_box(result);
            });
        });
    }

    // Phase 3: Full pipeline
    group.bench_function("full_pipeline", |b| {
        b.iter(|| run_new_pipeline(&source));
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_real_fixtures,
    bench_synthetic_fixtures,
    bench_template_heavy_breakdown,
);
criterion_main!(benches);
