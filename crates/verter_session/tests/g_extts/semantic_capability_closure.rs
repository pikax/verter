//! Contract cases for the TypeScript semantic capability closure — the
//! semantic plane's dormant half of the dual-plane identity contract.
//!
//! Every case drives the real witness chain (resolver → `ProjectBinding` →
//! `EnsureProject` → `BoundProject` → [`CertifiedTypeEngineBinding`]), so a
//! green run here is evidence the sole-route discipline holds end to end, not
//! just that the types compose.

use verter_identity::encoding::CanonicalDigest;
use verter_identity::identity::InputBasisId;
use verter_session::external_ts::{
    BoundProject, CarrierOwnershipResolution, EngineCapabilities, EngineIdentity,
    EngineSessionFacts, QueryFeature, ServeMode,
};
use verter_session::semantic_capability::{
    capability_row, CertificationRefusal, CertifiedTypeEngineBinding, SEMANTIC_CAPABILITY_CATALOG,
};

use super::shared::resolve_with;

/// The closed TypeScript semantic capability set as the engine plane models
/// it (`QueryFeature`). This list IS the public contract: a capability added
/// to `QueryFeature` without a closure row must fail
/// `catalog_closes_every_query_feature_exactly_once`.
const EVERY_CAPABILITY: &[verter_session::external_ts::QueryFeature] = &[
    verter_session::external_ts::QueryFeature::Hover,
    verter_session::external_ts::QueryFeature::Definition,
    verter_session::external_ts::QueryFeature::TypeDefinition,
    verter_session::external_ts::QueryFeature::References,
    verter_session::external_ts::QueryFeature::Rename,
    verter_session::external_ts::QueryFeature::Completion,
    verter_session::external_ts::QueryFeature::SignatureHelp,
    verter_session::external_ts::QueryFeature::DocumentHighlights,
    verter_session::external_ts::QueryFeature::SemanticTokens,
    verter_session::external_ts::QueryFeature::InlayHints,
];

/// Negotiated capabilities that carry a recorded handshake version — the
/// observation certification requires.
fn observed_capabilities(version: &str) -> EngineCapabilities {
    EngineCapabilities {
        static_module_resolution_map: false,
        async_cancellable_queries: false,
        reported_version: Some(std::sync::Arc::<str>::from(version)),
    }
}

/// The serving session certification records: mode, wire pin, and generation
/// are first-class identity, so OWNED and SHARED sessions over identical
/// facts never compose the same observed profile.
fn serving_identity(
    mode: ServeMode,
    version: &str,
    wire_pin: u64,
    generation: u64,
) -> EngineIdentity {
    EngineIdentity::for_mode(
        mode,
        &EngineSessionFacts {
            observed_version: std::sync::Arc::<str>::from(version),
            wire_pin,
            editor_session_generation: generation,
        },
    )
}

/// The default OWNED serving session for tests that do not vary the serving
/// axis. Pinned wire pin and generation so serving-identity changes are
/// always deliberate at the call site.
fn owned_serving(version: &str) -> EngineIdentity {
    serving_identity(ServeMode::Owned, version, 7, 3)
}

/// Mint a `CertifiedTypeEngineBinding` through the full production witness
/// chain: a configured project resolved for a real carrier source, an
/// `EnsureProject` request minted from the binding, a `BoundProject` witness
/// minted from that request, then certification over negotiated capabilities
/// and the identified serving session.
fn certified_binding(
    capabilities: EngineCapabilities,
    serving: &EngineIdentity,
    basis_args: &[u8],
) -> Result<CertifiedTypeEngineBinding, CertificationRefusal> {
    certified_binding_for(
        &[
            ("d:/ws/tsconfig.json", r#"{ "include": ["src/**/*"] }"#),
            ("d:/ws/src/Foo.vue", "<template></template>"),
        ],
        &["d:/ws/tsconfig.json"],
        "d:/ws/src/Foo.vue",
        capabilities,
        serving,
        basis_args,
    )
}

/// [`certified_binding`] over an explicit workspace: the composition cases
/// resolve alias-bearing and multi-project workspaces through this same real
/// ownership chain.
fn certified_binding_for(
    files: &[(&str, &str)],
    tsconfigs: &[&str],
    source_uri: &str,
    capabilities: EngineCapabilities,
    serving: &EngineIdentity,
    basis_args: &[u8],
) -> Result<CertifiedTypeEngineBinding, CertificationRefusal> {
    let resolution = resolve_with(files, tsconfigs, source_uri);
    let binding = match resolution {
        CarrierOwnershipResolution::Bound(b) => b,
        other => panic!("expected ProjectBinding, got {other:?}"),
    };
    let witness = BoundProject::from_ensured(&binding.ensure_project_request(), capabilities);
    let basis = InputBasisId::from_canonical(&BasisArgs(basis_args));
    CertifiedTypeEngineBinding::certify(&witness, serving, basis)
}

struct BasisArgs<'a>(&'a [u8]);

