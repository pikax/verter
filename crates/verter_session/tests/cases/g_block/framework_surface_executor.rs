//! Framework-surface executor wire-entry coverage.
//!
//! Discriminating coverage for `VerterHost::type_info_with_audit`:
//! - VALIDATION FIRST: a malformed envelope (op/payload mismatch, framework
//!   schema-version mismatch) returns the typed wire `error` arm BEFORE any
//!   registry lookup or semantic dispatch — no `framework_surface` arm, no
//!   registered adapter selection observable.
//! - TYPED PROFILE (schema 11): the selector names a profile the host
//!   advertised through the capability handshake; a reference the host does
//!   not admit (unknown, disabled) is the typed `UnknownFrameworkProfile`
//!   error before any semantic work, and the response echoes the admitted
//!   profile.
//! - VUE PARITY: a Vue SFC request resolves its props / emits / slots through
//!   the registry → `VueFrameworkAdapter` plan/normalize → relocated `vue_exec`
//!   resolution and yields the SAME members the live `vue_macro_dtos` path
//!   produces (behavior parity).
//! - STATUS: the response carries EXACTLY ONE entry per known kind; a
//!   supported-but-empty kind is SUPPORTED-empty, distinct from an UNSUPPORTED
//!   kind.

use std::collections::HashMap;
use std::sync::Arc;

use verter_audit::RequestTargetIdentity;
use verter_protocol::typeinfo::graph::{
    self as wire, FrameworkSurfaceKind, FrameworkSurfaceKindSupport,
};
use verter_protocol::verter::v1::{
    graph_closure_policy, type_info_graph_request as wire_request, type_info_graph_response,
    type_info_request_error,
};
use verter_session::typeinfo::types::{TypeInfoQueryLevel, VueMacroSurfaceRequest};
use verter_session::VerterHost;
use verter_session_query::analysis::types::AnalyzedMacroKind;

use super::harness;

const PARITY_VUE: &str = r#"<script setup lang="ts">
interface Props { count: number; label?: string }
defineProps<Props>();
defineEmits<{ change: [next: number] }>();
defineSlots<{ default(props: { item: string }): unknown }>();
</script>
<template><div></div></template>
"#;

fn default_context() -> wire::ProjectionReductionContext {
    wire::ProjectionReductionContext {
        mode: wire::ProjectionMode::Expanded as i32,
        demand: wire::ReductionDemand::Published as i32,
    }
}

fn one_level_closure() -> wire::ClosurePolicy {
    wire::ClosurePolicy {
        kind: Some(graph_closure_policy::Kind::OneLevel(
            wire::ClosureOneLevel {},
        )),
    }
}

fn default_display_policy() -> wire::DisplayPolicy {
    wire::DisplayPolicy {
        qualification: wire::DisplayQualification::Qualified as i32,
        branding: wire::DisplayBranding::On as i32,
        budgets: Some(wire::DisplayBudgets {
            max_string_length: 4096,
            max_depth: 16,
        }),
    }
}

/// The profile `host` advertises for `family`, discovered through the
/// capability handshake — the only way a client learns a profile reference.
fn advertised_profile(host: &VerterHost, family: &str) -> wire::FrameworkProfileRef {
    host.typeinfo_capabilities(&wire::TypeInfoCapabilityHandshakeRequest {
        client_version: wire::TYPEINFO_GRAPH_SCHEMA_VERSION,
    })
    .framework_profiles
    .into_iter()
    .find(|advertised| advertised.family == family)
    .and_then(|advertised| advertised.profile)
    .unwrap_or_else(|| panic!("the host advertises a `{family}` profile"))
}

/// Build a well-formed current-schema framework-surface envelope for
/// `canonical`, naming `family`'s advertised profile and demanding every
/// known kind.
fn framework_envelope(
    host: &VerterHost,
    canonical: &str,
    family: &str,
) -> wire::TypeInfoGraphRequest {
    let version = wire::TYPEINFO_GRAPH_SCHEMA_VERSION;
    framework_envelope_versioned(
        canonical,
        Some(advertised_profile(host, family)),
        version,
        version,
    )
}

/// Build a framework-surface envelope with an explicit profile reference and
/// envelope / payload schema versions (so a mismatch can be exercised),
/// demanding every known kind.
fn framework_envelope_versioned(
    canonical: &str,
    framework_profile: Option<wire::FrameworkProfileRef>,
    envelope_version: u32,
    payload_version: u32,
) -> wire::TypeInfoGraphRequest {
    wire::TypeInfoGraphRequest {
        schema_version: envelope_version,
        operation: wire::Operation::FrameworkSurfaces as i32,
        payload: Some(wire_request::Payload::FrameworkSurface(
            wire::FrameworkSurfaceRequest {
                selector: Some(wire::ComponentSelector {
                    canonical_id: canonical.to_string(),
                    export_name: String::new(),
                    has_export_name: false,
                    framework_profile,
                }),
                context: Some(default_context()),
                closure: Some(one_level_closure()),
                display_policy: Some(default_display_policy()),
                include_provenance: false,
                include_diagnostics: false,
                include_projection: vec![],
                schema_version: payload_version,
                requested_kinds: verter_session::framework::ALL_FRAMEWORK_SURFACE_KINDS
                    .iter()
                    .map(|kind| *kind as i32)
                    .collect(),
            },
        )),
    }
}

/// A current-schema envelope carrying an explicit facet demand.
fn framework_envelope_demanding(
    host: &VerterHost,
    canonical: &str,
    family: &str,
    requested_kinds: &[FrameworkSurfaceKind],
) -> wire::TypeInfoGraphRequest {
    let mut envelope = framework_envelope(host, canonical, family);
    if let Some(wire_request::Payload::FrameworkSurface(request)) = envelope.payload.as_mut() {
        request.requested_kinds = requested_kinds.iter().map(|kind| *kind as i32).collect();
    }
    envelope
}

