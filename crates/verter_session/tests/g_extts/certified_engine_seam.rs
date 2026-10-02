//! Contract cases for the ACTIVATED dual-plane semantic seam: the publication
//! route a real TypeScript engine answer travels.
//!
//! The dormant half of this plane is covered by `semantic_capability_closure`.
//! These cases cover the part that only exists once the plane is switched ON:
//! the snapshot identity a certification is admitted against, and the
//! publication rule that refuses a superseded basis before it can warm. They
//! drive the same production types the publish coordinator drives
//! (`EngineBackend::publish_snapshot` takes the certified binding), so a green
//! run is evidence about the activated route rather than about a helper.

use std::sync::Arc;

use verter_identity::encoding::CanonicalDigest;
use verter_session::external_ts::{
    BoundProject, CarrierOwnershipResolution, EngineCapabilities, EngineIdentity,
    EngineSessionFacts, OpenState, PublishSnapshot, QueryFeature, ScriptKind, ServeMode,
    SnapshotFile, SnapshotRole,
};
use verter_session::semantic_capability::CertifiedTypeEngineBinding;

use super::shared::resolve_with;

const PROJECT: &str = "file:///project/tsconfig.json";
const SOURCE: &str = "file:///project/src/Foo.vue";

/// Negotiated capabilities that carry a recorded handshake version — the
/// observation certification requires.
fn observed_capabilities(version: &str) -> EngineCapabilities {
    EngineCapabilities {
        static_module_resolution_map: false,
        async_cancellable_queries: false,
        reported_version: Some(Arc::<str>::from(version)),
    }
}

/// The serving session certification records. The pin and generation are
/// pinned so an accidental axis change is always deliberate at a call site.
fn owned_serving(version: &str) -> EngineIdentity {
    EngineIdentity::for_mode(
        ServeMode::Owned,
        &EngineSessionFacts {
            observed_version: Arc::<str>::from(version),
            wire_pin: 7,
            editor_session_generation: 3,
        },
    )
}

/// One published carrier companion over the given carrier bytes, at `version`.
fn snapshot_file(content: &str, version: u64) -> SnapshotFile {
    SnapshotFile {
        source_uri: Arc::from(SOURCE),
        provider_uri: Arc::from("file:///project/src/Foo.vue.tsx"),
        role: SnapshotRole::CarrierIde,
        script_kind: ScriptKind::Tsx,
        content: Arc::from(content),
        content_hash: hash_of(content),
        map_hash: hash_of("carrier-geometry"),
        map_json: None,
        structure: None,
        version,
        open_state: OpenState::Closed,
    }
}

/// A stable 16-byte identity for the test's synthetic values. The real route
/// carries producer-computed hashes; the basis composition under test never
/// hashes content itself (it encodes the identity), so a deterministic filler
/// is the honest stand-in for one.
fn hash_of(seed: &str) -> [u8; 16] {
    let digest = CanonicalDigest::of_bytes(seed.as_bytes());
    let mut out = [0_u8; 16];
    out.copy_from_slice(&digest.as_bytes()[..16]);
    out
}

fn snapshot_of(files: Vec<SnapshotFile>) -> PublishSnapshot {
    PublishSnapshot {
        project: Arc::from(PROJECT),
        files,
        resolution_map_version: 4,
        fs_generation: 11,
    }
}

