//! Tests for the per-canonical reverse indices that back
//! [`SchedulerDag::supersede_old_file_generations`].
//!
//! Two test families:
//!
//! 1. **Cleanup equivalence** — a source bump must apply the four
//!    cleanups (signal `Superseded`, cancel stale nodes, drop stale
//!    artifact blockers, scrub stale terminal failures) with the
//!    SAME observable effect as a crate-global scan, and the entries
//!    the reverse index hands the sweep must EQUAL the entries a full
//!    crate-global scan would produce.
//!
//! 2. **Reverse-index-equals-full-scan invariant** — after every
//!    typed mutation funnel (submit / dedup-merge / complete / cancel /
//!    register / record / insert / remove / drain / scrub / shutdown),
//!    the set computed from the reverse index for a canonical EQUALS
//!    the set a full-scan oracle computes. The full-scan oracle is the
//!    discriminating gate: it FAILS if any funnel forgets to maintain
//!    the index.

use super::*;

use crate::job::completion_pair;

fn canonical(s: &str) -> Arc<str> {
    Arc::from(s)
}

fn file_stage(s: &str, gen: u64, stage: FileStageKey) -> WorkNodeIdentity {
    WorkNodeIdentity::FileStage {
        canonical: canonical(s),
        generation: gen,
        stage,
    }
}

fn artifact(s: &str, gen: u64, profile: u64) -> WorkNodeIdentity {
    WorkNodeIdentity::Artifact {
        canonical: canonical(s),
        generation: gen,
        profile_hash: profile_hash_to_bytes(profile),
        content_hash: [0u8; 16],
    }
}

fn analysis_dep(s: &str, gen: u64) -> DepKey {
    DepKey::FileStage {
        canonical: canonical(s),
        generation: gen,
        stage: FileStageKey::Analysis,
    }
}

fn failed_record(dep_key: DepKey) -> FailedDepRecord {
    FailedDepRecord {
        dep_key,
        cause: SchedulerError::FileNotFound {
            file_id: "/dep".to_string(),
        },
    }
}

// ── Full-scan oracles (the discriminating side) ──
//
// Each oracle walks the authoritative surface map directly, exactly
// as a crate-global scan would, and returns the set for `c`. The
// matching `*_index` reader returns what the reverse index believes.
// Equality of the two is the invariant every funnel must preserve.

fn scan_fw_gens(dag: &SchedulerDag, c: &str) -> BTreeSet<u64> {
    dag.file_waiters
        .keys()
        .filter(|k| k.canonical.as_ref() == c)
        .map(|k| k.generation)
        .collect()
}

fn index_fw_gens(dag: &SchedulerDag, c: &str) -> BTreeSet<u64> {
    dag.canonical_index
        .file_waiter_gens
        .get(c)
        .map(|s| s.iter().copied().collect())
        .unwrap_or_default()
}

fn scan_node_tokens(dag: &SchedulerDag, c: &str) -> BTreeSet<SubmissionToken> {
    dag.nodes
        .iter()
        .filter(|(_, n)| !n.cancelled)
        .filter(|(_, n)| match &n.identity {
            WorkNodeIdentity::FileStage { canonical, .. }
            | WorkNodeIdentity::Artifact { canonical, .. } => canonical.as_ref() == c,
            WorkNodeIdentity::CacheNode { .. } => false,
        })
        .map(|(tok, _)| *tok)
        .collect()
}

fn index_node_tokens(dag: &SchedulerDag, c: &str) -> BTreeSet<SubmissionToken> {
    dag.canonical_index
        .node_tokens
        .get(c)
        .map(|s| s.iter().copied().collect())
        .unwrap_or_default()
}

fn scan_blocker_gens(dag: &SchedulerDag, c: &str) -> BTreeSet<u64> {
    dag.artifact_blocker_deps
        .keys()
        .filter(|(owner, _)| owner.as_ref() == c)
        .map(|(_, gen)| *gen)
        .collect()
}

