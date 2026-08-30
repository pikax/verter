//! TDD coverage for `StandaloneCompiler::prepare`, `::compile_prepared`,
//! and `::compile_batch` — the prepared-carrier / direct-batch closure over
//! the frozen direct compile core. Extracted to a sibling file per the Rust test
//! file organization rule: the existing inline `standalone::tests` module
//! was already near the ~400-line threshold before this coverage.
//!
//! Every test asserts a real value (an exact digest, an exact count, a
//! specific error variant) — never an always-true predicate.

use super::*;
use crate::compile_request::{
    AnalysisProductRequest, CompileProduct, DeclarationProductRequest, IdeProductRequest,
    ProductKind, PublicApiProductRequest, RuntimeProductRequest, SvelteCompileRequest,
    VueCompileRequest,
};
use crate::svelte::parser::template_ast::{SvelteAttributeKind, SvelteNode};

// ── Corpus ──────────────────────────────────────────────────────────
// These fixture FILES are shared with the `compiler_route_overhead` bench
// harness by path (not a shared Rust binding — the harness lives in a
// different crate); see that file's own doc.
//
// The two corpora are deliberately NOT identical. The harness measures
// carrier-parse work per DISTINCT source and asserts its sources are
// pairwise distinct, so it takes each file once. The identity corpus below
// additionally compiles `VUE_MEDIUM` a second time under a different
// request (map on) and adds an inline diagnostic-producing source, because
// the identity oracle hashes map and diagnostic payloads and would compare
// both vacuously if every fixture left them empty.
//
// The repeated `VUE_MEDIUM` entry also earns its place on the batch route:
// two items with identical source and identical parse options but different
// REQUESTS share one prepared carrier, so their differing output is direct
// evidence that a shared carrier cannot carry request-derived meaning.

const VUE_SIMPLE: &str = include_str!("../../verter_bench/benches/fixtures/simple.vue");
const VUE_MEDIUM: &str = include_str!("../../verter_bench/benches/fixtures/medium.vue");
const VUE_LARGE: &str = include_str!("../../verter_bench/benches/fixtures/large.vue");
const VUE_VAPOR: &str = include_str!("../../verter_bench/benches/fixtures/vapor_simple.vue");
const SVELTE_MARKUP_ONLY: &str = include_str!(
    "../../verter_svelte_conformance/corpus/fixtures/attr-lit-lit-attr-q-el-ext-plain-m.svelte"
);
const SVELTE_PROPS: &str = include_str!(
    "../../verter_svelte_conformance/corpus/fixtures/attr-dyn-lit-attr-q-el-ext-plain-m.svelte"
);
const SVELTE_STATE: &str = include_str!(
    "../../verter_svelte_conformance/corpus/fixtures/attr-dec-lit-attr-q-dynel-ext-plain-m.svelte"
);

fn vue_request(products: Vec<CompileProduct>) -> CompileRequest {
    CompileRequest::new(
        products,
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        Some("Comp.vue".to_string()),
        None,
        false,
        false,
    )
    .expect("test request constructs")
}

fn svelte_request(products: Vec<CompileProduct>) -> CompileRequest {
    CompileRequest::new(
        products,
        FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
        None,
        Some("Comp.svelte".to_string()),
        None,
        false,
        false,
    )
    .expect("test request constructs")
}

static LEAKED_VUE_EXECUTION_INPUTS: &VueExecutionInputs = &VueExecutionInputs {
    macro_runtime: None,
    prop_constness_overrides: None,
    style_v_bind_vars: Vec::new(),
    style_v_bind_usage_complete: None,
    template_binding_metadata: None,
    template_used_vars: None,
    runtime_template_hole: false,
    runtime_inline_template_chunk: false,
    prepared_styles: Vec::new(),
};
static LEAKED_VUE_MACROS: &VueMacroSemanticInput = &VueMacroSemanticInput::Unavailable;

fn vue_inputs() -> DirectExecutionInputs<'static> {
    DirectExecutionInputs::Vue {
        execution: LEAKED_VUE_EXECUTION_INPUTS,
        macros: LEAKED_VUE_MACROS,
    }
}

static LEAKED_SVELTE_EXECUTION_INPUTS: &SvelteExecutionInputs = &SvelteExecutionInputs {
    css_hash_override: None,
    prepared_styles: Vec::new(),
};

fn svelte_inputs() -> DirectExecutionInputs<'static> {
    DirectExecutionInputs::Svelte {
        execution: LEAKED_SVELTE_EXECUTION_INPUTS,
    }
}

struct CorpusFixture {
    name: &'static str,
    source: &'static str,
    request: CompileRequest,
    inputs: DirectExecutionInputs<'static>,
}

/// A small corpus spanning both frameworks' compile shapes: mixed Vue
/// VDOM-shaped inputs (including one dual-runtime `RuntimeClient` +
/// `RuntimeServer` case and one Vapor case, `<template vapor>`) and
/// Svelte-shaped inputs (a markup-only component, a `$props`-driven
/// component, and a `$state`-driven component) — one Vue dual-runtime
/// case and one Svelte single case, per the identity test's own
/// requirement.
fn corpus() -> Vec<CorpusFixture> {
    vec![
        CorpusFixture {
            name: "vue_simple",
            source: VUE_SIMPLE,
            request: vue_request(vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )]),
            inputs: vue_inputs(),
        },
        CorpusFixture {
            name: "vue_medium",
            source: VUE_MEDIUM,
            request: vue_request(vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )]),
            inputs: vue_inputs(),
        },
        CorpusFixture {
            name: "vue_large_dual_runtime",
            source: VUE_LARGE,
            request: vue_request(vec![
                CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
                CompileProduct::RuntimeServer(RuntimeProductRequest::default()),
            ]),
            inputs: vue_inputs(),
        },
        CorpusFixture {
            name: "vue_vapor",
            source: VUE_VAPOR,
            request: vue_request(vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )]),
            inputs: vue_inputs(),
        },
        CorpusFixture {
            name: "svelte_markup_only",
            source: SVELTE_MARKUP_ONLY,
            request: svelte_request(vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )]),
            inputs: svelte_inputs(),
        },
        CorpusFixture {
            name: "svelte_props",
            source: SVELTE_PROPS,
            request: svelte_request(vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )]),
            inputs: svelte_inputs(),
        },
        CorpusFixture {
            name: "svelte_state",
            source: SVELTE_STATE,
            request: svelte_request(vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )]),
            inputs: svelte_inputs(),
        },
        // Every fixture above leaves `runtime_source_map` at its default of
        // false, so the digest's two map slots are `None` for all of them and
        // the identity comparison never actually compares map CONTENT. This
        // fixture requests the map, so the map slot carries a real payload
        // through all four routes.
        CorpusFixture {
            name: "vue_runtime_source_map",
            source: VUE_MEDIUM,
            request: vue_request(vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
                runtime_source_map: true,
                ..RuntimeProductRequest::default()
            })]),
            inputs: vue_inputs(),
        },
        // Likewise, a clean fixture publishes zero diagnostics, so the
        // digest's diagnostic section is empty everywhere. This source
        // compiles successfully but carries a duplicate cached directive,
        // which publishes a warning — giving the diagnostic section real
        // content to compare across routes.
        CorpusFixture {
            name: "vue_with_diagnostics",
            source: VUE_DUPLICATE_DIRECTIVE,
            request: vue_request(vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )]),
            inputs: vue_inputs(),
        },
    ]
}

/// A Vue SFC that compiles successfully but publishes a warning: the second
/// `v-if` on one element is a duplicate cached directive (first occurrence
/// wins, the duplicate is reported).
const VUE_DUPLICATE_DIRECTIVE: &str = r#"<template>
  <div v-if="a" v-if="b">{{ a }}</div>
</template>
<script setup lang="ts">
const a = 1
const b = 2
</script>
"#;

// ── 1. Result identity (exit #1) ───────────────────────────────────

