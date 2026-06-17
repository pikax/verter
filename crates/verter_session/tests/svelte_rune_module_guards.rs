//! D-bk architecture guards: `.svelte.ts`/`.svelte.js` are first-class
//! NON-COMPONENT rune-module carriers.
//!
//! These guards pin the B8j contract:
//! - `svelte_rune_module_not_component_carrier` — a `.svelte.ts`/`.svelte.js`
//!   path classifies as the rune-module carrier (NOT plain `Script`, NOT the
//!   component carrier) and exposes NO component-API surface (no synthesised
//!   component default, no component api virtual file); a plain `.ts`/`.js`
//!   stays plain and a `.svelte` stays the component carrier (negatives). The
//!   B8a discriminating `.svelte.ts`-is-NOT-a-carrier guard fixture (D-bg) is
//!   RETIRED in the same change (it was the registry test
//!   `svelte_ts_and_svelte_js_are_plain_scripts_not_carriers`, replaced by the
//!   positive `svelte_rune_modules_classify_as_non_component_adapter_modules`).
//! - `svelte_feature_rows_supported_no_diagnostic` (B8j row) — a rune module
//!   emits NO typed-unsupported diagnostic and serves a real type-checked
//!   surface (the rune-module provider content); it does NOT route through the
//!   carrier `unsupported_language` path.

use std::sync::Arc;

use verter_language::{FrameworkAdapterId, LanguageId, ScriptSourceType};
use verter_protocol::typeinfo::graph::{self as wire, FrameworkSurfaceKindSupport};
use verter_protocol::verter::v1::{
    type_info_graph_request as wire_request, type_info_graph_response,
};
use verter_session::framework::rune_module_provider_content;
use verter_session::{FileLanguage, HostConfig, LanguageRegistry, UpsertRequest, VerterHost};

fn host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert(host: &VerterHost, id: &str, src: &str, lang: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(id.into()),
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: lang,
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert `{id}` must succeed: {e:?}"));
}

fn rune_module_ts() -> FileLanguage {
    FileLanguage::adapter_module(
        ScriptSourceType::Ts,
        FrameworkAdapterId::svelte(),
        LanguageId::new(verter_language::SVELTE_RUNE_MODULE_LANGUAGE_ID),
    )
}