fn index_blocker_gens(dag: &SchedulerDag, c: &str) -> BTreeSet<u64> {
    dag.canonical_index
        .blocker_owner_gens
        .get(c)
        .map(|s| s.iter().copied().collect())
        .unwrap_or_default()
}

fn scan_terminal_keys(dag: &SchedulerDag, c: &str) -> BTreeSet<DepKey> {
    dag.terminal_dep_failures
        .keys()
        .filter(|k| match k {
            DepKey::FileStage { canonical, .. } | DepKey::Artifact { canonical, .. } => {
                canonical.as_ref() == c
            }
            DepKey::CacheNode { .. } => false,
        })
        .cloned()
        .collect()
}

fn index_terminal_keys(dag: &SchedulerDag, c: &str) -> BTreeSet<DepKey> {
    dag.canonical_index
        .terminal_failure_keys
        .get(c)
        .map(|s| s.iter().cloned().collect())
        .unwrap_or_default()
}

/// Assert every reverse index bucket for `c` equals its full-scan
/// oracle. This is the central discriminating check.
fn assert_index_matches_scan(dag: &SchedulerDag, c: &str) {
    assert_eq!(
        index_fw_gens(dag, c),
        scan_fw_gens(dag, c),
        "file-waiter index diverged from full scan for {c}",
    );
    assert_eq!(
        index_node_tokens(dag, c),
        scan_node_tokens(dag, c),
        "node-token index diverged from full scan for {c}",
    );
    assert_eq!(
        index_blocker_gens(dag, c),
        scan_blocker_gens(dag, c),
        "artifact-blocker index diverged from full scan for {c}",
    );
    assert_eq!(
        index_terminal_keys(dag, c),
        scan_terminal_keys(dag, c),
        "terminal-failure index diverged from full scan for {c}",
    );
}

fn submit_node(
    dag: &mut SchedulerDag,
    identity: WorkNodeIdentity,
    kind: WorkKind,
) -> SubmissionToken {
    dag.submit(identity, kind, Priority::Interactive, Vec::new(), None)
}

// ─────────────────────────────────────────────────────────────────
// 1. Cleanup equivalence regressions
// ─────────────────────────────────────────────────────────────────

/// Stale file waiters are signalled `Superseded` after a source bump,
/// the live-generation waiter is untouched, and the reverse index the
/// sweep drove off equals the full scan afterwards.
#[test]
fn supersede_signals_superseded_to_stale_file_waiters() {
    let mut dag = SchedulerDag::new();
    let c = canonical("/a.vue");

    let (stale_handle, stale_sender) = completion_pair::<RequestResult>();
    let _ = dag.register_request(&c, 1, TargetStage::Source, stale_sender, None);
    let (live_handle, live_sender) = completion_pair::<RequestResult>();
    let _ = dag.register_request(&c, 2, TargetStage::Source, live_sender, None);
    // Unrelated file must be untouched.
    let other = canonical("/b.vue");
    let (other_handle, other_sender) = completion_pair::<RequestResult>();
    let _ = dag.register_request(&other, 1, TargetStage::Source, other_sender, None);

    dag.supersede_old_file_generations(&c, 2);

    assert!(
        matches!(stale_handle.try_get(), Some(CompletionState::Superseded)),
        "stale gen=1 waiter must be signalled Superseded",
    );
    assert!(
        live_handle.try_get().is_none(),
        "live gen=2 waiter must remain pending",
    );
    assert!(
        other_handle.try_get().is_none(),
        "unrelated /b.vue waiter must remain pending",
    );
    assert_index_matches_scan(&dag, "/a.vue");
    assert_index_matches_scan(&dag, "/b.vue");
    // gen=1 gone from the surface AND the index; gen=2 retained.
    assert_eq!(index_fw_gens(&dag, "/a.vue"), BTreeSet::from([2]));
}