fn build_host() -> Arc<VerterHost> {
    harness::build_hermetic_host_with_lib(
        &[("/Parity.vue", PARITY_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    )
}

/// Extract the success `FrameworkSurfacePayload` from a response, or panic.
fn expect_payload(response: &wire::TypeInfoGraphResponse) -> &wire::FrameworkSurfacePayload {
    match &response.kind {
        Some(type_info_graph_response::Kind::FrameworkSurface(p)) => p,
        other => panic!("expected a framework_surface response arm, got {other:?}"),
    }
}

/// Index a payload's entries by kind.
fn entries_by_kind(
    payload: &wire::FrameworkSurfacePayload,
) -> HashMap<i32, &wire::FrameworkSurfaceKindEntry> {
    payload.surfaces.iter().map(|e| (e.kind, e)).collect()
}

/// Resolve a payload entry's member NAMES (interned through the graph string
/// table).
fn member_keys(
    payload: &wire::FrameworkSurfacePayload,
    entry: &wire::FrameworkSurfaceKindEntry,
) -> Vec<String> {
    let strings = payload
        .graph
        .as_ref()
        .and_then(|g| g.strings.as_ref())
        .map(|t| t.entries.clone())
        .unwrap_or_default();
    entry
        .members
        .iter()
        .map(|m| strings.get(m.name_id as usize).cloned().unwrap_or_default())
        .collect()
}

/// A component whose props surface the substrate FAILED to compute must not
/// encode as `SUPPORTED` + `GRAPH_EXACTNESS_EXACT_RESOLVED`.
///
/// Vue and Svelte are co-equal on this wire and were byte-indistinguishable
/// from a clean answer: `encode_kind` maps `ResolvedOutcome::Resolved` (at
/// ANY member count) and `::Missing` to SUPPORTED + EXACT_RESOLVED, and the
/// flow substrate's degradation never reached `ResolvedOutcome`. So a
/// consumer could not tell "this component declares these props" from "we
/// could not work out its props".
///
/// Each row's props come from `ReturnType<typeof makeProps>` where
/// `makeProps` returns an object with ONE member this substrate cannot type:
/// a call of `notDeclared`, a name declared nowhere (TS2304), whose error
/// type is recovery for a program that does not type-check, which the
/// flow-return lane does not model (TypeScript 7.0.2 `tsc`: `{ label:
/// string; made: any }`).
///
/// The CLEAN controls are the discrimination: an ordinary component must
/// still encode SUPPORTED + EXACT_RESOLVED, so a blanket downgrade fails
/// them.
///
/// Mutation recipe: dropping the per-demand completeness read in
/// `ExecutorResolveCtx::resolve_demand` returns both degraded rows to
/// SUPPORTED / EXACT_RESOLVED while leaving the controls green.
#[test]
fn a_degraded_framework_surface_never_encodes_exact_resolved() {
    use verter_session::{FileLanguage, HostConfig, UpsertRequest};

    const DEGRADED_BODY: &str =
        "function makeProps() { const f = () => notDeclared(); return { label: \"x\", made: f() } }\n";

    let vue_degraded = format!(
        "<script setup lang=\"ts\">\n{DEGRADED_BODY}\
         defineProps<ReturnType<typeof makeProps>>()\n</script>\n<template><div></div></template>\n"
    );
    let vue_clean = "<script setup lang=\"ts\">\n\
         function makeProps() { return { label: \"x\", n: 1 } }\n\
         defineProps<ReturnType<typeof makeProps>>()\n</script>\n<template><div></div></template>\n";
    let svelte_degraded = format!(
        "<script lang=\"ts\">\n{DEGRADED_BODY}\
         let {{ label, made }}: ReturnType<typeof makeProps> = $props();\n</script>\n<p>{{label}}</p>\n"
    );
    let svelte_clean = "<script lang=\"ts\">\n\
         function makeProps() { return { label: \"x\", n: 1 } }\n\
         let { label, n }: ReturnType<typeof makeProps> = $props();\n</script>\n<p>{label}</p>\n";

    for (canonical, source, language, adapter, degraded) in [
        (
            "/D.vue",
            vue_degraded.as_str(),
            FileLanguage::vue(),
            "vue",
            true,
        ),
        ("/C.vue", vue_clean, FileLanguage::vue(), "vue", false),
        (
            "/D.svelte",
            svelte_degraded.as_str(),
            FileLanguage::svelte(),
            "svelte",
            true,
        ),
        (
            "/C.svelte",
            svelte_clean,
            FileLanguage::svelte(),
            "svelte",
            false,
        ),
    ] {
        let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.to_string()),
                input_id: canonical.to_string(),
                source: Arc::from(source),
                file_language: language,
                aliases: Vec::new(),
            })
            .unwrap_or_else(|e| panic!("{canonical}: upsert {e:?}"));

        let result = host.type_info_with_audit(framework_envelope(&host, canonical, adapter));
        let response = result.as_result().expect("structural response");
        let payload = expect_payload(response);
        let entries = entries_by_kind(payload);
        let props = entries
            .get(&(FrameworkSurfaceKind::Props as i32))
            .unwrap_or_else(|| panic!("{canonical}: a PROPS entry"));
        let status = props
            .status
            .as_ref()
            .unwrap_or_else(|| panic!("{canonical}: PROPS carries a status"));
        let members = member_keys(payload, props);

        if degraded {
            assert_ne!(
                status.support,
                FrameworkSurfaceKindSupport::Supported as i32,
                "{canonical}: a surface the substrate failed to compute must not claim SUPPORTED \
                 (members={members:?})"
            );
            assert_ne!(
                status.exactness,
                wire::Exactness::ExactResolved as i32,
                "{canonical}: a degraded resolution must not claim EXACT_RESOLVED \
                 (members={members:?})"
            );
        } else {
            assert_eq!(
                status.support,
                FrameworkSurfaceKindSupport::Supported as i32,
                "{canonical}: the clean control stays SUPPORTED (members={members:?})"
            );
            assert_eq!(
                status.exactness,
                wire::Exactness::ExactResolved as i32,
                "{canonical}: the clean control stays EXACT_RESOLVED (members={members:?})"
            );
            assert!(
                members.iter().any(|m| m == "label"),
                "{canonical}: the clean control publishes its props, got {members:?}"
            );
        }
    }
}

