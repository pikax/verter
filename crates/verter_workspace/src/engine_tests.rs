//! Workspace-default env-hash caching tests, plus the aggregate-aware
//! evidence-refresh contract.
//!
//! The workspace-default env-hash array is a pure function of the engine's
//! `default_resolve_extensions` list (every other input is a workspace
//! constant), and the workspace-default project identity is a process-wide
//! constant. The engine caches both so per-store-view reads
//! (`host_view_project_identity` / `host_view_env_hashes_for` no-owner
//! fallback on the session side) stop re-running the full
//! `crate::resolver::ide_project_config` → membership-glob-compile → 4×hash pipeline.
//!
//! These tests pin: cached values are byte-equal to an uncached fresh
//! computation; an extension-list republish invalidates the cached array;
//! readers racing a concurrent extension change only ever observe a value
//! derived from one published extension list (never a torn mix).

use std::sync::Arc;

use super::{
    compute_workspace_default_env_hash_array, workspace_default_env_hash_array_for_engine,
    workspace_default_project_identity_hash_for_engine, Engine,
};
use crate::env_hash::IdeProjectConfigEnvHash;
use crate::published_state::ProjectEnvHashArray;
use crate::published_state::PublishedRoot;
use crate::traits::{WorkspaceAccess, WorkspaceRead};

/// Uncached reference computation from the engine's LIVE extension list —
/// the exact semantics the cached read path must preserve.
fn fresh_default_env_hash_array(engine: &Engine) -> ProjectEnvHashArray {
    compute_workspace_default_env_hash_array(&engine.default_resolve_extensions.load_full())
}

#[test]
fn cached_default_env_hash_array_equals_fresh_computation() {
    let engine = Engine::new();
    let fresh = fresh_default_env_hash_array(&engine);

    let cold = workspace_default_env_hash_array_for_engine(&engine);
    let warm = workspace_default_env_hash_array_for_engine(&engine);

    assert_eq!(cold, fresh, "cold read must equal uncached computation");
    assert_eq!(
        warm, fresh,
        "warm (cached) read must equal uncached computation"
    );
    assert_ne!(
        cold, [[0u8; 16]; 4],
        "workspace default is deliberately non-zero (distinct from the all-zero trait fallback)"
    );
}

#[test]
fn cached_default_project_identity_equals_fresh_computation() {
    let engine = Engine::new();
    let fresh =
        crate::resolver::ide_project_config(String::new(), String::new(), None).project_identity();

    assert_eq!(
        workspace_default_project_identity_hash_for_engine(&engine),
        fresh,
        "cold read must equal uncached computation"
    );
    assert_eq!(
        workspace_default_project_identity_hash_for_engine(&engine),
        fresh,
        "warm (cached) read must equal uncached computation"
    );
    // Engine-independent constant: a second engine observes the same value.
    let other = Engine::new();
    assert_eq!(
        workspace_default_project_identity_hash_for_engine(&other),
        fresh
    );
    assert_ne!(
        fresh, [0u8; 16],
        "default identity must not collapse to all-zero"
    );
}

#[test]
fn extension_republish_invalidates_cached_default_env_hash_array() {
    let engine = Engine::new();
    let before = workspace_default_env_hash_array_for_engine(&engine);

    // Novel extension (not in `probe_extensions()`) — the merged list changes.
    engine.set_default_resolve_extensions(vec![".verterext".to_string()]);

    let after = workspace_default_env_hash_array_for_engine(&engine);
    assert_eq!(
        after,
        fresh_default_env_hash_array(&engine),
        "post-republish read must equal uncached computation over the NEW list"
    );

    // Extensions feed exactly the resolve dimension (R21): parse/type/lib
    // are extension-independent, resolve must move.
    assert_eq!(
        before[0], after[0],
        "parse_env_hash must not depend on extensions"
    );
    assert_ne!(
        before[1], after[1],
        "resolve_env_hash must change with the extension list"
    );
    assert_eq!(
        before[2], after[2],
        "type_env_hash must not depend on extensions"
    );
    assert_eq!(
        before[3], after[3],
        "lib_env_hash must not depend on extensions"
    );

    // Republishing the SAME list is value-stable.
    engine.set_default_resolve_extensions(vec![".verterext".to_string()]);
    assert_eq!(workspace_default_env_hash_array_for_engine(&engine), after);
}