/// Stale DAG nodes are cancelled and cannot dispatch after a bump,
/// the live generation survives, unrelated files are untouched, and
/// the node index equals the full scan.
#[test]
fn supersede_cancels_stale_dag_nodes_and_blocks_dispatch() {
    let mut dag = SchedulerDag::new();

    let stale_src = file_stage("/a.vue", 1, FileStageKey::Source);
    let stale_art = artifact("/a.vue", 1, 7);
    let live_src = file_stage("/a.vue", 2, FileStageKey::Source);
    let unrelated = file_stage("/b.vue", 1, FileStageKey::Source);
    submit_node(&mut dag, stale_src.clone(), WorkKind::Load);
    submit_node(&mut dag, stale_art.clone(), WorkKind::Artifact);
    let live_tok = submit_node(&mut dag, live_src.clone(), WorkKind::Load);
    submit_node(&mut dag, unrelated.clone(), WorkKind::Load);

    dag.supersede_old_file_generations(&canonical("/a.vue"), 2);

    assert!(
        dag.token_for(&stale_src).is_none(),
        "stale gen=1 Source node must be cancelled",
    );
    assert!(
        dag.token_for(&stale_art).is_none(),
        "stale gen=1 Artifact node must be cancelled",
    );
    assert_eq!(
        dag.token_for(&live_src),
        Some(live_tok),
        "live gen=2 node must survive the bump",
    );
    assert!(
        dag.token_for(&unrelated).is_some(),
        "unrelated /b.vue node must survive",
    );
    // The only dispatchable nodes left are the live gen=2 and the
    // unrelated gen=1; neither stale node may dispatch.
    let mut dispatched = Vec::new();
    while let Some(job) = dag.next_ready() {
        dispatched.push(job.identity);
    }
    assert!(
        dispatched.contains(&live_src) && dispatched.contains(&unrelated),
        "live and unrelated nodes dispatch",
    );
    assert!(
        !dispatched.contains(&stale_src) && !dispatched.contains(&stale_art),
        "no stale-generation node may dispatch after supersede",
    );
}

/// Stale artifact blocker deps are dropped after a bump; the live
/// generation entry and unrelated owners survive; index equals scan.
#[test]
fn supersede_drops_stale_artifact_blocker_deps() {
    let mut dag = SchedulerDag::new();
    let c = canonical("/a.vue");
    let other = canonical("/b.vue");

    let set = || PendingBlockerSet::from_deps(BTreeSet::from([analysis_dep("/dep.ts", 1)]));
    dag.record_artifact_blockers(&c, 1, set());
    dag.record_artifact_blockers(&c, 2, set());
    dag.record_artifact_blockers(&other, 1, set());

    dag.supersede_old_file_generations(&c, 2);

    assert!(
        dag.peek_artifact_blockers(&c, 1).is_empty(),
        "stale gen=1 blocker entry must be dropped",
    );
    assert!(
        !dag.peek_artifact_blockers(&c, 2).is_empty(),
        "live gen=2 blocker entry must survive",
    );
    assert!(
        !dag.peek_artifact_blockers(&other, 1).is_empty(),
        "unrelated /b.vue blocker entry must survive",
    );
    assert_eq!(index_blocker_gens(&dag, "/a.vue"), BTreeSet::from([2]));
    assert_index_matches_scan(&dag, "/a.vue");
    assert_index_matches_scan(&dag, "/b.vue");
}