#[test]
fn identity_corpus_actually_populates_the_map_and_diagnostic_digest_slots() {
    // The identity oracle hashes both source-map slots and the diagnostic
    // list. If every corpus fixture leaves them empty, the identity test
    // compares those fields vacuously and a regression that corrupted a map
    // or dropped a diagnostic would still produce equal digests on every
    // route. This asserts the corpus is not in that state.
    let compiler = StandaloneCompiler;
    let fixtures = corpus();

    let mapped = fixtures
        .iter()
        .find(|f| f.name == "vue_runtime_source_map")
        .expect("corpus must carry a map-requesting fixture");
    let output = compiler
        .compile(mapped.source, &mapped.request, mapped.inputs)
        .expect("the map fixture must compile");
    let map = output
        .artifacts
        .artifacts()
        .iter()
        .find_map(|a| a.runtime_source_map())
        .expect("requesting runtime_source_map must publish a runtime map");
    assert!(
        !map.is_empty(),
        "the published runtime map must have content for the digest to compare"
    );

    let diagnosed = fixtures
        .iter()
        .find(|f| f.name == "vue_with_diagnostics")
        .expect("corpus must carry a diagnostic-producing fixture");
    let output = compiler
        .compile(diagnosed.source, &diagnosed.request, diagnosed.inputs)
        .expect("the diagnostic fixture must still compile");
    assert!(
        !output.diagnostics.is_empty(),
        "the diagnostic fixture must publish at least one diagnostic, got none"
    );

    // The style section is six of the digest's hashed fields and had no guard
    // of its own: a fixture swap that left no fixture publishing a style would
    // silently make the four-route style comparison compare nothing, which is
    // the exact failure mode the two checks above exist to prevent.
    let styled = fixtures
        .iter()
        .filter(|f| {
            compiler
                .compile(f.source, &f.request, f.inputs)
                .is_ok_and(|out| !out.styles.is_empty())
        })
        .count();
    assert!(
        styled > 0,
        "at least one corpus fixture must publish a style block, or the digest's style \
         section is compared vacuously on every route"
    );
}

#[test]
fn every_route_exposes_artifacts_in_the_same_order() {
    // ArtifactSet::artifacts() exposes publication ORDER, so order is part of
    // the observable result. Asserted directly as well as through the digest,
    // because a route that reordered a multi-product result is a real
    // divergence and should name itself as one.
    let compiler = StandaloneCompiler;
    let request = vue_request(vec![
        CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
        CompileProduct::RuntimeServer(RuntimeProductRequest::default()),
    ]);
    let kinds = |output: &DirectCompileOutput| -> Vec<ProductKind> {
        output
            .artifacts
            .artifacts()
            .iter()
            .map(|a| a.kind())
            .collect()
    };

    let direct = compiler
        .compile(VUE_LARGE, &request, vue_inputs())
        .expect("direct dual-runtime compile must succeed");
    let direct_kinds = kinds(&direct);
    // Pinned, not merely self-consistent. Comparing the routes only against
    // each other cannot see a reorder they all share, because publication is
    // downstream of the route split — so the observable sequence is named
    // here. Note it is NOT the request order: the server half is built first.
    // A change to this sequence is a change to what callers observe and
    // should have to be written down, not absorbed silently.
    assert_eq!(
        direct_kinds,
        vec![ProductKind::RuntimeServer, ProductKind::RuntimeClient],
        "published artifact order changed"
    );

    let prepared = compiler.prepare(VUE_LARGE, &request);
    let first = compiler
        .compile_prepared(VUE_LARGE, &prepared, &request, vue_inputs())
        .expect("prepared-first must succeed");
    assert_eq!(
        direct_kinds,
        kinds(&first),
        "prepared-first reordered artifacts"
    );
    let repeat = compiler
        .compile_prepared(VUE_LARGE, &prepared, &request, vue_inputs())
        .expect("prepared-repeat must succeed");
    assert_eq!(
        direct_kinds,
        kinds(&repeat),
        "prepared-repeat reordered artifacts"
    );

    let items = vec![BatchCompileItem {
        source: VUE_LARGE,
        request: &request,
        inputs: vue_inputs(),
    }];
    let batch = compiler.compile_batch(&items);
    let batch_output = batch.results[0]
        .as_ref()
        .expect("batch dual-runtime compile must succeed");
    assert_eq!(
        direct_kinds,
        kinds(batch_output),
        "batch reordered artifacts"
    );
}

#[test]
fn direct_prepared_prepared_repeat_and_batch_produce_identical_output_across_the_corpus() {
    let compiler = StandaloneCompiler;

    for fixture in corpus() {
        let direct = compiler
            .compile(fixture.source, &fixture.request, fixture.inputs)
            .unwrap_or_else(|e| panic!("{}: direct compile failed: {e:?}", fixture.name));
        let direct_digest = direct_compile_output_digest(&direct);

        let prepared = compiler.prepare(fixture.source, &fixture.request);

        let first = compiler
            .compile_prepared(fixture.source, &prepared, &fixture.request, fixture.inputs)
            .unwrap_or_else(|e| panic!("{}: prepared-first compile failed: {e:?}", fixture.name));
        assert_eq!(
            direct_digest,
            direct_compile_output_digest(&first),
            "{}: direct vs prepared-first output diverged",
            fixture.name
        );

        // (c) the SAME prepared carrier, reused for a SECOND
        // `compile_prepared` call — prepared-repeat.
        let second = compiler
            .compile_prepared(fixture.source, &prepared, &fixture.request, fixture.inputs)
            .unwrap_or_else(|e| panic!("{}: prepared-repeat compile failed: {e:?}", fixture.name));
        assert_eq!(
            direct_digest,
            direct_compile_output_digest(&second),
            "{}: direct vs prepared-repeat output diverged",
            fixture.name
        );
    }

    // (d) `compile_batch` over the WHOLE corpus in one call, compared item
    // for item against that same fixture's own direct digest.
    let fixtures = corpus();
    let items: Vec<BatchCompileItem<'_>> = fixtures
        .iter()
        .map(|f| BatchCompileItem {
            source: f.source,
            request: &f.request,
            inputs: f.inputs,
        })
        .collect();
    let batch = compiler.compile_batch(&items);
    assert_eq!(batch.results.len(), fixtures.len());

    for (fixture, result) in fixtures.iter().zip(batch.results.iter()) {
        let batch_output = result
            .as_ref()
            .unwrap_or_else(|e| panic!("{}: batch compile failed: {e:?}", fixture.name));
        let direct = compiler
            .compile(fixture.source, &fixture.request, fixture.inputs)
            .expect("direct compile must not be refused");
        assert_eq!(
            direct_compile_output_digest(&direct),
            direct_compile_output_digest(batch_output),
            "{}: direct vs batch output diverged",
            fixture.name
        );
    }
}

// ── 2. Stable reuse without stale inputs (exit #2) ─────────────────

#[test]
fn compile_prepared_rejects_a_different_source_as_stale() {
    let compiler = StandaloneCompiler;
    let request = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let prepared = compiler.prepare(VUE_SIMPLE, &request);

    let error = compiler
        .compile_prepared(VUE_MEDIUM, &prepared, &request, vue_inputs())
        .expect_err("a different source must never be silently compiled against a stale carrier");
    assert_eq!(
        error,
        DirectCompileError::StalePreparedInput {
            reason: StalePreparedReason::SourceChanged,
        }
    );
}

/// The Svelte half of the same refusal. `compile_prepared` checks the source
/// digest separately in each framework arm, so the Vue test above cannot reach
/// this one: deleting this arm's check left every test in the crate green.
///
/// It is not cosmetic. Without it, `compile_svelte_from_parsed` receives the
/// NEW source alongside the OLD parse, and the runtime lowering and the output
/// descriptor both read that source — mixing stale spans with fresh bytes into
/// a silently wrong compiled result, which is exactly what the refusal exists
/// to prevent.
#[test]
fn compile_prepared_rejects_a_different_svelte_source_as_stale() {
    let compiler = StandaloneCompiler;
    let request = svelte_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let prepared = compiler.prepare(SVELTE_MARKUP_ONLY, &request);

    let error = compiler
        .compile_prepared(SVELTE_PROPS, &prepared, &request, svelte_inputs())
        .expect_err("a different Svelte source must never be compiled against a stale carrier");
    assert_eq!(
        error,
        DirectCompileError::StalePreparedInput {
            reason: StalePreparedReason::SourceChanged,
        }
    );
}

fn vue_request_with_delimiters(
    products: Vec<CompileProduct>,
    delimiters: Option<(&str, &str)>,
) -> CompileRequest {
    CompileRequest::new(
        products,
        FrameworkCompileRequest::Vue(VueCompileRequest {
            delimiters: delimiters.map(|(o, c)| (o.to_string(), c.to_string())),
            ..Default::default()
        }),
        None,
        Some("Comp.vue".to_string()),
        None,
        false,
        false,
    )
    .expect("test request constructs")
}

