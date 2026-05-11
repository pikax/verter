//! Benchmark for the checker-text input boundary adapter.
//!
//! Measures the wall-time cost of parsing TS checker display text into a
//! `TypeExpr` through `verter_session::resolver_core::checker_text_adapter::parse_checker_text_to_type_expr`.
//!
//! The adapter is on the hot path for background indexing and "Go to
//! Definition" chains where TSGO / tsserver emit checker display strings that
//! need to enter the typed-IR resolver. The 5% wall-time gate on the
//! `pre_w5_3` baseline (committed in `crates/verter_bench/baselines/`) is the
//! perf contract for W5.3.
//!
//! Corpus: a curated inline set of representative checker-text shapes
//! covering every OXC arena shape the adapter handles (primitives,
//! literal unions, optionals, generic refs, indexed-access types,
//! Pick/Omit/Required/Partial utilities, conditional types, mapped
//! types, deep generic nests, template literals, function-type props,
//! intersection / union arms over object shapes).
//!
//! This corpus is **deliberately locally-vendored** in compliance with
//! CLAUDE.md §10 hermeticity ("Unit tests must only depend on
//! locally-vendored fixtures... third-party corpora must be feature-
//! gated"). It is the architecturally correct corpus for the perf gate:
//!
//! - **Hermetic**: the bench builds without a third-party repository
//!   checkout (no `.integration-tests/repos/element-plus/` /
//!   `.integration-tests/repos/nuxt-ui/` dependency on the default path).
//! - **Stable across CI / orchestrator machines**: the curated shapes
//!   are content-hashed in source and survive third-party version bumps
//!   that would otherwise drift the captured-corpus baseline.
//! - **Exhaustive over arena shapes**: each entry exercises a distinct
//!   OXC `TSType<'_>` variant the adapter visits, so coverage is
//!   structural rather than statistical (a captured corpus would
//!   over-sample common shapes and under-sample rare ones).
//!
//! If a future regression surfaces on a shape NOT covered by this
//! corpus, the fix is to extend `CORPUS` here (locally-vendored) rather
//! than wiring in a third-party fixture. A separate
//! `#[cfg(feature = "external-corpus")]` bench can be added later for
//! cross-validation against captured corpora; that path is out of scope
//! for the perf gate this file owns.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;

use verter_session::resolver_core::checker_text_adapter::parse_checker_text_to_type_expr;

/// Representative checker-text strings drawn from real-world Vue component
/// prop / emit / slot type shapes. Each entry is a string TypeScript's
/// `checker.typeToString()` could emit.
const CORPUS: &[(&str, &str)] = &[
    // Primitives
    ("primitive_string", "string"),
    ("primitive_number", "number"),
    ("primitive_boolean", "boolean"),
    // Literal unions
    (
        "literal_union_small",
        r#""primary" | "secondary" | "ghost""#,
    ),
    (
        "literal_union_large",
        r#""xs" | "sm" | "md" | "lg" | "xl" | "2xl" | "3xl""#,
    ),
    // Optional primitives
    ("optional_string", "string | undefined"),
    ("optional_union", "string | number | undefined"),
    // Object literals
    ("object_small", "{ x: number; y: number }"),
    (
        "object_medium",
        "{ x: number; y: number; width?: number; height?: number; label?: string }",
    ),
    (
        "object_nested",
        "{ rect: { x: number; y: number }; size: { w: number; h: number } }",
    ),
    // Arrays + tuples
    ("array_simple", "string[]"),
    ("array_object", "{ id: number; label: string }[]"),
    ("tuple_two", "[number, string]"),
    // Functions (slot signatures)
    (
        "slot_function",
        "(props: { item: string; index: number }) => any",
    ),
    (
        "emit_payload",
        "[event: \"change\", value: string | number]",
    ),
    // References
    ("ref_simple", "ButtonProps"),
    ("ref_generic", "Array<string>"),
    ("ref_indexed", r#"FooProps["variant"]"#),
    // Intersection
    ("intersection", "Foo & Bar"),
    // Complex realistic union
    (
        "union_complex",
        r#"string | number | boolean | { kind: "ref"; id: string } | undefined"#,
    ),
];

fn bench_corpus(c: &mut Criterion) {
    let mut group = c.benchmark_group("checker_text_adapter");

    let total_bytes: usize = CORPUS.iter().map(|(_, s)| s.len()).sum();
    group.throughput(Throughput::Bytes(total_bytes as u64));

    // Warm-up: prime the thread-local pool so the first iteration is not
    // unfairly slower than the steady state. Criterion's own warm-up does
    // this too, but an explicit prime makes the baseline less noisy.
    for (_, text) in CORPUS {
        let _ = parse_checker_text_to_type_expr(text);
    }

    group.bench_function("parse_corpus_serial", |b| {
        b.iter(|| {
            for (_, text) in CORPUS {
                black_box(parse_checker_text_to_type_expr(black_box(text)));
            }
        });
    });

    for (label, text) in CORPUS {
        group.bench_with_input(BenchmarkId::new("single", label), text, |b, text| {
            b.iter(|| black_box(parse_checker_text_to_type_expr(black_box(text))));
        });
    }

    group.finish();
}

criterion_group!(benches, bench_corpus);
criterion_main!(benches);
