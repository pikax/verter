//! The reachable-success surface of the host's framework product routes, and
//! the publication contract each route actually honours.
//!
//! Scope: every PUBLIC or DEFAULT spelling by which a caller can request a
//! framework product OTHER than Vue runtime-render output. Vue's `Main` and
//! `Template` virtual nodes appear here only for their PUBLICATION contract —
//! which products exist, under which profile axes, and what survives a refusal.
//! Nothing here asserts anything about their rendered content.
//!
//! The inventory itself is the committed
//! `framework_product_surface_inventory.json` beside this file. These tests are
//! what make it machine-checkable:
//!
//! * [`every_virtual_node_kind_is_named_by_the_inventory`] and
//!   [`every_public_api_mode_is_named_by_the_inventory`] map each product-axis
//!   enum variant onto its inventory id through an EXHAUSTIVE `match` with no
//!   wildcard arm. A new `VirtualNodeKind` / `PublicApiMode` variant is a
//!   COMPILE error here, not a silently-unnamed product — the completeness
//!   check is the type system's, not a scan over the source tree.
//! * Every `hostEntryPoint` the inventory names is CALLED by the tests below,
//!   so its continued existence is compiler-enforced too.
//!
//! What this cannot enforce structurally, and does not pretend to: the
//! transport `routeAliases` (NAPI / WASM / bundler). Those live in crates and
//! packages `verter_session` cannot call, and the project forbids landing a
//! name-keyed source-tree scanner as a guard, so they are read-verified
//! citations recorded in the artifact rather than executed here.
//!
//! Run it with:
//!
//! ```text
//! cargo test -p verter_session --lib framework_product_surface -- --test-threads=1
//! ```
//!
//! No feature is required. Add `--ignored` for the conformance target, which
//! fails by design. Read the `running N tests` line, never the exit code:
//! libtest's filter is one literal substring with no alternation, so `"a\|b"`
//! matches nothing and still exits 0.

use std::sync::Arc;

use verter_compiler::svelte::runtime::UnsupportedSvelteRuntimeSurface;

use crate::{
    CompileProfile, HostConfig, HostError, PublicApiMode, UpsertRequest, VerterHost,
    VirtualNodeKind, VirtualQuery,
};

const INVENTORY: &str = include_str!("framework_product_surface_inventory.json");

fn inventory() -> serde_json::Value {
    serde_json::from_str(INVENTORY).expect("the committed product-surface inventory is JSON")
}

/// Every product id the inventory names, across all of its semantic cases.
/// The inventory's `tsc-declaration` product is the one
/// [`the_tsc_product_is_published_only_when_its_target_bit_is_requested`]
/// drives; naming it here keeps the product-id set and the driven set in step.
const TSC_PRODUCT_ID: &str = "tsc-declaration";

fn inventory_product_ids() -> std::collections::BTreeSet<String> {
    inventory()["semanticCases"]
        .as_array()
        .expect("`semanticCases` is an array")
        .iter()
        .flat_map(|case| {
            case["products"]
                .as_array()
                .expect("each case names its products")
                .iter()
                .map(|product| {
                    product
                        .as_str()
                        .expect("a product id is a string")
                        .to_string()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(crate) fn host_with(
    canonical: &str,
    source: &str,
    language: verter_language::FileLanguage,
) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: language,
            aliases: Vec::new(),
        })
        .unwrap_or_else(|error| panic!("upsert {canonical}: {error:?}"));
    host
}

/// What one `get_virtual_file` request returned, reduced to the publication
/// facts this suite reasons about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NodeOutcome {
    Published {
        code_len: usize,
        lang: Option<String>,
        has_map: bool,
    },
    Missing,
    Refused {
        diagnostic_code: String,
    },
}

pub(crate) fn read_node(
    host: &VerterHost,
    canonical: &str,
    kind: VirtualNodeKind,
    profile: &CompileProfile,
) -> NodeOutcome {
    match host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical.to_string()),
        node_kind: Some(kind),
        compile_profile: profile.clone(),
    }) {
        Ok(response) => NodeOutcome::Published {
            code_len: response.code.len(),
            lang: response.lang.clone(),
            has_map: response.source_map.is_some(),
        },
        Err(HostError::MissingVirtualNode { .. }) => NodeOutcome::Missing,
        Err(HostError::RuntimeSurfaceRefused {
            diagnostic_code, ..
        }) => NodeOutcome::Refused { diagnostic_code },
        Err(other) => panic!("{canonical}: unmodelled host outcome {other:?}"),
    }
}

/// Every `VirtualNodeKind` this suite sweeps.
fn all_node_kinds() -> Vec<VirtualNodeKind> {
    vec![
        VirtualNodeKind::Main,
        VirtualNodeKind::Script,
        VirtualNodeKind::Template,
        VirtualNodeKind::Style { index: 0 },
        VirtualNodeKind::Custom { index: 0 },
    ]
}

// The harness's committed fixtures, shared with the framework-conformance
// goldens so every probe describes the same authored sources.
const VUE_PROPS_EMIT: &str =
    include_str!("../../../../packages/framework-conformance-harness/fixtures/vue/props-emit.vue");
const SVELTE_BASIC_RUNES: &str = include_str!(
    "../../../../packages/framework-conformance-harness/fixtures/svelte/basic-runes.svelte"
);
const SVELTE_PROPS_EVENTS: &str = include_str!(
    "../../../../packages/framework-conformance-harness/fixtures/svelte/props-events.svelte"
);

/// A Vue SFC carrying a scoped `<style>` — the CSS product's representative
/// input. The committed Vue fixtures carry no style block.
const VUE_WITH_STYLE: &str = "<script setup lang=\"ts\">\nconst a: number = 1\n</script>\n<template><div class=\"x\">{{ a }}</div></template>\n<style scoped>.x{color:red}</style>\n";

/// A Svelte component with a TYPED `$props()` destructure and an instance
/// export — the public-API product's representative input.
const SVELTE_TYPED_PROPS: &str = "<script lang=\"ts\">\n  let { label, disabled = false }: { label: string; disabled?: boolean } = $props();\n  export function focus(): void {}\n</script>\n\n<button {disabled}>{label}</button>\n";

/// A Svelte client component with a scoped `<style>` — the cold-path cell.
const SVELTE_STYLED: &str = "<script>\n  let count = $state(0);\n</script>\n\n<div class=\"root\">{count}</div>\n\n<style>\n  .root { color: red; }\n</style>\n";