#[test]
fn memory_workspace_trait_surface_tracks_extension_republish() {
    let ws = crate::memory::MemoryWorkspace::new(crate::memory::MemoryOptions::default());
    let before = ws.workspace_default_env_hash_array();
    assert_eq!(before, fresh_default_env_hash_array(&ws.engine));

    ws.set_default_resolve_extensions(vec![".verterext".to_string()]);

    let after = ws.workspace_default_env_hash_array();
    assert_eq!(after, fresh_default_env_hash_array(&ws.engine));
    assert_ne!(before, after, "trait surface must observe the invalidation");

    // Identity is extension-independent and stable across the republish.
    assert_eq!(
        ws.workspace_default_project_identity_hash(),
        crate::resolver::ide_project_config(String::new(), String::new(), None).project_identity()
    );
}

#[test]
fn concurrent_readers_racing_extension_change_observe_only_published_values() {
    let engine = Arc::new(Engine::new());
    let expected_old = fresh_default_env_hash_array(&engine);

    // Deterministic expected NEW value: an identical engine with the same
    // republish produces the same merged list, hence the same array.
    let reference = Engine::new();
    reference.set_default_resolve_extensions(vec![".verterext".to_string()]);
    let expected_new = fresh_default_env_hash_array(&reference);
    assert_ne!(expected_old, expected_new);

    let writer = {
        let engine = Arc::clone(&engine);
        std::thread::spawn(move || {
            engine.set_default_resolve_extensions(vec![".verterext".to_string()]);
        })
    };
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let engine = Arc::clone(&engine);
            std::thread::spawn(move || {
                for _ in 0..200 {
                    let observed = workspace_default_env_hash_array_for_engine(&engine);
                    assert!(
                        observed == expected_old || observed == expected_new,
                        "observed a value not derived from any published extension list"
                    );
                }
            })
        })
        .collect();

    writer.join().unwrap();
    for r in readers {
        r.join().unwrap();
    }
    // Post-quiescence: cache settles on the new published list.
    assert_eq!(
        workspace_default_env_hash_array_for_engine(&engine),
        expected_new
    );
}

// ---------------------------------------------------------------------------
// Aggregate-aware evidence refresh.
//
// `refresh_resolution_evidence` heals a retained candidate by re-observing
// the canonicals THAT CANDIDATE'S WITNESS RECORDED. A witness whose
// `Resolution` bucket compacted records none of them: the terminal
// aggregate stands in for every precise resolution fact the compute read,
// and names no canonical at all. Projecting canonicals out of such a
// witness therefore yields an under-approximation, not a smaller set — the
// healing pass would silently cover nothing and a recorded `Absent` would
// keep validating for the life of the process.
// ---------------------------------------------------------------------------

use crate::fact_cache::{
    AggregatePopulation, AggregateStamp, CompactionDomain, DomainGenerationFact, FactVersionRef,
    ReadSetSignature, ResolveImportsFactRef,
};
use crate::memory::{MemoryOptions, MemoryWorkspace};
use crate::resolution_currency::ResolutionEvidenceSource;
use verter_semantic::resolver_core::{
    ResolutionContext, ResolutionPopulation, ResolutionWorldId, ResolvePhase, ResolveRequestKind,
};