/// Stale terminal failures are scrubbed after a bump so a later
/// admission cannot be pinned `Failed`; the live generation's record
/// and unrelated files survive; index equals scan.
#[test]
fn supersede_scrubs_stale_terminal_failures_so_no_later_pin() {
    let mut dag = SchedulerDag::new();
    let c = canonical("/a.vue");

    let stale_key = analysis_dep("/a.vue", 1);
    let live_key = analysis_dep("/a.vue", 2);
    let other_key = analysis_dep("/b.vue", 1);
    dag.insert_terminal_dep_failure(failed_record(stale_key.clone()));
    dag.insert_terminal_dep_failure(failed_record(live_key.clone()));
    dag.insert_terminal_dep_failure(failed_record(other_key.clone()));

    dag.supersede_old_file_generations(&c, 2);

    assert!(
        dag.lookup_terminal_dep_failure(&stale_key).is_none(),
        "stale gen=1 terminal failure must be scrubbed — it must not pin \
         a later admission as Failed",
    );
    assert!(
        dag.lookup_terminal_dep_failure(&live_key).is_some(),
        "live gen=2 terminal failure must survive",
    );
    assert!(
        dag.lookup_terminal_dep_failure(&other_key).is_some(),
        "unrelated /b.vue terminal failure must survive",
    );
    assert_eq!(
        index_terminal_keys(&dag, "/a.vue"),
        BTreeSet::from([live_key]),
    );
    assert_index_matches_scan(&dag, "/a.vue");
    assert_index_matches_scan(&dag, "/b.vue");
}

// ─────────────────────────────────────────────────────────────────
// 2. Reverse-index-equals-full-scan invariant
// ─────────────────────────────────────────────────────────────────

/// After submit + the dedup-merge path + the in-flight dedup path, the
/// node index still equals the full scan. The merge paths reuse the
/// existing token without producing a new node, so the index must NOT
/// double-count or drop the entry.
#[test]
fn reverse_index_matches_scan_across_submit_and_dedup_merge() {
    let mut dag = SchedulerDag::new();
    let id = file_stage("/a.vue", 1, FileStageKey::Source);

    let tok1 = submit_node(&mut dag, id.clone(), WorkKind::Load);
    assert_eq!(index_node_tokens(&dag, "/a.vue"), BTreeSet::from([tok1]));
    assert_index_matches_scan(&dag, "/a.vue");

    // Pre-dispatch dedup-merge: same identity, same token, no new node.
    let tok2 = submit_node(&mut dag, id.clone(), WorkKind::Load);
    assert_eq!(tok1, tok2, "dedup-merge returns the same token");
    assert_eq!(
        index_node_tokens(&dag, "/a.vue"),
        BTreeSet::from([tok1]),
        "dedup-merge must not double-index",
    );
    assert_index_matches_scan(&dag, "/a.vue");

    // In-flight dedup: dispatch, then submit again.
    let job = dag.next_ready().expect("ready");
    assert_eq!(job.token, tok1);
    let tok3 = submit_node(&mut dag, id.clone(), WorkKind::Load);
    assert_eq!(tok1, tok3, "in-flight dedup returns the same token");
    assert_index_matches_scan(&dag, "/a.vue");

    // Completion removes the node from the index in lock-step.
    dag.complete(&id);
    assert!(index_node_tokens(&dag, "/a.vue").is_empty());
    assert_index_matches_scan(&dag, "/a.vue");
}

