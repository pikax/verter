//! Contract cases for the TypeScript semantic capability closure — the
//! semantic plane's dormant half of the dual-plane identity contract.
//!
//! Every case drives the real witness chain (resolver → `ProjectBinding` →
//! `EnsureProject` → `BoundProject` → [`CertifiedTypeEngineBinding`]), so a
//! green run here is evidence the sole-route discipline holds end to end, not
//! just that the types compose.

use verter_identity::encoding::CanonicalDigest;
use verter_identity::identity::InputBasisId;
use verter_session::external_ts::{BoundProject, CarrierOwnershipResolution, EngineCapabilities};
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

/// Mint a `CertifiedTypeEngineBinding` through the full production witness
/// chain: a configured project resolved for a real carrier source, an
/// `EnsureProject` request minted from the binding, a `BoundProject` witness
/// minted from that request, then certification over negotiated capabilities.
fn certified_binding(
    capabilities: EngineCapabilities,
    basis_args: &[u8],
) -> Result<CertifiedTypeEngineBinding, CertificationRefusal> {
    let resolution = resolve_with(
        &[
            ("d:/ws/tsconfig.json", r#"{ "include": ["src/**/*"] }"#),
            ("d:/ws/src/Foo.vue", "<template></template>"),
        ],
        &["d:/ws/tsconfig.json"],
        "d:/ws/src/Foo.vue",
    );
    let binding = match resolution {
        CarrierOwnershipResolution::Bound(b) => b,
        other => panic!("expected ProjectBinding, got {other:?}"),
    };
    let witness = BoundProject::from_ensured(&binding.ensure_project_request(), capabilities);
    let basis = InputBasisId::from_canonical(&BasisArgs(basis_args));
    CertifiedTypeEngineBinding::certify(&witness, basis)
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
            .filter(|row| row.feature == *feature)
            .count();
        assert_eq!(rows, 1, "exactly one row for {feature:?}");
        assert!(
            capability_row(*feature).feature == *feature,
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
            pair[0].query_kind_domain_tag < pair[1].query_kind_domain_tag,
            "domain tags must strictly ascend: {} !< {}",
            pair[0].query_kind_domain_tag,
            pair[1].query_kind_domain_tag
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
    let binding = certified_binding(observed_capabilities("7.0.2"), b"basis-1")
        .expect("certification over negotiated capabilities must succeed");
    assert_eq!(binding.project(), "d:/ws/tsconfig.json");
    assert_eq!(
        binding.input_basis(),
        &InputBasisId::from_canonical(&BasisArgs(b"basis-1"))
    );
    // The same observation composes the same profile; the profile is carried
    // by value, so re-certification over identical facts agrees.
    let again = certified_binding(observed_capabilities("7.0.2"), b"basis-1")
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
    let refused = certified_binding(EngineCapabilities::default(), b"basis-1");
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
    let hover = capability_row(verter_session::external_ts::QueryFeature::Hover);
    let args = CanonicalDigest::of_bytes(b"hover-args");

    let base = certified_binding(observed_capabilities("7.0.2"), b"basis-1")
        .expect("certification over negotiated capabilities must succeed");
    let flipped = {
        let mut caps = observed_capabilities("7.0.2");
        caps.async_cancellable_queries = true;
        certified_binding(caps, b"basis-1").expect("certification must succeed")
    };
    let versioned = certified_binding(observed_capabilities("7.0.3"), b"basis-1")
        .expect("certification over negotiated capabilities must succeed");

    let base_qid = base.query_identity::<()>(hover, args);
    assert_eq!(
        base.query_identity::<()>(hover, args),
        base_qid,
        "the same binding must compose the same identity for the same question"
    );
    assert_ne!(
        flipped.query_identity::<()>(hover, args),
        base_qid,
        "a different negotiated capability must compose a different identity"
    );
    assert_ne!(
        versioned.query_identity::<()>(hover, args),
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
    let hover = capability_row(verter_session::external_ts::QueryFeature::Hover);
    let definition = capability_row(verter_session::external_ts::QueryFeature::Definition);
    let args = CanonicalDigest::of_bytes(b"same-args");

    let first = certified_binding(observed_capabilities("7.0.2"), b"basis-1")
        .expect("certification over negotiated capabilities must succeed");
    let second = certified_binding(observed_capabilities("7.0.2"), b"basis-2")
        .expect("certification over negotiated capabilities must succeed");

    let qid_first = first.query_identity::<()>(hover, args);
    assert_eq!(
        second.query_identity::<()>(hover, args),
        qid_first,
        "the basis must not enter the cross-snapshot question identity"
    );
    assert_ne!(
        first.query_identity::<()>(definition, args),
        qid_first,
        "another capability is another question"
    );

    let flight_first = first.flight_key::<()>(hover, args);
    let flight_second = second.flight_key::<()>(hover, args);
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
