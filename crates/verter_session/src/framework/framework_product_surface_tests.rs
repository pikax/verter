//! Reachable-success surface of host framework product routes (not Vue
//! runtime-render content). Vue `Main`/`Template` appear only for their
//! publication contract.
//!
//! Inventory: `framework_product_surface_inventory.json`. Exhaustive
//! `match` maps every `VirtualNodeKind` / `PublicApiMode` onto an id —
//! a new variant is a compile error. Every named `hostEntryPoint` is
//! called. Transport `routeAliases` live outside this crate and are
//! recorded citations, not executed here.
//!
//! ```text
//! cargo test -p verter_session --lib framework_product_surface -- --test-threads=1
//! ```
//!
//! Read the `running N tests` line, never the exit code: a libtest
//! filter that matches nothing still exits 0.

use std::sync::Arc;

use verter_compiler::svelte::runtime::UnsupportedSvelteRuntimeSurface;

use crate::{
    CompileProfile, HostConfig, HostError, PublicApiMode, UpsertRequest, VerterHost,
    VirtualNodeKind, VirtualQuery,
};

/// Mutual, compile-enforced census registration.
///
/// The census lives outside this module so emptying the suite cannot
/// delete the check. This test consumes a census-owned item and the
/// census names this function, so removing either `mod` is a compile
/// error — not a filter that matches nothing and still exits 0.
#[test]
pub(crate) fn this_suite_is_registered_with_the_census() {
    assert!(
        super::suite_census::covers(&this_suite_is_registered_with_the_census),
        "{}: the census carries no test for this suite, so this suite's documented invocation \
         could match nothing and still report success",
        super::suite_census::witness_identity(&this_suite_is_registered_with_the_census)
    );
}

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
/// A Svelte component whose runtime surface the client backend refuses: an
/// instance-script prop WRITE, which official lowers through the prop SETTER
/// and this backend does not emit. An instance-script prop READ is a SUPPORTED
/// surface, so a read-only component is no longer a refusal witness.
const SVELTE_ADVANCED_RUNE_REFUSAL: &str = "<script>\n  let { count = 0 } = $props();\n  function inc() { count += 1; }\n</script>\n\n<button onclick={inc}>{count}</button>\n";

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

// The inventory is complete over every product axis the type system has

/// Inventory id for one virtual-node product. Exhaustive match: a new
/// `VirtualNodeKind` is a compile error until the inventory names it.
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
    // Every `product_id_for` arm must be in `all_node_kinds`.
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
        // Every case needs a host entry point or a transport alias.
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

    // Standalone CSS has no host route.
    assert!(
        inventory_product_ids().contains(TSC_PRODUCT_ID),
        "the inventory names no `{TSC_PRODUCT_ID}` product, but the TSC target bit publishes one"
    );

    for (id, spelling) in [
        (
            "css.prepare-style-for-preprocessor",
            "prepare_style_for_preprocessor (private helper, not a module export)",
        ),
        (
            "css.transform-vue-style",
            "transform_vue_style (private helper, not a module export)",
        ),
        (
            "css.analyze-style",
            "analyze_style (private helper, not a module export)",
        ),
    ] {
        let case = cases
            .iter()
            .find(|case| case["id"] == id)
            .unwrap_or_else(|| panic!("the inventory does not classify `{id}`"));
        assert!(case["hostEntryPoint"].is_null(), "{id} gained a host route");
        let aliases = case["routeAliases"]
            .as_array()
            .expect("style cases carry route aliases");
        assert_eq!(aliases.len(), 1, "{id} must classify one private helper");
        assert_eq!(aliases[0]["transport"], "napi");
        assert_eq!(aliases[0]["spelling"], spelling);
        assert!(
            aliases[0]["spelling"]
                .as_str()
                .is_some_and(|value| value.contains("private helper, not a module export")),
            "{id} must stay a private helper, not a live NAPI export"
        );
    }

    let css = cases
        .iter()
        .find(|case| case["id"] == "css.process-style")
        .expect("the inventory names the standalone CSS spelling");
    assert!(
        css["hostEntryPoint"].is_null(),
        "the standalone CSS spelling gained a host route; the inventory's claim that it \
         bypasses the host is stale"
    );
    let live_aliases = css["routeAliases"]
        .as_array()
        .expect("the live style route carries aliases");
    assert!(
        live_aliases.iter().any(|alias| {
            alias["transport"] == "napi"
                && alias["spelling"] == "processStyle (free function, not a host method)"
        }),
        "the live processStyle NAPI export must remain classified as reachable"
    );
    assert!(
        live_aliases
            .iter()
            .any(|alias| alias["transport"] == "bundler"),
        "the retained bundler route must remain represented"
    );
}

// Every enumerated cell, driven, with its exact result recorded

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