#[test]
fn compile_prepared_rejects_changed_vue_delimiters_as_stale() {
    let compiler = StandaloneCompiler;
    let prepared_under = vue_request_with_delimiters(
        vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )],
        Some(("{{", "}}")),
    );
    let called_with_different_delimiters = vue_request_with_delimiters(
        vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )],
        Some(("[[", "]]")),
    );

    let prepared = compiler.prepare(VUE_SIMPLE, &prepared_under);
    let error = compiler
        .compile_prepared(
            VUE_SIMPLE,
            &prepared,
            &called_with_different_delimiters,
            vue_inputs(),
        )
        .expect_err(
            "changed Vue delimiters must never be silently compiled against a stale carrier",
        );
    assert_eq!(
        error,
        DirectCompileError::StalePreparedInput {
            reason: StalePreparedReason::ParseOptionsChanged,
        }
    );
}

fn vue_request_with_custom_elements(
    products: Vec<CompileProduct>,
    is_custom_element: Vec<String>,
) -> CompileRequest {
    CompileRequest::new(
        products,
        FrameworkCompileRequest::Vue(VueCompileRequest {
            is_custom_element,
            ..Default::default()
        }),
        None,
        Some("Comp.vue".to_string()),
        None,
        false,
        false,
    )
    .expect("test request constructs")
}

#[test]
fn compile_prepared_rejects_changed_vue_custom_elements_as_stale() {
    let compiler = StandaloneCompiler;
    let prepared_under = vue_request_with_custom_elements(
        vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )],
        vec!["my-".to_string()],
    );
    let called_with_different_custom_elements = vue_request_with_custom_elements(
        vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )],
        vec!["ion-".to_string()],
    );

    let prepared = compiler.prepare(VUE_SIMPLE, &prepared_under);
    let error = compiler
        .compile_prepared(
            VUE_SIMPLE,
            &prepared,
            &called_with_different_custom_elements,
            vue_inputs(),
        )
        .expect_err(
            "changed Vue is_custom_element must never be silently compiled against a stale carrier",
        );
    assert_eq!(
        error,
        DirectCompileError::StalePreparedInput {
            reason: StalePreparedReason::ParseOptionsChanged,
        }
    );
}

#[test]
fn compile_prepared_rejects_a_svelte_carrier_reused_with_a_vue_request_and_inputs() {
    let compiler = StandaloneCompiler;
    let svelte_prepared = compiler.prepare(
        SVELTE_MARKUP_ONLY,
        &svelte_request(vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )]),
    );
    let vue_req = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);

    let error = compiler
        .compile_prepared(SVELTE_MARKUP_ONLY, &svelte_prepared, &vue_req, vue_inputs())
        .expect_err(
            "a Svelte-prepared carrier reused under a Vue request must never silently compile",
        );
    assert_eq!(
        error,
        DirectCompileError::FrameworkMismatch {
            expected: "Vue",
            actual: "Svelte",
        }
    );
}

#[test]
fn compile_prepared_rejects_a_vue_carrier_reused_with_a_svelte_request_and_inputs() {
    let compiler = StandaloneCompiler;
    let vue_prepared = compiler.prepare(
        VUE_SIMPLE,
        &vue_request(vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )]),
    );
    let svelte_req = svelte_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);

    let error = compiler
        .compile_prepared(VUE_SIMPLE, &vue_prepared, &svelte_req, svelte_inputs())
        .expect_err(
            "a Vue-prepared carrier reused under a Svelte request must never silently compile",
        );
    assert_eq!(
        error,
        DirectCompileError::FrameworkMismatch {
            expected: "Svelte",
            actual: "Vue",
        }
    );
}

// ── 3. Atomic per-request results (exit #3) ─────────────────────────

#[test]
fn compile_batch_partial_failure_does_not_affect_other_items() {
    let compiler = StandaloneCompiler;
    let bad_request = vue_request(vec![CompileProduct::Analysis(AnalysisProductRequest {
        want_script_bindings: true,
        want_template_data: false,
    })]);
    let good_request = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);

    let items = vec![
        BatchCompileItem {
            source: VUE_SIMPLE,
            request: &bad_request,
            inputs: vue_inputs(),
        },
        BatchCompileItem {
            source: VUE_SIMPLE,
            request: &good_request,
            inputs: vue_inputs(),
        },
    ];
    let batch = compiler.compile_batch(&items);
    assert_eq!(batch.results.len(), 2);

    match &batch.results[0] {
        Err(DirectCompileError::UnsupportedProduct(ProductKind::Analysis)) => {}
        other => panic!("item 0: expected UnsupportedProduct(Analysis), got {other:?}"),
    }
    let ok = batch.results[1]
        .as_ref()
        .expect("item 1 must compile fully despite item 0's refusal");
    assert!(
        ok.artifacts.artifact(ProductKind::RuntimeClient).is_some(),
        "item 1's own artifact must still be the real, complete compile"
    );
}

#[test]
fn compile_prepared_failure_constructs_no_partial_output() {
    let compiler = StandaloneCompiler;
    let request = vue_request(vec![CompileProduct::Analysis(AnalysisProductRequest {
        want_script_bindings: true,
        want_template_data: false,
    })]);
    let prepared = compiler.prepare(VUE_SIMPLE, &request);

    let result = compiler.compile_prepared(VUE_SIMPLE, &prepared, &request, vue_inputs());
    match result {
        Err(DirectCompileError::UnsupportedProduct(ProductKind::Analysis)) => {}
        other => panic!("expected UnsupportedProduct(Analysis), got {other:?}"),
    }
}

// ── 4. Zero unrequested work (exit #4) ──────────────────────────────

#[test]
fn compile_batch_shares_one_prepare_across_items_with_identical_source_and_parse_options() {
    let compiler = StandaloneCompiler;
    let client_request = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let ide_request = vue_request(vec![CompileProduct::IdeCompanion(
        IdeProductRequest::default(),
    )]);

    let items = vec![
        BatchCompileItem {
            source: VUE_SIMPLE,
            request: &client_request,
            inputs: vue_inputs(),
        },
        BatchCompileItem {
            source: VUE_SIMPLE,
            request: &ide_request,
            inputs: vue_inputs(),
        },
    ];
    let batch = compiler.compile_batch(&items);
    assert!(
        batch.results.iter().all(Result::is_ok),
        "both items must compile: {:?}",
        batch.results.iter().map(Result::is_ok).collect::<Vec<_>>()
    );
    assert_eq!(
        batch.report.cold_build_count, 1,
        "identical source + parse-options, different products, must share ONE prepare"
    );
    assert_eq!(batch.report.reuse_count, 2);
    let client = batch.results[0].as_ref().expect("client item");
    let ide = batch.results[1].as_ref().expect("ide item");
    assert!(
        client
            .artifacts
            .artifact(ProductKind::RuntimeClient)
            .is_some(),
        "results[0] must carry the requested RuntimeClient, not the sibling's product"
    );
    assert!(
        ide.artifacts.artifact(ProductKind::IdeCompanion).is_some(),
        "results[1] must carry the requested IdeCompanion, not the first item's product"
    );
    assert_eq!(client.artifacts.artifacts().len(), 1);
    assert_eq!(ide.artifacts.artifacts().len(), 1);
}

#[test]
fn compile_batch_prepares_separately_for_items_with_different_sources() {
    let compiler = StandaloneCompiler;
    let request_a = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let request_b = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let items = vec![
        BatchCompileItem {
            source: VUE_SIMPLE,
            request: &request_a,
            inputs: vue_inputs(),
        },
        BatchCompileItem {
            source: VUE_MEDIUM,
            request: &request_b,
            inputs: vue_inputs(),
        },
    ];
    let batch = compiler.compile_batch(&items);
    assert_eq!(batch.report.cold_build_count, 2);
    assert_eq!(batch.report.reuse_count, 2);
}