#[test]
fn svelte_rune_module_not_component_carrier() {
    let registry = LanguageRegistry::built_in();

    // (1) CLASSIFICATION: a `.svelte.ts`/`.svelte.js` path classifies as the
    // NON-COMPONENT rune-module adapter module — NOT plain Script, NOT the
    // component carrier.
    let ts = registry
        .classify_static("/src/store.svelte.ts")
        .static_resolution();
    assert_eq!(ts, rune_module_ts(), "`.svelte.ts` is the rune-module row");
    assert!(!ts.is_framework_carrier(), "a rune module is NOT a carrier");
    assert_ne!(
        ts,
        FileLanguage::script(ScriptSourceType::Ts),
        "a rune module is NOT a plain script"
    );
    assert!(
        ts.adapter_script_language().is_some(),
        "the owning adapter is exposed via adapter_script_language()"
    );
    assert_eq!(
        ts.adapter_id(),
        None,
        "adapter_id() must NOT answer for a rune module (no carrier dispatch)"
    );
    assert_eq!(ts.carrier_language_id(), None);

    let js = registry
        .classify_static("/src/store.svelte.js")
        .static_resolution();
    assert!(!js.is_framework_carrier());
    assert!(js.adapter_script_language().is_some());

    // NEGATIVE: a plain `.ts`/`.js` stays plain.
    assert_eq!(
        registry.classify_static("/src/util.ts").static_resolution(),
        FileLanguage::script(ScriptSourceType::Ts)
    );
    assert!(registry
        .classify_static("/src/util.ts")
        .static_resolution()
        .adapter_script_language()
        .is_none());
    // NEGATIVE: a `.svelte` component stays the component carrier.
    let component = registry
        .classify_static("/src/Box.svelte")
        .static_resolution();
    assert!(component.is_framework_carrier());
    assert!(component.is_svelte());
    assert!(component.adapter_script_language().is_none());

    // (2) NO COMPONENT-API SURFACE: a rune module is a MODULE OF REACTIVE
    // VALUES, not a component — it synthesises NO component default. Observed at
    // the public export-graph boundary: the rune module's resolved exports are
    // its REAL value exports (`s`, `d`) — the export graph treats it as an
    // ordinary module, and it surfaces NO synthesised `default`. A `.svelte`
    // COMPONENT is the opposite: a framework CARRIER whose public surface IS the
    // synthesised `default` component export (reachable cross-file as
    // `import Box from './Box.svelte'`). This split is discriminating: were a
    // rune module synth-gated as a component (the bug), it would surface a
    // synthesised `default` instead of its reactive value exports.
    let host = host();
    upsert(
        &host,
        "/store.svelte.ts",
        "export const s = $state(0);\nexport const d = $derived(s * 2);\n",
        rune_module_ts(),
    );
    let rune_names: Vec<String> = host
        .resolve_exports("/store.svelte.ts")
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert!(
        rune_names.iter().any(|n| n == "s") && rune_names.iter().any(|n| n == "d"),
        "the rune module's real reactive value exports surface as ordinary \
         module exports: got {rune_names:?}"
    );
    assert!(
        !rune_names.iter().any(|n| n == "default"),
        "a rune module exposes NO synthesised component `default` export \
         (it is not a component); got {rune_names:?}"
    );

    // DISCRIMINATING CONTRAST: a real `.svelte` COMPONENT surfaces exactly the
    // SYNTHESISED `default` component export at the export-graph boundary (the
    // carrier-generic synthesised-default short-circuit), and NONE of the rune
    // module's userland reactive values. A rune module is the opposite (a real
    // value module: `s`/`d`, no `default`). The split proves the rune module is
    // NOT routed through the component build, and the component IS.
    upsert(
        &host,
        "/Box.svelte",
        "<script lang=\"ts\">let { label }: { label: string } = $props();</script>\n<div>{label}</div>\n",
        FileLanguage::svelte(),
    );
    let component_value_exports: Vec<String> = host
        .resolve_exports("/Box.svelte")
        .into_iter()
        .filter(|e| !e.is_type)
        .map(|e| e.name)
        .collect();
    assert!(
        component_value_exports.iter().any(|n| n == "default"),
        "a `.svelte` COMPONENT surfaces its synthesised `default` component \
         export at the export-graph boundary (carrier-generic synthesised \
         default); got {component_value_exports:?}"
    );
    assert!(
        !component_value_exports.iter().any(|n| n == "s" || n == "d"),
        "the `.svelte` COMPONENT surface is the synthesised component, NOT a \
         verbatim module — it carries none of the rune module's userland \
         reactive values; got {component_value_exports:?}"
    );

    // (3) NO COMPONENT API VIRTUAL FILE: the rune module is not a carrier, so
    // it serves its own provider path with prelude-augmented content — there is
    // no `{carrier}.ts`/`.tsx` dual file. (The path-level guard lives in
    // verter_workspace::resolver_tests; here we pin the provider-content
    // builder produces the same-file content for a rune module and `None` for a
    // component carrier / plain script.)
    assert!(
        rune_module_provider_content(&rune_module_ts(), "export const s = $state(0);\n").is_some(),
        "a rune module serves prelude-augmented content from its own path"
    );
    assert!(
        rune_module_provider_content(&FileLanguage::svelte(), "x").is_none(),
        "a `.svelte` COMPONENT carrier has no rune-module provider content"
    );
    assert!(
        rune_module_provider_content(&FileLanguage::script_ts(), "x").is_none(),
        "a plain `.ts` has no rune-module provider content"
    );
}