/// A broad mix of mutations across multiple canonicals leaves every
/// reverse index in agreement with the full scan, including after
/// complete / cancel / drain / scrub / shutdown removals.
#[test]
fn reverse_index_matches_scan_after_removals_and_shutdown() {
    let mut dag = SchedulerDag::new();
    let files = ["/a.vue", "/b.vue", "/c.vue"];

    // Populate all four surfaces across files and generations.
    for f in files {
        for gen in 1..=2u64 {
            submit_node(
                &mut dag,
                file_stage(f, gen, FileStageKey::Source),
                WorkKind::Load,
            );
            submit_node(&mut dag, artifact(f, gen, 3), WorkKind::Artifact);
            let (_h, sender) = completion_pair::<RequestResult>();
            let _ = dag.register_request(&canonical(f), gen, TargetStage::Source, sender, None);
            dag.record_artifact_blockers(
                &canonical(f),
                gen,
                PendingBlockerSet::from_deps(BTreeSet::from([analysis_dep("/dep.ts", gen)])),
            );
            dag.insert_terminal_dep_failure(failed_record(analysis_dep(f, gen)));
        }
    }
    for f in files {
        assert_index_matches_scan(&dag, f);
    }

    // complete a node, cancel a node.
    dag.complete(&file_stage("/a.vue", 1, FileStageKey::Source));
    dag.cancel(&artifact("/b.vue", 2, 3));
    // signal_stage_complete removes a satisfied waiter group.
    let result = RequestResult::Source(Arc::new(crate::node::SourceSnapshot::new_empty(
        Arc::from(""),
        1,
    )));
    dag.signal_stage_complete(&canonical("/c.vue"), 1, &TaskKind::Load, &result);
    // drain a blocker, scrub terminal failures + blockers for a dep.
    let _ = dag.drain_artifact_blockers(&canonical("/a.vue"), 2);
    dag.scrub_terminal_dep_failures_referencing("/b.vue");
    dag.scrub_artifact_blockers_referencing("/dep.ts");
    for f in files {
        assert_index_matches_scan(&dag, f);
    }
    // remove-owner + per-file shutdown.
    dag.artifact_blocker_deps_remove_owner("/c.vue");
    dag.signal_file_shutdown(&canonical("/a.vue"));
    for f in files {
        assert_index_matches_scan(&dag, f);
    }

    // Global waiter drain empties the file-waiter index.
    dag.signal_all_shutdown();
    for f in files {
        assert!(
            index_fw_gens(&dag, f).is_empty() && scan_fw_gens(&dag, f).is_empty(),
            "global shutdown must empty the file-waiter index for {f}",
        );
        assert_index_matches_scan(&dag, f);
    }

    // Full reset clears every index map.
    dag.clear();
    for f in files {
        assert_index_matches_scan(&dag, f);
    }
    assert!(dag.canonical_index.node_tokens.is_empty());
    assert!(dag.canonical_index.blocker_owner_gens.is_empty());
    assert!(dag.canonical_index.terminal_failure_keys.is_empty());
}

/// The node candidate set a supersede iterates is the bumped
/// canonical's bucket alone — its size tracks that file's live nodes,
/// independent of how many unrelated nodes exist. This is the
/// scan-bound property: supersede does O(affected) work, not O(total).
#[test]
fn supersede_node_candidate_set_is_per_canonical_not_total() {
    let mut dag = SchedulerDag::new();

    // Many unrelated single-node canonicals.
    const UNRELATED: u64 = 40;
    for i in 0..UNRELATED {
        let f = format!("/u{i}.vue");
        submit_node(
            &mut dag,
            file_stage(&f, 1, FileStageKey::Source),
            WorkKind::Load,
        );
    }
    // The target: two stale gen=1 nodes + one live gen=2 node.
    submit_node(
        &mut dag,
        file_stage("/target.vue", 1, FileStageKey::Source),
        WorkKind::Load,
    );
    submit_node(
        &mut dag,
        file_stage("/target.vue", 1, FileStageKey::Analysis),
        WorkKind::Analysis,
    );
    submit_node(
        &mut dag,
        file_stage("/target.vue", 2, FileStageKey::Source),
        WorkKind::Load,
    );

    assert_eq!(dag.total_active(), UNRELATED as usize + 3);
    // The supersede's candidate bucket is exactly the target's three
    // nodes — NOT the 43 total. Each unrelated file occupies its own
    // single-entry bucket.
    assert_eq!(
        index_node_tokens(&dag, "/target.vue").len(),
        3,
        "the per-canonical bucket holds only the target's nodes",
    );
    for i in 0..UNRELATED {
        assert_eq!(index_node_tokens(&dag, &format!("/u{i}.vue")).len(), 1);
    }

    dag.supersede_old_file_generations(&canonical("/target.vue"), 2);

    // Only the two stale target nodes are gone; everything else stays.
    assert_eq!(
        dag.total_active(),
        UNRELATED as usize + 1,
        "supersede cancelled only the two stale target nodes",
    );
    assert_eq!(index_node_tokens(&dag, "/target.vue").len(), 1);
    assert_index_matches_scan(&dag, "/target.vue");
    for i in 0..UNRELATED {
        let f = format!("/u{i}.vue");
        assert_index_matches_scan(&dag, &f);
    }
}