/// The `parse_identity_digest` half of the batch group key, exercised
/// INSIDE a grouping decision. Every other grouping test varies
/// `source_digest` (a different source) while holding Vue parse-options
/// identical, or holds `source_digest` identical while never touching
/// parse-options within a `compile_batch` call at all — none of them reach
/// [`BatchGroupKey::Vue`](crate::standalone) INSIDE a batch grouping
/// decision (the direct staleness tests only reach that check through
/// `compile_prepared` called directly, never through `compile_batch`'s own
/// grouping). Two items sharing one `source_digest` but different Vue
/// `delimiters` must be treated as SEPARATE groups (`cold_build_count ==
/// 2`) and BOTH must compile successfully — a group key that dropped
/// `parse_identity_digest` would merge them under item 0's carrier and
/// spuriously fail item 1 with `StalePreparedInput` even though its own
/// request is perfectly valid.
#[test]
fn compile_batch_prepares_separately_for_items_with_the_same_source_but_different_vue_delimiters() {
    let compiler = StandaloneCompiler;
    let request_braces = vue_request_with_delimiters(
        vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )],
        Some(("{{", "}}")),
    );
    let request_brackets = vue_request_with_delimiters(
        vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )],
        Some(("[[", "]]")),
    );
    let items = vec![
        BatchCompileItem {
            source: VUE_SIMPLE,
            request: &request_braces,
            inputs: vue_inputs(),
        },
        BatchCompileItem {
            source: VUE_SIMPLE,
            request: &request_brackets,
            inputs: vue_inputs(),
        },
    ];
    let batch = compiler.compile_batch(&items);
    assert_eq!(
        batch.report.cold_build_count, 2,
        "same source, different Vue delimiters, must build TWO separate carriers"
    );
    assert!(
        batch.results.iter().all(Result::is_ok),
        "both items must compile successfully, never a spurious StalePreparedInput \
         from an incorrectly-shared carrier: {:?}",
        batch.results.iter().map(|r| r.is_ok()).collect::<Vec<_>>()
    );
}

// ── 5. Bounded memory (exit #5) ─────────────────────────────────────

#[test]
fn compile_batch_cold_build_count_is_bounded_by_distinct_sources_not_batch_size() {
    let compiler = StandaloneCompiler;
    let sources = [VUE_SIMPLE, VUE_MEDIUM, VUE_LARGE];
    let requests: Vec<CompileRequest> = (0..50)
        .map(|i| {
            // Vary the requested product across items to prove grouping is
            // by (source, parse-options) only, never by product/request
            // identity.
            if i % 2 == 0 {
                vue_request(vec![CompileProduct::RuntimeClient(
                    RuntimeProductRequest::default(),
                )])
            } else {
                vue_request(vec![CompileProduct::IdeCompanion(
                    IdeProductRequest::default(),
                )])
            }
        })
        .collect();
    let items: Vec<BatchCompileItem<'_>> = (0..50)
        .map(|i| BatchCompileItem {
            source: sources[i % sources.len()],
            request: &requests[i],
            inputs: vue_inputs(),
        })
        .collect();

    let batch = compiler.compile_batch(&items);
    assert_eq!(items.len(), 50);
    assert!(
        batch.results.iter().all(Result::is_ok),
        "every item across 50 requests over 3 sources must compile"
    );
    assert_eq!(
        batch.report.cold_build_count, 3,
        "50 items over only 3 distinct sources must still build exactly 3 carriers"
    );
    assert_eq!(batch.report.reuse_count, 50);
}

// ── 6. No cross-call state ───────────────────────────────────────────

#[test]
fn compile_batch_has_no_hidden_cache_surviving_across_calls() {
    let compiler = StandaloneCompiler;
    let request = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let items = vec![BatchCompileItem {
        source: VUE_SIMPLE,
        request: &request,
        inputs: vue_inputs(),
    }];

    let first = compiler.compile_batch(&items);
    assert_eq!(first.report.cold_build_count, 1);

    let second = compiler.compile_batch(&items);
    assert_eq!(
        second.report.cold_build_count, 1,
        "a second call over the SAME source must still cold-build its own carrier — \
         no cache may survive across separate compile_batch calls"
    );
}

// ── Inspectable retained weight, borrowed vs owned, safe drop ────────

#[test]
fn prepared_carrier_exposes_positive_retained_weight() {
    let compiler = StandaloneCompiler;
    let vue_request = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let svelte_request = svelte_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);

    let vue = compiler.prepare(VUE_SIMPLE, &vue_request);
    let vue_large = compiler.prepare(VUE_LARGE, &vue_request);
    let svelte = compiler.prepare(SVELTE_MARKUP_ONLY, &svelte_request);
    assert!(
        vue.retained_weight() > 0,
        "a prepared Vue carrier must expose a positive retained weight, got {}",
        vue.retained_weight()
    );
    assert!(
        vue_large.retained_weight() > vue.retained_weight(),
        "a larger SFC must retain more parsed inventory than a smaller one: large={} simple={}",
        vue_large.retained_weight(),
        vue.retained_weight()
    );
    assert!(
        svelte.retained_weight() > 0,
        "a prepared Svelte carrier must expose a positive retained weight, got {}",
        svelte.retained_weight()
    );
    assert!(
        vue.retained_source().is_none(),
        "borrowed prepare must not retain the caller's source"
    );
    assert!(
        svelte.retained_source().is_none(),
        "borrowed prepare must not retain the caller's source"
    );
}

#[test]
fn retained_weight_counts_nested_svelte_children_not_just_top_level_nodes() {
    let compiler = StandaloneCompiler;
    let request = svelte_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    // Two trees of IDENTICAL shape — same nesting depth, same node count at
    // every level, same attribute count. The only difference is the length of
    // an owned attribute-name string on the CHILD element. So a walk that
    // never descends into children, and any node-count oracle standing in for
    // a byte count, both produce a delta of exactly zero here.
    const SHORT_NAME: &str = "da";
    const LONG_NAME: &str = "daaaaaaaaaaaaaaaaaaaaaaa";
    let shallow_src = format!("<div><span {SHORT_NAME}=\"1\"></span></div>\n");
    let nested_src = format!("<div><span {LONG_NAME}=\"1\"></span></div>\n");
    let shallow = compiler.prepare(&shallow_src, &request);
    let nested = compiler.prepare(&nested_src, &request);
    let (a, b) = match (&shallow, &nested) {
        (PreparedCarrier::Svelte(a), PreparedCarrier::Svelte(b)) => (&a.parsed, &b.parsed),
        other => panic!("expected Svelte carriers, got {other:?}"),
    };
    fn count_nodes(nodes: &[SvelteNode]) -> usize {
        nodes
            .iter()
            .map(|node| match node {
                SvelteNode::Element(element) => 1 + count_nodes(&element.children),
                _ => 1,
            })
            .sum()
    }
    assert_eq!(
        count_nodes(&a.template),
        count_nodes(&b.template),
        "control: the two trees must contain the same number of nodes at every \
         level, so a node-count oracle cannot distinguish them"
    );
    assert!(
        count_nodes(&a.template) > a.template.len(),
        "control: the fixture must actually nest, or a children walk is untested"
    );
    let name_bytes =
        |parsed: &crate::svelte::parser::template_ast::ParsedSvelte| match &parsed.template[0] {
            SvelteNode::Element(root) => match &root.children[0] {
                SvelteNode::Element(child) => match &child.attributes[0].kind {
                    SvelteAttributeKind::Plain { name, .. } => name.capacity(),
                    other => panic!("expected a plain attribute, got {other:?}"),
                },
                other => panic!("expected a nested child element, got {other:?}"),
            },
            other => panic!("expected a root element, got {other:?}"),
        };
    let expected = name_bytes(b) - name_bytes(a);
    assert!(
        expected > 0,
        "the two fixtures must differ in retained attribute-name capacity"
    );
    assert_eq!(
        nested.retained_weight() - shallow.retained_weight(),
        expected,
        "the only difference between the two trees is one CHILD element's owned \
         attribute-name string, so the whole retained_weight() delta must be \
         exactly that string's retained capacity: nested={} shallow={}",
        nested.retained_weight(),
        shallow.retained_weight()
    );
}