pub(crate) fn bundler_profile(source_map: bool) -> CompileProfile {
    CompileProfile {
        source_map,
        ..CompileProfile::default()
    }
}

fn ssr_profile() -> CompileProfile {
    CompileProfile {
        ssr: true,
        source_map: true,
        ..CompileProfile::default()
    }
}

// ══════════════════════════════════════════════════════════════════════════
// The inventory is complete over every product axis the type system has
// ══════════════════════════════════════════════════════════════════════════

/// The inventory id for one virtual-node product.
///
/// EXHAUSTIVE, no wildcard arm: a new `VirtualNodeKind` variant fails to
/// compile here until the inventory names its product. That is the
/// completeness mechanism — the type system, not a source scan.
fn product_id_for(kind: &VirtualNodeKind) -> &'static str {
    match kind {
        VirtualNodeKind::Main => "virtual-node.main",
        VirtualNodeKind::Script => "virtual-node.script",
        VirtualNodeKind::Template => "virtual-node.template",
        VirtualNodeKind::Style { .. } => "virtual-node.style",
        VirtualNodeKind::Custom { .. } => "virtual-node.custom",
    }
}

/// The inventory profile-axis label for one public-API mode. Exhaustive for
/// the same reason.
fn profile_axis_for(mode: PublicApiMode) -> &'static str {
    match mode {
        PublicApiMode::Public => "mode:Public",
        PublicApiMode::Testing => "mode:Testing",
        PublicApiMode::Declaration => "mode:Declaration",
    }
}

#[test]
fn every_virtual_node_kind_is_named_by_the_inventory() {
    let named = inventory_product_ids();
    let mut seen = std::collections::BTreeSet::new();
    for kind in all_node_kinds() {
        let id = product_id_for(&kind);
        assert!(
            named.contains(id),
            "the tree publishes `{kind:?}` under product id `{id}`, which the committed \
             inventory does not name. Inventory ids: {named:?}"
        );
        seen.insert(id);
    }
    // The sweep must reach every arm the mapping can produce; an arm reachable
    // through `product_id_for` but absent from `all_node_kinds` would be
    // named-but-never-probed.
    assert_eq!(
        seen.len(),
        5,
        "the node-kind sweep covers only {} distinct product ids",
        seen.len()
    );
}

#[test]
fn every_public_api_mode_is_named_by_the_inventory() {
    let inventory = inventory();
    let case = inventory["semanticCases"]
        .as_array()
        .expect("`semanticCases` is an array")
        .iter()
        .find(|case| case["id"] == "public-api.render")
        .expect("the inventory names the public-API render case");
    let axes: Vec<String> = case["profileAxes"]
        .as_array()
        .expect("the case names its profile axes")
        .iter()
        .map(|axis| axis.as_str().expect("an axis is a string").to_string())
        .collect();
    for mode in [
        PublicApiMode::Public,
        PublicApiMode::Testing,
        PublicApiMode::Declaration,
    ] {
        let axis = profile_axis_for(mode);
        assert!(
            axes.contains(&axis.to_string()),
            "the tree exposes public-API mode `{mode:?}` (`{axis}`), which the committed \
             inventory does not name. Named axes: {axes:?}"
        );
    }
}

#[test]
fn the_inventory_is_internally_well_formed() {
    let inventory = inventory();
    let cases = inventory["semanticCases"]
        .as_array()
        .expect("`semanticCases` is an array");
    assert!(!cases.is_empty(), "an empty inventory proves nothing");

    let mut ids = std::collections::BTreeSet::new();
    for case in cases {
        let id = case["id"].as_str().expect("every case has a string id");
        assert!(ids.insert(id), "duplicate semantic case id `{id}`");
        assert!(
            !case["route"]
                .as_str()
                .expect("every case states its route")
                .is_empty(),
            "{id}: empty route"
        );
        assert!(
            !case["products"]
                .as_array()
                .expect("every case names its products")
                .is_empty(),
            "{id}: names no product"
        );
        // Every case must be reachable by at least one spelling: a host entry
        // point or a transport alias. A case with neither is not a reachable
        // surface and does not belong in this inventory.
        let has_host = case["hostEntryPoint"].is_string();
        let aliases = case["routeAliases"]
            .as_array()
            .expect("every case carries a routeAliases array");
        assert!(
            has_host || !aliases.is_empty(),
            "{id}: no host entry point and no transport alias — unreachable"
        );
        for alias in aliases {
            for field in ["transport", "entryPoint", "spelling"] {
                assert!(
                    alias[field].as_str().is_some_and(|value| !value.is_empty()),
                    "{id}: a route alias is missing `{field}`"
                );
            }
        }
    }

    // The one case recorded as having NO host route must stay marked that way:
    // it is the reason the standalone CSS product has no publication
    // relationship to any other product.
    assert!(
        inventory_product_ids().contains(TSC_PRODUCT_ID),
        "the inventory names no `{TSC_PRODUCT_ID}` product, but the TSC target bit publishes one"
    );

    let css = cases
        .iter()
        .find(|case| case["id"] == "css.process-style")
        .expect("the inventory names the standalone CSS spelling");
    assert!(
        css["hostEntryPoint"].is_null(),
        "the standalone CSS spelling gained a host route; the inventory's claim that it \
         bypasses the host is stale"
    );
}

// ══════════════════════════════════════════════════════════════════════════
// Every enumerated cell, driven, with its exact result recorded
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn the_vue_virtual_node_surface_publishes_exactly_the_listed_nodes() {
    let canonical = "/probe/Styled.vue";
    let host = host_with(
        canonical,
        VUE_WITH_STYLE,
        verter_language::FileLanguage::vue(),
    );
    let listed = host.list_virtual_files(canonical);
    assert_eq!(
        listed,
        vec![
            VirtualNodeKind::Main,
            VirtualNodeKind::Script,
            VirtualNodeKind::Template,
            VirtualNodeKind::Style { index: 0 },
        ],
        "the Vue node inventory moved"
    );

    let profile = bundler_profile(true);
    for kind in all_node_kinds() {
        let outcome = read_node(&host, canonical, kind.clone(), &profile);
        if listed.contains(&kind) {
            assert!(
                matches!(outcome, NodeOutcome::Published { code_len, .. } if code_len > 0),
                "{kind:?} is listed but did not publish: {outcome:?}"
            );
        } else {
            assert_eq!(
                outcome,
                NodeOutcome::Missing,
                "{kind:?} is not listed, so it must not publish"
            );
        }
    }
}