/// A witness whose `Resolution` bucket compacted: it carries the terminal
/// aggregate and nothing else, so it names zero canonicals.
fn compacted_resolution_witness() -> ReadSetSignature {
    ReadSetSignature::new(Arc::from([FactVersionRef::DomainGeneration(
        DomainGenerationFact {
            domain: CompactionDomain::Resolution,
            population: AggregatePopulation::Resolution(ResolutionPopulation::Base),
            stamp: AggregateStamp::ResolutionRoots {
                base: ResolutionWorldId::from_raw(1),
                session: None,
            },
        },
    )]))
}

/// A precise witness naming exactly `canonical`.
fn precise_witness(canonical: &str) -> ReadSetSignature {
    ReadSetSignature::new(Arc::from([FactVersionRef::FileWholeHash {
        canonical_id: canonical.to_string(),
        hash: [7u8; 16],
    }]))
}

/// Resolve through the Engine and return its real derived Decision witness.
/// The node is resolution evidence, is not a domain aggregate, and exposes no
/// path canonical for a live evidence source to re-read.
fn dag_rooted_unenumerable_resolution_witness(workspace: &MemoryWorkspace) -> ReadSetSignature {
    const CONTEXT: ResolutionContext = ResolutionContext {
        phase: ResolvePhase::ProviderGraph,
        kind: ResolveRequestKind::TypeImport,
    };
    let outcome = WorkspaceRead::resolve_import_outcome(workspace, "/p/owner.ts", "./dep", CONTEXT);
    let crate::SignatureAdmission::Cacheable(signature) = outcome.admission else {
        panic!("fixture invariant: the live resolution must admit its Decision witness")
    };
    let resolution_facts: Vec<_> = signature
        .facts
        .iter()
        .filter_map(|fact| match fact {
            FactVersionRef::ResolveImports(ResolveImportsFactRef::Resolution(fact)) => Some(fact),
            _ => None,
        })
        .collect();
    assert_eq!(
        resolution_facts.len(),
        1,
        "an admitted resolution must root on one derived fact"
    );
    assert!(
        resolution_facts[0].is_decision(),
        "the derived fact must be the query's Decision node"
    );
    assert!(
        !signature.aggregates_domain(CompactionDomain::Resolution),
        "a Decision witness is not a domain aggregate"
    );
    assert!(
        signature.resolution_path_canonical_ids().is_empty(),
        "a Decision is computed from its DAG edges, never re-read off a path"
    );
    signature
}

fn memory_workspace_with(canonical: &str) -> MemoryWorkspace {
    let workspace = MemoryWorkspace::new(MemoryOptions::default());
    workspace.inject_file(canonical.to_string(), Arc::from("export const v = 1\n"));
    workspace
}

#[test]
fn a_compacted_resolution_witness_still_reobserves_the_pending_ledger() {
    let dep = "/p/dep.ts";
    let workspace = memory_workspace_with(dep);
    let generation = workspace.engine.bump_content_generation_for(dep);
    assert!(
        workspace.engine.pending_resolution_refresh_for_test(dep),
        "precondition: a content transition enqueues the canonical for \
         re-observation"
    );

    workspace.engine.refresh_resolution_evidence(
        &workspace,
        ResolutionEvidenceSource::ReaderAuthoritative,
        &compacted_resolution_witness(),
    );

    assert_eq!(
        workspace.engine.evidence_verified_generation_for_test(dep),
        Some(generation),
        "a compacted `Resolution` witness cannot enumerate the canonicals it \
         depends on, so the healing pass must fall back to the WHOLE pending \
         ledger. Projecting `canonical_ids()` out of the aggregate yields an \
         empty target set, the pass covers nothing, and the recorded \
         evidence keeps validating for the life of the process"
    );
    assert!(
        !workspace.engine.pending_resolution_refresh_for_test(dep),
        "and the re-observed canonical must drain from the pending ledger — \
         an entry that never drains defeats the ledger's `is_empty()` \
         early-out for every resolution in the process"
    );
}

