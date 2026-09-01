//! The canonical-request compile seam: one call executes a
//! caller-supplied [`CompileRequest`] against an already-registered
//! source and returns the typed result.
//!
//! What these prove, per acceptance boundary:
//! - one call compiles-and-returns for a source registered once, and the
//!   route builds no `CompileProfile` from the request (structural: the
//!   seam's own demand is the request, and its output is byte-equal to
//!   the legacy profile-derived route's for the same demand);
//! - every requested product kind is returned in request order, one row
//!   per kind, byte-equivalent to the legacy route;
//! - a request whose framework arm contradicts the registered carrier is
//!   refused, never compiled under the registered carrier;
//! - an unregistered canonical id fails; the public-API and declaration
//!   kinds return the typed unsupported outcome, not an empty success;
//! - a refusal fails the WHOLE request and publishes no sibling product;
//! - the batch returns one entry per input in input order, registers each
//!   source exactly once, and isolates a per-input failure.

use std::sync::Arc;

use verter_compiler::compile_request::{
    AnalysisProductRequest, CompileProduct, CompileRequest, DeclarationProductRequest,
    FrameworkCompileRequest, IdeProductRequest, ProductKind, PublicApiProductRequest,
    RuntimeProductRequest, SvelteCompileRequest, VueBackendRequest, VueCompileRequest,
};

use crate::host_compile::{CompileRequestBatchInput, CompileRequestBatchOptions};
use crate::types::{
    BlockContentAvailability, BlockContentRefusal, BlockOverrideEntry, BlockOverrideRequest,
    CompileProfile, CompileRequestFailure, CompiledProduct, FileLanguage, HostError,
    VirtualNodeKind,
};
use crate::{CompileTarget, HostConfig, UpsertRequest, VerterHost};

const VUE_SFC: &str = r#"<script setup lang="ts">
const greeting = 'hello'
</script>
<template><div class="box">{{ greeting }}</div></template>
<style scoped>.box { color: red }</style>
"#;

const SVELTE_SFC: &str = r#"<script lang="ts">
let name = 'world';
</script>
<h1>hello {name}</h1>
<style>h1 { color: blue }</style>
"#;

/// The Vue equivalence fixture: one of EVERY node kind the runtime
/// product publishes — main, script, template, style, custom — so the
/// oracle below compares a complete node set rather than the subset a
/// minimal carrier happens to emit.
const VUE_SFC_EVERY_NODE: &str = r#"<script setup lang="ts">
const greeting = 'hello'
</script>
<template><div class="box">{{ greeting }}</div></template>
<style scoped>.box { color: red }</style>
<i18n>{ "en": { "hi": "hello" } }</i18n>
"#;

fn new_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

/// A host over an in-memory workspace, for the fixtures whose carrier
/// blocks resolve to real files (`<template src="...">`).
fn new_host_with_files(files: &[(&str, &str)]) -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    for (path, content) in files {
        workspace.inject_file((*path).to_string(), Arc::from(*content));
    }
    Arc::new(VerterHost::new(HostConfig::default(), workspace))
}

fn upsert(host: &VerterHost, canonical_id: &str, source: &str) {
    let language = if canonical_id.ends_with(".svelte") {
        FileLanguage::svelte()
    } else {
        FileLanguage::vue()
    };
    let _registered = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::from(source),
            file_language: language,
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert {canonical_id}: {e:?}"));
}

fn vue_request(products: Vec<CompileProduct>) -> CompileRequest {
    CompileRequest::new(
        products,
        FrameworkCompileRequest::Vue(VueCompileRequest {
            backend: VueBackendRequest::Inferred,
            ssr: false,
            script_custom_element: Some(false),
            ..VueCompileRequest::default()
        }),
        None,
        None,
        None,
        false,
        false,
    )
    .expect("the demand constructs")
}

fn svelte_request(products: Vec<CompileProduct>) -> CompileRequest {
    CompileRequest::new(
        products,
        FrameworkCompileRequest::Svelte(SvelteCompileRequest {
            custom_element: Some(false),
            ..SvelteCompileRequest::default()
        }),
        None,
        None,
        None,
        false,
        false,
    )
    .expect("the demand constructs")
}

fn runtime_client(source_map: bool) -> CompileProduct {
    CompileProduct::RuntimeClient(RuntimeProductRequest {
        runtime_source_map: source_map,
        ..RuntimeProductRequest::default()
    })
}

fn ide(source_map: bool) -> CompileProduct {
    CompileProduct::IdeCompanion(IdeProductRequest {
        want_source_map: source_map,
        ..IdeProductRequest::default()
    })
}

fn analysis() -> CompileProduct {
    CompileProduct::Analysis(AnalysisProductRequest {
        want_script_bindings: false,
        want_template_data: true,
    })
}

/// The nodes of the (single) runtime row, by kind.
fn runtime_nodes(
    response: &crate::types::CompileRequestResponse,
) -> Vec<&crate::types::CompiledVirtualNode> {
    response
        .products
        .iter()
        .find_map(|product| match product {
            CompiledProduct::Runtime { nodes, .. } => Some(nodes.iter().collect()),
            _ => None,
        })
        .expect("a runtime product row is present")
}

/// `VirtualMeta` as a comparable tuple. The scope id is the scoped-CSS
/// linkage between a Main node and its style nodes, so a seam that
/// published the right bytes under the wrong scope id would hand a
/// consumer output whose styles no longer bind.
fn meta_key(
    meta: &crate::types::VirtualMeta,
) -> (Option<&str>, Option<&str>, Option<usize>, Option<usize>) {
    (
        meta.scope_id.as_deref(),
        meta.block_type.as_deref(),
        meta.style_index,
        meta.custom_index,
    )
}

/// The seam's published node kinds for the (single) runtime row, as a
/// set of debug spellings. The oracle's "no node was dropped and none was
/// invented" half — a per-node comparison that iterates the seam's OWN
/// list can only see nodes the seam published, so a dropped node is
/// invisible to it.
fn published_node_kinds(response: &crate::types::CompileRequestResponse) -> Vec<String> {
    let mut kinds: Vec<String> = runtime_nodes(response)
        .into_iter()
        .map(|node| format!("{:?}", node.node))
        .collect();
    kinds.sort();
    kinds
}