/// The source-map axis, per node kind, exactly as it behaves.
///
/// Recorded rather than assumed: the profile's `source_map` reaches the
/// main/script/template products on Vue and the module and CSS products on
/// Svelte, but NOT the Vue style product.
#[test]
fn the_source_map_axis_reaches_the_products_it_currently_reaches() {
    let vue = "/probe/Styled.vue";
    let host = host_with(vue, VUE_WITH_STYLE, verter_language::FileLanguage::vue());
    for (kind, expected_map) in [
        (VirtualNodeKind::Main, true),
        (VirtualNodeKind::Script, true),
        (VirtualNodeKind::Template, true),
        (VirtualNodeKind::Style { index: 0 }, false),
    ] {
        let with_map = read_node(&host, vue, kind.clone(), &bundler_profile(true));
        let without_map = read_node(&host, vue, kind.clone(), &bundler_profile(false));
        assert!(
            matches!(with_map, NodeOutcome::Published { has_map, .. } if has_map == expected_map),
            "vue {kind:?} under source_map=true: {with_map:?} (expected has_map={expected_map})"
        );
        assert!(
            matches!(without_map, NodeOutcome::Published { has_map: false, .. }),
            "vue {kind:?} under source_map=false must carry no map: {without_map:?}"
        );
    }

    let svelte = "/probe/Styled.svelte";
    let host = host_with(
        svelte,
        SVELTE_STYLED,
        verter_language::FileLanguage::svelte(),
    );
    for kind in [VirtualNodeKind::Main, VirtualNodeKind::Style { index: 0 }] {
        let with_map = read_node(&host, svelte, kind.clone(), &bundler_profile(true));
        let without_map = read_node(&host, svelte, kind.clone(), &bundler_profile(false));
        assert!(
            matches!(with_map, NodeOutcome::Published { has_map: true, .. }),
            "svelte {kind:?} under source_map=true must carry a map: {with_map:?}"
        );
        assert!(
            matches!(without_map, NodeOutcome::Published { has_map: false, .. }),
            "svelte {kind:?} under source_map=false must carry no map: {without_map:?}"
        );
    }
}

/// The public-API product, per framework and mode, exactly as it resolves.
#[test]
fn the_public_api_product_resolves_per_framework_and_mode() {
    let vue = "/probe/PropsEmit.vue";
    let host = host_with(vue, VUE_PROPS_EMIT, verter_language::FileLanguage::vue());
    for mode in [
        PublicApiMode::Public,
        PublicApiMode::Testing,
        PublicApiMode::Declaration,
    ] {
        let response = host
            .get_public_api_with_mode(vue, mode, None)
            .unwrap_or_else(|error| panic!("vue public api {mode:?}: {error:?}"))
            .unwrap_or_else(|| panic!("vue publishes a public API for {mode:?}"));
        assert!(
            response.ts_labeled_code().contains("label: string"),
            "vue {mode:?} public API drops the declared `label` prop:\n{}",
            response.ts_labeled_code()
        );
        assert!(
            response.source_map.is_some(),
            "vue {mode:?} public API carries no source map"
        );
    }
    assert_eq!(
        host.declaration_carrier_path(vue).as_deref(),
        Some("/probe/PropsEmit.d.vue.ts"),
        "the Vue declaration carrier path moved"
    );
    assert!(
        host.get_public_api_projection(vue)
            .expect("vue projection")
            .is_some(),
        "the Vue projection entry stopped composing"
    );

    let svelte = "/probe/Typed.svelte";
    let host = host_with(
        svelte,
        SVELTE_TYPED_PROPS,
        verter_language::FileLanguage::svelte(),
    );
    for mode in [PublicApiMode::Public, PublicApiMode::Declaration] {
        let response = host
            .get_public_api_with_mode(svelte, mode, None)
            .unwrap_or_else(|error| panic!("svelte public api {mode:?}: {error:?}"))
            .unwrap_or_else(|| panic!("svelte publishes a public API for {mode:?}"));
        // The props / exports / bindings SEMANTICS of this surface are asserted
        // by `a_typed_svelte_props_surface_types_its_props_exports_and_bindings`,
        // which reads the CHECKER's own view of it inside the pinned Svelte
        // closure. What this route test owns is that the surface publishes at
        // all, and that it carries its map.
        assert!(
            !response.ts_labeled_code().is_empty(),
            "svelte {mode:?} public API published an empty surface"
        );
        assert!(
            response.source_map.is_some(),
            "svelte {mode:?} public API carries no source map"
        );
    }
    assert!(
        host.get_public_api_with_mode(svelte, PublicApiMode::Testing, None)
            .expect("svelte testing mode is not an error")
            .is_none(),
        "the Svelte projector's Testing-mode `None` is its documented behaviour; a rendered \
         response here is a change to report"
    );
    assert_eq!(
        host.declaration_carrier_path(svelte).as_deref(),
        Some("/probe/Typed.d.svelte.ts"),
        "the Svelte declaration carrier path moved"
    );
}

/// The scalar, batch and projection spellings converge on one render, so all
/// three return the same declaration bytes for the same canonical.
#[test]
fn the_scalar_batch_and_projection_public_api_spellings_agree_byte_for_byte() {
    let canonical = "/probe/PropsEmit.vue";
    let host = host_with(
        canonical,
        VUE_PROPS_EMIT,
        verter_language::FileLanguage::vue(),
    );
    let scalar = host
        .get_public_api(canonical)
        .expect("scalar public api")
        .expect("scalar renders");
    let with_mode = host
        .get_public_api_with_mode(canonical, PublicApiMode::Public, None)
        .expect("moded public api")
        .expect("moded renders");
    let batch = host
        .get_public_api_batch(&[canonical])
        .into_iter()
        .next()
        .expect("the batch preserves input order")
        .expect("batch public api")
        .expect("batch renders");
    let projection = host
        .get_public_api_projection(canonical)
        .expect("projection public api")
        .expect("projection renders");

    assert_eq!(
        scalar.ts_labeled_code(),
        with_mode.ts_labeled_code(),
        "the scalar and moded spellings diverged"
    );
    assert_eq!(
        scalar.ts_labeled_code(),
        batch.ts_labeled_code(),
        "the scalar and batch spellings diverged"
    );
    assert_eq!(
        scalar.ts_labeled_code(),
        projection.response.ts_labeled_code(),
        "the scalar and projection spellings diverged"
    );
}

// ══════════════════════════════════════════════════════════════════════════
// Atomic publication, both directions
// ══════════════════════════════════════════════════════════════════════════