#[test]
fn malformed_envelope_op_payload_mismatch_returns_error_before_dispatch() {
    // An operation discriminator that disagrees with the payload arm (a
    // ResolveSymbol operation carrying a FrameworkSurface payload) is malformed.
    // The executor must return the typed wire error BEFORE any registry lookup.
    let host = build_host();
    let mut envelope = framework_envelope(&host, "/Parity.vue", "vue");
    // Corrupt the operation discriminator to a non-framework operation.
    envelope.operation = wire::Operation::ResolveSymbol as i32;

    let result = host.type_info_with_audit(envelope);
    // A malformed envelope is the audited Err outcome carrying the typed wire
    // error — no framework_surface payload, no registered adapter selection.
    result
        .as_result()
        .expect_err("a malformed envelope must be the Err outcome");
}

#[test]
fn framework_schema_version_mismatch_rejected_before_dispatch() {
    // The framework-surface operation requires schema 11 (the typed profile
    // selector); every globally supported version below it is rejected with
    // MalformedPayload BEFORE any profile lookup — even one carrying a valid
    // profile reference.
    let host = build_host();
    let profile = advertised_profile(&host, "vue");
    for version in [2, 3, 9, 10] {
        let envelope =
            framework_envelope_versioned("/Parity.vue", Some(profile.clone()), version, version);
        let result = host.type_info_with_audit(envelope);
        let error = result
            .as_result()
            .expect_err("a sub-minimum schema version must be the Err outcome");
        assert!(
            matches!(
                error.kind,
                Some(type_info_request_error::Kind::MalformedPayload(_))
            ),
            "a framework schema {version} request is a MalformedPayload, got {:?}",
            error.kind
        );
    }
}

#[test]
fn empty_framework_selector_canonical_is_not_applicable_in_rejected_audit() {
    let host = build_host();
    let envelope = framework_envelope(&host, "", "vue");
    let result = host.type_info_with_audit(envelope);

    result
        .as_result()
        .expect_err("an empty selector canonical must be rejected");
    assert_eq!(
        result.audit().canonical_id,
        "",
        "the retained legacy projection stays empty for the rejected request"
    );
    assert_eq!(
        result.audit().target_identity,
        Some(RequestTargetIdentity::NotApplicable),
        "an empty selector canonical must never be tagged RegisteredCanonical(\"\")"
    );
}

/// The typed `UnknownFrameworkProfile` error carrying exactly the rejected
/// reference, or a panic.
fn expect_unknown_profile(
    result: &verter_audit::AuditedResult<wire::TypeInfoGraphResponse, wire::TypeInfoRequestError>,
) -> wire::FrameworkProfileRef {
    let error = result
        .as_result()
        .expect_err("an unadmitted profile must be the Err outcome");
    match &error.kind {
        Some(type_info_request_error::Kind::UnknownFrameworkProfile(unknown)) => unknown
            .profile
            .clone()
            .expect("the error echoes the rejected reference"),
        other => panic!("an unadmitted profile is UnknownFrameworkProfile, got {other:?}"),
    }
}

#[test]
fn an_unminted_profile_reference_is_unknown_framework_profile() {
    // A well-shaped 32-byte reference the host never minted is the typed
    // UnknownFrameworkProfile error (echoing the reference, never a guessed
    // family or release), surfaced after validation and before any semantic
    // work.
    let host = build_host();
    let forged = wire::FrameworkProfileRef {
        digest: vec![0xAB; wire::FRAMEWORK_PROFILE_DIGEST_LEN],
    };
    let envelope = framework_envelope_versioned(
        "/Parity.vue",
        Some(forged.clone()),
        wire::TYPEINFO_GRAPH_SCHEMA_VERSION,
        wire::TYPEINFO_GRAPH_SCHEMA_VERSION,
    );
    let result = host.type_info_with_audit(envelope);
    assert_eq!(expect_unknown_profile(&result), forged);
}

/// A component whose owner file the host does not know is refused as
/// `UnknownSource`, echoing the path, before any planning: never a
/// structural answer for a component that is not there.
#[test]
fn an_unknown_component_source_is_refused_as_unknown_source() {
    let host = build_host();
    let result = host.type_info_with_audit(framework_envelope(&host, "/Missing.vue", "vue"));
    let error = result
        .as_result()
        .expect_err("an unknown owner file is the Err outcome");
    match &error.kind {
        Some(type_info_request_error::Kind::UnknownSource(unknown)) => {
            assert_eq!(unknown.path, "/Missing.vue");
        }
        other => panic!("an unknown owner file is UnknownSource, got {other:?}"),
    }
}

