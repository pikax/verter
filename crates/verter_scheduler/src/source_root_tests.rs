//! Contract tests for the epoch-indexed MVCC source root.
//!
//! The invariants under test:
//!
//! 1. **As-of sealing** — a captured root answers with the SUPERSEDED
//!    state after the live node has moved on. This is the property the
//!    live `Scheduler::try_get_source` cannot provide at all.
//! 2. **Atomic publication** — no capture can observe a window where the
//!    lifecycle transition has landed but the root has not, or the
//!    reverse.
//! 3. **Lease-gated reclamation** — a version reachable only from a live
//!    captured root is retained; once that root drops it is reclaimed.
//! 4. **O(1) capture** — capture cost does not scale with the number of
//!    tracked files.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::executor::{StageError, StageExecutor};
use crate::node::{FileNode, SourceSnapshot};
use crate::scheduler::{Request, Scheduler, SchedulerConfig};
use crate::source_loader::MemorySourceLoader;
use crate::source_root::{SchedulerSourceDirectory, SourceStateAt};
use crate::stage::{Priority, TargetStage};
use verter_language::FileLanguage;

// ── Fixtures ──

fn canonical(id: &str) -> Arc<str> {
    Arc::from(id)
}

fn hash(byte: u8) -> [u8; 16] {
    [byte; 16]
}

/// A stage executor that stamps a content-derived `whole_hash` /
/// `semantic_hash`, so a test can tell one committed source version from
/// another. The default executor leaves both zeroed.
#[derive(Debug, Default)]
struct HashingExecutor;