/// Whether a typed unsupported-surface variant is DRIVEN by this suite's
/// atomic-publication cells, or is not reachable through the shipped `.svelte`
/// public route with a stated reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefusalCoverage {
    /// A representative input reaches this refusal through the shipped route,
    /// and the publication contract is proven over it.
    Driven,
    /// Not reachable by driving a `.svelte` source through the public route
    /// under a profile this suite can express.
    NotReachableHere(&'static str),
}

/// The refusal inventory, DERIVED from the compiler's own typed
/// unsupported-surface taxonomy.
///
/// EXHAUSTIVE `match`, no wildcard arm: a new
/// `UnsupportedSvelteRuntimeSurface` variant fails to COMPILE here until it is
/// classified, so the atomic-publication contract can never silently cover only
/// the refusals someone happened to think of. The classifier takes a value it
/// never needs to be called with — exhaustiveness is checked at compile time
/// regardless — and the two `Driven` arms are additionally called with
/// constructed samples below, so a variant marked `Driven` without a cell is
/// caught too.
fn refusal_coverage(surface: &UnsupportedSvelteRuntimeSurface) -> RefusalCoverage {
    use UnsupportedSvelteRuntimeSurface as S;
    const PROFILE_ONLY: &str =
        "a compile-option / profile surface the public `CompileProfile` cannot express";
    const AUTHORED: &str =
        "reachable only from an authored construct outside this suite's representative inputs";
    match surface {
        S::ServerGenerate { .. } => RefusalCoverage::Driven,
        S::AdvancedRune { .. } => RefusalCoverage::Driven,
        S::DevMode { .. } | S::CompileOptionUnsupported { .. } | S::NamespaceUnsupported { .. } => {
            RefusalCoverage::NotReachableHere(PROFILE_ONLY)
        }
        S::DynamicAttribute { .. }
        | S::Binding { .. }
        | S::NonDelegatedEvent { .. }
        | S::Block { .. }
        | S::ComponentOrSnippet { .. }
        | S::SlotLetUnbound { .. }
        | S::HostOrCustomElement { .. }
        | S::Element { .. }
        | S::ElementName { .. }
        | S::DestructuringWrite { .. }
        | S::RootTextRegion { .. }
        | S::ComponentExportBinding { .. }
        | S::LegacyRuneReference { .. }
        | S::ExperimentalAsync { .. }
        | S::StyleCssAnalysis { .. }
        | S::StyleSelectorUnsupported { .. }
        | S::StyleCssModeUnsupported { .. }
        | S::ComplexInterpolation { .. }
        | S::ScriptImport { .. }
        | S::ModuleScriptItem { .. }
        | S::TypeScript { .. }
        | S::ComplexTextChunk { .. }
        | S::InstanceScriptItem { .. }
        | S::MagicIdentifier { .. }
        | S::ParagraphAutoclose { .. }
        | S::ConstFoldThrow { .. }
        | S::StoreScopedSubscription { .. }
        | S::ExpressionFactRecovery { .. }
        | S::OfficialReject { .. } => RefusalCoverage::NotReachableHere(AUTHORED),
    }
}

/// The representative input that reaches each `Driven` refusal, paired with the
/// typed code the variant's own `diagnostic_code()` produces — so the cell's
/// expected code is the COMPILER's, never a transcribed string.
fn refusal_cells() -> Vec<(
    &'static str,
    &'static str,
    &'static str,
    CompileProfile,
    String,
)> {
    vec![
        (
            "server-generate",
            "/probe/Server.svelte",
            SVELTE_STYLED,
            ssr_profile(),
            UnsupportedSvelteRuntimeSurface::ServerGenerate {
                span: verter_span::Span::new(0, 0),
            }
            .diagnostic_code()
            .to_string(),
        ),
        (
            "advanced-rune",
            "/probe/PropsEvents.svelte",
            SVELTE_PROPS_EVENTS,
            bundler_profile(true),
            UnsupportedSvelteRuntimeSurface::AdvancedRune {
                rune: "sample",
                span: verter_span::Span::new(0, 0),
            }
            .diagnostic_code()
            .to_string(),
        ),
    ]
}

/// Every variant the classifier marks `Driven` has a cell, and every cell's
/// code is a `Driven` variant's own code.
///
/// This is the half the exhaustive match alone cannot give: adding a variant
/// forces a classification at compile time, and marking one `Driven` without
/// adding its cell fails here.
#[test]
fn every_driven_refusal_variant_has_a_cell_and_every_cell_names_a_driven_variant() {
    let samples: Vec<UnsupportedSvelteRuntimeSurface> = vec![
        UnsupportedSvelteRuntimeSurface::ServerGenerate {
            span: verter_span::Span::new(0, 0),
        },
        UnsupportedSvelteRuntimeSurface::AdvancedRune {
            rune: "sample",
            span: verter_span::Span::new(0, 0),
        },
    ];
    let driven_codes: std::collections::BTreeSet<String> = samples
        .iter()
        .filter(|surface| refusal_coverage(surface) == RefusalCoverage::Driven)
        .map(|surface| surface.diagnostic_code().to_string())
        .collect();
    assert_eq!(
        driven_codes.len(),
        samples.len(),
        "a sampled variant is no longer classified `Driven`, so its cell below is stale"
    );
    let cell_codes: std::collections::BTreeSet<String> = refusal_cells()
        .into_iter()
        .map(|(_, _, _, _, code)| code)
        .collect();
    assert_eq!(
        driven_codes, cell_codes,
        "the driven refusal variants and the cells that exercise them have diverged"
    );
}

#[test]
fn a_refused_runtime_surface_publishes_no_javascript_no_css_and_no_source_map() {
    for (label, canonical, source, profile, expected_code) in refusal_cells() {
        let host = host_with(canonical, source, verter_language::FileLanguage::svelte());

        let main = read_node(&host, canonical, VirtualNodeKind::Main, &profile);
        assert_eq!(
            main,
            NodeOutcome::Refused {
                diagnostic_code: expected_code.clone()
            },
            "[{label}] the runtime product must be an explicit typed refusal"
        );

        // Every OTHER node kind must be absent — in particular no CSS, and no
        // partially-emitted script or template. `Missing` is the only
        // acceptable outcome: a `Published` here is a partial product
        // surviving a refusal.
        for kind in all_node_kinds() {
            if kind == VirtualNodeKind::Main {
                continue;
            }
            let outcome = read_node(&host, canonical, kind.clone(), &profile);
            assert_eq!(
                outcome,
                NodeOutcome::Missing,
                "[{label}] {kind:?} survived the runtime refusal"
            );
        }
    }
}