/// The legacy profile-derived route's bytes for the SAME demand, read
/// through its own public entry. The oracle for byte-equivalence.
fn legacy_virtual_file(
    host: &VerterHost,
    canonical_id: &str,
    node: VirtualNodeKind,
    profile: &CompileProfile,
) -> (Arc<str>, Option<Arc<str>>) {
    let response = legacy_virtual_response(host, canonical_id, node, profile);
    (response.code, response.source_map)
}

fn legacy_virtual_response(
    host: &VerterHost,
    canonical_id: &str,
    node: VirtualNodeKind,
    profile: &CompileProfile,
) -> crate::types::VirtualFileResponse {
    host.get_virtual_file(crate::types::VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical_id.to_string()),
        node_kind: Some(node.clone()),
        compile_profile: profile.clone(),
    })
    .unwrap_or_else(|e| panic!("legacy read of {node:?}: {e:?}"))
}

/// One diagnostic set as a comparable, order-independent key.
fn diagnostic_keys(diagnostics: &crate::types::DiagnosticsSnapshot) -> Vec<(String, u32, u32)> {
    let mut keys: Vec<(String, u32, u32)> = diagnostics
        .diagnostics
        .iter()
        .map(|d| (d.code.clone(), d.span.start, d.span.end))
        .collect();
    keys.sort();
    keys
}

/// Compare the seam's runtime nodes against the legacy route in BOTH
/// directions: every kind the seam published must match the legacy read,
/// and the published set must be exactly `expected` — so a node the seam
/// silently stopped publishing fails here rather than passing vacuously.
fn assert_runtime_nodes_match_legacy(
    host: &VerterHost,
    canonical_id: &str,
    response: &crate::types::CompileRequestResponse,
    profile: &CompileProfile,
    expected: &[&str],
) {
    let mut expected_sorted: Vec<String> = expected.iter().map(|k| (*k).to_string()).collect();
    expected_sorted.sort();
    assert_eq!(
        published_node_kinds(response),
        expected_sorted,
        "the runtime row must publish exactly this node set"
    );

    let nodes = runtime_nodes(response);
    assert!(!nodes.is_empty(), "a zero-node runtime row proves nothing");
    for published in nodes {
        let node = &published.node;
        let legacy = legacy_virtual_response(host, canonical_id, node.clone(), profile);
        assert_eq!(
            published.code.as_ref(),
            legacy.code.as_ref(),
            "{node:?} bytes must match the legacy route"
        );
        assert_eq!(
            published.source_map.as_deref(),
            legacy.source_map.as_deref(),
            "{node:?} source map must match the legacy route"
        );
        // A node carries more than bytes: consumers route on `lang` and
        // bind scoped styles through `meta.scope_id`, so an equivalence
        // that compares only code and map lets both drift silently.
        assert_eq!(
            published.lang.as_deref(),
            legacy.lang.as_deref(),
            "{node:?} output language must match the legacy route"
        );
        assert_eq!(
            meta_key(&published.meta),
            meta_key(&legacy.meta),
            "{node:?} virtual metadata must match the legacy route"
        );
    }
}

// ── one call, one registration, no profile ───────────────────────────

/// The whole transaction is one call: register the source once, hand the
/// entry a canonical id plus a request, receive the response. No ensure
/// step, no cached-read step, no ordering for the caller to get right.
#[test]
fn one_call_compiles_a_registered_source_from_its_canonical_request() {
    let host = new_host();
    upsert(&host, "/src/App.vue", VUE_SFC);

    let response = host
        .compile_request("/src/App.vue", vue_request(vec![runtime_client(false)]))
        .expect("the request executes");

    assert_eq!(response.canonical_id, "/src/App.vue");
    assert_eq!(response.products.len(), 1);
    let nodes = runtime_nodes(&response);
    assert!(
        nodes.iter().any(|n| n.node == VirtualNodeKind::Main),
        "the runtime row publishes its assembled main module: {:?}",
        nodes.iter().map(|n| &n.node).collect::<Vec<_>>()
    );
    assert!(nodes.iter().any(|n| n.node == VirtualNodeKind::Script));
    assert!(nodes.iter().any(|n| n.node == VirtualNodeKind::Template));
    assert!(nodes
        .iter()
        .any(|n| matches!(n.node, VirtualNodeKind::Style { index: 0 })));
}

/// An alias resolves ONCE, at the entry: the response reports the
/// canonical id the request executed against, not the spelling the
/// caller used.
#[test]
fn an_alias_resolves_once_and_the_response_reports_the_canonical_id() {
    let host = new_host();
    let _registered = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/App.vue".to_string()),
            input_id: "/src/App.vue".to_string(),
            source: Arc::from(VUE_SFC),
            file_language: FileLanguage::vue(),
            aliases: vec!["/alias/App.vue".to_string()],
        })
        .expect("upsert with alias");

    let response = host
        .compile_request("/alias/App.vue", vue_request(vec![runtime_client(false)]))
        .expect("the aliased request executes");
    assert_eq!(response.canonical_id, "/src/App.vue");
}

// ── per-kind equivalence against the legacy route ────────────────────