impl verter_identity::encoding::CanonicalEncode for BasisArgs<'_> {
    const DOMAIN_TAG: &'static str = "verter.session.tests.semantic_capability.basis.v1";

    fn encode_fields(&self, encoder: &mut verter_identity::encoding::CanonicalEncoder) {
        encoder.field_bytes(1, self.0);
    }
}

#[test]
fn catalog_closes_every_query_feature_exactly_once() {
    // Completeness + no dual owner: every TypeScript semantic capability the
    // engine plane models has exactly one closure row, and the catalog holds
    // nothing besides them. A dropped or duplicated row fails here.
    assert_eq!(
        SEMANTIC_CAPABILITY_CATALOG.len(),
        EVERY_CAPABILITY.len(),
        "the catalog must hold exactly one row per closed capability"
    );
    for feature in EVERY_CAPABILITY {
        let rows = SEMANTIC_CAPABILITY_CATALOG
            .iter()
            .filter(|row| row.feature() == *feature)
            .count();
        assert_eq!(rows, 1, "exactly one row for {feature:?}");
        assert!(
            capability_row(*feature).feature() == *feature,
            "the sole-row lookup must answer {feature:?}"
        );
    }
}

#[test]
fn catalog_rows_are_canonically_ordered_with_unique_tags() {
    // Deterministic ordering is part of the contract bytes: the catalog reads
    // in strictly ascending query-kind domain-tag order, so two reads (or two
    // builds) can never disagree about what order the closure enumerates.
    for pair in SEMANTIC_CAPABILITY_CATALOG.windows(2) {
        assert!(
            pair[0].query_kind_domain_tag() < pair[1].query_kind_domain_tag(),
            "domain tags must strictly ascend: {} !< {}",
            pair[0].query_kind_domain_tag(),
            pair[1].query_kind_domain_tag()
        );
    }
}

#[test]
fn result_contract_is_derived_deterministically_from_the_row() {
    // Provenance: a row's result contract is derived from the row's own
    // canonical bytes — same row, same contract, every call; two rows never
    // share one contract.
    let first = SEMANTIC_CAPABILITY_CATALOG[0];
    assert_eq!(
        first.result_contract(),
        first.result_contract(),
        "the same row must derive the same contract on every call"
    );
    for pair in SEMANTIC_CAPABILITY_CATALOG.windows(2) {
        assert_ne!(
            pair[0].result_contract(),
            pair[1].result_contract(),
            "two capabilities must not share a result contract"
        );
    }
}

#[test]
fn certification_records_observed_profile_project_and_live_basis() {
    // The witness is provenance-readable: which project was resolved, which
    // capability interpretation was observed, and which basis answers will be
    // attributed to.
    let serving = owned_serving("7.0.2");
    let binding = certified_binding(observed_capabilities("7.0.2"), &serving, b"basis-1")
        .expect("certification over negotiated capabilities must succeed");
    assert_eq!(binding.project(), "d:/ws/tsconfig.json");
    assert_eq!(
        binding.input_basis(),
        &InputBasisId::from_canonical(&BasisArgs(b"basis-1"))
    );
    // The same observation composes the same profile; the profile is carried
    // by value, so re-certification over identical facts agrees.
    let again = certified_binding(observed_capabilities("7.0.2"), &serving, b"basis-1")
        .expect("certification over negotiated capabilities must succeed");
    assert_eq!(
        binding.observed_profile(),
        again.observed_profile(),
        "identical negotiated facts must compose the identical observed profile"
    );
}

#[test]
fn certification_refuses_unobserved_engine_capabilities() {
    // Fail-closed: default-constructed capabilities carry no recorded
    // handshake, so certifying them would record an ASSUMED interpretation —
    // the self-certified status this closure exists to refuse.
    let serving = owned_serving("7.0.2");
    let refused = certified_binding(EngineCapabilities::default(), &serving, b"basis-1");
    assert!(
        matches!(
            refused,
            Err(CertificationRefusal::UnobservedEngineCapabilities)
        ),
        "certification over unobserved capabilities must refuse, got {refused:?}"
    );
}