/// The other direction, recorded rather than asserted as a rule: the refusal is
/// scoped to the RUNTIME product. The IDE/TSX projection and the public-API
/// declaration are still published for the same refused component.
///
/// This is a characterization of the current contract, not an endorsement. It
/// fails if either product starts or stops being published.
#[test]
fn a_refused_runtime_surface_still_publishes_the_ide_and_public_api_products() {
    for (label, canonical, source, profile, _) in refusal_cells() {
        let host = host_with(canonical, source, verter_language::FileLanguage::svelte());

        let ensured = host
            .ensure_ide_compiled(canonical, &profile)
            .unwrap_or_else(|error| panic!("[{label}] ensure_ide_compiled: {error:?}"));
        assert!(
            ensured,
            "[{label}] the IDE projection is withheld on a runtime refusal"
        );
        let ide = host
            .get_ide(canonical, &profile)
            .unwrap_or_else(|| panic!("[{label}] no IDE product after a successful ensure"));
        assert!(
            !ide.code.is_empty(),
            "[{label}] the IDE product is empty on a runtime refusal"
        );

        let public_api = host
            .get_public_api_with_mode(canonical, PublicApiMode::Public, None)
            .unwrap_or_else(|error| panic!("[{label}] public api: {error:?}"));
        assert!(
            public_api.is_some(),
            "[{label}] the public-API declaration is withheld on a runtime refusal"
        );
    }
}

/// The parse-derived node list keeps naming `Main` for a component whose
/// runtime surface is refused, so a caller that trusts the list and then reads
/// it receives the typed refusal rather than a product.
#[test]
fn the_node_list_names_main_for_a_component_whose_runtime_surface_is_refused() {
    let canonical = "/probe/PropsEvents.svelte";
    let host = host_with(
        canonical,
        SVELTE_PROPS_EVENTS,
        verter_language::FileLanguage::svelte(),
    );
    assert!(
        host.list_virtual_files(canonical)
            .contains(&VirtualNodeKind::Main),
        "the parse-derived node list stopped naming Main"
    );
    assert!(
        matches!(
            read_node(
                &host,
                canonical,
                VirtualNodeKind::Main,
                &bundler_profile(true)
            ),
            NodeOutcome::Refused { .. }
        ),
        "the listed Main node no longer refuses"
    );
}

/// On success, exactly the requested products are published and nothing else:
/// a `source_map: false` request yields no map on any node, on either
/// framework.
#[test]
fn a_success_publishes_no_unrequested_source_map() {
    for (canonical, source, language) in [
        (
            "/probe/Styled.vue",
            VUE_WITH_STYLE,
            verter_language::FileLanguage::vue(),
        ),
        (
            "/probe/Styled.svelte",
            SVELTE_STYLED,
            verter_language::FileLanguage::svelte(),
        ),
    ] {
        let host = host_with(canonical, source, language);
        for kind in all_node_kinds() {
            let outcome = read_node(&host, canonical, kind.clone(), &bundler_profile(false));
            if let NodeOutcome::Published { has_map, .. } = outcome {
                assert!(
                    !has_map,
                    "{canonical} {kind:?} published a source map that was never requested"
                );
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Cold-path preservation
// ══════════════════════════════════════════════════════════════════════════

/// A supported Svelte client component keeps publishing its runtime module and
/// its scoped CSS together, each carrying its own map on demand.
///
/// This is the cell no recorded finding implicates, and the one a
/// correction to the refusal or publication paths is most likely to over-reach
/// into.
#[test]
fn a_supported_svelte_client_component_keeps_publishing_its_module_and_its_css() {
    let canonical = "/probe/Cold.svelte";
    let host = host_with(
        canonical,
        SVELTE_STYLED,
        verter_language::FileLanguage::svelte(),
    );

    // BEHAVIOUR, not existence: the exact emitted bytes. A correction that
    // over-reached into this cell — a different helper sequence, a dropped
    // scope class, a changed template shape — fails here; a length check would
    // not.
    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical.to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: bundler_profile(true),
        })
        .expect("the supported client module stopped publishing");
    assert_eq!(
        response.code.as_ref(),
        "import 'svelte/internal/disclose-version';\nimport * as $ from 'svelte/internal/client';\n\nvar root = $.from_html(`<div class=\"root svelte-qeeybc\"></div>`);\n\nexport default function Cold($$anchor) {\n\tlet count = 0;\n\tvar div = root();\n\tdiv.textContent = '0';\n\t$.append($$anchor, div);\n}\n",
        "the supported client module's emitted bytes moved"
    );
    assert_eq!(
        response.lang.as_deref(),
        Some("js"),
        "the module's lang moved"
    );
    assert!(
        response.source_map.is_some(),
        "the module's source map was withheld"
    );

    let style = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical.to_string()),
            node_kind: Some(VirtualNodeKind::Style { index: 0 }),
            compile_profile: bundler_profile(true),
        })
        .expect("the scoped CSS stopped publishing");
    assert_eq!(
        style.code.as_ref(),
        "\n  .root.svelte-qeeybc { color: red; }\n",
        "the scoped CSS's emitted bytes moved"
    );
    assert_eq!(style.lang.as_deref(), Some("css"), "the CSS lang moved");
    assert!(
        style.source_map.is_some(),
        "the CSS source map was withheld"
    );
}

/// A Vue SFC keeps publishing its NON-runtime products: the script node with
/// its map, and a declaration surface whose declared prop and declaration-only
/// property are read from the CHECKER, not from its bytes.
///
/// The prop and the ambient-context property are asserted by
/// `public_api_typescript_observation`, which observes the same surface inside
/// the pinned Vue closure. What this cold-path test pins is that the products
/// still publish and that the script node's exact bytes and map are unchanged.
#[test]
fn a_vue_carrier_keeps_publishing_its_non_runtime_products() {
    let canonical = "/probe/Cold.vue";
    let host = host_with(
        canonical,
        VUE_PROPS_EMIT,
        verter_language::FileLanguage::vue(),
    );

    let script = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical.to_string()),
            node_kind: Some(VirtualNodeKind::Script),
            compile_profile: bundler_profile(true),
        })
        .expect("the Vue script node stopped publishing");
    assert_eq!(
        script.lang.as_deref(),
        Some("ts"),
        "the Vue script node's lang moved"
    );
    assert!(
        script.source_map.is_some(),
        "the Vue script node's map was withheld"
    );
    // BEHAVIOUR: the script node's own declarations, pinned exactly. Any
    // change to what the setup block emits fails here.
    assert_eq!(
        script.code.as_ref(),
        include_str!("cold_path_vue_script_node.txt"),
        "the Vue script node's emitted bytes moved"
    );

    // The declaration surface still publishes for the same canonical, and both
    // declaration modes still differ from each other (so a correction cannot
    // silently collapse `Declaration` into `Public`).
    let declaration = host
        .get_public_api_with_mode(canonical, PublicApiMode::Declaration, None)
        .expect("vue declaration mode")
        .expect("vue publishes a declaration surface");
    let public = host
        .get_public_api_with_mode(canonical, PublicApiMode::Public, None)
        .expect("vue public mode")
        .expect("vue publishes a public surface");
    assert_ne!(
        declaration.ts_labeled_code(),
        public.ts_labeled_code(),
        "the Declaration and Public surfaces collapsed into one"
    );
    assert!(
        declaration.source_map.is_some(),
        "the declaration surface's map was withheld"
    );
}