#[test]
fn retained_weight_counts_svelte_custom_element_descriptor_strings() {
    let compiler = StandaloneCompiler;
    let request = svelte_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let plain = compiler.prepare("<svelte:options />\n<div></div>\n", &request);
    let tagged = compiler.prepare(
        "<svelte:options customElement=\"xxxxxxxxxxxxxxxxxxxxxxxx-el\" />\n<div></div>\n",
        &request,
    );
    // Bind directly to the retained descriptor field the claim is about — a
    // node-count or unrelated-string oracle must not satisfy this test.
    let plain_tags = match &plain {
        PreparedCarrier::Svelte(carrier) => &carrier.parsed.options_custom_element_text_tags,
        other => panic!("expected a Svelte carrier, got {other:?}"),
    };
    let tagged_tags = match &tagged {
        PreparedCarrier::Svelte(carrier) => &carrier.parsed.options_custom_element_text_tags,
        other => panic!("expected a Svelte carrier, got {other:?}"),
    };
    assert!(
        plain_tags.is_empty(),
        "an untagged <svelte:options /> must retain no custom-element text tag, got {plain_tags:?}"
    );
    assert_eq!(
        tagged_tags.len(),
        1,
        "the tagged fixture must retain exactly one custom-element text tag, got {tagged_tags:?}"
    );
    assert_eq!(
        tagged_tags[0].descriptor.tag.as_deref(),
        Some("xxxxxxxxxxxxxxxxxxxxxxxx-el"),
        "the retained descriptor must carry the exact tag string"
    );
    assert!(
        tagged.retained_weight() > plain.retained_weight(),
        "a retained custom-element tag string must increase weight: tagged={} plain={}",
        tagged.retained_weight(),
        plain.retained_weight()
    );

    // The inequality above cannot tell the retained tag STRING apart from the
    // enclosing Vec allocation, so bind the weight to the string exactly: a
    // second tagged fixture differing ONLY in tag length must move
    // retained_weight() by exactly that string's own retained capacity, and by
    // nothing else.
    let longer = compiler.prepare(
        "<svelte:options customElement=\"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx-el\" />\n<div></div>\n",
        &request,
    );
    let longer_tags = match &longer {
        PreparedCarrier::Svelte(carrier) => &carrier.parsed.options_custom_element_text_tags,
        other => panic!("expected a Svelte carrier, got {other:?}"),
    };
    assert_eq!(
        longer_tags.len(),
        1,
        "the longer-tag fixture must also retain exactly one custom-element text tag, got {longer_tags:?}"
    );
    let tag_capacity =
        |tags: &[crate::svelte::parser::template_ast::OptionsCustomElementTextTag]| {
            tags[0]
                .descriptor
                .tag
                .as_ref()
                .expect("an accepted customElement descriptor must carry a tag string")
                .capacity()
        };
    let expected = tag_capacity(longer_tags) - tag_capacity(tagged_tags);
    assert!(
        expected > 0,
        "the two fixtures must differ in retained tag capacity for this to discriminate"
    );
    assert_eq!(
        longer.retained_weight() - tagged.retained_weight(),
        expected,
        "the two tagged fixtures differ ONLY in the custom-element tag string, so the whole \
         retained_weight() delta must be exactly that string's retained capacity: longer={} tagged={}",
        longer.retained_weight(),
        tagged.retained_weight()
    );
}

/// Whether the [`ElementNodeCondition::prop`] modifier `SmallVec` on the
/// element's cached `v-if`/`v-else-if` condition has spilled to the heap.
fn v_if_modifier_spill(carrier: &PreparedCarrier) -> Option<bool> {
    let PreparedCarrier::Vue(vue) = carrier else {
        panic!("expected a Vue carrier, got {carrier:?}");
    };
    let ast = vue.parsed.template_ast().expect("template must parse");
    ast.nodes.iter().find_map(|node| match &node.kind {
        crate::ast::types::AstNodeKind::Element(element) => element
            .v_condition
            .as_ref()
            .map(|condition| condition.prop.modifiers.spilled()),
        _ => None,
    })
}

/// The exact byte count the cached `v-if`/`v-else-if` condition's modifier
/// `SmallVec` contributes when spilled — the same `capacity * size_of::<Span>()`
/// formula `spilled_bytes_2` uses in production, computed independently here
/// so the assertion binds to the field's OWN bytes, not to whatever
/// `retained_weight()` happens to report.
fn v_if_modifier_spill_bytes(carrier: &PreparedCarrier) -> usize {
    let PreparedCarrier::Vue(vue) = carrier else {
        panic!("expected a Vue carrier, got {carrier:?}");
    };
    let ast = vue.parsed.template_ast().expect("template must parse");
    ast.nodes
        .iter()
        .find_map(|node| match &node.kind {
            crate::ast::types::AstNodeKind::Element(element) => {
                element.v_condition.as_ref().map(|condition| {
                    let modifiers = &condition.prop.modifiers;
                    if modifiers.spilled() {
                        modifiers
                            .capacity()
                            .saturating_mul(std::mem::size_of::<verter_span::Span>())
                    } else {
                        0
                    }
                })
            }
            _ => None,
        })
        .unwrap_or(0)
}

#[test]
fn retained_weight_counts_vue_cached_directive_modifier_spills() {
    let compiler = StandaloneCompiler;
    let request = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let plain = compiler.prepare("<template><div v-if=\"x\"></div></template>", &request);
    let spilled = compiler.prepare(
        "<template><div v-if.a.b.c=\"x\"></div></template>",
        &request,
    );
    // Bind directly to the cached `v-if` condition's modifier field — the
    // claim under test — not to total retained weight, which a different
    // counted allocation moving could also satisfy.
    assert_eq!(
        v_if_modifier_spill(&plain),
        Some(false),
        "a bare v-if has no modifiers and must not spill its inline SmallVec"
    );
    assert_eq!(
        v_if_modifier_spill(&spilled),
        Some(true),
        "v-if.a.b.c has 3 modifiers and must spill the cached condition's inline-2 SmallVec"
    );
    // Control: a same-shape extra payload that is NOT modifiers (a longer
    // inline v-if expression) must not report a modifier spill — otherwise
    // this assertion would not distinguish "modifiers spilled" from "some
    // other field grew".
    let longer_expression = compiler.prepare(
        "<template><div v-if=\"xxxxxxxxxxxxxxxxxxxxxxxx\"></div></template>",
        &request,
    );
    assert_eq!(
        v_if_modifier_spill(&longer_expression),
        Some(false),
        "a longer v-if expression string must not itself report a modifier spill"
    );
    // Bind the actual `retained_weight()` delta to exactly the modifier
    // spill's own byte count — not merely "some increase" — so a plant that
    // skips counting the v_condition modifier spill (while some unrelated
    // field still grows) cannot satisfy this test.
    let plain_spill_bytes = v_if_modifier_spill_bytes(&plain);
    let spilled_spill_bytes = v_if_modifier_spill_bytes(&spilled);
    assert_eq!(
        plain_spill_bytes, 0,
        "a bare v-if must contribute zero spill bytes"
    );
    assert!(
        spilled_spill_bytes > 0,
        "v-if.a.b.c must spill onto the heap and contribute > 0 bytes"
    );
    let weight_delta = spilled
        .retained_weight()
        .checked_sub(plain.retained_weight())
        .expect("spilled must retain at least as much as plain");
    assert_eq!(
        weight_delta,
        spilled_spill_bytes - plain_spill_bytes,
        "the ONLY structural difference between these two fixtures is the v-if \
         modifier count, so the whole retained_weight() delta must equal exactly \
         the modifier spill's own byte delta: spilled_weight={} plain_weight={}",
        spilled.retained_weight(),
        plain.retained_weight()
    );
}

#[test]
fn compile_batch_unsupported_svelte_runtime_server_performs_zero_carrier_prepares() {
    let compiler = StandaloneCompiler;
    let request = svelte_request(vec![CompileProduct::RuntimeServer(
        RuntimeProductRequest::default(),
    )]);
    let items = vec![BatchCompileItem {
        source: SVELTE_MARKUP_ONLY,
        request: &request,
        inputs: svelte_inputs(),
    }];
    let batch = compiler.compile_batch(&items);
    match &batch.results[0] {
        Err(DirectCompileError::Svelte(ClientCompileError::Unsupported(
            UnsupportedSvelteRuntimeSurface::ServerGenerate { .. },
        ))) => {}
        other => panic!("expected Svelte SSR ServerGenerate refusal, got {other:?}"),
    }
    assert_eq!(
        batch.report.cold_build_count, 0,
        "an unproducible Svelte RuntimeServer item must not parse a carrier"
    );
}

#[test]
fn prepare_owned_retains_source_and_reports_greater_weight_than_borrowed() {
    let compiler = StandaloneCompiler;
    let request = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let borrowed = compiler.prepare(VUE_SIMPLE, &request);
    let mut oversized = String::with_capacity(8192);
    oversized.push_str(VUE_SIMPLE);
    let owned = compiler.prepare_owned(oversized, &request);

    assert_eq!(owned.retained_source(), Some(VUE_SIMPLE));
    assert!(
        owned.retained_weight() >= borrowed.retained_weight() + 8192,
        "owned preparation must count allocated source capacity, not just String::len(): owned={} borrowed={}",
        owned.retained_weight(),
        borrowed.retained_weight()
    );
}

