//! Dependency-edge lifecycle: direct edge removal, generation retirement
//! through the file index, edge-admission ordering, and correctness of
//! never-admitted producers, cancellation/arrival races and delayed old
//! incarnations.

use super::super::*;

const SIZES: [usize; 4] = [128, 256, 512, 1024];

fn file_stage(
    path: &str,
    incarnation: u64,
    generation: u64,
    stage: FileStageKey,
) -> WorkNodeIdentity {
    WorkNodeIdentity::FileStage {
        canonical: Arc::from(path),
        incarnation,
        generation,
        stage,
    }
}

fn artifact(path: &str, generation: u64, profile: u64) -> WorkNodeIdentity {
    WorkNodeIdentity::Artifact {
        canonical: Arc::from(path),
        incarnation: 1,
        generation,
        profile_hash: profile_hash_to_bytes(profile),
        content_hash: [0u8; 16],
    }
}

fn analysis(path: &str, incarnation: u64, generation: u64) -> DepKey {
    DepKey::from_identity(&file_stage(
        path,
        incarnation,
        generation,
        FileStageKey::Analysis,
    ))
}

fn gated(dag: &mut SchedulerDag, identity: WorkNodeIdentity, deps: Vec<DepKey>) -> SubmissionToken {
    let kind = match identity {
        WorkNodeIdentity::Artifact { .. } => WorkKind::Artifact,
        _ => WorkKind::Analysis,
    };
    dag.submit_expect(identity, kind, Priority::Interactive, deps, None)
}

/// Dispatch and complete every ready node, then assert the DAG holds no
/// node, edge, index entry or permit.
fn drain_and_assert_empty(dag: &mut SchedulerDag) {
    while let Some(job) = dag.next_ready() {
        let identity = job.identity.clone();
        drop(job);
        let _ = dag.complete(&identity);
    }
    assert_eq!(dag.total_active(), 0, "every node terminalized");
    assert!(dag.dep_edges.is_drained(), "every dependency edge unlinked");
    assert!(dag.canonical_index.node_tokens.is_empty());
    assert_eq!(dag.in_flight_permits(), 0, "every permit returned");
}

/// Cancelling every sibling gated on one shared dependency removes each
/// edge by key: the waiter entries visited grow linearly with the sibling
/// count, never with the siblings still linked.
#[test]
fn sibling_cancellations_remove_each_edge_directly() {
    let mut visits = Vec::new();
    for siblings in SIZES {
        let mut dag = SchedulerDag::new();
        let shared = analysis("/shared.ts", 1, 1);
        let identities: Vec<WorkNodeIdentity> = (0..siblings)
            .map(|i| artifact(&format!("/sibling-{i}.vue"), 1, 7))
            .collect();
        for identity in &identities {
            gated(&mut dag, identity.clone(), vec![shared.clone()]);
        }
        assert_eq!(dag.dep_edges.edge_count(), siblings);

        let before = dag.dep_edge_observations().unlink_entries_visited;
        // Interleave from both ends so neither end of the edge order is
        // privileged.
        let mut order: Vec<usize> = Vec::with_capacity(siblings);
        let (mut lo, mut hi) = (0, siblings);
        while lo < hi {
            hi -= 1;
            order.push(hi);
            if lo < hi {
                order.push(lo);
                lo += 1;
            }
        }
        for index in order {
            assert!(
                dag.cancel(&identities[index]).is_empty(),
                "a cancelled waiter strands nothing",
            );
        }
        visits.push(dag.dep_edge_observations().unlink_entries_visited - before);
        drain_and_assert_empty(&mut dag);
    }
    for (siblings, visited) in SIZES.iter().zip(&visits) {
        assert_eq!(
            *visited, *siblings as u64,
            "{siblings} sibling cancellations visit one edge each",
        );
    }
    assert_eq!(
        visits[3] / visits[0],
        8,
        "8x the siblings is 8x the removal work, not 64x",
    );
}

/// Superseding one file visits only that file's retired dependencies,
/// however many unrelated files hold pending dependency edges, and still
/// releases waiters on producers that were never admitted.
#[test]
fn unrelated_supersessions_visit_only_retired_dependencies() {
    let mut visits = Vec::new();
    for unrelated in SIZES {
        let mut dag = SchedulerDag::new();
        let unrelated_waiters: Vec<WorkNodeIdentity> = (0..unrelated)
            .map(|i| {
                let identity = artifact(&format!("/owner-{i}.vue"), 1, 1);
                gated(
                    &mut dag,
                    identity.clone(),
                    vec![analysis(&format!("/unrelated-{i}.ts"), 1, 1)],
                );
                identity
            })
            .collect();
        // Three waiters on one never-admitted dependency at the retired
        // generation, one waiter at the surviving generation.
        let retired: Vec<SubmissionToken> = (0..3)
            .map(|profile| {
                gated(
                    &mut dag,
                    artifact("/target-owner.vue", 1, profile),
                    vec![analysis("/target.ts", 1, 1)],
                )
            })
            .collect();
        let surviving = artifact("/target-owner.vue", 1, 9);
        gated(
            &mut dag,
            surviving.clone(),
            vec![analysis("/target.ts", 1, 2)],
        );

        let before = dag.dep_edge_observations().retire_keys_visited;
        let stranded = dag.retire_generations_below(&Arc::from("/target.ts"), 2);
        visits.push(dag.dep_edge_observations().retire_keys_visited - before);

        assert_eq!(
            stranded.iter().copied().collect::<BTreeSet<_>>(),
            retired.iter().copied().collect::<BTreeSet<_>>(),
            "every waiter on the never-admitted retired producer is released",
        );
        assert!(
            dag.has_pending_deps(&surviving),
            "the live generation still gates"
        );
        assert!(
            unrelated_waiters
                .iter()
                .all(|identity| dag.has_pending_deps(identity)),
            "unrelated waiters keep their dependencies",
        );
        // Release everything else and drain.
        let _ = dag.retire_generations_below(&Arc::from("/target.ts"), 3);
        for i in 0..unrelated {
            let _ = dag.retire_generations_below(&Arc::from(format!("/unrelated-{i}.ts")), 2);
        }
        drain_and_assert_empty(&mut dag);
    }
    assert!(
        visits.iter().all(|visited| *visited == 1),
        "supersession visits the one retired dependency at every scale: {visits:?}",
    );
}