/// The OPTIONS-taking audited compile entry is a distinct public spelling from
/// [`VerterHost::compile_with_audit`] — the convenience wrapper hard-codes
/// `source_map: true` while this one takes an explicit `VerterCompileOptions`
/// (`crates/verter_session/src/host_compile_audit.rs:90-116`).
///
/// The `source_map` axis is asserted on the PRODUCT, not on the record's
/// canonical: with the axis on, the compiled script block carries a non-empty
/// source map; with it off, that map is empty. A route that ignored the axis
/// would produce the same map both ways and fail here.
#[test]
fn the_options_taking_audited_compile_entry_honours_its_explicit_source_map_axis() {
    use verter_compiler::compile::{CompileTarget, VerterCompileOptions};

    for (canonical, source) in [
        ("/probe/AuditOpts.vue", VUE_PROPS_EMIT),
        ("/probe/AuditOptsStyled.vue", VUE_WITH_STYLE),
    ] {
        let host = host_with(canonical, source, verter_language::FileLanguage::vue());
        let mut observed = Vec::new();
        for source_map in [true, false] {
            let audited = host.compile_with_audit_options(
                canonical,
                CompileTarget::BUNDLER,
                VerterCompileOptions {
                    source_map,
                    ..VerterCompileOptions::default()
                },
            );
            assert_eq!(
                audited.audit().canonical_id.as_str(),
                canonical,
                "the options-taking audited entry recorded a different canonical"
            );
            let compiled = audited.as_result().unwrap_or_else(|error| {
                panic!("{canonical}: the audited compile failed: {error:?}")
            });
            let script = compiled
                .script
                .as_ref()
                .unwrap_or_else(|| panic!("{canonical}: the compile produced no script block"));
            observed.push((source_map, script.code.clone(), script.source_map.clone()));
        }

        let (_, on_code, on_map) = &observed[0];
        let (_, off_code, off_map) = &observed[1];
        assert!(
            !on_map.is_empty(),
            "{canonical}: `source_map: true` produced an EMPTY script source map, so the axis \
             is not reaching the product"
        );
        assert!(
            off_map.is_empty(),
            "{canonical}: `source_map: false` produced a source map anyway ({} bytes), so the \
             axis is ignored",
            off_map.len()
        );
        assert_eq!(
            on_code, off_code,
            "{canonical}: the source-map axis changed the emitted script bytes"
        );
    }
}

/// The convenience wrapper is the options entry at its own fixed axis: it
/// produces byte-identical output to an explicit `source_map: true` request,
/// map included.
#[test]
fn the_audited_wrapper_equals_the_options_entry_at_the_axis_the_wrapper_fixes() {
    use verter_compiler::compile::{CompileTarget, VerterCompileOptions};

    for (canonical, source, language) in [
        (
            "/probe/AuditWrap.vue",
            VUE_PROPS_EMIT,
            verter_language::FileLanguage::vue(),
        ),
        (
            "/probe/AuditWrap.svelte",
            SVELTE_BASIC_RUNES,
            verter_language::FileLanguage::svelte(),
        ),
    ] {
        let host = host_with(canonical, source, language);
        let wrapper = host.compile_with_audit(canonical, CompileTarget::BUNDLER);
        let explicit = host.compile_with_audit_options(
            canonical,
            CompileTarget::BUNDLER,
            VerterCompileOptions {
                source_map: true,
                ..VerterCompileOptions::default()
            },
        );
        assert_eq!(
            wrapper.audit().canonical_id,
            explicit.audit().canonical_id,
            "the two audited spellings recorded different canonicals for one request"
        );
        let left = wrapper
            .as_result()
            .unwrap_or_else(|error| panic!("{canonical}: the wrapper failed: {error:?}"));
        let right = explicit
            .as_result()
            .unwrap_or_else(|error| panic!("{canonical}: the explicit request failed: {error:?}"));
        assert_eq!(
            left.script.as_ref().map(|block| &block.code),
            right.script.as_ref().map(|block| &block.code),
            "{canonical}: the wrapper and the explicit request produced different script bytes"
        );
        assert_eq!(
            left.script.as_ref().map(|block| &block.source_map),
            right.script.as_ref().map(|block| &block.source_map),
            "{canonical}: the wrapper and the explicit request produced different script maps"
        );
        assert_eq!(
            left.styles.len(),
            right.styles.len(),
            "{canonical}: the two spellings produced different style-block counts"
        );
    }
}