fn framework_envelope(canonical: &str, adapter_id: &str) -> wire::TypeInfoGraphRequest {
    wire::TypeInfoGraphRequest {
        schema_version: 3,
        operation: wire::Operation::FrameworkSurfaces as i32,
        payload: Some(wire_request::Payload::FrameworkSurface(
            wire::FrameworkSurfaceRequest {
                selector: Some(wire::ComponentSelector {
                    canonical_id: canonical.to_string(),
                    export_name: String::new(),
                    has_export_name: false,
                    framework_adapter_id: adapter_id.to_string(),
                }),
                context: Some(wire::ProjectionReductionContext {
                    mode: wire::ProjectionMode::Expanded as i32,
                    demand: wire::ReductionDemand::Published as i32,
                }),
                closure: Some(wire::ClosurePolicy {
                    kind: Some(
                        verter_protocol::verter::v1::graph_closure_policy::Kind::OneLevel(
                            wire::ClosureOneLevel {},
                        ),
                    ),
                }),
                display_policy: Some(wire::DisplayPolicy {
                    qualification: wire::DisplayQualification::Qualified as i32,
                    branding: wire::DisplayBranding::On as i32,
                    budgets: Some(wire::DisplayBudgets {
                        max_string_length: 4096,
                        max_depth: 16,
                    }),
                }),
                include_provenance: false,
                include_diagnostics: false,
                include_projection: vec![],
                schema_version: 3,
            },
        )),
    }
}

#[test]
fn svelte_feature_rows_supported_no_diagnostic() {
    // B8j row (D-bl): a rune module is SUPPORTED (it serves a real type-checked
    // rune-module surface) and emits NO typed-unsupported diagnostic. It does
    // NOT route through the carrier `unsupported_language` path — the upsert
    // SUCCEEDS as a script (asserted by `host()` + `upsert` not panicking).
    let host = host();
    upsert(
        &host,
        "/store.svelte.ts",
        "export const s = $state(0);\n",
        rune_module_ts(),
    );

    // A framework-surface request targeting a rune module resolves structurally
    // (a registered Svelte adapter answers) — NOT a typed wire error. Since a
    // rune module has NO component, every component surface kind is structurally
    // UNSUPPORTED / not-supported — NOT supported-empty (the typed distinction
    // the executor preserves). It must NOT be SUPPORTED for any component kind.
    let envelope = framework_envelope("/store.svelte.ts", "svelte");
    let result = host.resolve_framework_surface_with_audit(envelope);
    let response = result
        .as_result()
        .expect("a rune-module framework-surface request resolves structurally, not a wire error");
    let payload = match &response.kind {
        Some(type_info_graph_response::Kind::FrameworkSurface(p)) => p,
        other => panic!("expected the framework_surface arm, got {other:?}"),
    };
    // A rune module is not a component, so EVERY known component surface kind is
    // structurally UNSUPPORTED — NOT supported-empty, NOT Partial, NOT absent.
    // The response carries EXACTLY ONE entry per known kind (the executor
    // contract), so the guard asserts BOTH completeness (all six known kinds
    // present, distinct) AND exactness (every entry is precisely UNSUPPORTED) —
    // an empty list, a missing kind, or a Partial would NOT pass.
    use std::collections::BTreeSet;
    let all_kinds: BTreeSet<i32> = [
        wire::FrameworkSurfaceKind::Props as i32,
        wire::FrameworkSurfaceKind::Emits as i32,
        wire::FrameworkSurfaceKind::Slots as i32,
        wire::FrameworkSurfaceKind::Options as i32,
        wire::FrameworkSurfaceKind::Expose as i32,
        wire::FrameworkSurfaceKind::Model as i32,
    ]
    .into_iter()
    .collect();
    let present_kinds: BTreeSet<i32> = payload.surfaces.iter().map(|e| e.kind).collect();
    assert_eq!(
        present_kinds, all_kinds,
        "the rune-module response carries exactly one entry per known component \
         surface kind (completeness — never an empty / truncated list); got {present_kinds:?}"
    );
    for entry in &payload.surfaces {
        let support = entry.status.as_ref().map(|s| s.support).unwrap_or(-1);
        assert_eq!(
            support,
            FrameworkSurfaceKindSupport::Unsupported as i32,
            "a rune module's component surface kind ({}) must be precisely UNSUPPORTED \
             (not Supported, not supported-empty, not Partial); it is a non-component \
             module of reactive values",
            entry.kind
        );
    }

    // DISCRIMINATING CONTRAST: a real `.svelte` COMPONENT is SUPPORTED for at
    // least the Props kind at the SAME wire boundary — so the all-UNSUPPORTED
    // verdict above is specific to the rune module, not a blanket adapter
    // behaviour.
    upsert(
        &host,
        "/Box.svelte",
        "<script lang=\"ts\">let { label }: { label: string } = $props();</script>\n<div>{label}</div>\n",
        FileLanguage::svelte(),
    );
    let component_result =
        host.resolve_framework_surface_with_audit(framework_envelope("/Box.svelte", "svelte"));
    let component_response = component_result
        .as_result()
        .expect("a `.svelte` component framework-surface request resolves structurally");
    let component_payload = match &component_response.kind {
        Some(type_info_graph_response::Kind::FrameworkSurface(p)) => p,
        other => panic!("expected the framework_surface arm for the component, got {other:?}"),
    };
    let props_supported = component_payload.surfaces.iter().any(|e| {
        e.kind == wire::FrameworkSurfaceKind::Props as i32
            && e.status.as_ref().map(|s| s.support)
                == Some(FrameworkSurfaceKindSupport::Supported as i32)
    });
    assert!(
        props_supported,
        "a `.svelte` COMPONENT SUPPORTS the Props surface kind (the contrast that \
         proves the rune module's all-UNSUPPORTED verdict is component-specific)"
    );
}