impl StageExecutor for HashingExecutor {
    fn execute_source(
        &self,
        _canonical_id: &str,
        _file_language: FileLanguage,
        content: Arc<str>,
        generation: u64,
    ) -> Result<SourceSnapshot, StageError> {
        // FNV-1a over the content, splatted into both hash slots. Only
        // distinguishability matters here, not the production digest.
        let mut acc: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in content.as_bytes() {
            acc ^= u64::from(*byte);
            acc = acc.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let mut whole = [0u8; 16];
        whole[..8].copy_from_slice(&acc.to_le_bytes());
        whole[8..].copy_from_slice(&acc.to_be_bytes());
        Ok(SourceSnapshot {
            source: content,
            whole_hash: whole,
            semantic_hash: whole,
            generation,
            data: Arc::new(crate::node::EmptyData),
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn hashing_scheduler(files: &[(&str, &str)]) -> Arc<Scheduler> {
    let loader = Arc::new(MemorySourceLoader::new());
    for (id, content) in files {
        loader.insert((*id).to_string(), Arc::from(*content));
    }
    Scheduler::test_new_sync_with_executor(
        SchedulerConfig::default(),
        loader,
        Arc::new(HashingExecutor),
    )
}

fn load(scheduler: &Arc<Scheduler>, file_id: &str) {
    scheduler.submit_request(Request {
        file_id: file_id.to_string(),
        target: TargetStage::Source,
        priority: Priority::Interactive,
        source: None,
        file_language: None,
        request_context: None,
    });
    scheduler.drive_all();
}

fn upsert(scheduler: &Arc<Scheduler>, file_id: &str, source: &str) {
    scheduler.submit_request(Request {
        file_id: file_id.to_string(),
        target: TargetStage::Source,
        priority: Priority::Interactive,
        source: Some(Arc::from(source)),
        file_language: None,
        request_context: None,
    });
    scheduler.drive_all();
}

// ── 1. As-of sealing ──

#[test]
fn lookup_is_sealed_to_the_captured_epoch() {
    let directory = Arc::new(SchedulerSourceDirectory::new());
    let file = canonical("/a.ts");

    directory.publish_transition(|publication| {
        publication.present(&file, 1, 1, hash(0xAA));
    });
    let root = directory.capture_root();

    directory.publish_transition(|publication| {
        publication.present(&file, 1, 2, hash(0xBB));
    });

    assert_eq!(
        root.lookup("/a.ts").whole_hash(),
        Some(hash(0xAA)),
        "a captured root must keep answering the version visible at its \
         own epoch, never the live one",
    );
    assert_eq!(
        directory.capture_root().lookup("/a.ts").whole_hash(),
        Some(hash(0xBB)),
        "a freshly captured root must see the current version",
    );
}

#[test]
fn untracked_canonical_reads_unknown_not_absent() {
    let directory = Arc::new(SchedulerSourceDirectory::new());
    let root = directory.capture_root();
    assert_eq!(root.lookup("/never-seen.ts"), SourceStateAt::Unknown);

    directory.publish_transition(|publication| {
        publication.absent(&canonical("/removed.ts"), 3, 9);
    });
    let after = directory.capture_root();
    assert_eq!(
        after.lookup("/removed.ts"),
        SourceStateAt::Absent {
            incarnation: 3,
            generation: 9,
        },
        "a published removal is a recorded transition, distinct from \
         `Unknown` (never published at all)",
    );
    assert_eq!(
        root.lookup("/removed.ts"),
        SourceStateAt::Unknown,
        "the earlier root predates the removal's epoch",
    );
}

#[test]
fn a_batch_transition_publishes_exactly_one_epoch() {
    let directory = Arc::new(SchedulerSourceDirectory::new());
    let before = directory.current_epoch();

    directory.publish_transition(|publication| {
        for i in 0..8u8 {
            publication.absent(&canonical(&format!("/f{i}.ts")), 1, 1);
        }
    });

    assert_eq!(
        directory.current_epoch(),
        before + 1,
        "a batch publishes ONE epoch covering all changed members",
    );
    let root = directory.capture_root();
    for i in 0..8u8 {
        assert!(
            matches!(
                root.lookup(&format!("/f{i}.ts")),
                SourceStateAt::Absent { .. }
            ),
            "every batch member must be visible at the single published epoch",
        );
    }
}

#[test]
fn a_transition_that_records_nothing_does_not_advance_the_epoch() {
    let directory = Arc::new(SchedulerSourceDirectory::new());
    directory.publish_transition(|publication| {
        publication.absent(&canonical("/a.ts"), 1, 1);
    });
    let epoch = directory.current_epoch();

    let observed = directory.publish_transition(|publication| {
        assert!(publication.is_empty());
        7_u32
    });

    assert_eq!(observed, 7, "the transition's value is returned verbatim");
    assert_eq!(
        directory.current_epoch(),
        epoch,
        "a transition with no published version must not advance the epoch \
         — node creation leaves `try_get_source`'s answer unchanged",
    );
}

// ── 2. Atomic publication ──

/// The transition and its publication are ONE critical section: a
/// capture racing the window blocks and observes both halves, never the
/// torn pair.
///
/// The mutation the closure performs here stands in for a scheduler node
/// transition (`bump_generation` / `source.store` / `nodes.remove`). If
/// that mutation is moved OUTSIDE the `publish_transition` closure, the
/// capturer observes the mutated node with the pre-mutation root, and
/// this test fails.
#[test]
fn capture_cannot_observe_a_transition_that_the_root_has_not_published() {
    let directory = Arc::new(SchedulerSourceDirectory::new());
    let file = canonical("/a.ts");
    directory.publish_transition(|publication| {
        publication.present(&file, 1, 1, hash(0x11));
    });

    // Stands in for `FileNode::generation`: the live execution state a
    // lifecycle transition mutates.
    let live_generation = Arc::new(AtomicU64::new(1));
    let (in_window_tx, in_window_rx) = mpsc::channel::<()>();
    let (attempted_tx, attempted_rx) = mpsc::channel::<()>();

    let mutator = {
        let directory = Arc::clone(&directory);
        let live_generation = Arc::clone(&live_generation);
        let file = Arc::clone(&file);
        std::thread::spawn(move || {
            directory.publish_transition(move |publication| {
                let generation = live_generation.fetch_add(1, Ordering::AcqRel) + 1;
                in_window_tx.send(()).expect("window signal");
                // Hold the window open until the capturer has entered
                // its capture call.
                attempted_rx.recv().expect("capture-attempt signal");
                publication.present(&file, 1, generation, hash(0x22));
            });
        })
    };

    in_window_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("mutator must enter the transition window");

    attempted_tx.send(()).expect("capture-attempt signal");
    let root = directory.capture_root();
    let observed_generation = live_generation.load(Ordering::Acquire);
    let observed_state = root.lookup("/a.ts");
    mutator.join().expect("mutator thread");

    // The capture is totally ordered against the transition: either it
    // won the lock first (generation 1, root still at hash 0x11) or it
    // waited (generation 2, root already at hash 0x22). The torn pair —
    // generation 2 with hash 0x11 — is what a mutation performed
    // outside the publication hold produces.
    let coherent = match (observed_generation, observed_state) {
        (1, SourceStateAt::Present { whole_hash, .. }) => whole_hash == hash(0x11),
        (2, SourceStateAt::Present { whole_hash, .. }) => whole_hash == hash(0x22),
        _ => false,
    };
    assert!(
        coherent,
        "capture observed a TORN pair: live generation {observed_generation} \
         with root state {observed_state:?} — publication is not atomic with \
         the transition",
    );
}

/// The wiring half of the same invariant: `Scheduler::invalidate`
/// performs its generation bump INSIDE the publication hold.
///
/// While another thread holds the publication lock, no lifecycle
/// transition may advance a node's generation. A bump moved outside the
/// hold advances it immediately and fails this test.
#[test]
fn the_invalidate_generation_bump_runs_inside_the_publication_hold() {
    let scheduler = hashing_scheduler(&[("/a.vue", "one")]);
    load(&scheduler, "/a.vue");
    let node = Arc::clone(
        scheduler
            .nodes
            .get("/a.vue")
            .expect("node published")
            .value(),
    );
    let generation_before = node.generation();

    let (holding_tx, holding_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::sync_channel::<()>(0);
    // Sample generation FROM THE HOLDER while the publication lock is
    // still held. The invalidator signals once it has taken the DAG
    // lock and is about to take the publication hold, so this is an
    // exact lock-attempt receipt rather than a yield-count race.
    let holder = {
        let directory = Arc::clone(scheduler.source_directory());
        let node_for_holder = Arc::clone(&node);
        std::thread::spawn(move || {
            directory.publish_transition(move |publication| {
                holding_tx.send(()).expect("hold signal");
                release_rx
                    .recv()
                    .expect("driver must release after sampling");
                assert_eq!(
                    node_for_holder.generation(),
                    generation_before,
                    "generation advanced while the publication lock is held — \
                     bump_generation ran outside the hold"
                );
                publication.absent(&canonical("/unrelated.ts"), 1, 1);
            });
        })
    };
    holding_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("holder must acquire the publication lock");

    let (attempt_tx, attempt_rx) = mpsc::sync_channel::<()>(0);
    let invalidator = {
        let scheduler = Arc::clone(&scheduler);
        std::thread::spawn(move || {
            scheduler.invalidate_signaling_before_publication("/a.vue", attempt_tx);
        })
    };
    attempt_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("invalidator must reach the publication hold");
    assert_eq!(
        node.generation(),
        generation_before,
        "generation advanced before the publication hold — bump ran outside it"
    );
    release_tx
        .send(())
        .expect("holder must still be waiting to release");
    holder.join().expect("holder thread");
    invalidator.join().expect("invalidator thread");

    assert_eq!(
        node.generation(),
        generation_before + 1,
        "the bump must land once the publication hold is released",
    );
    assert!(
        matches!(
            scheduler.capture_source_root().lookup("/a.vue"),
            SourceStateAt::Absent { .. }
        ),
        "invalidation publishes an `Absent` version",
    );
}

// ── 3. Lease-gated reclamation ──

#[test]
fn a_version_reachable_only_from_a_live_root_is_retained_then_reclaimed() {
    let directory = Arc::new(SchedulerSourceDirectory::new());
    let file = canonical("/a.ts");

    directory.publish_transition(|publication| {
        publication.present(&file, 1, 1, hash(0xA1));
    });
    let leased = directory.capture_root();
    directory.publish_transition(|publication| {
        publication.present(&file, 1, 2, hash(0xA2));
    });

    assert_eq!(directory.retained_version_count("/a.ts"), 2);
    let reclaimed = directory.reclaim_superseded_versions();
    assert_eq!(
        reclaimed, 0,
        "a version a live captured root still selects must NOT be reclaimed",
    );
    assert_eq!(directory.retained_version_count("/a.ts"), 2);
    assert_eq!(
        leased.lookup("/a.ts").whole_hash(),
        Some(hash(0xA1)),
        "the lease keeps the root's own world resolvable",
    );

    drop(leased);
    assert_eq!(directory.live_root_count(), 0);
    let reclaimed = directory.reclaim_superseded_versions();
    assert_eq!(
        reclaimed, 1,
        "once the last root addressing it drops, the superseded version is \
         physically reclaimed",
    );
    assert_eq!(directory.retained_version_count("/a.ts"), 1);
    assert_eq!(
        directory.capture_root().lookup("/a.ts").whole_hash(),
        Some(hash(0xA2)),
        "reclamation never disturbs the current root's answer",
    );
}

/// Retention is decided PER LIVE ROOT, not by a floor.
///
/// Every live root pins exactly the version IT selects, and releasing a
/// newer root never frees an older root's version. What retention must
/// NOT do is pin a version no root selects: a version born and superseded
/// strictly between two roots' epochs is unreachable from both, and from
/// the current root, the moment it is superseded. A floor rule
/// ("everything born after the oldest live root stays") keeps those
/// too — which is how ONE stale root came to pin the entire future.
#[test]
fn retention_keeps_the_version_each_live_root_selects_and_nothing_else() {
    let directory = Arc::new(SchedulerSourceDirectory::new());
    let file = canonical("/a.ts");

    directory.publish_transition(|p| p.present(&file, 1, 1, hash(1)));
    let oldest = directory.capture_root();
    // Born AND superseded strictly between `oldest` and `middle`: no
    // root can ever select it.
    directory.publish_transition(|p| p.present(&file, 1, 2, hash(2)));
    directory.publish_transition(|p| p.present(&file, 1, 3, hash(3)));
    let middle = directory.capture_root();
    directory.publish_transition(|p| p.present(&file, 1, 4, hash(4)));
    let newest = directory.capture_root();

    assert_eq!(directory.retained_version_count("/a.ts"), 4);
    assert_eq!(
        directory.reclaim_superseded_versions(),
        1,
        "the version no live root selects — born after `oldest` and \
         superseded before `middle` — is unreachable and must be freed",
    );
    assert_eq!(
        directory.retained_version_count("/a.ts"),
        3,
        "one version per live root remains: hash(1), hash(3), hash(4)",
    );

    drop(newest);
    assert_eq!(
        directory.reclaim_superseded_versions(),
        0,
        "dropping the NEWEST root frees nothing — hash(4) is the CURRENT \
         version and hash(1)/hash(3) are each selected by a live root",
    );
    assert_eq!(oldest.lookup("/a.ts").whole_hash(), Some(hash(1)));
    assert_eq!(middle.lookup("/a.ts").whole_hash(), Some(hash(3)));

    drop(oldest);
    assert_eq!(
        directory.reclaim_superseded_versions(),
        1,
        "with the oldest root gone its version — and only its version — is \
         freed",
    );
    assert_eq!(middle.lookup("/a.ts").whole_hash(), Some(hash(3)));

    drop(middle);
    assert_eq!(directory.reclaim_superseded_versions(), 1);
    assert_eq!(directory.retained_version_count("/a.ts"), 1);
}

/// ONE root held across an edit loop must retain ONE version, not one
/// version per edit.
///
/// This is the unbounded-growth shape: a root at epoch `E` can only ever
/// select the version visible at `E`, so every version born after it and
/// superseded before the current epoch is unreachable from every root.
/// Retaining them anyway turns a single long-lived view — the manager's
/// own cached base view is one — into an OOM on a keystroke loop.
#[test]
fn one_pinned_root_does_not_retain_a_version_per_edit() {
    let directory = Arc::new(SchedulerSourceDirectory::new());
    let file = canonical("/pinned.ts");

    directory.publish_transition(|p| p.present(&file, 1, 1, hash(1)));
    let pinned = directory.capture_root();

    let mut retained_at = Vec::new();
    for generation in 2..=400u64 {
        directory.publish_transition(|p| {
            p.present(&file, 1, generation, hash(generation as u8));
        });
        if generation == 100 || generation == 400 {
            directory.reclaim_superseded_versions();
            retained_at.push(directory.retained_version_count("/pinned.ts"));
        }
    }

    assert_eq!(
        retained_at[0], retained_at[1],
        "retention under a pinned root must be INDEPENDENT of the edit \
         count — 100 edits retained {} versions, 400 retained {}",
        retained_at[0], retained_at[1],
    );
    assert!(
        retained_at[1] <= 4,
        "a pinned root retains its OWN version plus the current one, not \
         the whole history (retained {})",
        retained_at[1],
    );
    assert_eq!(
        pinned.lookup("/pinned.ts").whole_hash(),
        Some(hash(1)),
        "the pinned root still resolves its own world",
    );
    assert_eq!(
        directory.capture_root().lookup("/pinned.ts").whole_hash(),
        Some(hash(400u64 as u8)),
        "the current root still resolves the current world",
    );

    drop(pinned);
    directory.reclaim_superseded_versions();
    assert_eq!(
        directory.retained_version_count("/pinned.ts"),
        1,
        "once the last root drops only the live version remains",
    );
}

/// The epoch counter saturates instead of wrapping, and a root captured
/// on the exhausted line FAILS CLOSED.
///
/// Wrapping is the unacceptable outcome: past `u64::MAX` a version
/// published "after" a root would be born at epoch 0 and compare as
/// published BEFORE it, inverting visibility for every subsequent read.
/// Debug builds would panic mid-transition (leaving the node mutated and
/// the history stale); release builds would wrap silently.
#[test]
fn the_publication_epoch_saturates_and_the_exhausted_root_fails_closed() {
    let directory = Arc::new(SchedulerSourceDirectory::new());
    let file = canonical("/exhausted.ts");

    directory.seed_epoch_for_test(u64::MAX - 2);
    directory.publish_transition(|p| p.present(&file, 1, 1, hash(1)));
    let usable = directory.capture_root();
    assert!(!usable.is_exhausted());
    assert_eq!(
        usable.lookup("/exhausted.ts").whole_hash(),
        Some(hash(1)),
        "the last usable epochs address membership normally"
    );

    // Two more publications reach the terminal epoch; a third must
    // neither wrap nor panic.
    for generation in 2..=4u64 {
        directory.publish_transition(|p| {
            p.present(&file, 1, generation, hash(generation as u8));
        });
    }
    assert_eq!(
        directory.current_epoch(),
        u64::MAX,
        "the counter saturates at the terminal epoch — it never wraps",
    );

    let exhausted = directory.capture_root();
    assert!(exhausted.is_exhausted());
    assert_eq!(
        exhausted.lookup("/exhausted.ts"),
        SourceStateAt::Unknown,
        "an exhausted root addresses no published state — it fails closed \
         rather than answering from a world whose ordering it cannot \
         express",
    );
    assert_eq!(
        usable.lookup("/exhausted.ts").whole_hash(),
        Some(hash(1)),
        "a root captured before exhaustion keeps its own answer",
    );
}

#[test]
fn a_root_free_edit_loop_does_not_retain_a_version_per_edit() {
    let directory = Arc::new(SchedulerSourceDirectory::new());
    let file = canonical("/a.ts");
    for generation in 1..=400u64 {
        directory.publish_transition(|publication| {
            publication.present(&file, 1, generation, hash(generation as u8));
        });
    }
    let retained = directory.retained_version_count("/a.ts");
    assert!(
        retained <= 64,
        "a root-free edit loop must stay bounded by the amortised sweep, \
         retained {retained} versions after 400 edits",
    );
}

// ── 4. O(1) capture ──

/// Capture is one mutex acquisition, one scalar read and one counter
/// bump — it must not scale with the number of tracked files.
///
/// Measured at two host sizes with a min-of-rounds estimator, so a
/// scheduler hiccup inflates a round rather than the verdict. An
/// implementation that enumerated nodes or versions would show roughly
/// the 12x size ratio; the assertion allows 4x for cache-locality
/// effects at the larger size.
#[test]
fn capture_cost_does_not_scale_with_the_number_of_tracked_files() {
    const SMALL: usize = 250;
    const LARGE: usize = 3000;
    const CAPTURES_PER_ROUND: usize = 2000;
    const ROUNDS: usize = 5;

    fn scheduler_with_files(count: usize) -> Arc<Scheduler> {
        let scheduler = hashing_scheduler(&[]);
        scheduler
            .source_directory()
            .publish_transition(|publication| {
                for i in 0..count {
                    let id = format!("/f{i}.ts");
                    scheduler.nodes.insert(
                        id.clone(),
                        Arc::new(FileNode::new(id.clone(), FileLanguage::script_ts())),
                    );
                    publication.present(&canonical(&id), 1, 1, hash(i as u8));
                }
            });
        scheduler
    }

    fn per_capture_nanos(scheduler: &Arc<Scheduler>) -> u128 {
        let mut best = u128::MAX;
        for _ in 0..ROUNDS {
            let start = Instant::now();
            for _ in 0..CAPTURES_PER_ROUND {
                let root = scheduler.capture_source_root();
                std::hint::black_box(root.epoch());
            }
            best = best.min(start.elapsed().as_nanos() / CAPTURES_PER_ROUND as u128);
        }
        best
    }

    let small = scheduler_with_files(SMALL);
    let large = scheduler_with_files(LARGE);
    assert_eq!(small.nodes.len(), SMALL);
    assert_eq!(large.nodes.len(), LARGE);

    // Warm both before measuring so first-touch page faults land in
    // neither measurement.
    let _ = per_capture_nanos(&small);
    let _ = per_capture_nanos(&large);

    let small_nanos = per_capture_nanos(&small).max(1);
    let large_nanos = per_capture_nanos(&large).max(1);

    assert!(
        large_nanos <= small_nanos * 4 + 200,
        "capture cost scaled with host size: {small_nanos}ns at {SMALL} files \
         vs {large_nanos}ns at {LARGE} files (size ratio {}x)",
        LARGE / SMALL,
    );
}

// ── Scheduler wiring: the transitions that publish ──

/// THE core property, end to end: a root captured before a keystroke
/// still answers the SUPERSEDED whole hash after the live node has been
/// bumped and recommitted.
#[test]
fn a_captured_root_answers_the_superseded_whole_hash_after_the_node_moves_on() {
    let scheduler = hashing_scheduler(&[("/a.vue", "first")]);
    load(&scheduler, "/a.vue");

    let committed = scheduler
        .try_get_source("/a.vue")
        .expect("source committed")
        .whole_hash;
    let root = scheduler.capture_source_root();
    assert_eq!(root.lookup("/a.vue").whole_hash(), Some(committed));

    upsert(&scheduler, "/a.vue", "second — a different content hash");

    let live = scheduler
        .try_get_source("/a.vue")
        .expect("recommitted")
        .whole_hash;
    assert_ne!(live, committed, "the edit must change the whole hash");

    assert_eq!(
        root.lookup("/a.vue").whole_hash(),
        Some(committed),
        "the captured root must still answer its OWN world's whole hash — \
         the live node's `bump_generation` made that snapshot unreachable, \
         which is exactly what the source root exists to fix",
    );
    assert_eq!(
        scheduler
            .capture_source_root()
            .lookup("/a.vue")
            .whole_hash(),
        Some(live),
        "a root captured after the edit sees the new world",
    );
}

#[test]
fn a_captured_root_answers_present_after_the_file_is_removed() {
    let scheduler = hashing_scheduler(&[("/a.vue", "content")]);
    load(&scheduler, "/a.vue");
    let committed = scheduler
        .try_get_source("/a.vue")
        .expect("source committed")
        .whole_hash;
    let root = scheduler.capture_source_root();

    scheduler.remove("/a.vue");

    assert!(
        scheduler.try_get_source("/a.vue").is_none(),
        "the live node is gone",
    );
    assert_eq!(
        root.lookup("/a.vue").whole_hash(),
        Some(committed),
        "removal is a published transition, so the pre-removal root keeps \
         resolving its own world",
    );
    assert!(
        matches!(
            scheduler.capture_source_root().lookup("/a.vue"),
            SourceStateAt::Absent { .. }
        ),
        "a root captured after the removal reads `Absent`",
    );
}

#[test]
fn close_file_publishes_absent_and_the_prior_root_keeps_present() {
    let scheduler = hashing_scheduler(&[("/a.vue", "on disk")]);
    load(&scheduler, "/a.vue");
    let committed = scheduler
        .try_get_source("/a.vue")
        .expect("source committed")
        .whole_hash;
    let root = scheduler.capture_source_root();

    scheduler.close_file("/a.vue");

    assert!(
        matches!(
            scheduler.capture_source_root().lookup("/a.vue"),
            SourceStateAt::Absent { .. }
        ),
        "`close_file` bumps the generation, so the file has no coherent \
         source until the reload commits",
    );
    assert_eq!(root.lookup("/a.vue").whole_hash(), Some(committed));
}

#[test]
fn reset_publishes_one_epoch_for_every_removed_file() {
    let scheduler = hashing_scheduler(&[("/a.vue", "a"), ("/b.vue", "b"), ("/c.vue", "c")]);
    for id in ["/a.vue", "/b.vue", "/c.vue"] {
        load(&scheduler, id);
    }
    let root = scheduler.capture_source_root();
    let epoch_before = scheduler.source_directory().current_epoch();

    scheduler.reset();

    assert_eq!(
        scheduler.source_directory().current_epoch(),
        epoch_before + 1,
        "a reset is ONE batch transition, not one epoch per removed file",
    );
    for id in ["/a.vue", "/b.vue", "/c.vue"] {
        assert!(
            root.lookup(id).is_present(),
            "the pre-reset root keeps resolving {id}",
        );
        assert!(
            matches!(
                scheduler.capture_source_root().lookup(id),
                SourceStateAt::Absent { .. }
            ),
            "the post-reset root reads {id} as absent",
        );
    }
    scheduler.restart_driver();
}

/// The root's answer tracks `try_get_source`'s LOGICAL result at every
/// quiescent point of a realistic edit sequence — including the states
/// where the two disagree only because the root is older.
#[test]
fn the_current_root_agrees_with_try_get_source_at_every_quiescent_point() {
    let scheduler = hashing_scheduler(&[("/a.vue", "disk")]);

    let checkpoint = |label: &str| {
        let live = scheduler.try_get_source("/a.vue").map(|s| s.whole_hash);
        let root = scheduler
            .capture_source_root()
            .lookup("/a.vue")
            .whole_hash();
        assert_eq!(
            live, root,
            "current root disagreed with the live node at: {label}",
        );
    };

    checkpoint("before any load");
    load(&scheduler, "/a.vue");
    checkpoint("after the initial load");
    upsert(&scheduler, "/a.vue", "edited once");
    checkpoint("after an editor upsert");
    scheduler.invalidate("/a.vue");
    checkpoint("after invalidate");
    load(&scheduler, "/a.vue");
    checkpoint("after the reload");
    scheduler.remove("/a.vue");
    checkpoint("after remove");
}

/// A canonical that has been closed leaves NO residue.
///
/// Its retained history collapses to a single `Absent` version, which
/// every root either predates (already `Unknown`) or selects — and the
/// answer's sole consumer treats the two identically. Keeping the entry
/// anyway retains one `SourceVersion` plus one `Arc<str>` per canonical
/// the process ever published, for its whole lifetime.
#[test]
fn a_closed_canonical_leaves_no_retained_entry() {
    let directory = Arc::new(SchedulerSourceDirectory::new());
    let opened = canonical("/opened.ts");
    let never_loaded = canonical("/never_loaded.ts");

    directory.publish_transition(|p| p.present(&opened, 1, 1, hash(1)));
    directory.publish_transition(|p| p.absent(&opened, 1, 2));
    // A node created and removed without ever committing a source: one
    // ABSENT publication, no predecessor.
    directory.publish_transition(|p| p.absent(&never_loaded, 1, 1));

    assert_eq!(directory.retained_version_count("/opened.ts"), 2);
    assert_eq!(directory.retained_version_count("/never_loaded.ts"), 1);

    directory.reclaim_superseded_versions();

    assert_eq!(
        directory.retained_version_count("/opened.ts"),
        0,
        "a closed canonical's whole entry is reclaimed — its residue is a \
         per-canonical leak for the process lifetime",
    );
    assert_eq!(
        directory.retained_version_count("/never_loaded.ts"),
        0,
        "a canonical whose ONLY publication was ABSENT is reclaimed too — \
         it never had a predecessor to mark it superseded",
    );
    // The answer is unchanged for every consumer: both read as "no
    // source", before and after.
    let root = directory.capture_root();
    assert!(!root.lookup("/opened.ts").is_present());
    assert!(!root.lookup("/never_loaded.ts").is_present());

    // Discrimination: a canonical that still HAS a source keeps its entry.
    let live = canonical("/live.ts");
    directory.publish_transition(|p| p.present(&live, 1, 1, hash(9)));
    directory.publish_transition(|p| p.present(&live, 1, 2, hash(10)));
    directory.reclaim_superseded_versions();
    assert_eq!(
        directory.retained_version_count("/live.ts"),
        1,
        "a canonical with a live source keeps exactly its current version",
    );
    assert_eq!(
        directory.capture_root().lookup("/live.ts").whole_hash(),
        Some(hash(10)),
    );
}