/// Waiters are released in the order their edges were linked, not in
/// token order: a pre-dispatch merge links an older node after younger
/// siblings.
#[test]
fn waiters_release_in_edge_admission_order() {
    let mut dag = SchedulerDag::new();
    let producer = file_stage("/dep.ts", 1, 1, FileStageKey::Analysis);
    let blocker = file_stage("/blocker.ts", 1, 1, FileStageKey::Analysis);
    let dep = DepKey::from_identity(&producer);
    let oldest = gated(
        &mut dag,
        artifact("/a.vue", 1, 1),
        vec![DepKey::from_identity(&blocker)],
    );
    let middle = gated(&mut dag, artifact("/b.vue", 1, 1), vec![dep.clone()]);
    let youngest = gated(&mut dag, artifact("/c.vue", 1, 1), vec![dep.clone()]);
    // The oldest node gains the shared dependency last, then its first
    // blocker clears so the shared dependency is its only gate.
    assert_eq!(
        gated(&mut dag, artifact("/a.vue", 1, 1), vec![dep.clone()]),
        oldest
    );
    let _ = dag.retire_generations_below(&Arc::from("/blocker.ts"), 2);

    gated(&mut dag, producer.clone(), Vec::new());
    let job = dag.next_ready().expect("the producer dispatches");
    assert_eq!(job.identity, producer);
    assert_eq!(
        dag.complete(&producer),
        vec![middle, youngest, oldest],
        "release follows edge admission order",
    );
    drop(job);
    drain_and_assert_empty(&mut dag);
}

/// A waiter cancelled before its producer completes leaves no edge behind,
/// and a fresh arrival of the same identity links a new edge that the
/// completion releases alone.
#[test]
fn cancellation_and_arrival_race_the_producer_cleanly() {
    let mut dag = SchedulerDag::new();
    let producer = file_stage("/dep.ts", 1, 1, FileStageKey::Analysis);
    let dep = DepKey::from_identity(&producer);
    let waiter = artifact("/owner.vue", 1, 1);
    let first = gated(&mut dag, waiter.clone(), vec![dep.clone()]);
    gated(&mut dag, producer.clone(), Vec::new());
    let job = dag.next_ready().expect("the producer dispatches");

    // Cancellation lands while the producer runs.
    assert!(dag.cancel(&waiter).is_empty());
    assert_eq!(dag.dep_edges.edge_count(), 0);
    // A new arrival of the same identity re-links.
    let second = gated(&mut dag, waiter.clone(), vec![dep.clone()]);
    assert_ne!(first, second, "the cancelled node is not revived");
    assert_eq!(
        dag.dep_edges.waiters_of(&dep).collect::<Vec<_>>(),
        vec![second]
    );

    assert_eq!(dag.complete(&producer), vec![second]);
    drop(job);
    // A late repeat of the producer's completion finds nothing to release.
    assert!(dag.complete(&producer).is_empty());
    drain_and_assert_empty(&mut dag);
}

/// A delayed producer of a retired incarnation cannot release waiters of
/// the successor incarnation at the same generation; generation retirement
/// still reaches both incarnations' edges.
#[test]
fn delayed_old_incarnation_cannot_release_successor_waiters() {
    let mut dag = SchedulerDag::new();
    let old_producer = file_stage("/dep.ts", 1, 1, FileStageKey::Analysis);
    let new_producer = file_stage("/dep.ts", 2, 1, FileStageKey::Analysis);
    let successor_waiter = artifact("/owner.vue", 1, 1);
    let successor = gated(
        &mut dag,
        successor_waiter.clone(),
        vec![DepKey::from_identity(&new_producer)],
    );

    gated(&mut dag, old_producer.clone(), Vec::new());
    let job = dag.next_ready().expect("the old producer dispatches");
    assert!(
        dag.complete(&old_producer).is_empty(),
        "the old incarnation releases nothing of its successor",
    );
    drop(job);
    assert!(dag.has_pending_deps(&successor_waiter));

    // An old-incarnation waiter at the same generation is retired together
    // with the successor's by the generation floor.
    let old_waiter = artifact("/owner.vue", 1, 2);
    let old = gated(
        &mut dag,
        old_waiter,
        vec![DepKey::from_identity(&old_producer)],
    );
    let stranded: BTreeSet<_> = dag
        .retire_generations_below(&Arc::from("/dep.ts"), 2)
        .into_iter()
        .collect();
    assert_eq!(stranded, BTreeSet::from([successor, old]));
    drain_and_assert_empty(&mut dag);
}

/// A node gated on its own identity is skipped as a cancelled waiter when
/// it is cancelled, yet both sides of its edge still go: nothing dangles.
#[test]
fn cancelling_a_self_gated_node_leaves_no_dangling_edge() {
    let mut dag = SchedulerDag::new();
    let identity = file_stage("/self.ts", 1, 1, FileStageKey::Analysis);
    gated(
        &mut dag,
        identity.clone(),
        vec![DepKey::from_identity(&identity)],
    );
    assert_eq!(dag.dep_edges.edge_count(), 1);
    assert!(dag.cancel(&identity).is_empty());
    drain_and_assert_empty(&mut dag);
}