#[test]
fn a_nonaggregate_unenumerable_witness_still_reobserves_the_pending_ledger() {
    let dep = "/p/dep.ts";
    let workspace = memory_workspace_with(dep);
    let witness = dag_rooted_unenumerable_resolution_witness(&workspace);
    let generation = workspace.engine.bump_content_generation_for(dep);

    workspace.engine.refresh_resolution_evidence(
        &workspace,
        ResolutionEvidenceSource::ReaderAuthoritative,
        &witness,
    );

    assert_eq!(
        workspace.engine.evidence_verified_generation_for_test(dep),
        Some(generation),
        "the fallback must key on whether the witness can enumerate live path evidence, not on \
         whether its Resolution facts happened to be compacted"
    );
    assert!(!workspace.engine.pending_resolution_refresh_for_test(dep));
}

#[test]
fn a_precise_witness_still_reobserves_only_the_canonicals_it_names() {
    let dep = "/p/dep.ts";
    let workspace = memory_workspace_with(dep);
    let _generation = workspace.engine.bump_content_generation_for(dep);
    assert!(workspace.engine.pending_resolution_refresh_for_test(dep));

    // Names a DIFFERENT canonical. The aggregate fallback must not degrade
    // into "always refresh the whole ledger": a precise witness stays
    // strictly O(its own facts).
    workspace.engine.refresh_resolution_evidence(
        &workspace,
        ResolutionEvidenceSource::ReaderAuthoritative,
        &precise_witness("/p/other.ts"),
    );

    assert_eq!(
        workspace.engine.evidence_verified_generation_for_test(dep),
        None,
        "a precise witness must re-observe only the canonicals it records; \
         widening it to the whole pending ledger would make every retained \
         candidate pay for every unrelated transition"
    );
    assert!(
        workspace.engine.pending_resolution_refresh_for_test(dep),
        "and an unnamed canonical must stay pending"
    );
}

#[test]
fn an_empty_witness_reobserves_nothing() {
    let dep = "/p/dep.ts";
    let workspace = memory_workspace_with(dep);
    let _generation = workspace.engine.bump_content_generation_for(dep);
    assert!(workspace.engine.pending_resolution_refresh_for_test(dep));

    // The fallback must key on the AGGREGATE, never on "the projection came
    // back empty" — an empty signature names nothing because it read
    // nothing, which is the opposite situation.
    workspace.engine.refresh_resolution_evidence(
        &workspace,
        ResolutionEvidenceSource::ReaderAuthoritative,
        &ReadSetSignature::empty(),
    );

    assert_eq!(
        workspace.engine.evidence_verified_generation_for_test(dep),
        None,
        "an empty witness observed no resolution facts at all, so it has \
         nothing to heal; the aggregate fallback must not fire for it"
    );
    assert!(workspace.engine.pending_resolution_refresh_for_test(dep));
}

/// A live source whose only job is to inhabit
/// [`ResolutionEvidenceSource::Uncovered`]. It observes nothing, because
/// these tests are about which witnesses are ELIGIBLE to be healed, not
/// about what the healing reads.
struct StubUncoveredSource;

impl crate::resolution_currency::LiveResolutionEvidence for StubUncoveredSource {
    fn observe_live_resolution_evidence(
        &self,
        _canonical_id: &str,
        _recorded: Option<&crate::resolution_currency::RecordedResolutionBaseline>,
    ) -> Option<crate::resolution_currency::LiveResolutionObservation> {
        None
    }
}