/// The audited compile entry, pinned by the BYTES and the block shape it
/// publishes for each carrier — not by the mere existence of a record.
///
/// Vue publishes a script block and a template block; the exact script bytes are
/// committed beside this file, so a change to the audited lane's output fails
/// here whether it improves or regresses. Svelte publishes NO neutral block at
/// all through this entry — its module comes from the carrier bundle — while the
/// same host's virtual-file route publishes a non-empty module for the identical
/// request. Both directions of that asymmetry are asserted.
#[test]
fn the_audited_compile_entry_publishes_these_exact_blocks_for_both_frameworks() {
    const AUDITED_VUE_SCRIPT: &str = include_str!("audited_entry_vue_script_block.txt");

    for (canonical, source, language, expects_neutral_blocks) in [
        (
            "/probe/Audited.vue",
            VUE_PROPS_EMIT,
            verter_language::FileLanguage::vue(),
            true,
        ),
        (
            "/probe/Audited.svelte",
            SVELTE_BASIC_RUNES,
            verter_language::FileLanguage::svelte(),
            false,
        ),
    ] {
        let host = host_with(canonical, source, language);
        let audited =
            host.compile_with_audit(canonical, verter_compiler::compile::CompileTarget::BUNDLER);
        assert_eq!(
            audited.audit().canonical_id.as_str(),
            canonical,
            "the audited compile recorded a different canonical"
        );
        let compiled = audited
            .as_result()
            .unwrap_or_else(|error| panic!("{canonical}: the audited compile failed: {error:?}"));

        assert_eq!(
            compiled.script.is_some(),
            expects_neutral_blocks,
            "{canonical}: the audited entry's script-block presence moved"
        );
        assert_eq!(
            compiled.template.is_some(),
            expects_neutral_blocks,
            "{canonical}: the audited entry's template-block presence moved"
        );
        assert!(
            compiled.styles.is_empty(),
            "{canonical}: the audited entry published {} style block(s) for a source that \
             authors none",
            compiled.styles.len()
        );

        if expects_neutral_blocks {
            let script = compiled
                .script
                .as_ref()
                .expect("presence was just asserted")
                .code
                .as_str()
                .replace("\r\n", "\n");
            assert_eq!(
                script,
                AUDITED_VUE_SCRIPT.replace("\r\n", "\n"),
                "{canonical}: the audited script block is no longer the committed bytes"
            );
        }

        // The route answers the same request through the carrier registry. For
        // Svelte that is where the module lives, so an empty audited block set
        // must NOT mean an empty product.
        let response = host
            .get_virtual_file(VirtualQuery {
                raw_id: None,
                canonical_id: Some(canonical.to_string()),
                node_kind: Some(VirtualNodeKind::Main),
                compile_profile: crate::host_compile::compile_profile_for_bundler(),
            })
            .unwrap_or_else(|error| panic!("{canonical}: the route refused: {error:?}"));
        assert!(
            !response.code.is_empty(),
            "{canonical}: the route published an empty module"
        );
    }
}

/// The standalone CSS spelling, driven at the exact function the transport
/// calls.
///
/// `processStyle` (`crates/verter_napi/src/lib.rs:157`) forwards straight to
/// `verter_compiler::css::process_style` (`crates/verter_compiler/src/css/mod.rs:92`)
/// with no `VerterHost` anywhere in the call. That is what this drives: the
/// same function, with the same option shape the transport builds.
///
/// Two facts are recorded. The CSS product itself is produced and carries the
/// requested scope id. The `sourcemap` REQUEST AXIS is inert: both return sites
/// of `process_style` hard-code `source_map: None`
/// (`crates/verter_compiler/src/css/mod.rs:110` and `:145`), so the option is
/// accepted and ignored and the transport's `sourceMap` result field can never
/// be populated through this route. This test states that as it stands — it
/// fails if the axis becomes live, which is exactly the change a correction
/// would make.
#[test]
fn the_standalone_css_spelling_publishes_css_and_ignores_its_source_map_axis() {
    let options = verter_compiler::css::ProcessStyleOptions {
        scope_id: "probe1234",
        scoped: true,
        is_module: false,
        module_name: None,
        filename: Some("/probe/Styled.vue"),
        sourcemap: true,
    };
    let requested = verter_compiler::css::process_style(".x{color:red}", &options)
        .expect("the standalone CSS route processes a valid block");
    // TYPED result fields plus an EXACT byte pin of the emitted CSS — not a
    // substring search for the scope id inside generated output.
    assert!(
        requested.scoped,
        "the standalone CSS route no longer reports the block as scoped"
    );
    assert!(
        requested.normalization_needed,
        "the standalone CSS route no longer normalizes a scoped block"
    );
    assert_eq!(
        requested.code.as_ref(),
        ".x[data-v-probe1234]{\n  color: red;\n}\n",
        "the standalone CSS route's emitted bytes moved"
    );
    assert!(
        requested.source_map.is_none(),
        "the standalone CSS route's `sourcemap` axis has become live; it was inert when this \
         inventory was taken and the recorded route description must be updated"
    );

    let unrequested = verter_compiler::css::process_style(
        ".x{color:red}",
        &verter_compiler::css::ProcessStyleOptions {
            sourcemap: false,
            ..options
        },
    )
    .expect("the standalone CSS route processes a valid block");
    assert!(
        unrequested.source_map.is_none(),
        "the standalone CSS route published a source map that was never requested"
    );
    assert_eq!(
        requested.code, unrequested.code,
        "the `sourcemap` axis changed the emitted CSS bytes"
    );
}

/// A requested standalone CSS map is a real product on both the passthrough
/// and transformed branches. Each map must be valid, retain the authored
/// source, and contain at least one mapping rather than being a placeholder.
#[test]
#[ignore = "standalone CSS still drops requested source maps on both processing branches"]
fn the_standalone_css_route_publishes_valid_requested_maps_for_passthrough_and_transformed_css() {
    let source = ".x{color:red}";
    let passthrough = verter_compiler::css::process_style(
        source,
        &verter_compiler::css::ProcessStyleOptions {
            scope_id: "probe1234",
            scoped: false,
            is_module: false,
            module_name: None,
            filename: Some("/probe/Passthrough.css"),
            sourcemap: true,
        },
    )
    .expect("valid passthrough CSS is processed");
    let transformed = verter_compiler::css::process_style(
        source,
        &verter_compiler::css::ProcessStyleOptions {
            scope_id: "probe1234",
            scoped: true,
            is_module: false,
            module_name: None,
            filename: Some("/probe/Transformed.css"),
            sourcemap: true,
        },
    )
    .expect("valid scoped CSS is processed");

    for (label, result) in [("passthrough", passthrough), ("transformed", transformed)] {
        let json = result.source_map.unwrap_or_else(|| {
            panic!("the {label} branch published CSS without its requested source map")
        });
        let map = verter_compiler::oxc_sourcemap::OwnedSourceMap::from_json_string(&json)
            .unwrap_or_else(|error| panic!("the {label} branch published an invalid map: {error}"));
        assert_eq!(
            map.get_source_content(0),
            Some(source),
            "the {label} map does not retain the authored CSS source"
        );
        assert!(
            map.get_tokens()
                .any(|token| token.get_source_id().is_some()),
            "the {label} map contains no authored-to-generated mappings"
        );
    }
}

// ══════════════════════════════════════════════════════════════════════════
// The remaining enumerated products
// ══════════════════════════════════════════════════════════════════════════

