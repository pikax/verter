//! Benchmark: old event-driven Syntax vs new AST-based Syntax.
//!
//! Compares three configurations across real Vue SFC fixtures:
//!
//! 1. **old_syntax** — Tokenizer → old `pipeline::Syntax` → `Vec<Event>`
//! 2. **old_syntax_ec** — Same as (1) + `ElementCompilerPlugin` pass.
//!    This is the closest comparison to new_syntax since the element compiler
//!    consolidates raw events into compiled elements (similar to what new_syntax
//!    does inline via the AST builder).
//! 3. **new_syntax** — Tokenizer → new `new_impl::syntax::Syntax` → `TemplateAst`

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;

use verter_core::syntax::pipeline::Syntax as OldSyntax;
use verter_core::syntax::plugin::{
    SyntaxPlugin, SyntaxPluginContext, SyntaxPluginOptions, SyntaxResult,
};
use verter_core::syntax::plugins::element_compiler::element_compiler::ElementCompilerPlugin;
use verter_core::syntax::types::Event;
use verter_core::tokenizer::byte::tokenize;

use verter_core::new_impl::syntax::Syntax as NewSyntax;

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

/// Run events through a single-plugin pipeline (element compiler).
fn run_element_compiler<'a>(
    events: Vec<Event<'a>>,
    ec: &mut ElementCompilerPlugin,
    ctx: &mut SyntaxPluginContext<'a>,
) {
    for event in events {
        match ec.process_event(event, ctx) {
            SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => {
                black_box(e);
            }
            SyntaxResult::Drop => {}
            SyntaxResult::Stop => break,
        }
    }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

struct Fixture {
    name: &'static str,
    source: String,
}

fn fixtures() -> Vec<Fixture> {
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
        name,
        source: load_fixture(name),
    })
    .collect()
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_old_syntax(c: &mut Criterion) {
    let fixtures = fixtures();
    let mut group = c.benchmark_group("syntax/old");

    for fixture in &fixtures {
        let bytes = fixture.source.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("events", fixture.name),
            &fixture.source,
            |b, input| {
                b.iter(|| {
                    let bytes = input.as_bytes();
                    let opts = SyntaxPluginOptions::default();
                    let ctx = SyntaxPluginContext {
                        input,
                        bytes,
                        options: &opts,
                        diagnostics: Vec::new(),
                    };
                    let mut syntax = OldSyntax::new(false);
                    tokenize(bytes, |e| syntax.handle(&e, &ctx));
                    syntax.finalize(bytes);
                    black_box(syntax.events());
                });
            },
        );
    }

    group.finish();
}

fn bench_old_syntax_with_element_compiler(c: &mut Criterion) {
    let fixtures = fixtures();
    let mut group = c.benchmark_group("syntax/old_ec");

    for fixture in &fixtures {
        let bytes = fixture.source.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("events+ec", fixture.name),
            &fixture.source,
            |b, input| {
                b.iter(|| {
                    let bytes = input.as_bytes();
                    let opts = SyntaxPluginOptions::default();
                    let mut ctx = SyntaxPluginContext {
                        input,
                        bytes,
                        options: &opts,
                        diagnostics: Vec::new(),
                    };
                    let mut syntax = OldSyntax::new(false);
                    tokenize(bytes, |e| syntax.handle(&e, &ctx));
                    syntax.finalize(bytes);
                    let events = syntax.events();

                    let mut ec = ElementCompilerPlugin::new();
                    run_element_compiler(events, &mut ec, &mut ctx);
                });
            },
        );
    }

    group.finish();
}

fn bench_new_syntax(c: &mut Criterion) {
    let fixtures = fixtures();
    let mut group = c.benchmark_group("syntax/new");

    for fixture in &fixtures {
        let bytes = fixture.source.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("ast", fixture.name),
            &fixture.source,
            |b, input| {
                b.iter(|| {
                    let bytes = input.as_bytes();
                    let opts = SyntaxPluginOptions::default();
                    let ctx = SyntaxPluginContext {
                        input,
                        bytes,
                        options: &opts,
                        diagnostics: Vec::new(),
                    };
                    let mut syntax = NewSyntax::new(false);
                    tokenize(bytes, |e| syntax.handle(&e, &ctx));
                    black_box(syntax.template_ast());
                    black_box(syntax.script());
                    black_box(syntax.script_setup());
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_old_syntax,
    bench_old_syntax_with_element_compiler,
    bench_new_syntax,
);
criterion_main!(benches);