/// Source-map axis as recorded: Vue main/script/template and Svelte
/// module/CSS, not Vue style.
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
        // Semantics live in the TypeScript observation suite; this owns
        // publication and map presence.
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

// Atomic publication, both directions

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

/// Refusal inventory from the compiler's typed taxonomy. Exhaustive
/// `match`: a new variant is a compile error until classified. `Driven`
/// without a cell fails below.
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
            SVELTE_ADVANCED_RUNE_REFUSAL,
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

/// Every `Driven` variant has a cell; every cell names a `Driven` variant.
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

        // Other node kinds must be Missing — a Published is a partial product.
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

/// Refusal is scoped to the requesting identity, not the component. IDE
/// and PublicApi identities still publish. Combined-identity refusal is
/// [`a_refused_combined_request_publishes_no_product_at_all`].
#[test]
fn a_runtime_refusal_is_scoped_to_its_identity_and_leaves_the_other_identities_publishing() {
    for (label, canonical, source, profile, _) in refusal_cells() {
        let host = host_with(canonical, source, verter_language::FileLanguage::svelte());

        // IDE-only identity: no runtime surface requested.
        let ide_profile = CompileProfile {
            target: crate::CompileTarget::IDE,
            ..profile.clone()
        };
        let ensured = host
            .ensure_ide_compiled(canonical, &ide_profile)
            .unwrap_or_else(|error| panic!("[{label}] ensure_ide_compiled: {error:?}"));
        assert!(
            ensured,
            "[{label}] the IDE-only identity reports no IDE surface for a component whose \
             RUNTIME surface is refused"
        );
        let ide = host
            .get_ide(canonical, &ide_profile)
            .unwrap_or_else(|| panic!("[{label}] no IDE product after a successful ensure"));
        assert!(
            !ide.code.is_empty(),
            "[{label}] the IDE-only identity published an empty IDE product"
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
        SVELTE_ADVANCED_RUNE_REFUSAL,
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

// Cold-path preservation

/// Cold path: supported Svelte client still publishes module + scoped CSS
/// with maps. The cell a refusal/publication fix is most likely to over-reach.
#[test]
fn a_supported_svelte_client_component_keeps_publishing_its_module_and_its_css() {
    let canonical = "/probe/Cold.svelte";
    let host = host_with(
        canonical,
        SVELTE_STYLED,
        verter_language::FileLanguage::svelte(),
    );

    // Exact bytes, not a length check.
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

/// Vue non-runtime cold path: script node bytes + map. Prop semantics live
/// in `public_api_typescript_observation`.
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
    // Exact script-node bytes.
    assert_eq!(
        script.code.as_ref(),
        include_str!("cold_path_vue_script_node.txt"),
        "the Vue script node's emitted bytes moved"
    );

    // Declaration and Public surfaces must stay distinct.
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

/// Options-taking audited compile vs the wrapper (hard-coded `source_map:
/// true`). Axis is on the product, not the audit record.
#[test]
fn the_options_taking_audited_compile_entry_honours_its_explicit_source_map_axis() {
    use crate::host_compile_audit::CompileAuditOverrides;
    use crate::CompileTarget;

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
                CompileAuditOverrides {
                    source_map,
                    ..CompileAuditOverrides::default()
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
    use crate::host_compile_audit::CompileAuditOverrides;
    use crate::CompileTarget;

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
            CompileAuditOverrides {
                source_map: true,
                ..CompileAuditOverrides::default()
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

/// Audited compile: Vue script/template bytes (committed); Svelte publishes
/// no neutral block here (module is the carrier bundle). Virtual-file
/// route still publishes a non-empty module for both.
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
        let audited = host.compile_with_audit(canonical, crate::CompileTarget::BUNDLER);
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

        // Virtual-file route still publishes (Svelte's module lives there).
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

/// Standalone CSS via `process_style` (no host). Product carries the
/// requested scope id. The `sourcemap` axis is inert (`source_map: None`
/// hard-coded) — fails if the axis becomes live.
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

// The remaining enumerated products

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
    use crate::CompileTarget;

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

/// A request identity that asks for BOTH the runtime products and the IDE
/// product is ATOMIC over the products it asked for: when its runtime surface
/// is refused it publishes NO product at all — no runtime module, no CSS, and
/// no IDE/TSX artifact.
///
/// The refusal is a property of the REQUESTED-PRODUCT SET, not of the source.
/// Four controls keep this from being satisfiable by "never publish anything":
///
/// * a DISTINCT IDE-only identity on the same refusing source still publishes a
///   non-empty IDE product (no runtime product was asked for, so nothing can be
///   refused);
/// * the separate PublicApi identity still renders;
/// * both orders — combined-first and IDE-only-first, on independent hosts —
///   produce the same outcomes, so neither identity leaks into the other;
/// * a SUPPORTED component under the SAME combined identity publishes BOTH its
///   runtime module and a non-empty IDE product.
#[test]
fn a_refused_combined_request_publishes_no_product_at_all() {
    use crate::CompileTarget;

    /// The combined identity: the runtime product set (`BUNDLER` — style,
    /// script, template) AND the IDE product (`TSX`), asked for by ONE request.
    fn combined(base: &CompileProfile) -> CompileProfile {
        CompileProfile {
            target: CompileTarget::BUNDLER | CompileTarget::IDE,
            ..base.clone()
        }
    }

    /// A DISTINCT identity that asks for the IDE product only.
    fn ide_only(base: &CompileProfile) -> CompileProfile {
        CompileProfile {
            target: CompileTarget::IDE,
            ..base.clone()
        }
    }

    /// The combined request publishes NOTHING: the runtime node is the typed
    /// refusal carrying the compiler's own code, every other node is absent,
    /// and the IDE product is neither cached nor ensurable.
    #[track_caller]
    fn assert_combined_publishes_nothing(
        label: &str,
        host: &VerterHost,
        canonical: &str,
        profile: &CompileProfile,
        expected_code: &str,
    ) {
        let main = read_node(host, canonical, VirtualNodeKind::Main, profile);
        assert_eq!(
            main,
            NodeOutcome::Refused {
                diagnostic_code: expected_code.to_string()
            },
            "[{label}] the combined request's runtime product must be the typed refusal"
        );

        for kind in all_node_kinds() {
            if kind == VirtualNodeKind::Main {
                continue;
            }
            assert_eq!(
                read_node(host, canonical, kind.clone(), profile),
                NodeOutcome::Missing,
                "[{label}] {kind:?} survived the combined request's runtime refusal"
            );
        }

        assert!(
            host.get_ide(canonical, profile).is_none(),
            "[{label}] the IDE/TSX product was published under the refused combined identity"
        );
        match host.ensure_ide_compiled(canonical, profile) {
            Err(HostError::RuntimeSurfaceRefused {
                diagnostic_code, ..
            }) => assert_eq!(
                diagnostic_code, expected_code,
                "[{label}] the IDE-ensure refusal names a different surface than the runtime one"
            ),
            other => panic!(
                "[{label}] the combined request's IDE-ensure must be a typed \
                 RuntimeSurfaceRefused — a refused request publishing an IDE projection is the \
                 mixed outcome this test forbids. Got: {other:?}"
            ),
        }
    }

    /// The IDE-only identity on the SAME source still publishes: it asked for
    /// no runtime product, so there is nothing to refuse.
    #[track_caller]
    fn assert_ide_only_still_publishes(
        label: &str,
        host: &VerterHost,
        canonical: &str,
        profile: &CompileProfile,
    ) {
        assert!(
            host.ensure_ide_compiled(canonical, profile)
                .unwrap_or_else(|error| panic!(
                    "[{label}] ide-only ensure_ide_compiled: {error:?}"
                )),
            "[{label}] the IDE-only identity reports no IDE surface"
        );
        let ide = host
            .get_ide(canonical, profile)
            .unwrap_or_else(|| panic!("[{label}] no IDE product under the IDE-only identity"));
        assert!(
            !ide.code.is_empty(),
            "[{label}] the IDE-only identity published an EMPTY IDE product"
        );
    }

    for (label, canonical, source, base_profile, expected_code) in refusal_cells() {
        let combined_profile = combined(&base_profile);
        let ide_profile = ide_only(&base_profile);

        // ── Order A: combined first, then the IDE-only identity. An earlier
        //    combined refusal must not poison the separate IDE-only request.
        let host = host_with(canonical, source, verter_language::FileLanguage::svelte());
        assert_combined_publishes_nothing(
            &format!("{label}/combined-first"),
            &host,
            canonical,
            &combined_profile,
            &expected_code,
        );
        assert_ide_only_still_publishes(
            &format!("{label}/combined-first"),
            &host,
            canonical,
            &ide_profile,
        );

        // ── Order B: the IDE-only identity first, then the combined one. An
        //    earlier IDE success must not leak into the combined refusal.
        let host = host_with(canonical, source, verter_language::FileLanguage::svelte());
        assert_ide_only_still_publishes(
            &format!("{label}/ide-first"),
            &host,
            canonical,
            &ide_profile,
        );
        assert_combined_publishes_nothing(
            &format!("{label}/ide-first"),
            &host,
            canonical,
            &combined_profile,
            &expected_code,
        );

        // ── Control: the PublicApi identity is independent of the compile
        //    request's product set and still renders.
        assert!(
            host.get_public_api_with_mode(canonical, PublicApiMode::Public, None)
                .unwrap_or_else(|error| panic!("[{label}] public api: {error:?}"))
                .is_some(),
            "[{label}] the separate public-API identity was withheld"
        );
    }

    // ── Control: "refuse every combined request" must NOT satisfy the above. A
    //    SUPPORTED component under the same combined identity publishes BOTH
    //    its runtime module and a non-empty IDE product.
    let supported = "/probe/AtomicSupported.svelte";
    let host = host_with(
        supported,
        SVELTE_STYLED,
        verter_language::FileLanguage::svelte(),
    );
    let supported_profile = combined(&bundler_profile(true));
    match read_node(&host, supported, VirtualNodeKind::Main, &supported_profile) {
        NodeOutcome::Published { code_len, .. } => assert!(
            code_len > 0,
            "the supported component published an EMPTY runtime module under the combined identity"
        ),
        other => panic!(
            "the supported component's runtime module was withheld under the combined identity — \
             the correction over-reached into a clean compile. Got: {other:?}"
        ),
    }
    assert!(
        host.ensure_ide_compiled(supported, &supported_profile)
            .expect("supported combined ensure_ide_compiled"),
        "the supported component reports no IDE surface under the combined identity"
    );
    assert!(
        !host
            .get_ide(supported, &supported_profile)
            .expect("the supported component has an IDE product under the combined identity")
            .code
            .is_empty(),
        "the supported component's IDE product is empty under the combined identity"
    );
}

// One transaction boundary: every route observes the same terminal answer,
// and a request publishes all and only the products it asked for.

/// A Vue SFC carrying every runtime block kind — script, template, scoped
/// style, AND a custom block — so a publication sweep can observe each virtual
/// node kind appear or stay absent.
const VUE_ALL_BLOCKS: &str = "<script setup lang=\"ts\">\nconst a: number = 1\n</script>\n<template><div class=\"x\">{{ a }}</div></template>\n<style scoped>.x{color:red}</style>\n<i18n>{\"en\":{\"k\":\"v\"}}</i18n>\n";

/// The host's cold-compile counter — the ARTIFACT that says whether a call
/// recompiled. Bumped once per cold run past the warm-hit consult.
pub(crate) fn cold_runs(host: &VerterHost) -> u64 {
    host.provenance_snapshot().compile_cold_runs
}

/// `ensure_compiled` reduced to a comparable answer.
fn ensure_compiled_answer(host: &VerterHost, canonical: &str, profile: &CompileProfile) -> String {
    match host.ensure_compiled(canonical, profile) {
        Ok(()) => "Ok".to_string(),
        Err(HostError::RuntimeSurfaceRefused {
            diagnostic_code, ..
        }) => format!("Refused({diagnostic_code})"),
        Err(HostError::MissingVirtualNode { .. }) => "MissingVirtualNode".to_string(),
        Err(other) => format!("Other({other:?})"),
    }
}

/// `ensure_compiled` gives the SAME answer for one identity whether it is
/// served cold or warm.
///
/// Two cells, each a distinct way the two paths could disagree:
///
/// * a REFUSED identity — the cold path resolves the typed refusal, so the warm
///   path must resolve it too rather than reporting a bare success from a
///   validated slot whose cached arm it never inspected;
/// * an identity that legitimately requests NO runtime module — it must not be
///   told a `Main` it never asked for is missing, on either path.
#[test]
fn ensure_compiled_answers_the_same_cold_and_warm_for_one_identity() {
    use crate::CompileTarget;

    // A refusing Svelte source under an identity that DOES ask for the runtime
    // product, and a supported source under an IDE-only identity.
    let cells: Vec<(
        &str,
        &str,
        &str,
        verter_language::FileLanguage,
        CompileProfile,
    )> = vec![
        (
            "refused/combined",
            "/probe/EnsureRefused.svelte",
            SVELTE_ADVANCED_RUNE_REFUSAL,
            verter_language::FileLanguage::svelte(),
            CompileProfile {
                target: CompileTarget::BUNDLER | CompileTarget::IDE,
                source_map: true,
                ..CompileProfile::default()
            },
        ),
        (
            "supported/ide-only",
            "/probe/EnsureIdeOnly.vue",
            VUE_WITH_STYLE,
            verter_language::FileLanguage::vue(),
            CompileProfile {
                target: CompileTarget::IDE | CompileTarget::TEMPLATE_DATA,
                ..CompileProfile::default()
            },
        ),
        (
            "supported/ide-only-svelte",
            "/probe/EnsureIdeOnly.svelte",
            SVELTE_STYLED,
            verter_language::FileLanguage::svelte(),
            CompileProfile {
                target: CompileTarget::IDE | CompileTarget::TEMPLATE_DATA,
                ..CompileProfile::default()
            },
        ),
    ];

    for (label, canonical, source, language, profile) in cells {
        let host = host_with(canonical, source, language);

        let cold = ensure_compiled_answer(&host, canonical, &profile);
        let after_cold = cold_runs(&host);
        assert!(
            after_cold > 0,
            "[{label}] the first call did not compile at all, so there is no cold answer to \
             compare a warm one against"
        );

        // The second call must be served from the warm slot the first one
        // published. This is asserted on the ARTIFACT — the cold-compile
        // counter — not on the returned string: two calls that both RECOMPILE
        // also return equal strings, so string equality alone proves nothing
        // about the warm path.
        let warm = ensure_compiled_answer(&host, canonical, &profile);
        let after_warm = cold_runs(&host);
        assert_eq!(
            after_cold, after_warm,
            "[{label}] the second call RECOMPILED (cold runs {after_cold} -> {after_warm}), so \
             it never exercised the warm path and the comparison below is cold-vs-cold"
        );
        assert_eq!(
            cold, warm,
            "[{label}] ensure_compiled answered differently cold vs warm for ONE identity \
             (cold={cold}, warm={warm}) — the two paths do not observe the same transaction \
             boundary"
        );

        // And the answer itself must be the right one, so "both paths agree on
        // the WRONG answer" cannot satisfy the equality above.
        match label {
            "refused/combined" => assert!(
                cold.starts_with("Refused("),
                "[{label}] an identity that asked for a refused runtime product must answer the \
                 typed refusal, got {cold}"
            ),
            _ => assert_eq!(
                cold, "Ok",
                "[{label}] an identity that asked for NO runtime module must not report a \
                 missing Main, got {cold}"
            ),
        }
    }
}

/// A refused transaction commits NO product-bearing scheduler artifact.
///
/// The scheduler artifact is the companion warm-hit substrate. A refusal
/// published there as an artifact carrying an empty output map is a refusal
/// re-encoded as an untyped successful empty compile: a consumer reading that
/// substrate cannot tell it apart from a component that genuinely produced
/// nothing.
///
/// The positive control is what keeps this from being satisfied by never
/// committing an artifact at all.
#[test]
fn a_refused_transaction_commits_no_scheduler_artifact() {
    use crate::CompileTarget;

    for (label, canonical, source, base_profile, _) in refusal_cells() {
        let combined = CompileProfile {
            target: CompileTarget::BUNDLER | CompileTarget::IDE,
            ..base_profile
        };
        let host = host_with(canonical, source, verter_language::FileLanguage::svelte());

        // Drive the refusal through the public route.
        assert!(
            matches!(
                read_node(&host, canonical, VirtualNodeKind::Main, &combined),
                NodeOutcome::Refused { .. }
            ),
            "[{label}] this cell no longer refuses, so it measures nothing"
        );

        assert!(
            !crate::for_tests::compile_scheduler_artifact_present_for_tests(
                &host, canonical, &combined
            ),
            "[{label}] a REFUSED transaction committed a scheduler artifact — a consumer \
             reading that substrate sees the refusal as a successful empty compile"
        );
        assert!(
            !crate::for_tests::compile_scheduler_last_known_good_artifact_present_for_tests(
                &host, canonical, &combined
            ),
            "[{label}] a refused transaction left an artifact behind in the map (invisible to \
             the generation-coherent read, visible here)"
        );
    }

    // Positive control: a SUPPORTED component under the same combined identity
    // DOES commit its artifact, so "never commit" cannot satisfy the above.
    let supported = "/probe/ArtifactSupported.svelte";
    let host = host_with(
        supported,
        SVELTE_STYLED,
        verter_language::FileLanguage::svelte(),
    );
    let combined = CompileProfile {
        target: CompileTarget::BUNDLER | CompileTarget::IDE,
        source_map: true,
        ..CompileProfile::default()
    };
    assert!(
        matches!(
            read_node(&host, supported, VirtualNodeKind::Main, &combined),
            NodeOutcome::Published { .. }
        ),
        "the supported control stopped publishing its runtime module"
    );
    assert!(
        crate::for_tests::compile_scheduler_artifact_present_for_tests(&host, supported, &combined),
        "a SUCCESSFUL transaction must still commit its scheduler artifact — otherwise this \
         test would pass by nothing ever being committed"
    );
}

/// Every published virtual node kind was ASKED FOR by the request's target.
///
/// The requested-product set is the target's bits. A request that asks for CSS
/// gets CSS and not a runtime module; a request that asks for template data
/// gets no runtime products at all, even though producing template data
/// legitimately runs script codegen as a prerequisite. A prerequisite is not a
/// product.
#[test]
fn each_request_publishes_exactly_the_node_kinds_its_target_requested() {
    use crate::CompileTarget;
    use std::collections::BTreeSet;

    /// The node kinds actually published for `(canonical, profile)`, as a
    /// comparable set. A refusal is a separate outcome and fails the sweep.
    fn published(
        host: &VerterHost,
        canonical: &str,
        profile: &CompileProfile,
        label: &str,
    ) -> BTreeSet<String> {
        let mut set = BTreeSet::new();
        for kind in all_node_kinds() {
            match read_node(host, canonical, kind.clone(), profile) {
                NodeOutcome::Published { .. } => {
                    set.insert(format!("{kind:?}"));
                }
                NodeOutcome::Missing => {}
                NodeOutcome::Refused { diagnostic_code } => panic!(
                    "[{label}] {kind:?} answered a runtime refusal ({diagnostic_code}); this \
                     sweep's sources are supported"
                ),
            }
        }
        set
    }

    fn set_of(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    let main = "Main";
    let script = "Script";
    let template = "Template";
    let style = "Style { index: 0 }";
    let custom = "Custom { index: 0 }";

    // Every cell states the target and the EXACT node-kind set that target asks
    // for. Vue carries all four runtime block kinds; Svelte's client module is a
    // single ESM plus its scoped CSS.
    let vue_cells: Vec<(&str, CompileTarget, BTreeSet<String>)> = vec![
        // Style alone: CSS, and no runtime module.
        ("STYLE", CompileTarget::STYLE, set_of(&[style])),
        // Template data is not a runtime product; script codegen runs only as
        // its prerequisite.
        (
            "ANALYSIS",
            CompileTarget::ANALYSIS,
            set_of(&[script, main, custom]),
        ),
        ("META", CompileTarget::META, set_of(&[script, main, custom])),
        (
            "IDE|TEMPLATE_DATA",
            CompileTarget::IDE | CompileTarget::TEMPLATE_DATA,
            set_of(&[]),
        ),
        ("IDE", CompileTarget::IDE, set_of(&[])),
        // Positive controls.
        (
            "BUNDLER",
            CompileTarget::BUNDLER,
            set_of(&[main, script, template, style, custom]),
        ),
        (
            "BUNDLER|IDE",
            CompileTarget::BUNDLER | CompileTarget::IDE,
            set_of(&[main, script, template, style, custom]),
        ),
    ];

    for (label, target, expected) in vue_cells {
        let canonical = format!("/probe/PubVue{label}.vue");
        let canonical = canonical.replace(['|'], "_");
        let host = host_with(
            &canonical,
            VUE_ALL_BLOCKS,
            verter_language::FileLanguage::vue(),
        );
        let profile = CompileProfile {
            target,
            source_map: true,
            ..CompileProfile::default()
        };
        assert_eq!(
            published(&host, &canonical, &profile, label),
            expected,
            "[vue/{label}] the published node-kind set is not the set this target asked for"
        );
    }

    let svelte_cells: Vec<(&str, CompileTarget, BTreeSet<String>)> = vec![
        ("STYLE", CompileTarget::STYLE, set_of(&[style])),
        ("ANALYSIS", CompileTarget::ANALYSIS, set_of(&[main])),
        ("META", CompileTarget::META, set_of(&[main])),
        (
            "IDE|TEMPLATE_DATA",
            CompileTarget::IDE | CompileTarget::TEMPLATE_DATA,
            set_of(&[]),
        ),
        ("IDE", CompileTarget::IDE, set_of(&[])),
        ("BUNDLER", CompileTarget::BUNDLER, set_of(&[main, style])),
        (
            "BUNDLER|IDE",
            CompileTarget::BUNDLER | CompileTarget::IDE,
            set_of(&[main, style]),
        ),
    ];

    for (label, target, expected) in svelte_cells {
        let canonical = format!("/probe/PubSvelte{label}.svelte");
        let canonical = canonical.replace(['|'], "_");
        let host = host_with(
            &canonical,
            SVELTE_STYLED,
            verter_language::FileLanguage::svelte(),
        );
        let profile = CompileProfile {
            target,
            source_map: true,
            ..CompileProfile::default()
        };
        assert_eq!(
            published(&host, &canonical, &profile, label),
            expected,
            "[svelte/{label}] the published node-kind set is not the set this target asked for"
        );
    }

    // Positive control on the IDE half: an IDE-bearing identity publishes NO
    // runtime node yet still produces its IDE product, so "publish nothing"
    // cannot satisfy the empty-set cells above.
    for (label, canonical, source, language) in [
        (
            "vue",
            "/probe/PubIdeControl.vue",
            VUE_ALL_BLOCKS,
            verter_language::FileLanguage::vue(),
        ),
        (
            "svelte",
            "/probe/PubIdeControl.svelte",
            SVELTE_STYLED,
            verter_language::FileLanguage::svelte(),
        ),
    ] {
        let host = host_with(canonical, source, language);
        let profile = CompileProfile {
            target: CompileTarget::IDE | CompileTarget::TEMPLATE_DATA,
            ..CompileProfile::default()
        };
        assert!(
            host.ensure_ide_compiled(canonical, &profile)
                .unwrap_or_else(|error| panic!("[{label}] ensure_ide_compiled: {error:?}")),
            "[{label}] the IDE-bearing identity reports no IDE surface"
        );
        assert!(
            !host
                .get_ide(canonical, &profile)
                .unwrap_or_else(|| panic!("[{label}] no IDE product"))
                .code
                .is_empty(),
            "[{label}] the IDE product is empty, so the empty runtime-set cells prove nothing"
        );
    }

    // Positive control on the runtime half: the BUNDLER cells above must carry
    // real bytes, not empty published nodes.
    let canonical = "/probe/PubBundlerBytes.vue";
    let host = host_with(
        canonical,
        VUE_ALL_BLOCKS,
        verter_language::FileLanguage::vue(),
    );
    let profile = CompileProfile {
        target: CompileTarget::BUNDLER,
        source_map: true,
        ..CompileProfile::default()
    };
    match read_node(&host, canonical, VirtualNodeKind::Main, &profile) {
        NodeOutcome::Published { code_len, .. } => assert!(
            code_len > 0,
            "the BUNDLER control published an EMPTY runtime module"
        ),
        other => panic!("the BUNDLER control stopped publishing its runtime module: {other:?}"),
    }
}

/// PRE-EXISTING DIVERGENCE, characterized — not introduced by, and not fixed
/// by, the transaction-boundary work in this suite.
///
/// Under [`CompileCacheMode::Content`], `ensure_ide_compiled` answers
/// `Ok(true)` while an immediately following `get_ide` answers `None`. That
/// contradicts `ensure_ide_compiled`'s own documented contract, which states
/// that `Ok(true)` means "an immediate `get_ide` returns `Some`".
///
/// **Why it happens.** `ensure_ide_compiled` reads the `tsx` off the value
/// `ensure_compile_artifacts` just computed, which exists regardless of WHICH
/// cache node the result was published to. `get_ide` reads only the
/// fact-validated SESSION slot (`peek_tsx`). A `Content`-mode compile publishes
/// to the content-addressed node instead, so the session slot stays empty and
/// the two routes disagree.
///
/// **Pre-existing.** Both halves are byte-identical on the base commit
/// `dd84e5fa2`: `ensure_ide_compiled` ended `Ok(served.tsx.is_some())` and
/// `get_ide` read `session_node.peek_tsx(&cc, profile_hash)?`. Neither was
/// touched by the atomicity work — only the refusal arm was added to
/// `ensure_ide_compiled`.
///
/// **Why `#[ignore]`d.** This test asserts the CORRECT contract, so it FAILS
/// today. It is ignored rather than fixed because the disposition of a
/// pre-existing divergence is not this suite's to make, and because "fixing" it
/// by weakening the assertion would erase the finding. Run it explicitly with
/// `--ignored` to observe the divergence. When the two routes are reconciled,
/// remove the `#[ignore]` and this test passes unchanged.
///
/// The `Session`-mode leg is asserted LIVE below it, so the contract is not
/// unguarded in the mode every interactive consumer actually uses.
#[test]
#[ignore = "pre-existing: Content-mode ensure_ide_compiled reports Ok(true) while get_ide reads the session slot and answers None"]
fn ensure_ide_compiled_and_get_ide_agree_under_content_cache_mode() {
    use crate::types::CompileCacheMode;
    use crate::CompileTarget;

    let canonical = "/probe/ContentModeIde.vue";
    let host = host_with(
        canonical,
        VUE_WITH_STYLE,
        verter_language::FileLanguage::vue(),
    );
    let profile = CompileProfile {
        target: CompileTarget::IDE | CompileTarget::TEMPLATE_DATA,
        requested_mode: CompileCacheMode::Content,
        ..CompileProfile::default()
    };

    let ensured = host
        .ensure_ide_compiled(canonical, &profile)
        .expect("the IDE-ensure must not fail for a supported Vue SFC");
    assert!(
        ensured,
        "precondition: a Vue SFC HAS an IDE surface, so the ensure must report one"
    );
    assert!(
        host.get_ide(canonical, &profile).is_some(),
        "`ensure_ide_compiled` returning Ok(true) documents that an IMMEDIATE `get_ide` \
         returns Some; under Content mode it returns None because the ensure reads the \
         freshly-computed value while `get_ide` reads only the fact-validated session slot"
    );
}

/// The live half of the contract above, in the mode every interactive consumer
/// uses: under `Session` mode `ensure_ide_compiled` reporting an IDE surface
/// means `get_ide` really can read one.
#[test]
fn ensure_ide_compiled_and_get_ide_agree_under_session_cache_mode() {
    use crate::types::CompileCacheMode;
    use crate::CompileTarget;

    for (label, canonical, source, language) in [
        (
            "vue",
            "/probe/SessionModeIde.vue",
            VUE_WITH_STYLE,
            verter_language::FileLanguage::vue(),
        ),
        (
            "svelte",
            "/probe/SessionModeIde.svelte",
            SVELTE_STYLED,
            verter_language::FileLanguage::svelte(),
        ),
    ] {
        let host = host_with(canonical, source, language);
        let profile = CompileProfile {
            target: CompileTarget::IDE | CompileTarget::TEMPLATE_DATA,
            requested_mode: CompileCacheMode::Session,
            ..CompileProfile::default()
        };
        assert!(
            host.ensure_ide_compiled(canonical, &profile)
                .unwrap_or_else(|error| panic!("[{label}] ensure_ide_compiled: {error:?}")),
            "[{label}] the carrier HAS an IDE surface"
        );
        assert!(
            host.get_ide(canonical, &profile).is_some(),
            "[{label}] `ensure_ide_compiled` reported an IDE surface but `get_ide` reads none"
        );
    }
}

/// Reading a TARGET-EXCLUDED node under a warm identity must not recompile.
///
/// With target-scoped publication, a node's ABSENCE is the terminal, correct
/// result for an identity whose target never asked for it. If the warm gate
/// treats that absence as an incomplete compile it recompiles — every read,
/// forever, and never produces the node, because the compile is deterministic
/// for a validated slot.
///
/// This is directly reachable: the LSP lists a carrier's parse-derived runtime
/// nodes and reads each under its `IDE | TEMPLATE_DATA` profile, and MCP probes
/// fixed optional node kinds.
///
/// Asserted on the ARTIFACT — the host's cold-compile counter — because the
/// returned `MissingVirtualNode` is identical whether or not a recompile
/// happened, so the outcome alone proves nothing.
#[test]
fn reading_a_target_excluded_node_under_a_warm_identity_does_not_recompile() {
    use crate::CompileTarget;

    for (label, canonical, source, language) in [
        (
            "vue",
            "/probe/ExcludedNodeWarm.vue",
            VUE_ALL_BLOCKS,
            verter_language::FileLanguage::vue(),
        ),
        (
            "svelte",
            "/probe/ExcludedNodeWarm.svelte",
            SVELTE_STYLED,
            verter_language::FileLanguage::svelte(),
        ),
    ] {
        let host = host_with(canonical, source, language);
        // An identity that asks for the IDE product and template data — and so
        // for NO runtime node at all.
        let profile = CompileProfile {
            target: CompileTarget::IDE | CompileTarget::TEMPLATE_DATA,
            ..CompileProfile::default()
        };

        // Warm the identity once.
        assert!(
            host.ensure_ide_compiled(canonical, &profile)
                .unwrap_or_else(|error| panic!("[{label}] ensure_ide_compiled: {error:?}")),
            "[{label}] the carrier has an IDE surface"
        );
        let warmed = cold_runs(&host);
        assert!(warmed > 0, "[{label}] nothing compiled, so nothing is warm");

        // Now read every runtime node this identity excluded. Each is correctly
        // Missing — and each must be answered from the warm slot.
        for kind in all_node_kinds() {
            let outcome = read_node(&host, canonical, kind.clone(), &profile);
            assert_eq!(
                outcome,
                NodeOutcome::Missing,
                "[{label}] {kind:?} is not excluded by this target, so this cell measures \
                 nothing"
            );
            let after = cold_runs(&host);
            assert_eq!(
                warmed, after,
                "[{label}] reading target-excluded {kind:?} triggered a cold recompile \
                 (cold runs {warmed} -> {after}). The compile is deterministic for a validated \
                 slot, so the recompile can never produce the node — it is futile work on every \
                 read, forever"
            );
        }

        // Reading them AGAIN must still not recompile: the first read must not
        // have quietly republished a slot that only now satisfies the gate.
        for kind in all_node_kinds() {
            let _ = read_node(&host, canonical, kind.clone(), &profile);
        }
        assert_eq!(
            warmed,
            cold_runs(&host),
            "[{label}] a second sweep of target-excluded nodes recompiled"
        );

        // Control: the IDE product this identity DID ask for is still served,
        // so "answer everything from a stale warm slot" is not what passed.
        assert!(
            !host
                .get_ide(canonical, &profile)
                .unwrap_or_else(|| panic!("[{label}] no IDE product"))
                .code
                .is_empty(),
            "[{label}] the requested IDE product went missing"
        );
    }
}