/// Every requested product kind is returned, in request order, and its
/// bytes and maps are what the legacy profile-derived route produces for
/// the same demand. This is the equivalence proof for the shared product
/// set on the Vue carrier: same node SET, same output bytes, same maps,
/// same IDE projection, same analysis payload, same diagnostics.
///
/// The node comparison runs in BOTH directions. Iterating only the seam's
/// own node list would let the seam silently stop publishing a kind —
/// every surviving row would still match, and the dropped one would never
/// be looked up. The exact-set assertion is what makes a dropped node a
/// failure.
#[test]
fn vue_products_are_byte_equivalent_to_the_legacy_route() {
    let host = new_host();
    upsert(&host, "/src/App.vue", VUE_SFC_EVERY_NODE);

    let response = host
        .compile_request(
            "/src/App.vue",
            vue_request(vec![runtime_client(true), ide(true), analysis()]),
        )
        .expect("the multi-product request executes");

    assert_eq!(
        response
            .products
            .iter()
            .map(|product| match product {
                CompiledProduct::Runtime { kind, .. } => format!("{kind:?}"),
                CompiledProduct::Ide(_) => "Ide".to_string(),
                CompiledProduct::Analysis(_) => "Analysis".to_string(),
            })
            .collect::<Vec<_>>(),
        vec!["RuntimeClient", "Ide", "Analysis"],
        "one row per requested kind, in request order"
    );

    // The legacy demand that matches this request: every runtime node, the
    // IDE projection, template data, and source maps on.
    let profile = CompileProfile {
        target: CompileTarget::SCRIPT
            | CompileTarget::TEMPLATE
            | CompileTarget::STYLE
            | CompileTarget::TSX
            | CompileTarget::TEMPLATE_DATA,
        source_map: true,
        ..CompileProfile::default()
    };

    assert_runtime_nodes_match_legacy(
        &host,
        "/src/App.vue",
        &response,
        &profile,
        &[
            "Main",
            "Script",
            "Template",
            "Style { index: 0 }",
            "Custom { index: 0 }",
        ],
    );

    let legacy_ide = host
        .get_ide("/src/App.vue", &profile)
        .expect("the legacy route projects the IDE surface");
    let ide_row = response
        .products
        .iter()
        .find_map(|product| match product {
            CompiledProduct::Ide(ide) => Some(ide),
            _ => None,
        })
        .expect("the IDE row is present");
    assert_eq!(ide_row.code.as_ref(), legacy_ide.code.as_ref());
    assert_eq!(
        ide_row.source_map.as_deref(),
        legacy_ide.source_map.as_deref()
    );
    assert_eq!(ide_row.is_jsx, legacy_ide.is_jsx);

    // The ANALYSIS row's payload, not merely its presence. The legacy
    // lane persists the same conversion into the profileless raw-template
    // slot, so that read is this row's oracle.
    let analysis_row = response
        .products
        .iter()
        .find_map(|product| match product {
            CompiledProduct::Analysis(analysis) => Some(analysis),
            _ => None,
        })
        .expect("the analysis row is present");
    let legacy_analysis = host
        .raw_template_analysis_for_file("/src/App.vue")
        .expect("the legacy route persisted its template analysis");
    assert_eq!(
        format!("{analysis_row:?}"),
        format!("{legacy_analysis:?}"),
        "the analysis payload must be the legacy route's, not merely present"
    );

    // Diagnostics are named in the equivalence acceptance beside the
    // bytes and the maps, so they are compared too.
    let legacy_main =
        legacy_virtual_response(&host, "/src/App.vue", VirtualNodeKind::Main, &profile);
    assert_eq!(
        diagnostic_keys(&response.diagnostics),
        diagnostic_keys(&legacy_main.diagnostics),
        "the response's diagnostics must be the legacy compile's"
    );
}

/// The Svelte carrier's shared product set, through the same seam and
/// against the same legacy oracle — including the exact published node
/// set, which is the half a per-node loop over the seam's own list cannot
/// see.
#[test]
fn svelte_products_are_byte_equivalent_to_the_legacy_route() {
    let host = new_host();
    upsert(&host, "/src/Widget.svelte", SVELTE_SFC);

    let response = host
        .compile_request(
            "/src/Widget.svelte",
            svelte_request(vec![runtime_client(true), ide(true)]),
        )
        .expect("the Svelte request executes");

    let profile = CompileProfile {
        target: CompileTarget::SCRIPT
            | CompileTarget::TEMPLATE
            | CompileTarget::STYLE
            | CompileTarget::TSX,
        source_map: true,
        ..CompileProfile::default()
    };

    assert_runtime_nodes_match_legacy(
        &host,
        "/src/Widget.svelte",
        &response,
        &profile,
        &["Main", "Style { index: 0 }"],
    );

    let legacy_ide = host
        .get_ide("/src/Widget.svelte", &profile)
        .expect("the legacy route projects the Svelte IDE surface");
    let ide_row = response
        .products
        .iter()
        .find_map(|product| match product {
            CompiledProduct::Ide(ide) => Some(ide),
            _ => None,
        })
        .expect("the IDE row is present");
    assert_eq!(ide_row.code.as_ref(), legacy_ide.code.as_ref());

    let legacy_main =
        legacy_virtual_response(&host, "/src/Widget.svelte", VirtualNodeKind::Main, &profile);
    assert_eq!(
        diagnostic_keys(&response.diagnostics),
        diagnostic_keys(&legacy_main.diagnostics),
        "the response's diagnostics must be the legacy compile's"
    );
}

/// The SERVER runtime kind is a distinct demand and produces distinct
/// bytes; the row reports the kind that was actually requested.
#[test]
fn the_server_runtime_kind_is_its_own_demand_and_output() {
    let host = new_host();
    upsert(&host, "/src/App.vue", VUE_SFC);

    let ssr_request = CompileRequest::new(
        vec![CompileProduct::RuntimeServer(
            RuntimeProductRequest::default(),
        )],
        FrameworkCompileRequest::Vue(VueCompileRequest {
            backend: VueBackendRequest::Inferred,
            ssr: true,
            script_custom_element: Some(false),
            ..VueCompileRequest::default()
        }),
        None,
        None,
        None,
        false,
        false,
    )
    .expect("the ssr demand constructs");

    let server = host
        .compile_request("/src/App.vue", ssr_request)
        .expect("the ssr request executes");
    assert!(matches!(
        server.products.first(),
        Some(CompiledProduct::Runtime {
            kind: ProductKind::RuntimeServer,
            ..
        })
    ));

    let client = host
        .compile_request("/src/App.vue", vue_request(vec![runtime_client(false)]))
        .expect("the client request executes");

    // The ssr demand is byte-equivalent to the legacy ssr route, the
    // Vite SSR-manifest registration on the assembled main module
    // included.
    let ssr_profile = CompileProfile {
        target: CompileTarget::SCRIPT | CompileTarget::TEMPLATE | CompileTarget::STYLE,
        ssr: true,
        ..CompileProfile::default()
    };
    assert_runtime_nodes_match_legacy(
        &host,
        "/src/App.vue",
        &server,
        &ssr_profile,
        &["Main", "Script", "Template", "Style { index: 0 }"],
    );

    let server_template = runtime_nodes(&server)
        .into_iter()
        .find(|n| n.node == VirtualNodeKind::Template)
        .map(|n| n.code.clone());
    let client_template = runtime_nodes(&client)
        .into_iter()
        .find(|n| n.node == VirtualNodeKind::Template)
        .map(|n| n.code.clone());
    assert_ne!(
        server_template, client_template,
        "the ssr and client runtime demands must not produce the same render output"
    );
}