#[test]
fn an_uncovered_backend_refuses_a_compacted_witness_it_cannot_reobserve() {
    let source = StubUncoveredSource;
    let uncovered = ResolutionEvidenceSource::Uncovered(&source);
    let workspace = memory_workspace_with("/p/dep.ts");
    let dag_witness = dag_rooted_unenumerable_resolution_witness(&workspace);

    assert!(
        Engine::witness_evidence_is_unenumerable(uncovered, &compacted_resolution_witness()),
        "an `Uncovered` backend's healing rule is stated over the witness's \
         OWN path observations. A compacted `Resolution` bucket enumerates \
         none of them and there is no ledger of every path canonical ever \
         observed to fall back to, so the witness cannot be certified and \
         the candidate must not be reused"
    );
    assert!(
        Engine::witness_evidence_is_unenumerable(uncovered, &dag_witness),
        "an `Uncovered` backend must refuse every witness whose resolution evidence cannot be \
         enumerated, including a non-aggregate derived witness"
    );
    assert!(
        !Engine::witness_evidence_is_unenumerable(uncovered, &precise_witness("/p/dep.ts")),
        "a precise witness enumerates exactly what it depends on, so it stays \
         reusable under the same backend"
    );
    assert!(
        !Engine::witness_evidence_is_unenumerable(uncovered, &ReadSetSignature::empty()),
        "an empty witness observed no resolution facts, so there is nothing \
         it fails to enumerate"
    );
}

#[test]
fn a_reader_authoritative_backend_still_reuses_a_compacted_witness() {
    // The refusal is scoped to the backend whose healing rule NEEDS the
    // witness's path canonicals. A reader-authoritative backend heals off
    // the pending ledger, which the aggregate fallback covers in full, so
    // widening the refusal to it would decline a reuse for no reason.
    assert!(
        !Engine::witness_evidence_is_unenumerable(
            ResolutionEvidenceSource::ReaderAuthoritative,
            &compacted_resolution_witness()
        ),
        "a reader-authoritative backend's healing is fully covered by the \
         pending-ledger fallback, so a compacted witness stays reusable"
    );
    assert!(
        !Engine::witness_evidence_is_unenumerable(
            ResolutionEvidenceSource::Inert,
            &compacted_resolution_witness()
        ),
        "an inert backend re-observes nothing at all and certifies nothing, \
         so it has no enumeration requirement to fail"
    );
}

#[test]
fn published_root_replacement_advances_strict_authority_when_snapshot_arc_is_reused() {
    let engine = Engine::new();
    let first_root = engine.load_published().expect("bootstrap root");
    let shared_snapshot = Arc::clone(&first_root.snapshot);
    let before = engine.current_strict_self_root_generation();

    engine.publish_snapshot(PublishedRoot::with_ext(
        Arc::clone(&shared_snapshot),
        Box::new(()),
    ));

    let second_root = engine.load_published().expect("replacement root");
    assert!(
        Arc::ptr_eq(&shared_snapshot, &second_root.snapshot),
        "fixture must reuse the exact WorkspaceSnapshot Arc",
    );
    assert!(
        engine.current_strict_self_root_generation() > before,
        "PublishedRoot-level replacement must move strict authority even when its snapshot is reused",
    );
}

#[test]
fn snapshot_publication_reaches_all_simultaneously_live_subscribers() {
    let engine = Engine::new();
    let current = engine.load_published().expect("bootstrap root");
    let expected_generation = current.snapshot.generation.0;
    let first = engine.subscribe_published();
    let second = engine.subscribe_published();

    engine.publish_snapshot(PublishedRoot::with_ext(
        Arc::clone(&current.snapshot),
        Box::new(()),
    ));

    let watchdog = std::time::Duration::from_millis(250);
    assert_eq!(
        first
            .recv_timeout(watchdog)
            .expect("the first live subscriber must receive the publication"),
        expected_generation,
    );
    assert_eq!(
        second
            .recv_timeout(watchdog)
            .expect("the second live subscriber must receive the same publication"),
        expected_generation,
    );
    assert!(
        first.try_recv().is_err() && second.try_recv().is_err(),
        "one publication must emit exactly one receipt to each subscriber",
    );
}

#[test]
fn resolution_only_world_publication_preserves_strict_self_root_authority() {
    let engine = Engine::new();
    let before = engine.current_strict_self_root_generation();

    engine.mutate_resolution_world(|_| ((), true));

    assert_eq!(
        engine.current_strict_self_root_generation(),
        before,
        "resolution evidence publication cannot change content presence or trackedness",
    );
}