#[test]
fn dropping_a_prepared_carrier_does_not_poison_later_compiles() {
    let compiler = StandaloneCompiler;
    let request = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    {
        let prepared = compiler.prepare(VUE_SIMPLE, &request);
        assert!(prepared.retained_weight() > 0);
        compiler
            .compile_prepared(VUE_SIMPLE, &prepared, &request, vue_inputs())
            .expect("compile from a live carrier must succeed");
        drop(prepared);
    }
    let items = vec![BatchCompileItem {
        source: VUE_SIMPLE,
        request: &request,
        inputs: vue_inputs(),
    }];
    let batch = compiler.compile_batch(&items);
    assert_eq!(
        batch.report.cold_build_count, 1,
        "dropping a prior carrier must not leave hidden state that suppresses a later prepare"
    );
    assert!(batch.results[0].is_ok());
}

// ── Slot correspondence for mixed repeating groups ──────────────────

#[test]
fn compile_batch_aba_cb_slot_correspondence_preserves_requested_product_and_digest() {
    let compiler = StandaloneCompiler;
    let request_a = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let request_b = vue_request(vec![CompileProduct::IdeCompanion(
        IdeProductRequest::default(),
    )]);
    let request_c = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);

    let expected_a = compiler
        .compile(VUE_SIMPLE, &request_a, vue_inputs())
        .expect("A");
    let expected_b = compiler
        .compile(VUE_MEDIUM, &request_b, vue_inputs())
        .expect("B");
    let expected_c = compiler
        .compile(VUE_VAPOR, &request_c, vue_inputs())
        .expect("C");
    let digest_a = direct_compile_output_digest(&expected_a);
    let digest_b = direct_compile_output_digest(&expected_b);
    let digest_c = direct_compile_output_digest(&expected_c);
    assert_ne!(
        digest_a, digest_b,
        "A and B must be distinct products/sources"
    );
    assert_ne!(digest_a, digest_c, "A and C must be distinct sources");
    assert_ne!(
        digest_b, digest_c,
        "B and C must be distinct products/sources"
    );

    let items = vec![
        BatchCompileItem {
            source: VUE_SIMPLE,
            request: &request_a,
            inputs: vue_inputs(),
        },
        BatchCompileItem {
            source: VUE_MEDIUM,
            request: &request_b,
            inputs: vue_inputs(),
        },
        BatchCompileItem {
            source: VUE_SIMPLE,
            request: &request_a,
            inputs: vue_inputs(),
        },
        BatchCompileItem {
            source: VUE_VAPOR,
            request: &request_c,
            inputs: vue_inputs(),
        },
        BatchCompileItem {
            source: VUE_MEDIUM,
            request: &request_b,
            inputs: vue_inputs(),
        },
    ];
    let batch = compiler.compile_batch(&items);
    assert_eq!(batch.results.len(), 5);
    assert_eq!(batch.report.cold_build_count, 3);
    assert_eq!(batch.report.reuse_count, 5);

    let want = [
        (0, ProductKind::RuntimeClient, digest_a, "A@0"),
        (1, ProductKind::IdeCompanion, digest_b, "B@1"),
        (2, ProductKind::RuntimeClient, digest_a, "A@2"),
        (3, ProductKind::RuntimeClient, digest_c, "C@3"),
        (4, ProductKind::IdeCompanion, digest_b, "B@4"),
    ];
    for (index, kind, digest, label) in want {
        let output = batch.results[index]
            .as_ref()
            .unwrap_or_else(|e| panic!("{label}: expected Ok, got {e:?}"));
        assert_eq!(
            output.artifacts.artifacts().len(),
            1,
            "{label}: must publish exactly the requested product, not a sibling's"
        );
        assert!(
            output.artifacts.artifact(kind).is_some(),
            "{label}: results[{index}] must carry {kind:?}"
        );
        assert_eq!(
            direct_compile_output_digest(output),
            digest,
            "{label}: results[{index}] digest must match the requested (source, product)"
        );
    }
}

/// Every [`ProductKind`]. The exhaustive match in [`product_for`] has no
/// wildcard, so a new variant fails to compile until it is listed here and
/// both per-framework oracles below decide what it means.
const ALL_PRODUCT_KINDS: [ProductKind; 6] = [
    ProductKind::RuntimeClient,
    ProductKind::RuntimeServer,
    ProductKind::IdeCompanion,
    ProductKind::PublicApi,
    ProductKind::Declarations,
    ProductKind::Analysis,
];

fn product_for(kind: ProductKind) -> CompileProduct {
    match kind {
        ProductKind::RuntimeClient => {
            CompileProduct::RuntimeClient(RuntimeProductRequest::default())
        }
        ProductKind::RuntimeServer => {
            CompileProduct::RuntimeServer(RuntimeProductRequest::default())
        }
        ProductKind::IdeCompanion => CompileProduct::IdeCompanion(IdeProductRequest::default()),
        ProductKind::PublicApi => CompileProduct::PublicApi(PublicApiProductRequest::default()),
        ProductKind::Declarations => {
            CompileProduct::Declarations(DeclarationProductRequest::default())
        }
        ProductKind::Analysis => CompileProduct::Analysis(AnalysisProductRequest::default()),
    }
}

/// Which Vue kinds this route can really produce, stated INDEPENDENTLY of the
/// production `VUE_PRODUCIBLE_KINDS` declaration on purpose: an oracle that
/// read the constant would move with it and could not see it change. Naming
/// one unproducible kind (the existing `Analysis` refusal tests) only binds
/// the list in the removal direction — it cannot see a kind being ADDED,
/// because the preflight would then admit it and fail later with a different
/// error.
fn vue_kind_is_producible(kind: ProductKind) -> bool {
    match kind {
        ProductKind::RuntimeClient
        | ProductKind::RuntimeServer
        | ProductKind::IdeCompanion
        | ProductKind::Declarations => true,
        ProductKind::PublicApi | ProductKind::Analysis => false,
    }
}

#[test]
fn vue_route_produces_exactly_the_kinds_it_declares_producible() {
    let compiler = StandaloneCompiler;
    for kind in ALL_PRODUCT_KINDS {
        let request = vue_request(vec![product_for(kind)]);
        let result = compiler.compile(VUE_SIMPLE, &request, vue_inputs());
        if vue_kind_is_producible(kind) {
            let output = result.unwrap_or_else(|e| {
                panic!("{kind:?} must be producible by the direct Vue route, got refusal {e:?}")
            });
            assert!(
                output.artifacts.artifact(kind).is_some(),
                "{kind:?} is admitted by the preflight but published no artifact of that kind — \
                 the producible-kind declaration over-promises"
            );
        } else {
            match result {
                Err(DirectCompileError::UnsupportedProduct(refused)) => assert_eq!(
                    refused, kind,
                    "an unproducible kind must be refused as itself, not as a sibling"
                ),
                other => panic!(
                    "{kind:?} is not producible, so the route must refuse it with \
                     UnsupportedProduct before parsing — got {other:?}"
                ),
            }
        }
        assert_prepares_only_for_producible(
            kind,
            VUE_SIMPLE,
            &request,
            vue_inputs(),
            vue_kind_is_producible(kind),
        );
    }
}

/// The refusal VALUE cannot tell an early refusal from a late one: the plan
/// preflight and the late completeness check both return
/// `UnsupportedProduct(kind)`. Admitting a kind the route cannot emit would
/// therefore be invisible in the error — and visible only in the work done
/// before it: an unproducible request must do no work at all. `compile_batch` reports that work: it preflights each item before
/// preparing its carrier, so an admitted-then-failed kind shows up as a
/// carrier prepare that should never have happened.
fn assert_prepares_only_for_producible(
    kind: ProductKind,
    source: &'static str,
    request: &CompileRequest,
    inputs: DirectExecutionInputs<'static>,
    producible: bool,
) {
    let compiler = StandaloneCompiler;
    let items = vec![BatchCompileItem {
        source,
        request,
        inputs,
    }];
    let batch = compiler.compile_batch(&items);
    let expected = usize::from(producible);
    assert_eq!(
        batch.report.cold_build_count, expected,
        "{kind:?}: expected {expected} carrier prepare(s), got {} — an unproducible kind \
         must be refused BEFORE the parse, and a producible one must actually parse",
        batch.report.cold_build_count,
    );
}