/// Source maps are per-product demand: a request that asks for none gets
/// none, and the same request asking for them gets them. The map axis
/// reaches the compile from the request alone.
#[test]
fn the_requests_source_map_axis_reaches_the_output() {
    let host = new_host();
    upsert(&host, "/src/App.vue", VUE_SFC);

    let without = host
        .compile_request("/src/App.vue", vue_request(vec![runtime_client(false)]))
        .expect("executes");
    assert!(
        runtime_nodes(&without)
            .iter()
            .all(|n| n.source_map.is_none()),
        "no node carries a map when the request asked for none"
    );

    let with = host
        .compile_request("/src/App.vue", vue_request(vec![runtime_client(true)]))
        .expect("executes");
    assert!(
        runtime_nodes(&with).iter().any(|n| n.source_map.is_some()),
        "asking for maps produces at least one"
    );
}

/// A compile's diagnostics reach the response as ONE set for the whole
/// transaction, not a per-product set the consumer has to merge and
/// de-duplicate itself.
#[test]
fn diagnostics_are_one_deduplicated_set_for_the_whole_request() {
    let host = new_host();
    // A carrier with no entry block: the parse-time diagnostic the Vue
    // backend's execution also clones onto its own channel, which is
    // exactly the double-count the response must not carry.
    upsert(&host, "/src/Empty.vue", "<style>.a{color:red}</style>\n");

    let response = host.compile_request("/src/Empty.vue", vue_request(vec![runtime_client(false)]));
    let diagnostics = match response {
        Ok(response) => response.diagnostics,
        Err(CompileRequestFailure::Refused { diagnostics, .. }) => diagnostics,
        Err(other) => panic!("unexpected failure: {other:?}"),
    };
    let mut seen: Vec<(String, u32, u32)> = diagnostics
        .diagnostics
        .iter()
        .map(|d| (d.code.clone(), d.span.start, d.span.end))
        .collect();
    let before = seen.len();
    seen.sort();
    seen.dedup();
    assert_eq!(
        seen.len(),
        before,
        "the response carries no duplicated diagnostic: {:?}",
        diagnostics.diagnostics
    );
}

// ── refusal paths ────────────────────────────────────────────────────

/// A request whose framework arm contradicts the registered carrier is
/// refused, naming both frameworks. It is never compiled under the
/// registered carrier instead.
#[test]
fn a_contradicting_framework_arm_is_refused_not_recompiled_under_the_carrier() {
    let host = new_host();
    upsert(&host, "/src/App.vue", VUE_SFC);

    let failure = host
        .compile_request("/src/App.vue", svelte_request(vec![runtime_client(false)]))
        .expect_err("a Svelte request against a registered Vue carrier must refuse");
    match failure {
        CompileRequestFailure::FrameworkMismatch {
            canonical_id,
            requested,
            registered,
        } => {
            assert_eq!(canonical_id, "/src/App.vue");
            assert_eq!(requested, "svelte");
            assert_eq!(registered, "vue");
        }
        other => panic!("expected a framework mismatch, got {other:?}"),
    }
}

/// The mismatch is symmetric: a Vue request against a registered Svelte
/// carrier refuses the same way.
#[test]
fn the_framework_mismatch_refusal_is_symmetric() {
    let host = new_host();
    upsert(&host, "/src/Widget.svelte", SVELTE_SFC);

    let failure = host
        .compile_request(
            "/src/Widget.svelte",
            vue_request(vec![runtime_client(false)]),
        )
        .expect_err("a Vue request against a registered Svelte carrier must refuse");
    assert!(
        matches!(
            failure,
            CompileRequestFailure::FrameworkMismatch {
                requested: "vue",
                ref registered,
                ..
            } if registered == "svelte"
        ),
        "expected a framework mismatch naming svelte, got {failure:?}"
    );
}

/// A request naming a canonical id no source is registered under fails —
/// it never compiles empty bytes and never returns an empty success.
#[test]
fn an_unregistered_canonical_id_fails() {
    let host = new_host();
    let failure = host
        .compile_request("/src/Missing.vue", vue_request(vec![runtime_client(false)]))
        .expect_err("an unregistered canonical must fail");
    assert!(
        matches!(
            failure,
            CompileRequestFailure::Host(crate::HostError::MissingSource { .. })
        ),
        "expected a missing-source failure, got {failure:?}"
    );
}

/// The public-API and declaration kinds have no host production route.
/// The seam reports the refused KIND, typed — never an empty success and
/// never a product row a consumer would read as produced-but-empty.
#[test]
fn the_public_api_and_declaration_kinds_return_the_typed_unsupported_outcome() {
    let host = new_host();
    upsert(&host, "/src/App.vue", VUE_SFC);

    for (product, expected) in [
        (
            CompileProduct::PublicApi(PublicApiProductRequest::default()),
            ProductKind::PublicApi,
        ),
        (
            CompileProduct::Declarations(DeclarationProductRequest::default()),
            ProductKind::Declarations,
        ),
    ] {
        let failure = host
            .compile_request("/src/App.vue", vue_request(vec![product]))
            .expect_err("an unproducible product kind must refuse");
        match failure {
            CompileRequestFailure::UnsupportedProduct { kind, .. } => assert_eq!(kind, expected),
            other => panic!("expected the typed unsupported outcome, got {other:?}"),
        }
    }
}

/// The refusal names the RULE. `generate` on a Svelte module compile is
/// gated by a capability that is unsupported fail-closed, and the request
/// carrying it cannot even be constructed — so the equivalent reachable
/// refusal at this seam is an option the host bundle execution cannot
/// route, which refuses at admission naming that exact option row rather
/// than a generic failure.
#[test]
fn an_unroutable_option_refuses_naming_the_rule_not_a_generic_error() {
    let host = new_host();
    upsert(&host, "/src/App.vue", VUE_SFC);

    let request = CompileRequest::new(
        vec![runtime_client(false)],
        FrameworkCompileRequest::Vue(VueCompileRequest {
            backend: VueBackendRequest::Inferred,
            ssr: false,
            script_custom_element: Some(false),
            // The bundle execution cannot route static hoisting, so
            // honouring the demand would mean silently dropping it.
            hoist_static: Some(true),
            ..VueCompileRequest::default()
        }),
        None,
        None,
        None,
        false,
        false,
    )
    .expect("the request itself constructs — the refusal is admission's");

    let failure = host
        .compile_request("/src/App.vue", request)
        .expect_err("an unroutable option must refuse at admission");
    let CompileRequestFailure::Refused { diagnostics, .. } = failure else {
        panic!("expected an admission refusal");
    };
    let message = diagnostics
        .diagnostics
        .iter()
        .map(|d| format!("{}: {}", d.code, d.message))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        message.contains("HOST_COMPILE_ADMISSION_REFUSED"),
        "the refusal carries the admission code: {message}"
    );
    assert!(
        message.contains("TransformOptionsHoistStatic"),
        "the refusal names the exact option row that refused it: {message}"
    );
}

