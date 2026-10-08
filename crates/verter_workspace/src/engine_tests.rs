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

use crate::memory::{MemoryOptions, MemoryWorkspace};
use crate::resolution_currency::ResolutionEvidenceSource;
use verter_session_query::facts::fact_cache::{
    AggregatePopulation, AggregateStamp, CompactionDomain, DomainGenerationFact, FactVersionRef,
    ReadSetSignature, ResolveImportsFactRef,
};
use verter_session_query::resolution::{
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
    let verter_session_query::facts::fact_cache::SignatureAdmission::Cacheable(signature) =
        outcome.admission
    else {
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
        None,
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
        None,
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
        None,
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
        None,
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

/// The parse-env flag's version suffix tracks the Svelte rune-prelude
/// version, so a prelude-surface change invalidates a rune module's stale
/// inferred exports through `parse_env_hash`. The version constant lives in
/// `verter_language`, the flag here; this pins them in lockstep.
#[test]
fn rune_ambient_parser_flag_tracks_the_prelude_version() {
    let expected = format!(
        "svelte-rune-ambient-v{}",
        verter_language::svelte_rune_ambient::RUNE_AMBIENT_PRELUDE_VERSION
    );
    assert_eq!(
        super::SVELTE_RUNE_AMBIENT_PARSER_FLAG,
        expected,
        "the rune-ambient parse-env flag must encode the current RUNE_AMBIENT_PRELUDE_VERSION; \
         bump the flag suffix when you bump the version"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Workspace-lane ownership: slots, decisions, edges and their history are
// owned by their importer, bounded across distinct owners, and charged to
// the retention account.
// ─────────────────────────────────────────────────────────────────────────

const OWNERSHIP_CONTEXT: ResolutionContext = ResolutionContext {
    phase: ResolvePhase::ProviderGraph,
    kind: ResolveRequestKind::EsmImport,
};

const OWNERSHIP_DEP: &str = "/p/dep.ts";

fn ownership_workspace() -> MemoryWorkspace {
    memory_workspace_with(OWNERSHIP_DEP)
}

fn residency(workspace: &MemoryWorkspace) -> crate::traits::ResolutionResidency {
    WorkspaceRead::resource_snapshot(workspace).resolution
}

/// Inject `importer` and resolve `../dep`-style `specifier` from it; the
/// answer must be the complete, admitted `/p/dep.ts`.
fn resolve_owned(
    workspace: &MemoryWorkspace,
    importer: &str,
    specifier: &str,
) -> crate::resolution_currency::ResolutionOutcome {
    let outcome =
        WorkspaceRead::resolve_import_outcome(workspace, importer, specifier, OWNERSHIP_CONTEXT);
    assert_eq!(
        outcome.result().map(|result| result.source_id.as_str()),
        Some(OWNERSHIP_DEP),
        "fixture invariant: {importer} resolves {specifier} to the dependency"
    );
    outcome
}

fn admitted_signature(outcome: crate::resolution_currency::ResolutionOutcome) -> ReadSetSignature {
    match outcome.admission {
        verter_session_query::facts::fact_cache::SignatureAdmission::Cacheable(signature) => {
            signature
        }
        other => panic!("fixture invariant: the resolution must admit, got {other:?}"),
    }
}

fn assert_owned_state_at_baseline(
    after: crate::traits::ResolutionResidency,
    baseline: crate::traits::ResolutionResidency,
) {
    assert_eq!(after.slots, baseline.slots, "slots: {after:?}");
    assert_eq!(
        after.candidates, baseline.candidates,
        "candidates: {after:?}"
    );
    assert_eq!(after.owners, baseline.owners, "owners: {after:?}");
    assert_eq!(
        after.derived_nodes, baseline.derived_nodes,
        "decisions and owner sets: {after:?}"
    );
    assert_eq!(
        after.decision_edges, baseline.decision_edges,
        "decision edges: {after:?}"
    );
    assert_eq!(
        after.dependency_buckets, baseline.dependency_buckets,
        "dependency buckets: {after:?}"
    );
    assert!(
        after.retired_decisions <= crate::resolution_currency::TOMBSTONE_RETIREMENT_MINIMUM,
        "retirement history stays bounded: {after:?}"
    );
}

/// Deleting an importer retires its slots, its decisions, its owner set and
/// their edges in the same mutation, and the decision tombstones that
/// leaves behind fold into the floor, so distinct-importer churn returns
/// every count to its baseline however many importers came and went.
///
/// Mutation recipe: drop the `retire_importer_in_world` call from
/// `mutate_content_for`. The slot, owner and decision counts then grow by
/// one per importer and every baseline assertion fails.
#[test]
fn distinct_importer_churn_retires_slots_decisions_edges_and_history() {
    let workspace = ownership_workspace();
    let population = workspace.engine.default_resolution_population();
    let baseline = residency(&workspace);
    let churn = 2 * crate::resolution_currency::TOMBSTONE_RETIREMENT_MINIMUM + 8;
    for index in 0..churn {
        let importer = format!("/p/churn/importer-{index}.ts");
        workspace.inject_file(importer.clone(), Arc::from("import '../dep'\n"));
        let _ = admitted_signature(resolve_owned(&workspace, &importer, "../dep"));
        assert!(
            workspace
                .engine
                .publish_owner_resolution_set(&importer, population)
                .is_some(),
            "fixture invariant: the importer publishes its owner set"
        );
        if index == 0 {
            let live = residency(&workspace);
            assert_eq!(live.slots, baseline.slots + 1, "{live:?}");
            assert_eq!(live.owners, baseline.owners + 1, "{live:?}");
            assert_eq!(
                live.derived_nodes,
                baseline.derived_nodes + 2,
                "one decision and one owner set: {live:?}"
            );
            assert!(live.decision_edges > baseline.decision_edges, "{live:?}");
        }
        workspace.remove_file(&importer);
    }
    assert_owned_state_at_baseline(residency(&workspace), baseline);
}

/// Importers that never retire — paths the workspace does not hold, never
/// deleted — are bounded by the lane's slot cap, and every evicted slot
/// takes its decision and edges with it.
///
/// Mutation recipe: make `enforce_slot_cap` return without evicting. The
/// slot and decision counts then reach one per importer.
#[test]
fn unknown_owner_resolutions_stay_within_the_slot_cap() {
    const CAP: usize = 16;
    let workspace = ownership_workspace();
    workspace
        .engine
        .lazy_resolution_cache
        .write()
        .set_slot_cap_for_test(CAP);
    let baseline = residency(&workspace);
    let _ = admitted_signature(resolve_owned(
        &workspace,
        "/ghost/importer-0.ts",
        "../p/dep",
    ));
    let one = residency(&workspace);
    assert_eq!(one.slots, baseline.slots + 1, "fixture invariant: {one:?}");
    let edges_per_decision = one.decision_edges - baseline.decision_edges;
    assert!(edges_per_decision > 0, "fixture invariant: {one:?}");

    for index in 1..(CAP * 8) {
        let importer = format!("/ghost/importer-{index}.ts");
        let _ = admitted_signature(resolve_owned(&workspace, &importer, "../p/dep"));
    }
    let after = residency(&workspace);
    assert_eq!(after.slots, CAP, "{after:?}");
    assert_eq!(after.owners, CAP, "{after:?}");
    assert_eq!(after.candidates, CAP, "{after:?}");
    assert_eq!(
        after.derived_nodes,
        baseline.derived_nodes + CAP,
        "one decision per retained slot: {after:?}"
    );
    assert!(
        after.decision_edges <= baseline.decision_edges + CAP * edges_per_decision,
        "{after:?}"
    );
    assert!(
        after.retired_decisions <= crate::resolution_currency::TOMBSTONE_RETIREMENT_MINIMUM,
        "{after:?}"
    );
    assert!(
        after.slot_queue_entries <= 2 * CAP + 64,
        "the admission queue stays proportional to the lane: {after:?}"
    );
}

/// Every evicted unknown-owner slot takes the dependency edge its answer
/// recorded: the dependency store and the dependency's reverse set hold
/// only the importers the lane still retains, however many came and went.
///
/// Mutation recipe: drop the `retract_lazy_dependencies` call from
/// `retire_resolution_decisions`. Every distinct importer then stays an
/// edge-store owner and a reverse dependent of the dependency.
#[test]
fn unknown_owner_dependency_edges_leave_with_their_evicted_slots() {
    const CAP: usize = 16;
    const IMPORTERS: usize = CAP * 8;
    let workspace = ownership_workspace();
    workspace
        .engine
        .lazy_resolution_cache
        .write()
        .set_slot_cap_for_test(CAP);
    let baseline_files = WorkspaceRead::resource_snapshot(&workspace).edge_file_count;
    let importer = |index: usize| format!("/ghost/importer-{index}.ts");
    for index in 0..IMPORTERS {
        let _ = admitted_signature(resolve_owned(&workspace, &importer(index), "../p/dep"));
    }

    let snapshot = WorkspaceRead::resource_snapshot(&workspace);
    assert_eq!(
        snapshot.edge_file_count,
        baseline_files + CAP,
        "one dependency owner per retained slot: {snapshot:?}"
    );
    let dependents = WorkspaceRead::reverse_deps_for(&workspace, OWNERSHIP_DEP);
    let mut expected: Vec<String> = (IMPORTERS - CAP..IMPORTERS).map(importer).collect();
    expected.sort();
    let mut actual = dependents.clone();
    actual.sort();
    assert_eq!(
        actual, expected,
        "the dependency's reverse set is exactly the resident importers"
    );
}

/// A lane drained after a large fill reports the backing storage it still
/// holds, distinct from its occupancy, and gives the excess back.
///
/// Mutation recipe: delete the two `shrink_to` branches of
/// `compact_order`. The drained slot capacity then stays at the fill's
/// high-water mark.
#[test]
fn a_drained_lane_reports_and_returns_its_backing_storage() {
    const FILL: usize = 1024;
    let workspace = ownership_workspace();
    let baseline = residency(&workspace);
    let importers: Vec<String> = (0..FILL)
        .map(|index| format!("/p/fill/importer-{index}.ts"))
        .collect();
    for importer in &importers {
        workspace.inject_file(importer.clone(), Arc::from("import '../dep'\n"));
        let _ = admitted_signature(resolve_owned(&workspace, importer, "../dep"));
    }
    let full = residency(&workspace);
    assert_eq!(full.slots, baseline.slots + FILL, "{full:?}");
    assert!(full.slot_capacity >= full.slots, "{full:?}");
    assert!(full.slot_queue_entries >= FILL, "{full:?}");
    assert!(
        full.slot_queue_capacity >= full.slot_queue_entries,
        "{full:?}"
    );

    for importer in &importers {
        workspace.remove_file(importer);
    }
    let drained = residency(&workspace);
    assert_owned_state_at_baseline(drained, baseline);
    assert!(drained.slot_queue_entries <= 64, "{drained:?}");
    assert!(
        drained.slot_capacity <= 256 && drained.slot_capacity < full.slot_capacity,
        "the drained slot table gave back its excess storage: {drained:?}"
    );
    assert!(
        drained.slot_queue_capacity <= 4 * drained.slot_queue_entries + 256,
        "the drained admission queue gave back its excess storage: {drained:?}"
    );
}

/// A subtree deletion retires every importer under it and no importer
/// that merely shares its name prefix.
#[test]
fn subtree_deletion_retires_exactly_the_importers_under_it() {
    let workspace = ownership_workspace();
    let population = workspace.engine.default_resolution_population();
    for importer in ["/p/sub/a.ts", "/p/sub/deep/b.ts", "/p/subway/c.ts"] {
        workspace.inject_file(importer.to_string(), Arc::from("export {}\n"));
    }
    let _ = resolve_owned(&workspace, "/p/sub/a.ts", "../dep");
    let _ = resolve_owned(&workspace, "/p/sub/deep/b.ts", "../../dep");
    let _ = resolve_owned(&workspace, "/p/subway/c.ts", "../dep");

    WorkspaceAccess::delete_dir_all(&workspace, "/p/sub").expect("subtree deletion");

    let slot = |importer: &str, specifier: &str| {
        workspace.engine.lazy_resolution_slot_len_for_test(
            importer,
            specifier,
            OWNERSHIP_CONTEXT,
            population,
        )
    };
    assert_eq!(slot("/p/sub/a.ts", "../dep"), 0);
    assert_eq!(slot("/p/sub/deep/b.ts", "../../dep"), 0);
    assert_eq!(
        slot("/p/subway/c.ts", "../dep"),
        1,
        "a sibling sharing the name prefix keeps its slot"
    );
    assert_eq!(residency(&workspace).owners, 1);
}

/// A base deletion leaves the session's slot for an importer an open
/// overlay still shows; closing that overlay retires it.
#[test]
fn an_open_overlay_keeps_its_importer_until_it_closes() {
    let workspace = ownership_workspace();
    let population = workspace.engine.default_resolution_population();
    let baseline = residency(&workspace);
    let importer = "/p/open.ts";
    workspace.inject_file(importer.to_string(), Arc::from("export {}\n"));
    WorkspaceAccess::notify_upsert(&workspace, importer, Arc::from("import './dep'\n"));
    let _ = admitted_signature(resolve_owned(&workspace, importer, "./dep"));

    workspace.remove_file(importer);
    assert_eq!(
        workspace.engine.lazy_resolution_slot_len_for_test(
            importer,
            "./dep",
            OWNERSHIP_CONTEXT,
            population,
        ),
        1,
        "the session still sees the importer through its overlay"
    );

    WorkspaceAccess::notify_close(&workspace, importer);
    assert_owned_state_at_baseline(residency(&workspace), baseline);
}

/// A snapshot captured before an importer retires keeps validating the
/// witnesses it validated; a fresh capture does not — not even after the
/// retired decision's tombstone has folded into the floor, where reading
/// `INITIAL` again would re-validate the pre-removal witness.
///
/// Mutation recipe: in `retire_tombstones_if_due`, keep `derived_floor`
/// unchanged instead of raising it to `fresh()`. The final fresh-capture
/// assertion fails.
#[test]
fn held_snapshots_stay_valid_across_retirement_and_its_floor() {
    let workspace = ownership_workspace();
    let importer = "/p/held.ts";
    workspace.inject_file(importer.to_string(), Arc::from("import './dep'\n"));
    let witness = admitted_signature(resolve_owned(&workspace, importer, "./dep"));
    let held = WorkspaceRead::capture_resolution_world(&workspace).expect("captured world");
    assert!(witness.validates(held.as_ref()));

    workspace.remove_file(importer);
    let fresh = WorkspaceRead::capture_resolution_world(&workspace).expect("captured world");
    assert!(
        witness.validates(held.as_ref()),
        "the held snapshot is immutable"
    );
    assert!(
        !witness.validates(fresh.as_ref()),
        "the retired importer's decision no longer validates"
    );

    for index in 0..(2 * crate::resolution_currency::TOMBSTONE_RETIREMENT_MINIMUM + 8) {
        let churned = format!("/p/churn/importer-{index}.ts");
        workspace.inject_file(churned.clone(), Arc::from("import '../dep'\n"));
        let _ = resolve_owned(&workspace, &churned, "../dep");
        workspace.remove_file(&churned);
    }
    assert!(
        residency(&workspace).retired_decisions
            <= crate::resolution_currency::TOMBSTONE_RETIREMENT_MINIMUM,
        "fixture invariant: the churn folded the history into the floor"
    );
    let folded = WorkspaceRead::capture_resolution_world(&workspace).expect("captured world");
    assert!(witness.validates(held.as_ref()));
    assert!(
        !witness.validates(folded.as_ref()),
        "a folded tombstone must not let the pre-removal witness validate again"
    );
}

struct OwnershipAccount {
    admit: bool,
    charged: Arc<std::sync::atomic::AtomicUsize>,
}

struct OwnershipCharge {
    bytes: usize,
    charged: Arc<std::sync::atomic::AtomicUsize>,
}

impl Drop for OwnershipCharge {
    fn drop(&mut self) {
        self.charged
            .fetch_sub(self.bytes, std::sync::atomic::Ordering::SeqCst);
    }
}

impl verter_session_query::retention::resolution_charge::ResolutionRetentionAccount
    for OwnershipAccount
{
    fn reserve_retained(
        &self,
        bytes: usize,
    ) -> Option<verter_session_query::retention::resolution_charge::ResolutionRetentionCharge> {
        if !self.admit {
            return None;
        }
        self.charged
            .fetch_add(bytes, std::sync::atomic::Ordering::SeqCst);
        Some(
            verter_session_query::retention::resolution_charge::ResolutionRetentionCharge::new(
                OwnershipCharge {
                    bytes,
                    charged: Arc::clone(&self.charged),
                },
            ),
        )
    }
}

/// A complete answer the account refuses to retain is served uncached —
/// typed as retention pressure, with no slot and no decision — and every
/// retained answer's charge drains when its importer retires.
///
/// Mutation recipe: ignore the reservation result and admit the candidate
/// anyway. The refused resolution then occupies a slot and a decision.
#[test]
fn refused_retention_serves_complete_answers_uncached_and_owners_drain() {
    let workspace = ownership_workspace();
    let baseline = residency(&workspace);
    WorkspaceAccess::install_resolution_retention(
        &workspace,
        Arc::new(OwnershipAccount {
            admit: false,
            charged: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }),
    );
    let importer = "/p/refused.ts";
    workspace.inject_file(importer.to_string(), Arc::from("import './dep'\n"));
    for _ in 0..2 {
        let outcome = resolve_owned(&workspace, importer, "./dep");
        assert_eq!(
            outcome.non_admission_reason(),
            Some(verter_audit::NonAdmissionReason::RetentionPressure),
            "a refused retention is retention pressure, not a budget or work refusal"
        );
        let after = residency(&workspace);
        assert_eq!(after.slots, baseline.slots, "{after:?}");
        assert_eq!(after.derived_nodes, baseline.derived_nodes, "{after:?}");
    }

    let charged = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    WorkspaceAccess::install_resolution_retention(
        &workspace,
        Arc::new(OwnershipAccount {
            admit: true,
            charged: Arc::clone(&charged),
        }),
    );
    let importers: Vec<String> = (0..8).map(|index| format!("/p/kept-{index}.ts")).collect();
    for importer in &importers {
        workspace.inject_file(importer.clone(), Arc::from("import './dep'\n"));
        let _ = admitted_signature(resolve_owned(&workspace, importer, "./dep"));
    }
    assert!(
        charged.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "retained answers are charged"
    );
    for importer in &importers {
        workspace.remove_file(importer);
    }
    assert_eq!(
        charged.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "every charge drains with its importer"
    );
    workspace.remove_file(importer);
    assert_owned_state_at_baseline(residency(&workspace), baseline);
}

/// A workspace importer's dependency edge is import-graph state, not a lane
/// resident: it survives its answer's eviction by the slot cap and a
/// retention refusal of the answer, and leaves with the importer itself.
///
/// Mutation recipe: record the edge only for a slot-backed answer (drop the
/// `importer_held_by_workspace` arm), or retract it for every importer. The
/// evicted or refused importer then vanishes from its dependency's reverse
/// set while it still imports it.
#[test]
fn a_workspace_importer_keeps_its_dependency_edge_whatever_the_lane_retains() {
    let workspace = ownership_workspace();
    workspace
        .engine
        .lazy_resolution_cache
        .write()
        .set_slot_cap_for_test(1);
    let evicted = "/p/edge-evicted.ts";
    let evictor = "/p/edge-evictor.ts";
    let refused = "/p/edge-refused.ts";
    for importer in [evicted, evictor, refused] {
        workspace.inject_file(importer.to_string(), Arc::from("import './dep'\n"));
    }
    let _ = admitted_signature(resolve_owned(&workspace, evicted, "./dep"));
    let _ = admitted_signature(resolve_owned(&workspace, evictor, "./dep"));
    assert_eq!(
        residency(&workspace).slots,
        1,
        "fixture invariant: one slot"
    );
    WorkspaceAccess::install_resolution_retention(
        &workspace,
        Arc::new(OwnershipAccount {
            admit: false,
            charged: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }),
    );
    assert_eq!(
        resolve_owned(&workspace, refused, "./dep").non_admission_reason(),
        Some(verter_audit::NonAdmissionReason::RetentionPressure),
        "fixture invariant: the account refuses the answer"
    );

    let dependents = WorkspaceRead::reverse_deps_for(&workspace, OWNERSHIP_DEP);
    for importer in [evicted, evictor, refused] {
        assert!(
            dependents.iter().any(|dependent| dependent == importer),
            "{importer} still imports the dependency: {dependents:?}"
        );
    }
    for importer in [evicted, evictor, refused] {
        workspace.remove_file(importer);
    }
    let dependents = WorkspaceRead::reverse_deps_for(&workspace, OWNERSHIP_DEP);
    assert!(
        dependents.is_empty(),
        "every edge leaves with its importer: {dependents:?}"
    );
}

/// A snapshot that outlives an importer's retirement still holds the
/// retired decision, so its bytes stay charged until that snapshot drops,
/// and are then released exactly once.
///
/// Mutation recipe: in `publish_charged_decision`, drop the
/// `decision_charges` insert. The charge then drains when the slot retires,
/// while the held snapshot still holds the decision.
#[test]
fn a_held_snapshot_keeps_its_retired_decision_charged_until_it_drops() {
    let workspace = ownership_workspace();
    let baseline = residency(&workspace);
    let charged = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    WorkspaceAccess::install_resolution_retention(
        &workspace,
        Arc::new(OwnershipAccount {
            admit: true,
            charged: Arc::clone(&charged),
        }),
    );
    let importer = "/p/held-charge.ts";
    workspace.inject_file(importer.to_string(), Arc::from("import './dep'\n"));
    let witness = admitted_signature(resolve_owned(&workspace, importer, "./dep"));
    let resolved = charged.load(std::sync::atomic::Ordering::SeqCst);
    assert!(resolved > 0, "fixture invariant: the answer is charged");

    let held = WorkspaceRead::capture_resolution_world(&workspace).expect("captured world");
    workspace.remove_file(importer);
    assert_owned_state_at_baseline(residency(&workspace), baseline);
    assert!(
        witness.validates(held.as_ref()),
        "fixture invariant: the held snapshot still holds the decision"
    );
    assert_eq!(
        charged.load(std::sync::atomic::Ordering::SeqCst),
        resolved,
        "the held snapshot keeps the retired decision's bytes charged"
    );

    drop(held);
    assert_eq!(
        charged.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "the charge drains once, with its last owner"
    );
}

/// A retention account with the production account's size rule: an entry
/// above its per-entry target is refused however much headroom remains.
struct SizedAccount(Arc<verter_session_query::retention::SemanticRetentionAccount>);

impl verter_session_query::retention::resolution_charge::ResolutionRetentionAccount
    for SizedAccount
{
    fn reserve_retained(
        &self,
        bytes: usize,
    ) -> Option<verter_session_query::retention::resolution_charge::ResolutionRetentionCharge> {
        self.0
            .reserve(
                verter_session_query::retention::ChargeClass::Retained,
                bytes,
            )
            .admitted()
            .map(verter_session_query::retention::resolution_charge::ResolutionRetentionCharge::new)
    }
}

/// An answer whose retained size exceeds the account's per-entry target is
/// still served complete — never truncated or qualified by its size — and
/// uncached: retention pressure, no slot, no decision, no charge. A small
/// answer under the same account is retained, and every owner drains.
///
/// Mutation recipe: admit the candidate when the reservation is refused.
/// The oversized importer then occupies a slot and a decision.
#[test]
fn an_answer_above_the_entry_target_is_served_complete_and_uncached() {
    const ENTRY_TARGET: usize = 16 * 1024;
    let account = verter_session_query::retention::SemanticRetentionAccount::new(
        verter_session_query::retention::RetentionLimits {
            max_entry_bytes: ENTRY_TARGET,
            ..verter_session_query::retention::RetentionLimits::defaults()
        },
    );
    let workspace = ownership_workspace();
    WorkspaceAccess::install_resolution_retention(
        &workspace,
        Arc::new(SizedAccount(Arc::clone(&account))),
    );
    let baseline = residency(&workspace);

    let small = "/p/small.ts";
    workspace.inject_file(small.to_string(), Arc::from("import './dep'\n"));
    let _ = admitted_signature(resolve_owned(&workspace, small, "./dep"));
    let small_bytes = account.snapshot().retained_bytes;
    assert!(
        small_bytes > 0 && small_bytes < ENTRY_TARGET,
        "fixture invariant: the small answer fits under the target ({small_bytes})"
    );
    let retained_small = residency(&workspace);

    let large = format!("/p/{}.ts", "x".repeat(4 * ENTRY_TARGET));
    workspace.inject_file(large.clone(), Arc::from("import './dep'\n"));
    for attempt in 1..=2 {
        // `resolve_owned` asserts the complete answer.
        let outcome = resolve_owned(&workspace, &large, "./dep");
        assert_eq!(
            outcome.non_admission_reason(),
            Some(verter_audit::NonAdmissionReason::RetentionPressure),
            "an oversized complete answer is retention pressure, not a budget refusal"
        );
        let snapshot = account.snapshot();
        assert_eq!(
            snapshot.refusals_oversized, attempt,
            "the size rule refused it, and the next demand computes it again"
        );
        assert_eq!(snapshot.retained_bytes, small_bytes, "{snapshot:?}");
        let after = residency(&workspace);
        assert_eq!(after.slots, retained_small.slots, "{after:?}");
        assert_eq!(
            after.derived_nodes, retained_small.derived_nodes,
            "{after:?}"
        );
    }

    workspace.remove_file(small);
    workspace.remove_file(&large);
    assert_eq!(account.snapshot().retained_bytes, 0);
    assert_owned_state_at_baseline(residency(&workspace), baseline);
}

/// An admission whose slot eviction folds the retirement history into the
/// floor still returns a witness valid in the world it published into.
///
/// Mutation recipe: in the admission fence, retire the evicted decisions
/// before publishing the admitted one. The fold then moves the floor the
/// unstored new decision reads, and the admission that folded returns a
/// witness a fresh capture rejects.
#[test]
fn an_admission_that_folds_the_history_returns_a_current_witness() {
    let workspace = ownership_workspace();
    workspace
        .engine
        .lazy_resolution_cache
        .write()
        .set_slot_cap_for_test(1);
    let mut previous = residency(&workspace).retired_decisions;
    let mut folded = false;
    for index in 0..(2 * crate::resolution_currency::TOMBSTONE_RETIREMENT_MINIMUM + 8) {
        let importer = format!("/ghost/fold-{index}.ts");
        let witness = admitted_signature(resolve_owned(&workspace, &importer, "../p/dep"));
        let fresh = WorkspaceRead::capture_resolution_world(&workspace).expect("captured world");
        assert!(
            witness.validates(fresh.as_ref()),
            "admission {index} returned a witness its own world rejects"
        );
        let retired = residency(&workspace).retired_decisions;
        folded |= retired < previous;
        previous = retired;
    }
    assert!(folded, "fixture invariant: an admission folded the history");
}

/// A residency read never pairs a lane whose slots already retired with a
/// decision graph that still holds their decisions: a read that meets a
/// deletion paused between slot retirement and root publication waits for
/// the publication and reports the post-deletion generation whole.
///
/// The reader is observed contending for the publication gate while the
/// deletion is still paused inside its window, so the overlap is proven
/// rather than left to scheduling; the timeouts only fail a stalled run.
///
/// Mutation recipe: drop the publication-gate guard from
/// `resolution_residency`. The reader then never contends for the gate and
/// the rendezvous below fails; reading the lane before taking the gate
/// instead reports the slot gone beside its still-live decision.
#[test]
fn a_residency_read_never_mixes_retirement_generations() {
    use crate::engine::resolution_test_hooks::{self, ResolutionPhase};
    use std::sync::mpsc::channel;
    use std::time::Duration;

    let workspace = ownership_workspace();
    let baseline = residency(&workspace);
    let importer = "/p/barrier.ts";
    workspace.inject_file(importer.to_string(), Arc::from("import './dep'\n"));
    let _ = admitted_signature(resolve_owned(&workspace, importer, "./dep"));
    let live = residency(&workspace);
    assert!(
        live.slots > baseline.slots && live.derived_nodes > baseline.derived_nodes,
        "fixture invariant: {live:?}"
    );

    let workspace = &workspace;
    let read = std::thread::scope(|scope| {
        let (paused_tx, paused_rx) = channel::<()>();
        let (contended_tx, contended_rx) = channel::<()>();
        // Owned by this closure, so a failed watchdog below drops it while
        // unwinding — before the scope joins the deleter — and the paused
        // deletion runs on instead of deadlocking the join.
        let (resume_tx, resume_rx) = channel::<()>();
        let deleter = scope.spawn(move || {
            resolution_test_hooks::with_hook(
                ResolutionPhase::ImporterSlotsRetired,
                move || {
                    let _ = paused_tx.send(());
                    let _ = resume_rx.recv();
                },
                || workspace.remove_file(importer),
            );
        });
        paused_rx
            .recv_timeout(Duration::from_secs(20))
            .expect("the deletion reaches its slot retirement");
        let reader = scope.spawn(move || {
            resolution_test_hooks::with_hook(
                ResolutionPhase::ResidencyGateContended,
                move || {
                    let _ = contended_tx.send(());
                },
                || residency(workspace),
            )
        });
        let contended = contended_rx.recv_timeout(Duration::from_secs(20));
        drop(resume_tx);
        contended.expect("the residency read waits on the paused deletion's publication gate");
        deleter.join().expect("deleter");
        reader.join().expect("reader")
    });
    assert!(
        read.slots == baseline.slots && read.derived_nodes == baseline.derived_nodes,
        "the read mixed generations: {read:?} (live {live:?}, baseline {baseline:?})"
    );
}

/// A warm reuse whose slot another admission evicted between the reuse and
/// its final fence has no live decision to root on: it returns the
/// candidate's own witness, which a later change to its target invalidates.
/// Its importer is a workspace file, so the import graph still records the
/// dependency the answer names, retained or not.
///
/// Mutation recipe: drop the `serves` re-check under the final fence. The
/// reuse then roots on the evicted decision's tombstone, which has no
/// edges, and still validates after the target is deleted.
#[test]
fn a_reuse_evicted_before_its_fence_returns_its_own_witness() {
    use crate::engine::resolution_test_hooks::{self, ResolutionPhase};

    let workspace = Arc::new(ownership_workspace());
    workspace
        .engine
        .lazy_resolution_cache
        .write()
        .set_slot_cap_for_test(1);
    let first = "/p/first.ts";
    let second = "/p/second.ts";
    for importer in [first, second] {
        workspace.inject_file(importer.to_string(), Arc::from("import './dep'\n"));
    }
    let _ = admitted_signature(resolve_owned(&workspace, first, "./dep"));

    let evictor = Arc::clone(&workspace);
    let outcome = resolution_test_hooks::with_hook(
        ResolutionPhase::PreAdmissionValidation,
        move || {
            let _ = admitted_signature(resolve_owned(&evictor, second, "./dep"));
        },
        || resolve_owned(&workspace, first, "./dep"),
    );
    assert!(
        outcome.trace().reused(),
        "fixture invariant: the first importer's demand reused its candidate"
    );
    let witness = admitted_signature(outcome);
    assert!(
        WorkspaceRead::reverse_deps_for(workspace.as_ref(), OWNERSHIP_DEP)
            .iter()
            .any(|dependent| dependent == first),
        "a workspace importer keeps the dependency edge its evicted answer names"
    );

    workspace.remove_file(OWNERSHIP_DEP);
    let fresh =
        WorkspaceRead::capture_resolution_world(workspace.as_ref()).expect("captured world");
    assert!(
        !witness.validates(fresh.as_ref()),
        "the reuse's witness must see its target's deletion"
    );
}

/// A warm reuse whose decision a concurrent write advanced after the reuse
/// validated its candidate is never stamped with the advanced version: the
/// write here creates a higher-priority target the candidate's negative
/// probe ruled out, and the demand retries against the new world instead of
/// serving the invalidated answer under a witness that world accepts.
///
/// Mutation recipe: drop the decision-version comparison from the final
/// fence's filter. The warm demand then returns `/p/dep.tsx`, admitted with
/// a witness the post-write world validates.
#[test]
fn a_reuse_whose_decision_advanced_before_its_fence_is_not_restamped() {
    use crate::engine::resolution_test_hooks::{self, ResolutionPhase};

    let shadowed = "/p/dep.tsx";
    let workspace = Arc::new(memory_workspace_with(shadowed));
    let importer = "/p/main.ts";
    workspace.inject_file(importer.to_string(), Arc::from("import './dep'\n"));
    let resolve = |workspace: &MemoryWorkspace| {
        WorkspaceRead::resolve_import_outcome(workspace, importer, "./dep", OWNERSHIP_CONTEXT)
    };
    let cold = resolve(&workspace);
    assert_eq!(
        cold.result().map(|result| result.source_id.as_str()),
        Some(shadowed),
        "fixture invariant: only the lower-priority target exists"
    );
    let _ = admitted_signature(cold);

    let writer = Arc::clone(&workspace);
    let warm = resolution_test_hooks::with_hook(
        ResolutionPhase::PreAdmissionValidation,
        move || writer.inject_file(OWNERSHIP_DEP.to_string(), Arc::from("export const v = 2\n")),
        || resolve(&workspace),
    );
    assert_eq!(
        warm.result().map(|result| result.source_id.as_str()),
        Some(OWNERSHIP_DEP),
        "the warm demand must not serve the answer the write invalidated"
    );
    let witness = admitted_signature(warm);
    let fresh =
        WorkspaceRead::capture_resolution_world(workspace.as_ref()).expect("captured world");
    assert!(
        witness.validates(fresh.as_ref()),
        "the retried answer is current"
    );
}

/// A demand naming an importer the workspace has already retired — an
/// in-flight request for a just-deleted file — is answered complete, with
/// its own precise witness, but leaves no slot, decision or dependency edge
/// behind: nothing would retire them, since the importer's retirement
/// already ran.
///
/// Mutation recipe: make `importer_known_absent` return `false`. The
/// deleted importer then owns a slot and a decision again.
#[test]
fn a_demand_from_a_retired_importer_is_served_but_not_retained() {
    let workspace = ownership_workspace();
    let baseline = residency(&workspace);
    let importer = "/p/gone.ts";
    workspace.inject_file(importer.to_string(), Arc::from("import './dep'\n"));
    let _ = admitted_signature(resolve_owned(&workspace, importer, "./dep"));
    workspace.remove_file(importer);
    assert_owned_state_at_baseline(residency(&workspace), baseline);

    // `resolve_owned` asserts the complete answer.
    let witness = admitted_signature(resolve_owned(&workspace, importer, "./dep"));
    assert_owned_state_at_baseline(residency(&workspace), baseline);
    assert!(
        !WorkspaceRead::reverse_deps_for(&workspace, OWNERSHIP_DEP)
            .iter()
            .any(|dependent| dependent == importer),
        "a retired importer records no dependency edge"
    );
    let fresh = WorkspaceRead::capture_resolution_world(&workspace).expect("captured world");
    assert!(
        witness.validates(fresh.as_ref()),
        "the served answer's own witness is current"
    );
    workspace.remove_file(OWNERSHIP_DEP);
    let moved = WorkspaceRead::capture_resolution_world(&workspace).expect("captured world");
    assert!(
        !witness.validates(moved.as_ref()),
        "with no decision node behind it, the witness observes the target itself"
    );
}