/// The real witness chain up to the certification: a configured project
/// resolved for a real carrier source, an `EnsureProject` request minted from
/// the binding, and a `BoundProject` minted from that request.
fn bound_witness(capabilities: EngineCapabilities) -> BoundProject {
    let resolution = resolve_with(
        &[
            ("d:/ws/tsconfig.json", r#"{ "include": ["src/**/*"] }"#),
            ("d:/ws/src/Foo.vue", "<template></template>"),
        ],
        &["d:/ws/tsconfig.json"],
        "d:/ws/src/Foo.vue",
    );
    let binding = match resolution {
        CarrierOwnershipResolution::Bound(binding) => binding,
        other => panic!("expected a bound ProjectBinding, got {other:?}"),
    };
    BoundProject::from_ensured(&binding.ensure_project_request(), capabilities)
}

/// Certify over a snapshot's OWN basis — what the publish coordinator does
/// before it reaches the store.
fn certify_for(snapshot: &PublishSnapshot) -> CertifiedTypeEngineBinding {
    CertifiedTypeEngineBinding::certify(
        &bound_witness(observed_capabilities("5.9.2")),
        &owned_serving("5.9.2"),
        snapshot.input_basis(),
    )
    .expect("an observed handshake certifies")
}

#[test]
fn a_certified_binding_admits_the_snapshot_it_was_certified_over() {
    let snapshot = snapshot_of(vec![snapshot_file("const a = 1;", 7)]);
    let certified = certify_for(&snapshot);

    assert!(
        certified.publish_admitted(&snapshot),
        "the snapshot the binding was certified over is the one it admits"
    );
}

#[test]
fn a_superseded_snapshot_is_refused_before_it_can_warm() {
    let certified = certify_for(&snapshot_of(vec![snapshot_file("const a = 1;", 7)]));

    // Each variant is the SAME publish arriving against a snapshot whose basis
    // has moved on: new carrier bytes, a new companion version, a new project,
    // a new generation. Every one must be refused, not downgraded and not
    // published — a stale basis has no result to return, let alone to warm.
    let superseded = [
        snapshot_of(vec![snapshot_file("const a = 2;", 7)]),
        snapshot_of(vec![snapshot_file("const a = 1;", 8)]),
        PublishSnapshot {
            project: Arc::from("file:///other/tsconfig.json"),
            files: vec![snapshot_file("const a = 1;", 7)],
            resolution_map_version: 4,
            fs_generation: 11,
        },
        PublishSnapshot {
            project: Arc::from(PROJECT),
            files: vec![snapshot_file("const a = 1;", 7)],
            resolution_map_version: 5,
            fs_generation: 11,
        },
        PublishSnapshot {
            project: Arc::from(PROJECT),
            files: vec![snapshot_file("const a = 1;", 7)],
            resolution_map_version: 4,
            fs_generation: 12,
        },
        // An empty publish is a DIFFERENT identity from a populated one, not a
        // subset of it: dropping the companions must not inherit the
        // certification of the populated snapshot.
        snapshot_of(Vec::new()),
    ];

    for candidate in &superseded {
        assert!(
            !certified.publish_admitted(candidate),
            "a superseded basis must be refused, not admitted: {candidate:?}"
        );
    }
}

#[test]
fn the_snapshot_basis_is_derived_not_named() {
    // The basis is composed from the snapshot's own identity, so it cannot be
    // supplied beside it. Equal identities compose ONE basis regardless of
    // order, and any identity difference composes a different one — otherwise a
    // caller could certify over one snapshot and publish another's bytes under
    // that certification.
    let first = snapshot_file("const a = 1;", 7);
    let second = snapshot_file("const b = 2;", 7);
    let forward = snapshot_of(vec![first.clone(), second.clone()]);
    let reversed = snapshot_of(vec![second, first.clone()]);
    let recomposed = snapshot_of(vec![first.clone(), first.clone()]);

    assert_eq!(
        forward.input_basis(),
        reversed.input_basis(),
        "file order is not part of a snapshot's identity"
    );

    // A repeated companion is one observation, not two: the file set is a SET,
    // so re-listing the same carrier cannot mint a second basis for one publish.
    assert_eq!(
        recomposed.input_basis(),
        snapshot_of(vec![first]).input_basis(),
        "a repeated file collapses to one observation"
    );

    assert_ne!(
        forward.input_basis(),
        snapshot_of(vec![snapshot_file("const a = 1;", 7)]).input_basis(),
        "a different companion set is a different publish, not a subset of one"
    );
}

#[test]
fn the_certified_project_is_the_project_the_snapshot_publishes_under() {
    // The binding carries the resolved project's identity, and the backend
    // routes on it — so a snapshot published under a project the certification
    // did not resolve is a mismatch the seam can see without any handle.
    let certified = certify_for(&snapshot_of(vec![snapshot_file("const a = 1;", 7)]));
    assert_eq!(
        certified.project(),
        "d:/ws/tsconfig.json",
        "certification records the RESOLVED project, not the one the caller named"
    );
}

#[test]
fn an_unobserved_engine_never_reaches_the_publication_rule() {
    // The publication rule is only meaningful over a certification. An engine
    // whose handshake never happened cannot be certified at all, so there is no
    // binding that would admit its snapshot — the route fails closed at the
    // first link rather than answering under an assumed profile.
    let snapshot = snapshot_of(vec![snapshot_file("const a = 1;", 7)]);
    let refusal = CertifiedTypeEngineBinding::certify(
        &bound_witness(EngineCapabilities::default()),
        &owned_serving("5.9.2"),
        snapshot.input_basis(),
    );

    assert!(
        refusal.is_err(),
        "an unobserved engine has no certification, so no snapshot is admitted"
    );
}

#[test]
fn the_flight_key_of_a_published_flight_is_basis_bound() {
    // The production key a flight is recorded under stays basis-bound: the
    // query identity is snapshot-independent, and pairing it with the
    // certified basis is what makes a superseded answer distinguishable from a
    // current one.
    let snapshot = snapshot_of(vec![snapshot_file("const a = 1;", 7)]);
    let certified = certify_for(&snapshot);
    let arguments = CanonicalDigest::of_bytes(b"Foo.vue:1:7");

    let query = certified.query_identity(QueryFeature::Hover, arguments);
    let flight = certified.flight_key::<u32>(QueryFeature::Hover, arguments);

    assert_eq!(
        &query, &flight.query_identity,
        "the flight key embeds the question identity unchanged"
    );
    assert_eq!(
        &flight.input_basis,
        certified.input_basis(),
        "the flight is attributed to the certified basis"
    );
}