/// The Svelte half. `RuntimeServer` is the one kind whose refusal is NOT
/// `UnsupportedProduct`: the runtime owns that answer and the route carries
/// the runtime's own typed error, so admitting the kind or substituting a
/// generic refusal both fail here.
#[test]
fn svelte_route_produces_exactly_the_kinds_it_declares_producible() {
    let compiler = StandaloneCompiler;
    for kind in ALL_PRODUCT_KINDS {
        let request = svelte_request(vec![product_for(kind)]);
        let result = compiler.compile(SVELTE_MARKUP_ONLY, &request, svelte_inputs());
        match kind {
            ProductKind::RuntimeClient => {
                let output = result.unwrap_or_else(|e| {
                    panic!("the Svelte client surface must be producible, got refusal {e:?}")
                });
                assert!(
                    output.artifacts.artifact(kind).is_some(),
                    "the Svelte client compile published no RuntimeClient artifact"
                );
            }
            ProductKind::RuntimeServer => match result {
                Err(DirectCompileError::Svelte(ClientCompileError::Unsupported(
                    UnsupportedSvelteRuntimeSurface::ServerGenerate { .. },
                ))) => {}
                other => panic!(
                    "the Svelte server surface must carry the runtime's own ServerGenerate \
                     refusal, got {other:?}"
                ),
            },
            ProductKind::IdeCompanion
            | ProductKind::PublicApi
            | ProductKind::Declarations
            | ProductKind::Analysis => match result {
                Err(DirectCompileError::UnsupportedProduct(refused)) => assert_eq!(
                    refused, kind,
                    "an unproducible kind must be refused as itself, not as a sibling"
                ),
                other => panic!(
                    "{kind:?} is not a Svelte runtime kind, so the route must refuse it with \
                     UnsupportedProduct before parsing — got {other:?}"
                ),
            },
        }
        assert_prepares_only_for_producible(
            kind,
            SVELTE_MARKUP_ONLY,
            &request,
            svelte_inputs(),
            kind == ProductKind::RuntimeClient,
        );
    }
}

// ── Zero unrequested carrier parses for unsupported products ─────────

/// A namespace the route cannot map is refused by the SAME preflight, and it
/// must refuse before the parse for the same reason an unproducible product
/// does. The refusal VALUE alone cannot witness that: deleting the namespace
/// check leaves `UnsupportedSvelteNamespace` exactly as it was and only the
/// work leaks, which is why `cold_build_count` is the load-bearing half here.
#[test]
fn compile_batch_refused_svelte_namespace_performs_zero_carrier_prepares() {
    let compiler = StandaloneCompiler;
    // Built inline rather than through `svelte_request`, which hardcodes
    // `SvelteCompileRequest::default()` and so cannot carry a namespace.
    let request = CompileRequest::new(
        vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )],
        FrameworkCompileRequest::Svelte(SvelteCompileRequest {
            namespace: Some(crate::compile_request::svelte::SvelteNamespaceRequest::Foreign),
            ..SvelteCompileRequest::default()
        }),
        None,
        Some("Comp.svelte".to_string()),
        None,
        false,
        false,
    )
    .expect("test request constructs");
    let items = vec![BatchCompileItem {
        source: SVELTE_MARKUP_ONLY,
        request: &request,
        inputs: svelte_inputs(),
    }];
    let batch = compiler.compile_batch(&items);
    match &batch.results[0] {
        Err(DirectCompileError::UnsupportedSvelteNamespace) => {}
        other => panic!("expected UnsupportedSvelteNamespace, got {other:?}"),
    }
    assert_eq!(
        batch.report.cold_build_count, 0,
        "a namespace the route refuses must not parse a carrier"
    );
}

#[test]
fn compile_batch_unsupported_svelte_item_performs_zero_carrier_prepares() {
    let compiler = StandaloneCompiler;
    let request = svelte_request(vec![CompileProduct::IdeCompanion(
        IdeProductRequest::default(),
    )]);
    let items = vec![BatchCompileItem {
        source: SVELTE_MARKUP_ONLY,
        request: &request,
        inputs: svelte_inputs(),
    }];
    let batch = compiler.compile_batch(&items);
    match &batch.results[0] {
        Err(DirectCompileError::UnsupportedProduct(ProductKind::IdeCompanion)) => {}
        other => panic!("expected UnsupportedProduct(IdeCompanion), got {other:?}"),
    }
    assert_eq!(
        batch.report.cold_build_count, 0,
        "an unsupported Svelte item must not parse a carrier"
    );
    assert_eq!(batch.report.reuse_count, 0);
}

#[test]
fn compile_batch_unsupported_svelte_does_not_parse_when_a_later_item_is_supported() {
    let compiler = StandaloneCompiler;
    let bad = svelte_request(vec![CompileProduct::IdeCompanion(
        IdeProductRequest::default(),
    )]);
    let good = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let items = vec![
        BatchCompileItem {
            source: SVELTE_MARKUP_ONLY,
            request: &bad,
            inputs: svelte_inputs(),
        },
        BatchCompileItem {
            source: VUE_SIMPLE,
            request: &good,
            inputs: vue_inputs(),
        },
    ];
    let batch = compiler.compile_batch(&items);
    match &batch.results[0] {
        Err(DirectCompileError::UnsupportedProduct(ProductKind::IdeCompanion)) => {}
        other => panic!("item 0: {other:?}"),
    }
    assert!(batch.results[1].is_ok(), "item 1 must still compile");
    assert_eq!(
        batch.report.cold_build_count, 1,
        "only the supported Vue item may prepare"
    );
    assert_eq!(batch.report.reuse_count, 1);
}

#[test]
fn compile_batch_framework_mismatch_is_refused_before_prepare() {
    let compiler = StandaloneCompiler;
    let svelte_unsupported = svelte_request(vec![CompileProduct::IdeCompanion(
        IdeProductRequest::default(),
    )]);
    let svelte_supported = svelte_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let items = vec![
        BatchCompileItem {
            source: SVELTE_MARKUP_ONLY,
            request: &svelte_unsupported,
            inputs: vue_inputs(),
        },
        BatchCompileItem {
            source: SVELTE_MARKUP_ONLY,
            request: &svelte_supported,
            inputs: vue_inputs(),
        },
    ];
    let batch = compiler.compile_batch(&items);
    match &batch.results[0] {
        Err(DirectCompileError::FrameworkMismatch {
            expected: "Svelte",
            actual: "Vue",
        }) => {}
        other => panic!("unsupported+mismatched item must be FrameworkMismatch, got {other:?}"),
    }
    match &batch.results[1] {
        Err(DirectCompileError::FrameworkMismatch {
            expected: "Svelte",
            actual: "Vue",
        }) => {}
        other => panic!("supported+mismatched item must be FrameworkMismatch, got {other:?}"),
    }
    assert_eq!(
        batch.report.cold_build_count, 0,
        "a framework mismatch must not parse a carrier"
    );
}

#[test]
fn explicit_prepare_of_unsupported_svelte_product_still_parses() {
    let compiler = StandaloneCompiler;
    let request = svelte_request(vec![CompileProduct::IdeCompanion(
        IdeProductRequest::default(),
    )]);
    let prepared = compiler.prepare(SVELTE_MARKUP_ONLY, &request);
    assert!(
        prepared.retained_weight() > 0,
        "an explicit prepare() is the requested operation and may parse"
    );
    // `retained_weight() > 0` alone is satisfied by `ParsedSvelte::default()`
    // plus the 32-byte digest padding — witness an ACTUAL parse instead: the
    // fixture has one top-level `<div>` and one `<style>` block, both of
    // which are empty on `ParsedSvelte::default()`.
    match &prepared {
        PreparedCarrier::Svelte(carrier) => {
            assert!(
                !carrier.parsed.template.is_empty(),
                "ParsedSvelte::default() has an empty template — a real parse must not"
            );
            let oracle = crate::svelte::parse_svelte(SVELTE_MARKUP_ONLY);
            assert_eq!(
                carrier.parsed.template.len(),
                oracle.template.len(),
                "the prepared parse must match a real parse of the same source"
            );
            assert_eq!(
                carrier.parsed.styles.len(),
                oracle.styles.len(),
                "the prepared parse must retain the real style-block count, \
                 not ParsedSvelte::default()'s zero"
            );
            assert_eq!(
                carrier.parsed.styles.len(),
                1,
                "the fixture has exactly one <style> block"
            );
        }
        other => panic!("expected a Svelte carrier, got {other:?}"),
    }
    match compiler.compile_prepared(SVELTE_MARKUP_ONLY, &prepared, &request, svelte_inputs()) {
        Err(DirectCompileError::UnsupportedProduct(ProductKind::IdeCompanion)) => {}
        other => panic!("expected UnsupportedProduct(IdeCompanion), got {other:?}"),
    }
}