#[test]
fn capability_interpretation_changes_the_question_not_just_the_answer() {
    // Two answers produced under different capability interpretations are
    // different answers: flipping one negotiated capability (or the recorded
    // engine version) changes the observed profile and therefore the composed
    // query identity for otherwise-identical arguments.
    let args = CanonicalDigest::of_bytes(b"hover-args");
    let serving = owned_serving("7.0.2");

    let base = certified_binding(observed_capabilities("7.0.2"), &serving, b"basis-1")
        .expect("certification over negotiated capabilities must succeed");
    let flipped = {
        let mut caps = observed_capabilities("7.0.2");
        caps.async_cancellable_queries = true;
        certified_binding(caps, &serving, b"basis-1").expect("certification must succeed")
    };
    let versioned = certified_binding(
        observed_capabilities("7.0.3"),
        &owned_serving("7.0.3"),
        b"basis-1",
    )
    .expect("certification over negotiated capabilities must succeed");

    let base_qid = base.query_identity::<()>(QueryFeature::Hover, args);
    assert_eq!(
        base.query_identity::<()>(QueryFeature::Hover, args),
        base_qid,
        "the same binding must compose the same identity for the same question"
    );
    assert_ne!(
        flipped.query_identity::<()>(QueryFeature::Hover, args),
        base_qid,
        "a different negotiated capability must compose a different identity"
    );
    assert_ne!(
        versioned.query_identity::<()>(QueryFeature::Hover, args),
        base_qid,
        "a different recorded engine version must compose a different identity"
    );
}

#[test]
fn query_identity_is_snapshot_independent_and_flight_key_is_basis_bound() {
    // The question is the question regardless of which snapshot answers it:
    // two bindings over different bases compose the SAME query identity, while
    // their flight keys differ — the flight key is strictly bigger and never
    // coerces back. The capability is part of the question: another row composes
    // another identity even for identical arguments.
    let args = CanonicalDigest::of_bytes(b"same-args");
    let serving = owned_serving("7.0.2");

    let first = certified_binding(observed_capabilities("7.0.2"), &serving, b"basis-1")
        .expect("certification over negotiated capabilities must succeed");
    let second = certified_binding(observed_capabilities("7.0.2"), &serving, b"basis-2")
        .expect("certification over negotiated capabilities must succeed");

    let qid_first = first.query_identity::<()>(QueryFeature::Hover, args);
    assert_eq!(
        second.query_identity::<()>(QueryFeature::Hover, args),
        qid_first,
        "the basis must not enter the cross-snapshot question identity"
    );
    assert_ne!(
        first.query_identity::<()>(QueryFeature::Definition, args),
        qid_first,
        "another capability is another question"
    );

    let flight_first = first.flight_key::<()>(QueryFeature::Hover, args);
    let flight_second = second.flight_key::<()>(QueryFeature::Hover, args);
    assert_ne!(
        flight_first, flight_second,
        "flight keys over different bases must differ"
    );
    assert_eq!(
        flight_first.input_basis,
        *first.input_basis(),
        "the flight key must attribute the answer to the certified basis"
    );
    assert_eq!(
        flight_first.query_identity, qid_first,
        "the flight key must carry the composed question identity unchanged"
    );
}

#[test]
fn serving_session_separates_question_identities() {
    // OWNED and SHARED sessions over identical negotiated facts must not
    // share a question identity: the serving mode, wire pin, and session
    // generation are first-class profile dimensions, so one engine's facts
    // cannot launder into the other's. Same for distinct wire pins and
    // generations under one mode.
    let args = CanonicalDigest::of_bytes(b"same-args");
    let caps = observed_capabilities("7.0.2");

    let owned = certified_binding(caps.clone(), &owned_serving("7.0.2"), b"basis-1")
        .expect("certification over negotiated capabilities must succeed");
    let shared = certified_binding(
        caps.clone(),
        &serving_identity(ServeMode::Shared, "7.0.2", 7, 3),
        b"basis-1",
    )
    .expect("certification over negotiated capabilities must succeed");
    let repinned = certified_binding(
        caps.clone(),
        &serving_identity(ServeMode::Owned, "7.0.2", 9, 3),
        b"basis-1",
    )
    .expect("certification over negotiated capabilities must succeed");
    let reconnected = certified_binding(
        caps,
        &serving_identity(ServeMode::Owned, "7.0.2", 7, 4),
        b"basis-1",
    )
    .expect("certification over negotiated capabilities must succeed");

    let owned_qid = owned.query_identity::<()>(QueryFeature::Hover, args);
    assert_ne!(
        shared.query_identity::<()>(QueryFeature::Hover, args),
        owned_qid,
        "SHARED must not share OWNED's question identity over identical facts"
    );
    assert_ne!(
        repinned.query_identity::<()>(QueryFeature::Hover, args),
        owned_qid,
        "a different wire pin must compose a different identity"
    );
    assert_ne!(
        reconnected.query_identity::<()>(QueryFeature::Hover, args),
        owned_qid,
        "a bumped session generation must compose a different identity"
    );
}