/// The post-parse `SSR x Vapor` rule is a CONSTRUCTION refusal the
/// canonical constructor can only make once the source's own backend
/// marker is known. It must surface through the seam naming that rule.
#[test]
fn a_post_parse_construction_refusal_surfaces_through_the_seam() {
    let host = new_host();
    upsert(
        &host,
        "/src/Vapor.vue",
        "<script setup>const a = 1</script>\n<template vapor><div>{{ a }}</div></template>\n",
    );

    let request = CompileRequest::new(
        vec![CompileProduct::RuntimeServer(
            RuntimeProductRequest::default(),
        )],
        FrameworkCompileRequest::Vue(VueCompileRequest {
            // Inferred at construction; the source's own `<template vapor>`
            // marker is what makes this the refused combination, and only
            // post-parse resolution can see it.
            backend: VueBackendRequest::Inferred,
            ssr: true,
            script_custom_element: Some(false),
            ..VueCompileRequest::default()
        }),
        None,
        None,
        None,
        false,
        false,
    )
    .expect("the request constructs — the marker is not visible until parse");

    let failure = host
        .compile_request("/src/Vapor.vue", request)
        .expect_err("ssr over a vapor-marked source must refuse");
    let CompileRequestFailure::Refused { diagnostics, .. } = failure else {
        panic!("expected a refusal, got a produced response");
    };
    let message = diagnostics
        .diagnostics
        .iter()
        .map(|d| format!("{}: {}", d.code, d.message))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        message.contains("HOST_COMPILE_REQUEST_EXECUTION_REFUSED")
            || message.contains("SsrVaporBackendUnsupported"),
        "the refusal names the ssr-vapor rule: {message}"
    );
}

/// A refusal ends the WHOLE request: no sibling product is published,
/// and there is no partial response, null, or ensure boolean to observe.
#[test]
fn a_refusal_publishes_no_sibling_product() {
    let host = new_host();
    upsert(&host, "/src/App.vue", VUE_SFC);

    // A runtime product the host CAN produce, beside a declaration
    // product it cannot. The whole request must fail, and the runtime
    // product must not come back on its own.
    let failure = host
        .compile_request(
            "/src/App.vue",
            vue_request(vec![
                runtime_client(false),
                CompileProduct::Declarations(DeclarationProductRequest::default()),
            ]),
        )
        .expect_err("a request carrying an unproducible kind must fail whole");
    assert!(
        matches!(
            failure,
            CompileRequestFailure::UnsupportedProduct {
                kind: ProductKind::Declarations,
                ..
            }
        ),
        "expected the typed unsupported outcome, got {failure:?}"
    );
}

// ── the legacy routes are untouched ──────────────────────────────────

/// The legacy ensure-then-read pair still behaves as it always has: the
/// read is a PURE cached read that produces nothing on its own, and the
/// ensure is what compiles. Running the typed seam first does not warm
/// that slot, because the typed route publishes into no compile cache.
#[test]
fn the_legacy_cached_read_stays_pure_and_the_typed_route_warms_no_slot() {
    let host = new_host();
    upsert(&host, "/src/App.vue", VUE_SFC);
    let profile = CompileProfile {
        target: CompileTarget::TSX,
        ..CompileProfile::default()
    };

    assert!(
        host.get_ide("/src/App.vue", &profile).is_none(),
        "the legacy read produces nothing on its own"
    );

    let _typed = host
        .compile_request("/src/App.vue", vue_request(vec![ide(false)]))
        .expect("the typed request executes");
    assert!(
        host.get_ide("/src/App.vue", &profile).is_none(),
        "the typed route publishes into no compile cache slot, so the legacy read is still cold"
    );

    host.ensure_ide_compiled("/src/App.vue", &profile)
        .expect("the legacy ensure compiles");
    assert!(
        host.get_ide("/src/App.vue", &profile).is_some(),
        "the legacy ensure is what warms the legacy read"
    );
}

// ── the typed batch projection ───────────────────────────────────────

/// One entry per input, in ORIGINAL input order, each holding its own
/// response. Two inputs naming one canonical with different requests are
/// one registration and two executions.
#[test]
fn the_batch_returns_one_entry_per_input_in_input_order() {
    let host = new_host();
    let entries = host.compile_request_many(
        vec![
            CompileRequestBatchInput {
                canonical_id: "/src/A.vue".to_string(),
                source: Arc::from(VUE_SFC),
                request: vue_request(vec![runtime_client(false)]),
            },
            CompileRequestBatchInput {
                canonical_id: "/src/B.svelte".to_string(),
                source: Arc::from(SVELTE_SFC),
                request: svelte_request(vec![ide(false)]),
            },
            CompileRequestBatchInput {
                canonical_id: "/src/A.vue".to_string(),
                source: Arc::from(VUE_SFC),
                request: vue_request(vec![ide(false)]),
            },
        ],
        CompileRequestBatchOptions::default(),
    );

    assert_eq!(entries.len(), 3);
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.canonical_id.as_str())
            .collect::<Vec<_>>(),
        vec!["/src/A.vue", "/src/B.svelte", "/src/A.vue"],
    );
    assert!(matches!(
        entries[0].outcome.as_ref().expect("A compiles").products[0],
        CompiledProduct::Runtime { .. }
    ));
    assert!(matches!(
        entries[1].outcome.as_ref().expect("B compiles").products[0],
        CompiledProduct::Ide(_)
    ));
    assert!(
        matches!(
            entries[2]
                .outcome
                .as_ref()
                .expect("A compiles again")
                .products[0],
            CompiledProduct::Ide(_)
        ),
        "the second request for the same canonical executes its OWN demand"
    );
}