/// The TSC product, driven.
///
/// `CompileTarget::TSC` is a public bit on the target a caller hands
/// `compile_with_audit` / `compile_with_audit_options`, and it drives the TSC
/// codegen stage (`crates/verter_compiler/src/compile/mod.rs:1741`,
/// `needs_tsc` at `crates/verter_compiler/src/compile/types.rs:112`). This
/// records what the product is for each carrier, and that the bit genuinely
/// gates it — a target without TSC must produce none.
#[test]
fn the_tsc_product_is_published_only_when_its_target_bit_is_requested() {
    use verter_compiler::compile::CompileTarget;

    for (canonical, source, language, expect_tsc) in [
        (
            "/probe/Tsc.vue",
            VUE_PROPS_EMIT,
            verter_language::FileLanguage::vue(),
            true,
        ),
        (
            "/probe/Tsc.svelte",
            SVELTE_BASIC_RUNES,
            verter_language::FileLanguage::svelte(),
            false,
        ),
    ] {
        let host = host_with(canonical, source, language);

        let with_tsc = host.compile_with_audit(canonical, CompileTarget::TSC);
        let compiled = with_tsc.as_result().unwrap_or_else(|error| {
            panic!("{canonical}: the TSC-target compile failed: {error:?}")
        });
        assert_eq!(
            compiled.tsc.is_some(),
            expect_tsc,
            "{canonical}: the TSC target produced tsc={:?}, recorded as {expect_tsc}",
            compiled.tsc.is_some()
        );
        if let Some(tsc) = &compiled.tsc {
            assert!(
                !tsc.code.is_empty(),
                "{canonical}: the TSC product is empty"
            );
        }

        // The bit GATES it: a bundler target produces no TSC product at all.
        let without_tsc = host.compile_with_audit(canonical, CompileTarget::BUNDLER);
        let bundler = without_tsc
            .as_result()
            .unwrap_or_else(|error| panic!("{canonical}: the bundler compile failed: {error:?}"));
        assert!(
            bundler.tsc.is_none(),
            "{canonical}: a target WITHOUT the TSC bit produced a TSC product anyway, so the bit \
             does not gate the product"
        );
    }
}

/// The diagnostics route, driven.
///
/// `get_diagnostics` reads the compile slot's own `DiagnosticsSnapshot`
/// (`crates/verter_session/src/host_manage/analysis_io.rs:1947`). It answers
/// `None` before a slot exists for the profile, and the snapshot afterwards —
/// and the same snapshot rides every `VirtualFileResponse`, which is what this
/// asserts rather than assuming.
#[test]
fn the_diagnostics_route_answers_from_the_compile_slot_it_names() {
    for (canonical, source, language) in [
        (
            "/probe/Diag.vue",
            VUE_PROPS_EMIT,
            verter_language::FileLanguage::vue(),
        ),
        (
            "/probe/Diag.svelte",
            SVELTE_BASIC_RUNES,
            verter_language::FileLanguage::svelte(),
        ),
    ] {
        let host = host_with(canonical, source, language);
        let profile = bundler_profile(true);

        // Before any compile for THIS profile there is no slot to read.
        assert!(
            host.get_diagnostics(canonical, &profile).is_none(),
            "{canonical}: the diagnostics route answered before a compile slot existed"
        );

        let response = host
            .get_virtual_file(VirtualQuery {
                raw_id: None,
                canonical_id: Some(canonical.to_string()),
                node_kind: Some(VirtualNodeKind::Main),
                compile_profile: profile.clone(),
            })
            .expect("the carrier publishes a Main module");

        let snapshot = host
            .get_diagnostics(canonical, &profile)
            .unwrap_or_else(|| panic!("{canonical}: no diagnostics after a successful compile"));
        // The route's answer IS the snapshot the product carried — same route,
        // same slot, one snapshot.
        assert_eq!(
            snapshot.diagnostics.len(),
            response.diagnostics.diagnostics.len(),
            "{canonical}: the diagnostics route and the published product disagree on the \
             snapshot"
        );
        for (from_route, from_product) in snapshot
            .diagnostics
            .iter()
            .zip(response.diagnostics.diagnostics.iter())
        {
            assert_eq!(
                (&from_route.code, &from_route.message),
                (&from_product.code, &from_product.message),
                "{canonical}: a diagnostic differs between the route and the published product"
            );
        }
    }
}

/// CONFORMANCE TARGET — currently FAILS, deliberately `#[ignore]`d.
///
/// **What is wrong:** a single compile request that asks for BOTH the runtime
/// and the IDE product publishes the IDE/TSX product even when the runtime
/// surface was refused. The carrier produces the IDE projection unconditionally
/// of the runtime outcome
/// (`crates/verter_compiler/src/svelte/carrier.rs:517-530`), so one request
/// returns a typed refusal for one product and a published artifact for
/// another.
///
/// **Behaviour this demands:** a request that is refused publishes NO product —
/// the IDE/TSX artifact is withheld alongside the runtime module, so the
/// request's outcome is atomic across the products it asked for.
///
/// **Acceptance:** un-ignoring this test is the acceptance gate for that
/// correction. This module owns no correction and adds no withholding path.
#[test]
#[ignore = "conformance target: a combined IDE-requesting compile publishes the TSX product after a runtime refusal"]
fn a_refused_combined_request_publishes_no_product_at_all() {
    for (label, canonical, source, profile) in [
        (
            "server-generate",
            "/probe/AtomicIde.svelte",
            SVELTE_STYLED,
            CompileProfile {
                target: verter_compiler::compile::CompileTarget::IDE,
                ssr: true,
                source_map: true,
                ..CompileProfile::default()
            },
        ),
        (
            "advanced-rune",
            "/probe/AtomicIdeRune.svelte",
            SVELTE_PROPS_EVENTS,
            CompileProfile {
                target: verter_compiler::compile::CompileTarget::IDE,
                source_map: true,
                ..CompileProfile::default()
            },
        ),
    ] {
        let host = host_with(canonical, source, verter_language::FileLanguage::svelte());

        // The runtime product IS refused for this request.
        let main = read_node(&host, canonical, VirtualNodeKind::Main, &profile);
        assert!(
            matches!(main, NodeOutcome::Refused { .. }),
            "[{label}] the runtime product is no longer refused ({main:?}), so this target is \
             measuring something else"
        );

        // Therefore no OTHER product of the same request may be published.
        assert!(
            !host
                .ensure_ide_compiled(canonical, &profile)
                .unwrap_or_else(|error| panic!("[{label}] ensure_ide_compiled: {error:?}")),
            "[{label}] the same request published an IDE projection alongside its typed runtime \
             refusal"
        );
        assert!(
            host.get_ide(canonical, &profile).is_none(),
            "[{label}] the IDE/TSX product survived the refusal"
        );
    }
}