#[test]
fn vue_retained_weight_counts_the_template_arena_once_not_twice() {
    // `TemplateAst` lives INLINE inside `ParsedSfc`, so `ParsedSfc`'s own
    // `size_of::<Self>()` already covers the arena's layout. A nested
    // contributor that starts from `size_of::<Self>()` as well adds that
    // layout a second time and inflates every Vue carrier's reported weight
    // by a constant — which no delta-based test can see, because a constant
    // cancels on both sides of a subtraction.
    //
    // A self-closing template makes every term of the arena's heap total
    // enumerable: no nodes, no root attributes, no content region. So the
    // whole retained figure must be the node arena's capacity and nothing
    // else, and the expected value is exact rather than a bound.
    use verter_parser::ast::types::{AstNode, TemplateAst};

    let compiler = StandaloneCompiler;
    let request = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let carrier = compiler.prepare("<template />\n", &request);
    let PreparedCarrier::Vue(vue) = &carrier else {
        panic!("expected a Vue carrier, got {carrier:?}");
    };
    let ast = vue
        .parsed
        .template_ast()
        .expect("the fixture has a template block");

    assert!(
        ast.nodes.is_empty(),
        "fixture must produce no AST nodes, got {}",
        ast.nodes.len()
    );
    assert_eq!(
        ast.root.attributes.capacity(),
        0,
        "fixture must leave the root-attribute buffer unallocated"
    );
    assert!(
        ast.root.content.is_none(),
        "a self-closing template must open no content region"
    );
    assert!(
        ast.nodes.capacity() > 0,
        "the node arena must be allocated, or this test cannot tell a heap total from zero"
    );

    let expected = ast.nodes.capacity() * std::mem::size_of::<AstNode>();
    assert_eq!(
        ast.retained_bytes(),
        expected,
        "the template arena must report only the HEAP it retains; an excess of \
         size_of::<TemplateAst>() ({}) means its own inline layout is counted here \
         as well as inside size_of::<ParsedSfc>()",
        std::mem::size_of::<TemplateAst>(),
    );
}

#[test]
fn compile_batch_of_no_items_returns_no_results_and_does_no_work() {
    // An implementation that indexed, prepared, or reported spurious work
    // for a zero-item batch must be visible: the empty batch is the one case
    // where every reported count has exactly one correct value.
    let compiler = StandaloneCompiler;
    let batch = compiler.compile_batch(&[]);
    assert!(
        batch.results.is_empty(),
        "an empty batch must produce no results, got {}",
        batch.results.len()
    );
    assert_eq!(
        batch.report.cold_build_count, 0,
        "an empty batch must prepare nothing"
    );
    assert_eq!(
        batch.report.reuse_count, 0,
        "an empty batch must serve nothing"
    );
}

// ── Result order is input order, never group order ───────────────────

/// `results` follows INPUT order; a group key only selects which shared
/// carrier an item compiles against, and never contributes an ordering.
///
/// The batch below interleaves seven distinct `(source, request)` cases
/// across six carrier groups so that its input order is deliberately NOT
/// group-major. Both halves are asserted: every slot carries its own item's
/// digest, AND that input-ordered sequence differs from the group-major one
/// the same items would produce if results were emitted per group — the
/// shape a map-ordered replacement for the carrier `Vec` invites. Without
/// the second assertion the first could pass vacuously on an order that was
/// already group-major.
///
/// Case `G` is `VUE_MEDIUM` with the runtime source map requested: it shares
/// `B`'s carrier group (same source, same Vue parse options) while producing
/// a different digest, so the grouped sequence genuinely reorders it away
/// from its input slot.
#[test]
fn compile_batch_results_follow_input_order_not_group_order() {
    let compiler = StandaloneCompiler;

    let vue_client = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let vue_ide = vue_request(vec![CompileProduct::IdeCompanion(
        IdeProductRequest::default(),
    )]);
    let vue_client_mapped =
        vue_request(vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
            runtime_source_map: true,
            ..RuntimeProductRequest::default()
        })]);
    let svelte_client = svelte_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);

    // (label, source, request, inputs) — index into this array is a case id.
    let cases: [(&str, &str, &CompileRequest, DirectExecutionInputs<'static>); 7] = [
        ("A", VUE_SIMPLE, &vue_client, vue_inputs()),
        ("B", VUE_MEDIUM, &vue_client, vue_inputs()),
        ("C", VUE_VAPOR, &vue_client, vue_inputs()),
        ("D", VUE_LARGE, &vue_ide, vue_inputs()),
        ("E", SVELTE_MARKUP_ONLY, &svelte_client, svelte_inputs()),
        ("F", SVELTE_PROPS, &svelte_client, svelte_inputs()),
        ("G", VUE_MEDIUM, &vue_client_mapped, vue_inputs()),
    ];
    /// Carrier group of each case id, hand-written: `G` shares `B`'s
    /// (same source, same Vue parse options; `runtime_source_map` is not
    /// part of the key). Asserted below against the grouping
    /// `batch_group_key` actually produces, so a wrong row here fails
    /// loudly instead of silently weakening the `assert_ne!` guard.
    const GROUP_OF: [usize; 7] = [0, 1, 2, 3, 4, 5, 1];
    {
        let mut keys: Vec<BatchGroupKey> = Vec::new();
        let derived: Vec<usize> = cases
            .iter()
            .map(|(_, source, request, _)| {
                let key = batch_group_key(source, request);
                match keys.iter().position(|k| *k == key) {
                    Some(idx) => idx,
                    None => {
                        keys.push(key);
                        keys.len() - 1
                    }
                }
            })
            .collect();
        assert_eq!(
            derived, GROUP_OF,
            "GROUP_OF must match the grouping batch_group_key produces"
        );
    }

    let expected: Vec<[u8; 32]> = cases
        .iter()
        .map(|(label, source, request, inputs)| {
            let output = compiler
                .compile(source, request, *inputs)
                .unwrap_or_else(|e| panic!("{label}: single-compile oracle failed: {e:?}"));
            direct_compile_output_digest(&output)
        })
        .collect();
    for (i, (label_i, ..)) in cases.iter().enumerate() {
        for (j, (label_j, ..)) in cases.iter().enumerate().skip(i + 1) {
            assert_ne!(
                expected[i], expected[j],
                "{label_i} and {label_j} must be distinguishable by digest"
            );
        }
    }

    // C A F B E A D C G B F A E — not group-major, and not sorted.
    const ORDER: [usize; 13] = [2, 0, 5, 1, 4, 0, 3, 2, 6, 1, 5, 0, 4];

    let items: Vec<BatchCompileItem<'_>> = ORDER
        .iter()
        .map(|&case| {
            let (_, source, request, inputs) = cases[case];
            BatchCompileItem {
                source,
                request,
                inputs,
            }
        })
        .collect();
    let batch = compiler.compile_batch(&items);
    assert_eq!(batch.results.len(), ORDER.len());
    assert_eq!(
        batch.report.cold_build_count, 6,
        "six distinct carrier groups, so six prepares"
    );
    assert_eq!(batch.report.reuse_count, ORDER.len());

    let observed: Vec<[u8; 32]> = ORDER
        .iter()
        .enumerate()
        .map(|(slot, &case)| {
            let output = batch.results[slot].as_ref().unwrap_or_else(|e| {
                panic!("{}@{slot}: batch compile failed: {e:?}", cases[case].0)
            });
            direct_compile_output_digest(output)
        })
        .collect();
    let by_input: Vec<[u8; 32]> = ORDER.iter().map(|&case| expected[case]).collect();
    assert_eq!(
        observed, by_input,
        "results[i] must be item i's own outcome, in input order"
    );

    // The same items emitted per group, in group first-appearance order —
    // derived from ORDER and the verified GROUP_OF above, so it stays
    // honest if either changes. `sort_by_key` is stable, so within a group
    // the items keep their relative input order.
    let mut group_first_seen: Vec<usize> = Vec::new();
    for &case in ORDER.iter() {
        if !group_first_seen.contains(&GROUP_OF[case]) {
            group_first_seen.push(GROUP_OF[case]);
        }
    }
    let mut group_major = ORDER;
    group_major.sort_by_key(|&case| {
        group_first_seen
            .iter()
            .position(|g| *g == GROUP_OF[case])
            .expect("every group was recorded above")
    });
    let by_group: Vec<[u8; 32]> = group_major.iter().map(|&case| expected[case]).collect();
    assert_ne!(
        by_input, by_group,
        "the fixture order is already group-major, so the assertion above proves nothing"
    );
}