/// A per-input failure isolates: the failing entry reports it and every
/// sibling still compiles.
#[test]
fn a_per_input_batch_failure_isolates_to_that_entry() {
    let host = new_host();
    let entries = host.compile_request_many(
        vec![
            CompileRequestBatchInput {
                canonical_id: "/src/Good.vue".to_string(),
                source: Arc::from(VUE_SFC),
                request: vue_request(vec![runtime_client(false)]),
            },
            CompileRequestBatchInput {
                // A Svelte request against a source registered as Vue.
                canonical_id: "/src/Wrong.vue".to_string(),
                source: Arc::from(VUE_SFC),
                request: svelte_request(vec![runtime_client(false)]),
            },
            CompileRequestBatchInput {
                canonical_id: "/src/AlsoGood.svelte".to_string(),
                source: Arc::from(SVELTE_SFC),
                request: svelte_request(vec![runtime_client(false)]),
            },
        ],
        CompileRequestBatchOptions::default(),
    );

    assert!(entries[0].outcome.is_ok(), "sibling compiles");
    assert!(matches!(
        entries[1].outcome,
        Err(CompileRequestFailure::FrameworkMismatch { .. })
    ));
    assert!(entries[2].outcome.is_ok(), "sibling compiles");
}

/// Two inputs naming one canonical with DIFFERENT bytes is a conflict:
/// the batch reports it on both entries and never picks a winner.
#[test]
fn conflicting_sources_for_one_canonical_fail_both_entries() {
    let host = new_host();
    let entries = host.compile_request_many(
        vec![
            CompileRequestBatchInput {
                canonical_id: "/src/A.vue".to_string(),
                source: Arc::from(VUE_SFC),
                request: vue_request(vec![runtime_client(false)]),
            },
            CompileRequestBatchInput {
                canonical_id: "/src/A.vue".to_string(),
                source: Arc::from("<template><span/></template>\n"),
                request: vue_request(vec![runtime_client(false)]),
            },
        ],
        CompileRequestBatchOptions::default(),
    );
    assert_eq!(entries.len(), 2);
    for entry in &entries {
        assert!(
            matches!(entry.outcome, Err(CompileRequestFailure::Refused { .. })),
            "a conflicting canonical fails both positions"
        );
    }
}

/// An empty batch registers nothing and returns nothing.
#[test]
fn an_empty_batch_returns_no_entries() {
    let host = new_host();
    assert!(host
        .compile_request_many(Vec::new(), CompileRequestBatchOptions::default())
        .is_empty());
}

/// Several inputs naming ONE canonical with identical bytes register that
/// source exactly once for the whole batch, then execute their own
/// requests against the single stored snapshot. The registration count is
/// observable through the source's version: a batch that registered the
/// canonical per input would leave it at a later version than a batch
/// that registered it once.
#[test]
fn the_batch_registers_each_source_exactly_once() {
    let once = new_host();
    let _single = once.compile_request_many(
        vec![CompileRequestBatchInput {
            canonical_id: "/src/A.vue".to_string(),
            source: Arc::from(VUE_SFC),
            request: vue_request(vec![runtime_client(false)]),
        }],
        CompileRequestBatchOptions::default(),
    );
    let single_generation = once
        .scheduler
        .try_get_source("/src/A.vue")
        .expect("registered")
        .generation;

    let many = new_host();
    let entries = many.compile_request_many(
        vec![
            CompileRequestBatchInput {
                canonical_id: "/src/A.vue".to_string(),
                source: Arc::from(VUE_SFC),
                request: vue_request(vec![runtime_client(false)]),
            },
            CompileRequestBatchInput {
                canonical_id: "/src/A.vue".to_string(),
                source: Arc::from(VUE_SFC),
                request: vue_request(vec![ide(false)]),
            },
            CompileRequestBatchInput {
                canonical_id: "/src/A.vue".to_string(),
                source: Arc::from(VUE_SFC),
                request: vue_request(vec![analysis()]),
            },
        ],
        CompileRequestBatchOptions::default(),
    );
    assert_eq!(entries.len(), 3, "one entry per input");
    for entry in &entries {
        assert!(entry.outcome.is_ok(), "each request executes");
    }
    assert_eq!(
        many.scheduler
            .try_get_source("/src/A.vue")
            .expect("registered")
            .generation,
        single_generation,
        "three inputs for one canonical registered its source once, not three times"
    );
}

// ── the mechanisms this route invented ───────────────────────────────

/// The route reads the REGISTERED carrier source and no supplied
/// (externally preprocessed) artifact bucket.
///
/// A supplied artifact is admitted under the compile profile its
/// admitting caller named. This route has no channel for admitting one,
/// so it is entitled to no bucket: a block whose authored dialect needs
/// external preprocessing refuses as unavailable rather than silently
/// compiling another route's preprocessed bytes under a demand that never
/// asked for them.
///
/// Discrimination: naming any profile bucket here — the default profile
/// included — makes the seam serve the artifact admitted below, and the
/// refusal assertion fails.
#[test]
fn the_route_reads_the_registered_source_and_no_supplied_artifact_bucket() {
    let host = new_host();
    let source = "<script setup lang=\"ts\">\nconst c = 'blue'\n</script>\n\
                  <template><div>x</div></template>\n\
                  <style lang=\"customcss\">authored preprocessing input</style>";
    let update = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/Themed.vue".to_string()),
            input_id: "/src/Themed.vue".to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("the carrier registers");
    let request = update
        .preprocessor_requests
        .iter()
        .find(|request| request.lang == "customcss")
        .expect("the non-native style dialect captures a preprocessing request");

    // Admit the preprocessed bytes under the DEFAULT profile's bucket —
    // the bucket a route that named a profile here would draw from.
    let profile = CompileProfile::default();
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: update.canonical_id.clone(),
            compile_profile: profile.clone(),
            overrides: vec![BlockOverrideEntry::supplied_for_test(
                request,
                ".authored { color: rebeccapurple }",
            )],
        })
        .expect("the supplied CSS is admitted for the default profile");

    // Positive control: the artifact IS reachable through a route that
    // names that bucket, so the refusal below is scope acting rather than
    // an artifact that was never admitted.
    let supplied = host
        .capture_compiler_block_content(
            "/src/Themed.vue",
            crate::block_content::SuppliedBlockScope::Profile(&profile),
        )
        .expect("the profile-scoped read serves the supplied artifact");
    assert!(
        supplied.has_supplied,
        "control: the default profile's bucket must hold the admitted artifact"
    );

    // THE PIN: the seam draws from no bucket, so the block that needs
    // external preprocessing is unavailable to it.
    let failure = host
        .compile_request("/src/Themed.vue", vue_request(vec![runtime_client(false)]))
        .expect_err("a block needing external preprocessing must refuse on this route");
    assert!(
        matches!(
            &failure,
            CompileRequestFailure::Host(HostError::BlockContentRefused(
                BlockContentRefusal::Unavailable {
                    availability: BlockContentAvailability::ProcessedContentRequired,
                    ..
                }
            ))
        ),
        "expected the unavailable refusal, got {failure:?}"
    );
}