/// A workspace where one carrier reaches shared code through a re-export
/// alias and another reaches it directly. Both carriers belong to the one
/// configured project; the alias spelling must not fork the composed
/// identity.
fn alias_workspace() -> Vec<(&'static str, &'static str)> {
    vec![
        ("d:/ws/tsconfig.json", r#"{ "include": ["src/**/*"] }"#),
        ("d:/ws/src/real.ts", "export const value: number = 1;"),
        ("d:/ws/src/alias.ts", "export { value } from './real';"),
        (
            "d:/ws/src/Direct.vue",
            "<template></template><script>import { value } from './real';</script>",
        ),
        (
            "d:/ws/src/Aliased.vue",
            "<template></template><script>import { value } from './alias';</script>",
        ),
    ]
}

#[test]
fn alias_and_direct_references_share_one_composed_identity() {
    // Project-wide reference composition: the direct and the aliased carrier
    // resolve through the same real ownership chain into the same project, so
    // certification records the same observed profile digest and composes the
    // same References identity for the same arguments. A carrier in a second
    // project composes a different one.
    let files = alias_workspace();
    let caps = observed_capabilities("7.0.2");
    let serving = owned_serving("7.0.2");

    let direct = certified_binding_for(
        &files,
        &["d:/ws/tsconfig.json"],
        "d:/ws/src/Direct.vue",
        caps.clone(),
        &serving,
        b"basis-1",
    )
    .expect("direct carrier must certify");
    let aliased = certified_binding_for(
        &files,
        &["d:/ws/tsconfig.json"],
        "d:/ws/src/Aliased.vue",
        caps.clone(),
        &serving,
        b"basis-1",
    )
    .expect("aliased carrier must certify");
    assert_eq!(
        direct.observed_profile(),
        aliased.observed_profile(),
        "alias spelling must not fork the observed profile: direct stable-ID/digest equality"
    );

    let args = CanonicalDigest::of_bytes(b"reference-args");
    assert_eq!(
        direct.query_identity::<()>(QueryFeature::References, args),
        aliased.query_identity::<()>(QueryFeature::References, args),
        "alias spelling must not fork the composed References identity"
    );

    let other_project = certified_binding_for(
        &[
            ("d:/ws/other.json", r#"{ "include": ["lib/**/*"] }"#),
            ("d:/ws/lib/Other.vue", "<template></template>"),
        ],
        &["d:/ws/other.json"],
        "d:/ws/lib/Other.vue",
        caps,
        &serving,
        b"basis-1",
    )
    .expect("other-project carrier must certify");
    assert_ne!(
        other_project.query_identity::<()>(QueryFeature::References, args),
        direct.query_identity::<()>(QueryFeature::References, args),
        "a different project must compose a different References identity"
    );
}

#[test]
fn auto_import_completion_identity_is_stable_over_reexport_aliases() {
    // Auto-import composition: Completion identities composed for an
    // importable symbol are identical whether the importing carrier spells
    // the direct path or the re-export alias, because both certify over the
    // same project, serving session, and capability interpretation.
    let files = alias_workspace();
    let caps = observed_capabilities("7.0.2");
    let serving = owned_serving("7.0.2");

    let direct = certified_binding_for(
        &files,
        &["d:/ws/tsconfig.json"],
        "d:/ws/src/Direct.vue",
        caps.clone(),
        &serving,
        b"basis-1",
    )
    .expect("direct carrier must certify");
    let aliased = certified_binding_for(
        &files,
        &["d:/ws/tsconfig.json"],
        "d:/ws/src/Aliased.vue",
        caps,
        &serving,
        b"basis-1",
    )
    .expect("aliased carrier must certify");

    let args = CanonicalDigest::of_bytes(b"auto-import-completion-args");
    assert_eq!(
        direct.query_identity::<()>(QueryFeature::Completion, args),
        aliased.query_identity::<()>(QueryFeature::Completion, args),
        "auto-import Completion identity must be stable over re-export aliases"
    );
    // The flight keys agree too: same question plus the same certified basis.
    assert_eq!(
        direct.flight_key::<()>(QueryFeature::Completion, args),
        aliased.flight_key::<()>(QueryFeature::Completion, args),
        "auto-import flight keys must agree over re-export aliases"
    );
}