/// `svelte_rune_module_not_in_carrier_extensions`: a `.svelte.ts` / `.svelte.js`
/// rune module is a NON-component carrier — its extension MUST NOT appear in
/// `carrier_extensions()` (the carrier-watch-glob authority), and MUST appear in
/// `adapter_module_extensions()` / `all_adapter_module_extensions()` (the
/// adapter-module-watch-glob authority). The two glob authorities are disjoint
/// for the rune module: it is covered by the dedicated adapter-module glob, NOT
/// the carrier glob.
#[test]
fn svelte_rune_module_not_in_carrier_extensions() {
    let registry = LanguageRegistry::built_in();

    let carriers = registry.carrier_extensions();
    assert!(
        !carriers.contains(&"svelte.ts") && !carriers.contains(&"svelte.js"),
        "carrier_extensions() must NOT include the rune-module extensions, got {carriers:?}"
    );
    // The carrier glob authority still covers the real `.svelte` component carrier.
    assert!(
        carriers.contains(&"svelte"),
        "carrier_extensions() still covers the `.svelte` component carrier, got {carriers:?}"
    );

    // The rune-module extensions ARE the adapter-module glob authority.
    let svelte_modules = registry.adapter_module_extensions(&FrameworkAdapterId::svelte());
    assert!(
        svelte_modules.contains(&"svelte.ts") && svelte_modules.contains(&"svelte.js"),
        "adapter_module_extensions(svelte) must include svelte.ts + svelte.js, got {svelte_modules:?}"
    );
    let all_modules = registry.all_adapter_module_extensions();
    assert!(
        all_modules.contains(&"svelte.ts") && all_modules.contains(&"svelte.js"),
        "all_adapter_module_extensions() must include svelte.ts + svelte.js, got {all_modules:?}"
    );
    // The two authorities are disjoint for the rune module.
    for ext in &all_modules {
        assert!(
            !carriers.contains(ext),
            "an adapter-module extension ({ext}) must never be a carrier extension"
        );
    }
}