/// A request carrying parse-affecting Vue template grammar is refused,
/// never compiled under the REGISTERED grammar.
///
/// The bound backend executes over the artifact's own registered parse.
/// Custom delimiters and custom-element tag matchers change what that
/// parse would have produced, so honouring them here would silently
/// compile a different template than the one the request describes.
///
/// Discrimination: without the gate, both requests below compile
/// successfully under the default grammar.
#[test]
fn a_request_carrying_custom_template_grammar_refuses() {
    let host = new_host();
    upsert(&host, "/src/App.vue", VUE_SFC);

    let grammar_variants = [
        (
            "custom delimiters",
            VueCompileRequest {
                backend: VueBackendRequest::Inferred,
                ssr: false,
                script_custom_element: Some(false),
                delimiters: Some(("[[".to_string(), "]]".to_string())),
                ..VueCompileRequest::default()
            },
        ),
        (
            "custom element tag matchers",
            VueCompileRequest {
                backend: VueBackendRequest::Inferred,
                ssr: false,
                script_custom_element: Some(false),
                is_custom_element: vec!["my-widget".to_string()],
                ..VueCompileRequest::default()
            },
        ),
    ];

    for (label, vue) in grammar_variants {
        let request = CompileRequest::new(
            vec![runtime_client(false)],
            FrameworkCompileRequest::Vue(vue),
            None,
            None,
            None,
            false,
            false,
        )
        .expect("the request itself constructs — the refusal is the route's");
        let failure = host
            .compile_request("/src/App.vue", request)
            .err()
            .unwrap_or_else(|| {
                panic!("{label} must refuse rather than compile under the registered grammar")
            });
        assert!(
            matches!(
                &failure,
                CompileRequestFailure::Host(HostError::GrammarMismatch(_))
            ),
            "{label}: expected the grammar-mismatch refusal, got {failure:?}"
        );
    }
}

/// A request naming BOTH runtime kinds refuses at admission.
///
/// `CompileRequest::new` rejects a duplicate product KIND, and client and
/// server are two distinct kinds — so the pair constructs. The bundle
/// execution runs exactly one ssr mode per pass, so admission is what
/// refuses it, and the response's single-runtime-row shape (the node vec
/// is MOVED into the first runtime row) depends on that refusal holding.
///
/// Discrimination: without the admission refusal the second runtime row
/// is published with an empty node vec.
#[test]
fn a_request_naming_both_runtime_kinds_refuses_at_admission() {
    let host = new_host();
    upsert(&host, "/src/App.vue", VUE_SFC);

    let request = CompileRequest::new(
        vec![
            runtime_client(false),
            CompileProduct::RuntimeServer(RuntimeProductRequest::default()),
        ],
        FrameworkCompileRequest::Vue(VueCompileRequest {
            backend: VueBackendRequest::Inferred,
            ssr: false,
            script_custom_element: Some(false),
            ..VueCompileRequest::default()
        }),
        None,
        None,
        None,
        false,
        false,
    )
    .expect("two distinct kinds are not a duplicate product");

    let failure = host
        .compile_request("/src/App.vue", request)
        .expect_err("a dual-runtime-kind request must refuse");
    let CompileRequestFailure::Refused { diagnostics, .. } = failure else {
        panic!("expected an admission refusal");
    };
    let message = diagnostics
        .diagnostics
        .iter()
        .map(|d| format!("{}: {}", d.code, d.message))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        message.contains("DualRuntimeKind"),
        "the refusal names the dual-kind rule: {message}"
    );
}

// ── admitted-but-unproduced, and dropped/substituted axes ────────────

/// An `Analysis` product with neither axis set would be admitted and then
/// publish nothing. It refuses at admission rather than reaching the
/// publication tail as a row with no payload.
#[test]
fn an_analysis_product_that_would_publish_nothing_refuses() {
    for (canonical, source, request) in [
        (
            "/src/App.vue",
            VUE_SFC,
            vue_request(vec![CompileProduct::Analysis(AnalysisProductRequest {
                want_script_bindings: false,
                want_template_data: false,
            })]),
        ),
        (
            "/src/Widget.svelte",
            SVELTE_SFC,
            svelte_request(vec![CompileProduct::Analysis(AnalysisProductRequest {
                want_script_bindings: false,
                want_template_data: false,
            })]),
        ),
    ] {
        let host = new_host();
        upsert(&host, canonical, source);
        let failure = host
            .compile_request(canonical, request)
            .expect_err("an analysis demand that produces nothing must refuse");
        let CompileRequestFailure::Refused { diagnostics, .. } = failure else {
            panic!("{canonical}: expected an admission refusal");
        };
        let message = diagnostics
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            message.contains("AnalysisProducesNothing"),
            "{canonical}: the refusal names the rule: {message}"
        );
    }
}