/// The semantic work a host has done so far: the shared graph's cold builds,
/// instantiations, relation checks, decided conditionals and substitutions,
/// and the file artifacts it holds.
fn semantic_work(host: &VerterHost) -> [(&'static str, u64); 6] {
    let store = host.project_type_store();
    let stats = store.semantic_graph().stats_snapshot();
    [
        ("cold builds", stats.misses),
        ("instantiations", stats.instantiate_count),
        ("relation checks", stats.relation_check_count),
        ("decided conditionals", stats.conditional_decided_count),
        ("substitutions", stats.substitute_memo_misses),
        (
            "file artifacts",
            store.file_artifacts().live_artifact_count() as u64,
        ),
    ]
}

/// The framework-surface wire entry validates the envelope first, then
/// admits the selector's profile, and only then resolves anything. A
/// malformed envelope is the typed malformed error even when its profile
/// reference is also unknown, and neither a malformed envelope nor an
/// unadmitted profile does any semantic work: the owner component stays
/// unresolved until a well-formed, admitted request names it.
///
/// Mutation recipes: resolving the owner before validation or before
/// admission makes a rejected request move the work counters; admitting
/// the profile before validation answers the malformed envelope with a
/// forged reference as `UnknownFrameworkProfile`.
#[test]
fn framework_surface_wire_executor_validates_first() {
    let host = build_host();
    let advertised = advertised_profile(&host, "vue");
    let forged = wire::FrameworkProfileRef {
        digest: vec![0xAB; wire::FRAMEWORK_PROFILE_DIGEST_LEN],
    };
    let current = wire::TYPEINFO_GRAPH_SCHEMA_VERSION;
    let before = semantic_work(&host);

    // An operation that disagrees with the payload arm.
    let mut mismatched =
        framework_envelope_versioned("/Parity.vue", Some(advertised.clone()), current, current);
    mismatched.operation = wire::Operation::ResolveSymbol as i32;
    host.type_info_with_audit(mismatched)
        .as_result()
        .expect_err("an operation / payload mismatch is the Err outcome");

    // A schema the operation does not accept, naming a profile the host
    // never minted: validation answers before admission could.
    let malformed_and_forged =
        framework_envelope_versioned("/Parity.vue", Some(forged.clone()), 10, 10);
    let error = host
        .type_info_with_audit(malformed_and_forged)
        .as_result()
        .expect_err("a sub-minimum schema version is the Err outcome")
        .clone();
    assert!(
        matches!(
            error.kind,
            Some(type_info_request_error::Kind::MalformedPayload(_))
        ),
        "the malformed envelope is rejected before its profile is admitted, got {:?}",
        error.kind
    );

    // A well-formed envelope naming a profile the host never minted.
    let unadmitted =
        framework_envelope_versioned("/Parity.vue", Some(forged.clone()), current, current);
    assert_eq!(
        expect_unknown_profile(&host.type_info_with_audit(unadmitted)),
        forged
    );

    assert_eq!(
        semantic_work(&host),
        before,
        "a rejected framework-surface request does no semantic work"
    );

    // Control: the same owner under the advertised profile resolves, so the
    // counters above move when the executor does resolve.
    let admitted = host.type_info_with_audit(framework_envelope(&host, "/Parity.vue", "vue"));
    expect_payload(admitted.as_result().expect("an admitted request resolves"));
    let after = semantic_work(&host);
    assert!(
        after[0].1 > before[0].1,
        "resolving the component builds semantic queries: {before:?} -> {after:?}"
    );
}

#[test]
fn a_disabled_framework_profile_is_neither_advertised_nor_admitted() {
    // Admission follows the host's FrameworkOptions: a Vue-only host neither
    // advertises nor admits the Svelte profile a default host advertises, so
    // that reference is UnknownFrameworkProfile — the same answer as a
    // forged digest. Discovery and admission read one registry.
    use verter_session::framework::FrameworkOptions;
    use verter_session::HostConfig;

    let default_host = VerterHost::new_standalone(HostConfig::default());
    let svelte_profile = advertised_profile(&default_host, "svelte");
    let vue_only = VerterHost::new_standalone(HostConfig {
        framework: FrameworkOptions::admitting_names(["vue"]).expect("the Vue vertical"),
        ..HostConfig::default()
    });
    let advertised: Vec<String> = vue_only
        .typeinfo_capabilities(&wire::TypeInfoCapabilityHandshakeRequest { client_version: 0 })
        .framework_profiles
        .into_iter()
        .map(|advertised| advertised.family)
        .collect();
    assert_eq!(advertised, vec!["vue".to_string()]);

    let envelope = framework_envelope_versioned(
        "/Box.svelte",
        Some(svelte_profile.clone()),
        wire::TYPEINFO_GRAPH_SCHEMA_VERSION,
        wire::TYPEINFO_GRAPH_SCHEMA_VERSION,
    );
    let result = vue_only.type_info_with_audit(envelope);
    assert_eq!(expect_unknown_profile(&result), svelte_profile);
}

#[test]
fn the_response_echoes_the_admitted_profile_and_the_discovered_profiles_are_distinct() {
    // The selector echo names the admitted profile; Vue and Svelte are two
    // distinct profiles, advertised in digest order.
    let host = build_host();
    let vue = advertised_profile(&host, "vue");
    let svelte = advertised_profile(&host, "svelte");
    assert_ne!(vue, svelte, "two registrations mint two profiles");
    let digests: Vec<Vec<u8>> = host
        .typeinfo_capabilities(&wire::TypeInfoCapabilityHandshakeRequest { client_version: 0 })
        .framework_profiles
        .into_iter()
        .filter_map(|advertised| advertised.profile.map(|profile| profile.digest))
        .collect();
    let mut sorted = digests.clone();
    sorted.sort();
    assert_eq!(digests, sorted, "advertisements are in digest order");

    let result = host.type_info_with_audit(framework_envelope(&host, "/Parity.vue", "vue"));
    let response = result.into_result().expect("a Vue request resolves");
    let payload = expect_payload(&response);
    let selector = payload.selector.as_ref().expect("the selector is echoed");
    assert_eq!(selector.canonical_id, "/Parity.vue");
    assert_eq!(selector.framework_profile.as_ref(), Some(&vue));
}

#[test]
fn a_retired_field_4_adapter_string_does_not_select_a_profile() {
    // A pre-11 client's selector carried only the open adapter string in the
    // now-reserved field 4. Decoded today the field is an unknown field, so
    // the selector has no profile reference: schema 11 rejects it as
    // MalformedPayload instead of reviving the adapter-string route.
    use prost::Message;

    let host = build_host();
    let mut envelope = framework_envelope_versioned(
        "/Parity.vue",
        None,
        wire::TYPEINFO_GRAPH_SCHEMA_VERSION,
        wire::TYPEINFO_GRAPH_SCHEMA_VERSION,
    );
    // Append field 4 (`framework_adapter_id`, wire type 2) = "vue" to the
    // encoded selector, then decode the whole envelope as a server would.
    if let Some(wire_request::Payload::FrameworkSurface(request)) = envelope.payload.as_mut() {
        let selector = request.selector.take().expect("selector");
        let mut bytes = selector.encode_to_vec();
        bytes.extend_from_slice(&[(4 << 3) | 2, 3, b'v', b'u', b'e']);
        request.selector =
            Some(wire::ComponentSelector::decode(bytes.as_slice()).expect("decodes"));
    }
    let bytes = envelope.encode_to_vec();
    let envelope = wire::TypeInfoGraphRequest::decode(bytes.as_slice()).expect("decodes");
    let result = host.type_info_with_audit(envelope);
    let error = result
        .as_result()
        .expect_err("a field-4-only selector must be the Err outcome");
    assert!(
        matches!(
            error.kind,
            Some(type_info_request_error::Kind::MalformedPayload(_))
        ),
        "a selector without framework_profile is a MalformedPayload, got {:?}",
        error.kind
    );
}

#[test]
fn response_carries_exactly_one_entry_per_known_kind() {
    let host = build_host();
    let envelope = framework_envelope(&host, "/Parity.vue", "vue");
    let result = host.type_info_with_audit(envelope);
    let response = result
        .into_result()
        .expect("a well-formed Vue request resolves");
    let payload = expect_payload(&response);
    assert_eq!(payload.surfaces.len(), 6, "one entry per known kind");
    // Every kind appears exactly once.
    let by_kind = entries_by_kind(payload);
    for kind in [
        FrameworkSurfaceKind::Props,
        FrameworkSurfaceKind::Emits,
        FrameworkSurfaceKind::Slots,
        FrameworkSurfaceKind::Options,
        FrameworkSurfaceKind::Expose,
        FrameworkSurfaceKind::Model,
    ] {
        assert!(
            by_kind.contains_key(&(kind as i32)),
            "missing entry for {kind:?}"
        );
    }
}

/// The facet demand selects the response: a current-schema request naming
/// `[Slots, Props]` answers exactly those two kinds, in enum-tag order, each
/// resolved (the SFC declares both macros), and carries NO entry for an
/// undemanded kind — not an UNSUPPORTED or supported-empty filler.
/// Discriminating: an executor that kept planning and encoding every known
/// kind would answer six entries.
#[test]
fn a_facet_demand_answers_exactly_the_requested_kinds_in_canonical_order() {
    let host = build_host();
    let envelope = framework_envelope_demanding(
        &host,
        "/Parity.vue",
        "vue",
        &[FrameworkSurfaceKind::Slots, FrameworkSurfaceKind::Props],
    );
    let result = host.type_info_with_audit(envelope);
    let response = result
        .into_result()
        .expect("a well-formed demanding request resolves");
    let payload = expect_payload(&response);
    let kinds: Vec<i32> = payload.surfaces.iter().map(|entry| entry.kind).collect();
    assert_eq!(
        kinds,
        vec![
            FrameworkSurfaceKind::Props as i32,
            FrameworkSurfaceKind::Slots as i32
        ],
        "exactly the demanded kinds, in enum-tag order regardless of request order"
    );
    for entry in &payload.surfaces {
        let support = entry
            .status
            .as_ref()
            .map(|status| status.support)
            .unwrap_or_default();
        assert_eq!(
            support,
            FrameworkSurfaceKindSupport::Supported as i32,
            "a demanded kind the SFC declares resolves SUPPORTED"
        );
        assert!(
            !entry.members.is_empty(),
            "a demanded kind the SFC declares carries its members"
        );
    }
}

#[test]
fn vue_props_emits_slots_match_live_macro_dtos() {
    // BEHAVIOR PARITY: the executor's props / emits / slots members equal the
    // members the live `vue_macro_dtos` path produces for the same SFC.
    let host = build_host();
    let envelope = framework_envelope(&host, "/Parity.vue", "vue");
    let result = host.type_info_with_audit(envelope);
    let response = result.into_result().expect("Vue request resolves");
    let payload = expect_payload(&response);
    let by_kind = entries_by_kind(payload);

    // --- Props parity ---
    // The macro index comes from the live analysis snapshot; `vue_macro_dtos`
    // re-derives the authoritative content hash internally, so a zeroed
    // `root_identity` is sufficient here.
    let snapshot = host.get_analysis("/Parity.vue").expect("the SFC analyzes");
    let props_index = snapshot
        .macros
        .iter()
        .position(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .expect("defineProps macro present");
    let props_dtos = host
        .vue_macro_dtos(&VueMacroSurfaceRequest {
            owner_canonical: Arc::from("/Parity.vue"),
            macro_index: props_index,
            macro_kind: AnalyzedMacroKind::DefineProps,
            root_identity: [0u8; 16],
            level: TypeInfoQueryLevel::FullMetadata,
        })
        .expect("the Vue adapter is admitted");
    let mut live_prop_names: Vec<String> = props_dtos
        .prop_fields()
        .iter()
        .map(|f| f.analysis.name.clone())
        .collect();
    live_prop_names.sort();
    let props_entry = by_kind
        .get(&(FrameworkSurfaceKind::Props as i32))
        .expect("props entry");
    let mut wire_prop_names = member_keys(payload, props_entry);
    wire_prop_names.sort();
    assert_eq!(
        wire_prop_names, live_prop_names,
        "executor props members must equal live vue_macro_dtos props"
    );
    assert!(
        live_prop_names.contains(&"count".to_string())
            && live_prop_names.contains(&"label".to_string()),
        "the fixture declares count + label props, got {live_prop_names:?}"
    );

    // --- Emits parity ---
    let emits_index = snapshot
        .macros
        .iter()
        .position(|m| m.kind == AnalyzedMacroKind::DefineEmits)
        .expect("defineEmits macro present");
    let emits_dtos = host
        .vue_macro_dtos(&VueMacroSurfaceRequest {
            owner_canonical: Arc::from("/Parity.vue"),
            macro_index: emits_index,
            macro_kind: AnalyzedMacroKind::DefineEmits,
            root_identity: [0u8; 16],
            level: TypeInfoQueryLevel::FullMetadata,
        })
        .expect("the Vue adapter is admitted");
    let mut live_emit_names: Vec<String> = emits_dtos
        .emit_fields()
        .iter()
        .map(|f| f.name.clone())
        .collect();
    live_emit_names.sort();
    let emits_entry = by_kind
        .get(&(FrameworkSurfaceKind::Emits as i32))
        .expect("emits entry");
    let mut wire_emit_names = member_keys(payload, emits_entry);
    wire_emit_names.sort();
    assert_eq!(
        wire_emit_names, live_emit_names,
        "executor emits members must equal live vue_macro_dtos emits"
    );
    assert!(
        live_emit_names.contains(&"change".to_string()),
        "the fixture declares a `change` event, got {live_emit_names:?}"
    );

    // --- Slots parity ---
    let slots_index = snapshot
        .macros
        .iter()
        .position(|m| m.kind == AnalyzedMacroKind::DefineSlots)
        .expect("defineSlots macro present");
    let slots_dtos = host
        .vue_macro_dtos(&VueMacroSurfaceRequest {
            owner_canonical: Arc::from("/Parity.vue"),
            macro_index: slots_index,
            macro_kind: AnalyzedMacroKind::DefineSlots,
            root_identity: [0u8; 16],
            level: TypeInfoQueryLevel::FullMetadata,
        })
        .expect("the Vue adapter is admitted");
    let mut live_slot_names: Vec<String> = slots_dtos
        .slot_fields()
        .iter()
        .map(|f| f.name.clone())
        .collect();
    live_slot_names.sort();
    let slots_entry = by_kind
        .get(&(FrameworkSurfaceKind::Slots as i32))
        .expect("slots entry");
    let mut wire_slot_names = member_keys(payload, slots_entry);
    wire_slot_names.sort();
    assert_eq!(
        wire_slot_names, live_slot_names,
        "executor slots members must equal live vue_macro_dtos slots"
    );
    assert!(
        live_slot_names.contains(&"default".to_string()),
        "the fixture declares a `default` slot, got {live_slot_names:?}"
    );
}

#[test]
fn supported_empty_kind_is_distinct_from_unsupported() {
    // The fixture has no defineOptions; Options is a SUPPORTED kind for the Vue
    // adapter, so its entry is SUPPORTED-empty (not UNSUPPORTED) — the
    // supported-empty vs unsupported distinction on the wire.
    let host = build_host();
    let envelope = framework_envelope(&host, "/Parity.vue", "vue");
    let result = host.type_info_with_audit(envelope);
    let response = result.into_result().expect("Vue request resolves");
    let payload = expect_payload(&response);
    let by_kind = entries_by_kind(payload);

    let options = by_kind
        .get(&(FrameworkSurfaceKind::Options as i32))
        .expect("options entry present");
    let status = options.status.as_ref().expect("options status present");
    assert_eq!(
        status.support,
        FrameworkSurfaceKindSupport::Supported as i32,
        "Options is a supported Vue kind, so its empty entry is SUPPORTED-empty, not UNSUPPORTED"
    );
    assert!(
        options.members.is_empty(),
        "the fixture has no defineOptions, so the options entry is empty"
    );

    // Props, by contrast, is SUPPORTED with non-empty members.
    let props = by_kind
        .get(&(FrameworkSurfaceKind::Props as i32))
        .expect("props entry present");
    assert_eq!(
        props.status.as_ref().unwrap().support,
        FrameworkSurfaceKindSupport::Supported as i32
    );
    assert!(!props.members.is_empty(), "props has members");
}

const MODEL_VUE: &str = r#"<script setup lang="ts">
const model = defineModel<string>();
</script>
<template><div></div></template>
"#;

#[test]
fn define_model_surface_carries_the_model_binding() {
    // A `defineModel<string>()` component must produce a non-empty MODEL surface
    // (the model binding) — NOT a silent SUPPORTED-empty model entry. This is
    // the model-slot fix: defineModel feeds BOTH the props slot (meta contract)
    // and the model slot (the MODEL framework surface).
    let host = harness::build_hermetic_host_with_lib(
        &[("/Model.vue", MODEL_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );
    let envelope = framework_envelope(&host, "/Model.vue", "vue");
    let response = host
        .type_info_with_audit(envelope)
        .into_result()
        .expect("Model.vue resolves");
    let payload = expect_payload(&response);
    let by_kind = entries_by_kind(payload);
    let model = by_kind
        .get(&(FrameworkSurfaceKind::Model as i32))
        .expect("model entry present");
    assert_eq!(
        model.status.as_ref().unwrap().support,
        FrameworkSurfaceKindSupport::Supported as i32
    );
    assert!(
        !model.members.is_empty(),
        "a defineModel component must surface a non-empty MODEL binding, got {:?}",
        member_keys(payload, model)
    );
    // The default model binding is named `modelValue`.
    let names = member_keys(payload, model);
    assert!(
        names.contains(&"modelValue".to_string()),
        "the default defineModel binding is `modelValue`, got {names:?}"
    );
}

#[test]
fn define_model_only_component_surfaces_modelvalue_in_props() {
    // A `defineModel`-only component (no `defineProps`) must STILL surface its
    // synthesized `modelValue` prop in the PROPS surface — the model binding is
    // also a prop. A regression here drops the prop entirely.
    let host = harness::build_hermetic_host_with_lib(
        &[("/Model.vue", MODEL_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );
    let envelope = framework_envelope(&host, "/Model.vue", "vue");
    let response = host
        .type_info_with_audit(envelope)
        .into_result()
        .expect("Model.vue resolves");
    let payload = expect_payload(&response);
    let by_kind = entries_by_kind(payload);
    let props = by_kind
        .get(&(FrameworkSurfaceKind::Props as i32))
        .expect("props entry present");
    let names = member_keys(payload, props);
    assert!(
        names.contains(&"modelValue".to_string()),
        "a defineModel-only component must surface `modelValue` in PROPS, got {names:?}"
    );
}

const MULTI_MODEL_VUE: &str = r#"<script setup lang="ts">
const title = defineModel<string>("title");
const count = defineModel<number>("count");
</script>
<template><div></div></template>
"#;

#[test]
fn multiple_define_model_calls_surface_every_binding() {
    // Two `defineModel` calls must surface BOTH bindings in the MODEL surface —
    // aggregation across all matching macros, not just the first.
    let host = harness::build_hermetic_host_with_lib(
        &[("/MultiModel.vue", MULTI_MODEL_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );
    let envelope = framework_envelope(&host, "/MultiModel.vue", "vue");
    let response = host
        .type_info_with_audit(envelope)
        .into_result()
        .expect("MultiModel.vue resolves");
    let payload = expect_payload(&response);
    let by_kind = entries_by_kind(payload);
    let model = by_kind
        .get(&(FrameworkSurfaceKind::Model as i32))
        .expect("model entry present");
    let names = member_keys(payload, model);
    assert!(
        names.contains(&"title".to_string()) && names.contains(&"count".to_string()),
        "both defineModel bindings must surface in MODEL, got {names:?}"
    );
    // The two model bindings also both appear in PROPS.
    let props = by_kind
        .get(&(FrameworkSurfaceKind::Props as i32))
        .expect("props entry present");
    let prop_names = member_keys(payload, props);
    assert!(
        prop_names.contains(&"title".to_string()) && prop_names.contains(&"count".to_string()),
        "both model bindings must surface in PROPS, got {prop_names:?}"
    );
}

const OPTIONS_VUE: &str = r#"<script setup lang="ts">
defineOptions<{ name: 'Widget'; inheritAttrs?: boolean }>();
defineProps<{ count: number }>();
</script>
<template><div></div></template>
"#;

const EXPOSE_VUE: &str = r#"<script setup lang="ts">
defineExpose<{ focus(): void; readonly count: number }>();
defineProps<{ label: string }>();
</script>
<template><div></div></template>
"#;

#[test]
fn define_options_present_resolves_supported_with_members() {
    // A `defineOptions<T>()` macro decodes SUPPORTED with the type argument's
    // declared members — NOT UNSUPPORTED-because-present (that flips a kind Vue
    // advertises as supported into UNSUPPORTED purely on content, breaking the
    // `supported_surfaces` ↔ runtime-status consistency rule).
    let host = harness::build_hermetic_host_with_lib(
        &[("/Options.vue", OPTIONS_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );
    let envelope = framework_envelope(&host, "/Options.vue", "vue");
    let response = host
        .type_info_with_audit(envelope)
        .into_result()
        .expect("Options.vue resolves");
    let payload = expect_payload(&response);
    let by_kind = entries_by_kind(payload);
    let options = by_kind
        .get(&(FrameworkSurfaceKind::Options as i32))
        .expect("options entry present");
    let status = options.status.as_ref().unwrap();
    assert_eq!(
        status.support,
        FrameworkSurfaceKindSupport::Supported as i32,
        "a present defineOptions<T> resolves SUPPORTED with its declared members, \
         not UNSUPPORTED-because-present"
    );
    let names = member_keys(payload, options);
    assert!(
        names.contains(&"name".to_string()) && names.contains(&"inheritAttrs".to_string()),
        "both defineOptions members must surface, got {names:?}"
    );
    // The optional member is non-required; the required member is required.
    let required: std::collections::HashMap<&str, bool> = options
        .members
        .iter()
        .map(|m| {
            let strings = payload
                .graph
                .as_ref()
                .and_then(|g| g.strings.as_ref())
                .map(|t| &t.entries)
                .unwrap();
            (strings[m.name_id as usize].as_str(), m.required)
        })
        .collect();
    assert_eq!(required.get("name"), Some(&true), "`name` is required");
    assert_eq!(
        required.get("inheritAttrs"),
        Some(&false),
        "`inheritAttrs?` is optional"
    );
    // Props on the same component still resolve SUPPORTED with members.
    let props = by_kind
        .get(&(FrameworkSurfaceKind::Props as i32))
        .expect("props entry present");
    assert_eq!(
        props.status.as_ref().unwrap().support,
        FrameworkSurfaceKindSupport::Supported as i32
    );
    assert!(!props.members.is_empty());
}

#[test]
fn define_expose_present_resolves_supported_with_members() {
    // A `defineExpose<T>()` macro decodes SUPPORTED with the type argument's
    // declared members (a method + a readonly property).
    let host = harness::build_hermetic_host_with_lib(
        &[("/Expose.vue", EXPOSE_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );
    let envelope = framework_envelope(&host, "/Expose.vue", "vue");
    let response = host
        .type_info_with_audit(envelope)
        .into_result()
        .expect("Expose.vue resolves");
    let payload = expect_payload(&response);
    let by_kind = entries_by_kind(payload);
    let expose = by_kind
        .get(&(FrameworkSurfaceKind::Expose as i32))
        .expect("expose entry present");
    let status = expose.status.as_ref().unwrap();
    assert_eq!(
        status.support,
        FrameworkSurfaceKindSupport::Supported as i32,
        "a present defineExpose<T> resolves SUPPORTED with its declared members"
    );
    let names = member_keys(payload, expose);
    assert!(
        names.contains(&"focus".to_string()) && names.contains(&"count".to_string()),
        "both defineExpose members must surface, got {names:?}"
    );
}

#[test]
fn every_supported_kind_resolves_supported_when_its_macro_is_used() {
    // CONSISTENCY (`supported_surfaces` ↔ runtime status): for every kind Vue's
    // descriptor advertises as supported, a component USING that kind's macro
    // resolves it SUPPORTED — never UNSUPPORTED-because-present. A content-
    // dependent support flip would mean `supported_surfaces` stops meaning "this
    // adapter supports this kind".
    const ALL_MACROS_VUE: &str = r#"<script setup lang="ts">
defineProps<{ count: number }>();
defineEmits<{ change: [next: number] }>();
defineSlots<{ default(p: { item: string }): unknown }>();
defineOptions<{ name: 'Widget' }>();
defineExpose<{ focus(): void }>();
defineModel<string>('title');
</script>
<template><div></div></template>
"#;
    let host = harness::build_hermetic_host_with_lib(
        &[("/AllMacros.vue", ALL_MACROS_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );
    // Vue's descriptor advertises all six kinds as supported.
    let descriptor = verter_session::framework::descriptor::vue_descriptor();
    let envelope = framework_envelope(&host, "/AllMacros.vue", "vue");
    let response = host
        .type_info_with_audit(envelope)
        .into_result()
        .expect("AllMacros.vue resolves");
    let payload = expect_payload(&response);
    let by_kind = entries_by_kind(payload);
    for &kind in descriptor.supported_surfaces {
        let entry = by_kind
            .get(&(kind as i32))
            .unwrap_or_else(|| panic!("missing entry for advertised-supported kind {kind:?}"));
        let support = entry.status.as_ref().unwrap().support;
        assert_eq!(
            support,
            FrameworkSurfaceKindSupport::Supported as i32,
            "kind {kind:?} is in Vue's supported_surfaces and its macro is used, so it must \
             resolve SUPPORTED — got support discriminant {support}"
        );
    }
}

#[test]
fn nonexistent_named_export_is_a_malformed_payload() {
    // A named-export selector for an export the owner does not declare must be a
    // typed MalformedPayload — NOT a silent fall-through to the default
    // component surface (which would resolve the WRONG component).
    let host = build_host();
    let mut envelope = framework_envelope(&host, "/Parity.vue", "vue");
    if let Some(wire_request::Payload::FrameworkSurface(r)) = envelope.payload.as_mut() {
        if let Some(selector) = r.selector.as_mut() {
            selector.has_export_name = true;
            selector.export_name = "NoSuchExport".to_string();
        }
    }
    let result = host.type_info_with_audit(envelope);
    let error = result
        .as_result()
        .expect_err("a nonexistent named export must be the Err outcome");
    assert!(
        matches!(
            error.kind,
            Some(type_info_request_error::Kind::MalformedPayload(_))
        ),
        "a nonexistent named export is a MalformedPayload, got {:?}",
        error.kind
    );
}

/// SECONDARY evidence, framework surface — an object return's ENTRY FORM
/// no longer costs a CALL-sourced spread its reduction, and the corrected
/// substrate answer reaches the Svelte props surface.
///
/// The thing under test is the substrate, asserted against tsgo in
/// `typeinfo_tests::value_inference::
/// object_return_entry_forms_lower_structurally_over_a_call_spread`. This
/// row set proves the answer survives the framework-surface encoder, and
/// that the encoder does not quietly turn a corrected surface into an
/// empty SUPPORTED one.
///
/// Oracle (TypeScript 7.0.2 `tsc`, `--noEmit --strict --ignoreConfig`),
/// `Eq<…>`-probed with a rejected negative control: the computed-key row is
/// `{ label: string; z: number }`, the `as const` row `{ readonly label:
/// string; readonly n: 1 }`, the `satisfies` row `{ label: string; n:
/// number }`.
///
/// Discrimination: restoring the whole-literal bail returns every row to
/// PARTIAL with zero members; the OPEN-key control fails under a change
/// that starts trusting a key the substrate cannot name.
#[test]
fn an_entry_form_over_a_call_spread_reaches_the_framework_surface_intact() {
    use verter_session::{FileLanguage, HostConfig, UpsertRequest};

    const PRELUDE: &str = "function base() { return { label: \"x\" } }\nconst k = \"z\"\n";

    /// `(row, makeProps body, expected member names — empty means the
    /// surface must fail closed)`.
    const ROWS: &[(&str, &str, &[&str])] = &[
        (
            "computed",
            "function makeProps() { return { ...base(), [k]: 1 } }\n",
            &["label", "z"],
        ),
        (
            "asconst",
            "function makeProps() { return { ...base(), n: 1 } as const }\n",
            &["label", "n"],
        ),
        (
            "asconstonly",
            "function makeProps() { return { ...base() } as const }\n",
            &["label"],
        ),
        (
            "satisfies",
            "function makeProps() { return { ...base(), n: 1 } satisfies object }\n",
            &["label", "n"],
        ),
        (
            "clean",
            "function makeProps() { return { ...base(), n: 1 } }\n",
            &["label", "n"],
        ),
        // CONTROL — an OPEN key domain still fails CLOSED. A key whose
        // value is not a literal provisions a property the surface cannot
        // name, so publishing the modelled siblings alone would declare a
        // member set missing a key the authored value has.
        (
            "openkey",
            "function makeProps(key: string) { return { ...base(), [key]: 1 } }\n",
            &[],
        ),
    ];

    for (row, body, expected) in ROWS {
        let canonical = format!("/EntryForm_{row}.svelte");
        let source = format!(
            "<script lang=\"ts\">\n{PRELUDE}{body}\
             let props: ReturnType<typeof makeProps> = $props();\n</script>\n<p>{{props}}</p>\n"
        );
        let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.clone()),
                input_id: canonical.clone(),
                source: Arc::from(source.as_str()),
                file_language: FileLanguage::svelte(),
                aliases: Vec::new(),
            })
            .unwrap_or_else(|e| panic!("{canonical}: upsert {e:?}"));

        let result = host.type_info_with_audit(framework_envelope(&host, &canonical, "svelte"));
        let response = result.as_result().expect("structural response");
        let payload = expect_payload(response);
        let entries = entries_by_kind(payload);
        let props = entries
            .get(&(FrameworkSurfaceKind::Props as i32))
            .unwrap_or_else(|| panic!("{canonical}: a PROPS entry"));
        let status = props
            .status
            .as_ref()
            .unwrap_or_else(|| panic!("{canonical}: PROPS carries a status"));
        let mut members = member_keys(payload, props);
        members.sort();
        let mut want: Vec<String> = expected.iter().map(|m| (*m).to_owned()).collect();
        want.sort();

        if expected.is_empty() {
            assert_ne!(
                status.support,
                FrameworkSurfaceKindSupport::Supported as i32,
                "{canonical}: the surface's key SET is unknown — claiming SUPPORTED would \
                 publish a props surface missing a key the authored value has \
                 (members={members:?})"
            );
            assert!(
                members.is_empty(),
                "{canonical}: fail-closed means NO members, not the modelled siblings alone; \
                 got {members:?}"
            );
        } else {
            assert_eq!(
                status.support,
                FrameworkSurfaceKindSupport::Supported as i32,
                "{canonical}: the literal lowers structurally and its spread reduces \
                 (members={members:?})"
            );
            assert_eq!(
                members, want,
                "{canonical}: the complete member set must reach the framework surface"
            );
        }
    }
}
