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
        workspace.engine.lazy_resolution_cache.read().queue_len() <= 2 * CAP + 64,
        "the admission queue stays proportional to the lane"
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