/// A product kind that was ADMITTED but whose payload the carrier did not
/// publish fails the whole request, typed, naming the kind.
///
/// The template-fact producer fails CLOSED to no payload when the
/// selected `<template src="...">` bytes are not the admitted host
/// block's — deliberate behaviour the compiler's own tests pin. Admission
/// cannot see it (it happens during execution), so the publication tail
/// has to handle an absent payload. A public entry must not abort the
/// caller's thread on it.
#[test]
fn an_admitted_product_with_no_published_payload_fails_typed() {
    let host = new_host_with_files(&[("/src/view.html", "<div>external</div>")]);
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/External.vue".to_string()),
            input_id: "/src/External.vue".to_string(),
            source: Arc::from(
                "<template src=\"./view.html\"></template>\n\
                 <script setup lang=\"ts\">\nconst ok = true\n</script>\n",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("the carrier registers");

    let failure = host
        .compile_request("/src/External.vue", vue_request(vec![analysis()]))
        .expect_err("an admitted analysis product with no payload must fail typed");
    assert!(
        matches!(
            &failure,
            CompileRequestFailure::ProductNotProduced {
                kind: ProductKind::Analysis,
                ..
            }
        ),
        "expected the typed unproduced-payload failure, got {failure:?}"
    );
}

/// Every caller-settable axis the host bundle execution would DROP (no
/// consumer reads it) or SUBSTITUTE (a consumer overwrites it) refuses
/// typed, naming the axis.
///
/// These axes only became caller-settable when this route started
/// accepting a caller-supplied request. Honouring them silently would
/// mean the response describes a compile the caller did not ask for —
/// `ide_chunk_boundaries` in particular is overwritten by the carrier
/// bridge with a value derived from the selected template block, so both
/// settings produce byte-identical output.
#[test]
fn dropped_and_substituted_request_axes_refuse() {
    use verter_identity::profile::{
        OutputProfileId, PresentationProfileId, SerializationProfileId, TypeScriptSemanticProfileId,
    };

    /// A resolved profile identity is opaque; any seed makes one.
    struct ProfileSeed(&'static str);
    impl verter_identity::encoding::CanonicalEncode for ProfileSeed {
        const DOMAIN_TAG: &'static str = "verter-session.compile-request-seam-profile-seed.v1";
        fn encode_fields(&self, encoder: &mut verter_identity::encoding::CanonicalEncoder) {
            encoder.field_str(1, self.0);
        }
    }

    let axis_cases: Vec<(
        &str,
        Vec<CompileProduct>,
        Option<TypeScriptSemanticProfileId>,
    )> = vec![
        (
            "RuntimeOutputProfile",
            vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
                output_profile: Some(OutputProfileId::from_canonical(&ProfileSeed("output"))),
                ..RuntimeProductRequest::default()
            })],
            None,
        ),
        (
            "RuntimeSerialization",
            vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
                serialization: Some(SerializationProfileId::from_canonical(&ProfileSeed(
                    "serialization",
                ))),
                ..RuntimeProductRequest::default()
            })],
            None,
        ),
        (
            "IdeChunkBoundaries",
            vec![CompileProduct::IdeCompanion(IdeProductRequest {
                ide_chunk_boundaries: true,
                ..IdeProductRequest::default()
            })],
            None,
        ),
        (
            "IdeOutputProfile",
            vec![CompileProduct::IdeCompanion(IdeProductRequest {
                output_profile: Some(OutputProfileId::from_canonical(&ProfileSeed("output"))),
                ..IdeProductRequest::default()
            })],
            None,
        ),
        (
            "IdeDiagnosticsPresentation",
            vec![CompileProduct::IdeCompanion(IdeProductRequest {
                diagnostics: Some(PresentationProfileId::from_canonical(&ProfileSeed(
                    "presentation",
                ))),
                ..IdeProductRequest::default()
            })],
            None,
        ),
        (
            "IdeSerialization",
            vec![CompileProduct::IdeCompanion(IdeProductRequest {
                serialization: Some(SerializationProfileId::from_canonical(&ProfileSeed(
                    "serialization",
                ))),
                ..IdeProductRequest::default()
            })],
            None,
        ),
        (
            "SemanticProfile",
            vec![runtime_client(false)],
            Some(TypeScriptSemanticProfileId::from_canonical(&ProfileSeed(
                "semantic",
            ))),
        ),
    ];

    for (axis, products, semantic_profile) in axis_cases {
        // Both carriers refuse the same rows through the same reader.
        for (canonical, source, framework) in [
            (
                "/src/App.vue",
                VUE_SFC,
                FrameworkCompileRequest::Vue(VueCompileRequest {
                    backend: VueBackendRequest::Inferred,
                    ssr: false,
                    script_custom_element: Some(false),
                    ..VueCompileRequest::default()
                }),
            ),
            (
                "/src/Widget.svelte",
                SVELTE_SFC,
                FrameworkCompileRequest::Svelte(SvelteCompileRequest {
                    custom_element: Some(false),
                    ..SvelteCompileRequest::default()
                }),
            ),
        ] {
            let host = new_host();
            upsert(&host, canonical, source);
            let request = CompileRequest::new(
                products.clone(),
                framework,
                semantic_profile.clone(),
                None,
                None,
                false,
                false,
            )
            .expect("the request itself constructs — the refusal is admission's");

            let failure = host
                .compile_request(canonical, request)
                .err()
                .unwrap_or_else(|| {
                    panic!("{canonical}: the {axis} axis must refuse, not compile silently")
                });
            let CompileRequestFailure::Refused { diagnostics, .. } = &failure else {
                panic!("{canonical}: expected an admission refusal for {axis}, got {failure:?}");
            };
            let message = diagnostics
                .diagnostics
                .iter()
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                message.contains(axis),
                "{canonical}: the refusal must name the {axis} axis: {message}"
            );
        }
    }
}

// ── the shared dependency axis ───────────────────────────────────────

/// The stateless route publishes no cache slot, so it owns no
/// invalidation record — and must not REPLACE another lane's.
///
/// `sync_transitive_macro_type_dependencies` overwrites the compiled
/// file's recorded transitive dependency/semantic edges. The seam derives
/// its macro-bundle depth from its OWN product set, so an IDE-only or
/// analysis-only request would narrow the set a warm profile-derived slot
/// invalidates against.
///
/// Discrimination: the profile-derived lane, which DOES own the axis,
/// must still restate it — so the counter proves the observable fires.
#[test]
fn the_stateless_route_does_not_restate_the_shared_dependency_axis() {
    use std::sync::atomic::Ordering;

    let seam_host = new_host();
    upsert(&seam_host, "/src/App.vue", VUE_SFC);
    let _ = seam_host
        .compile_request("/src/App.vue", vue_request(vec![ide(false)]))
        .expect("the typed request executes");
    assert_eq!(
        seam_host
            .test_force
            .wrapper_sync_transitive_count
            .load(Ordering::Relaxed),
        0,
        "the stateless route must not replace the shared dependency axis"
    );

    let legacy_host = new_host();
    upsert(&legacy_host, "/src/App.vue", VUE_SFC);
    legacy_host
        .ensure_ide_compiled(
            "/src/App.vue",
            &CompileProfile {
                target: CompileTarget::TSX,
                ..CompileProfile::default()
            },
        )
        .expect("the legacy ensure compiles");
    assert!(
        legacy_host
            .test_force
            .wrapper_sync_transitive_count
            .load(Ordering::Relaxed)
            > 0,
        "control: the slot-publishing lane must restate the axis, or the counter proves nothing"
    );
}
