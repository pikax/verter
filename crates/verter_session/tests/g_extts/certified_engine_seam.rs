//! Contract cases for the ACTIVATED dual-plane semantic seam: the publication
//! route a real TypeScript engine answer travels.
//!
//! The dormant half of this plane is covered by `semantic_capability_closure`.
//! These cases cover the part that only exists once the plane is switched ON:
//! the snapshot identity a certification is admitted against, and the
//! publication rule that refuses a superseded basis. They drive the SAME
//! production types the publish coordinator drives
//! (`CertifiedTypeEngineBinding::certify` over a real resolved project, and
//! `PublishSnapshot::input_basis` over the snapshot the coordinator publishes),
//! so a green run is evidence about the activated route's identity contract.
//!
//! What these cases are NOT evidence for: the store write itself. The refusal in
//! FRONT of the warm is `CertifiedTypeEngineBinding::publish_admitted`, and the
//! cases below call it directly; whether a refused publish actually leaves the
//! on-disk manifest untouched is the engine seam's own question, answered where
//! the store lives — in `verter_lsp`'s `tsserver_backend_tests` (a
//! certify-here/publish-there case and a rotated-session case, each asserting an
//! unchanged manifest).

use std::sync::Arc;

use verter_identity::encoding::CanonicalDigest;
use verter_session::external_ts::{
    BoundProject, CarrierOwnershipResolution, EngineCapabilities, EngineIdentity,
    EngineSessionFacts, EngineVersion, OpenState, PublishSnapshot, QueryFeature, ScriptKind,
    ServeMode, SnapshotFile, SnapshotRole, SnapshotStructureStamp,
};
use verter_session::semantic_capability::CertifiedTypeEngineBinding;

use super::shared::resolve_with;

const PROJECT: &str = "file:///project/tsconfig.json";
const SOURCE: &str = "file:///project/src/Foo.vue";

/// Negotiated capabilities whose version the engine REPORTED in-band — the
/// observation certification requires.
fn observed_capabilities(version: &str) -> EngineCapabilities {
    EngineCapabilities {
        static_module_resolution_map: false,
        async_cancellable_queries: false,
        version: EngineVersion::Reported(Arc::<str>::from(version)),
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

/// The content-free structure stamp the store copies onto the published row
/// beside the source map.
fn structure_stamp(token: &str) -> SnapshotStructureStamp {
    SnapshotStructureStamp {
        schema_version: 1,
        artifact_token: Arc::from(token),
        script_content_ranges: vec![[0, 12]],
        markup_opening_ranges: vec![[0, 10]],
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
    // Certified over a companion that carries a structure stamp: the stamp is
    // part of what the store publishes, so the variants below separate from THIS
    // snapshot on axes the declared hashes do not move.
    let mut certified_file = snapshot_file("const a = 1;", 7);
    certified_file.structure = Some(structure_stamp("token-a"));
    let certified = certify_for(&snapshot_of(vec![certified_file]));

    // Each variant is the SAME publish arriving against a snapshot whose basis
    // has moved on: new carrier bytes, a new companion version, a new project,
    // a new generation. Every one must be refused, not downgraded and not
    // published — a stale basis has no result to return, let alone to warm.
    //
    // The last three are the ones the store would act on while every DECLARED
    // identity stays put: a different content-free structure stamp, the same
    // stamp arriving unstamped, and different carrier bytes behind one declared
    // content hash (the store writes those bytes).
    let mut restamped = snapshot_file("const a = 1;", 7);
    restamped.structure = Some(structure_stamp("token-b"));
    let mut behind_one_identity = snapshot_file("const a = 1;", 7);
    behind_one_identity.structure = Some(structure_stamp("token-a"));
    behind_one_identity.content = Arc::from("const a = 2;");
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
        snapshot_of(vec![restamped]),
        snapshot_of(vec![snapshot_file("const a = 1;", 7)]),
        snapshot_of(vec![behind_one_identity]),
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
fn a_locally_declared_version_never_composes_an_engine_observation() {
    // A publisher that handshakes no engine still needs a version dimension, and
    // gets one by DECLARING its own local segment. The provenance is part of
    // what composes the profile, so the same text over the same project and
    // serving session is two profiles — a local segment can never be read as an
    // observation of the peer, which is the self-certification this plane
    // rejects. Two declarations of the same segment still compose ONE profile.
    let snapshot = snapshot_of(vec![snapshot_file("const a = 1;", 7)]);
    let declared = |version: &str| EngineCapabilities {
        static_module_resolution_map: false,
        async_cancellable_queries: false,
        version: EngineVersion::Declared(Arc::<str>::from(version)),
    };
    let reported = |version: &str| EngineCapabilities {
        static_module_resolution_map: false,
        async_cancellable_queries: false,
        version: EngineVersion::Reported(Arc::<str>::from(version)),
    };
    let profile_over = |capabilities: EngineCapabilities| {
        CertifiedTypeEngineBinding::certify(
            &bound_witness(capabilities),
            &owned_serving("5.9.2"),
            snapshot.input_basis(),
        )
        .expect("a declared or reported version certifies")
        .observed_profile()
        .clone()
    };

    assert_ne!(
        profile_over(declared("0.1.22")),
        profile_over(reported("0.1.22")),
        "a locally declared segment and an engine-reported version over the same text are \\
         different profiles"
    );
    assert_eq!(
        profile_over(declared("0.1.22")),
        profile_over(declared("0.1.22")),
        "the same local segment is the same profile"
    );
    assert_ne!(
        profile_over(declared("0.1.22")),
        profile_over(declared("0.1.23")),
        "two differently pinned publishers never compose one profile"
    );
}

#[test]
fn a_retained_binding_is_not_admitted_by_a_rotated_serving_session() {
    // The basis cannot express the serving session: a snapshot can sit unchanged
    // across a ledger session rotation, and the rotation changes what every
    // published row means. So the binding carries the session it observed, and
    // the seam re-checks it where the store write happens.
    let snapshot = snapshot_of(vec![snapshot_file("const a = 1;", 7)]);
    let certified = certify_for(&snapshot);

    assert!(
        certified.serving_admitted(&owned_serving("5.9.2")),
        "the session the binding was certified under is the one it admits"
    );

    let rotated = EngineIdentity::for_mode(
        ServeMode::Owned,
        &EngineSessionFacts {
            observed_version: Arc::<str>::from("5.9.2"),
            wire_pin: 7,
            editor_session_generation: 4,
        },
    );
    assert!(
        !certified.serving_admitted(&rotated),
        "a rotated session generation is a different serving session, even over identical \\
         bytes and an identical basis"
    );
    assert!(
        certified.publish_admitted(&snapshot),
        "the snapshot is unchanged — it is the SESSION, not the bytes, that moved"
    );

    let repinned = EngineIdentity::for_mode(
        ServeMode::Shared,
        &EngineSessionFacts {
            observed_version: Arc::<str>::from("5.9.2"),
            wire_pin: 7,
            editor_session_generation: 3,
        },
    );
    assert!(
        !certified.serving_admitted(&repinned),
        "a SHARED session over the same facts is a different serving session"
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
