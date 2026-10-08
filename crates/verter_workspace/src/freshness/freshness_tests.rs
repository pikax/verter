//! Freshness-history soundness, retirement and concurrency contracts.
//!
//! The answer for a canonical must never fall below its true last
//! transition and never decrease; a leased reader must never be made
//! stale by unrelated retirement; and evidence no reader owns must drain.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Barrier};

use parking_lot::Mutex;
use rustc_hash::FxHashMap;

use super::{FreshnessHistory, FreshnessReaders};

/// A history plus the content-generation counter its recorders advance, the
/// way the engine drives it: every transition bumps, then records at the
/// bumped generation.
struct Fixture {
    history: Arc<FreshnessHistory>,
    current: AtomicU64,
}

impl Fixture {
    fn new(retire_trigger: usize) -> Self {
        Self {
            history: Arc::new(FreshnessHistory::with_retire_trigger(retire_trigger)),
            current: AtomicU64::new(1),
        }
    }

    fn readers(&self) -> FreshnessReaders {
        FreshnessReaders::new(Arc::clone(&self.history))
    }

    fn current(&self) -> u64 {
        self.current.load(Ordering::SeqCst)
    }

    fn touch(&self, canonical: &str) -> u64 {
        let generation = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        self.history
            .record_exact(canonical, generation, self.current());
        generation
    }

    fn touch_subtree(&self, prefix: &str) -> u64 {
        let generation = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        self.history
            .record_subtree(prefix, generation, self.current());
        generation
    }

    fn last(&self, canonical: &str) -> u64 {
        self.history.last_transition(canonical)
    }
}

#[test]
fn a_leased_artifact_stays_stale_after_a_directory_event_and_unrelated_retirement() {
    let fixture = Fixture::new(4);
    let readers = fixture.readers();

    // Two retained artifacts, built at the same generation.
    let built_at = fixture.current();
    let stale_lease = readers.lease_canonical("/src/pkg/a.ts");
    let fresh_lease = readers.lease_canonical("/lib/b.ts");
    // An unleased artifact for a canonical whose own entry will retire.
    let unleased_built_at = built_at;
    fixture.touch("/loose/c.ts");

    // The directory event that makes the first artifact stale.
    fixture.touch_subtree("/src/pkg");
    assert!(fixture.last("/src/pkg/a.ts") > built_at);
    assert!(fixture.last("/lib/b.ts") <= built_at);

    // Unrelated churn: enough unleased evidence to retire everything at or
    // below the current generation, the directory event included.
    for index in 0..64 {
        fixture.touch(&format!("/unrelated/{index}.ts"));
    }
    let residency = fixture.history.residency();
    assert_eq!(
        residency.subtree_entries, 0,
        "the directory event's own entry must have retired for this test to \
         prove the fold into the leased entry"
    );
    assert!(
        residency.floor > built_at,
        "retirement must have raised the floor past the artifacts' build \
         generation"
    );

    assert!(
        fixture.last("/src/pkg/a.ts") > built_at,
        "retiring the directory event must not make the leased artifact \
         under it fresh again"
    );
    assert!(
        fixture.last("/lib/b.ts") <= built_at,
        "unrelated retirement must not make an untouched leased artifact \
         stale"
    );
    assert!(
        fixture.last("/loose/c.ts") > unleased_built_at,
        "a retired exact entry must stay covered by the floor"
    );
    drop((stale_lease, fresh_lease));
}

#[test]
fn a_directory_event_only_stales_canonicals_under_its_prefix() {
    let fixture = Fixture::new(usize::MAX);
    let before = fixture.current();
    fixture.touch_subtree("/src/");
    for (canonical, under) in [
        ("/src", true),
        ("/src/a.ts", true),
        ("/src/deep/b.ts", true),
        ("/srcx.ts", false),
        ("/sr", false),
        ("/other/src/a.ts", false),
    ] {
        assert_eq!(
            fixture.last(canonical) > before,
            under,
            "subtree containment for {canonical}"
        );
    }

    fixture.touch_subtree("/");
    assert!(fixture.last("/other/src/a.ts") > before);
    assert!(fixture.last("/srcx.ts") > before);
}

#[test]
fn unique_path_churn_drains_once_readers_leave() {
    let fixture = Fixture::new(16);
    let readers = fixture.readers();

    // A live view caps retirement at its captured generation.
    let view = readers.lease_view(fixture.current());
    // Leased canonicals keep their entries whatever the churn.
    let leases: Vec<_> = (0..50)
        .map(|index| readers.lease_canonical(&format!("/leased/{index}.ts")))
        .collect();
    for index in 0..50 {
        fixture.touch(&format!("/leased/{index}.ts"));
    }
    for index in 0..2000 {
        fixture.touch(&format!("/churn/{index}.ts"));
        if index % 10 == 0 {
            fixture.touch_subtree(&format!("/churn/dir{index}"));
        }
    }
    let pinned = fixture.history.residency();
    assert_eq!(pinned.exact_entries, 2050);
    assert_eq!(pinned.leased_entries, 50);
    assert_eq!(pinned.subtree_entries, 200);
    assert_eq!(pinned.view_leases, 1);

    drop(view);
    let after_view = fixture.history.residency();
    assert_eq!(
        after_view.exact_entries, 50,
        "only the leased entries survive once the view leaves"
    );
    assert_eq!(after_view.subtree_entries, 0);
    assert!(
        after_view.exact_capacity * 4 <= pinned.exact_capacity,
        "drained exact capacity must be released ({} -> {})",
        pinned.exact_capacity,
        after_view.exact_capacity
    );
    assert!(after_view.subtree_capacity < pinned.subtree_capacity);

    drop(leases);
    let drained = fixture.history.residency();
    assert_eq!(drained.exact_entries, 0);
    assert_eq!(drained.queued_entries, 0);
    assert_eq!(drained.leased_entries, 0);
    // The released entries answer from the floor, still past their own
    // transitions.
    for index in 0..50 {
        assert!(fixture.last(&format!("/leased/{index}.ts")) >= 2);
    }
}

#[test]
fn a_view_lease_caps_retirement_at_its_captured_generation() {
    let fixture = Fixture::new(4);
    let readers = fixture.readers();
    let captured = fixture.current();
    let view = readers.lease_view(captured);
    for index in 0..32 {
        fixture.touch(&format!("/a/{index}.ts"));
    }
    assert!(fixture.history.residency().floor <= captured);
    assert!(
        fixture.last("/untouched.ts") <= captured,
        "an untouched canonical must stay fresh for the live view"
    );
    drop(view);
    assert!(
        fixture.last("/untouched.ts") > captured,
        "with the view gone the next retirement may raise the floor"
    );
}

/// Several canonicals recorded under ONE generation, the way a bulk upsert
/// records: retirement passes that run between the records raise the floor
/// to that generation, and every record must still answer exactly it, never
/// a future generation an artifact built at the live generation is refused
/// under — and the records drain without another bump.
#[test]
fn records_under_one_generation_never_run_past_it_across_retirement() {
    let fixture = Fixture::new(4);
    let generation = fixture.current.fetch_add(1, Ordering::SeqCst) + 1;
    let canonicals: Vec<String> = (0..10).map(|index| format!("/bulk/{index}.ts")).collect();
    for canonical in &canonicals {
        fixture
            .history
            .record_exact(canonical, generation, generation);
    }
    assert_eq!(fixture.history.residency().floor, generation);
    for canonical in &canonicals {
        assert_eq!(fixture.last(canonical), generation, "{canonical}");
    }
    assert!(
        fixture.history.residency().queued_entries < 4,
        "records at the live generation must retire as the queue fills"
    );
}

#[test]
fn a_record_never_lowers_an_answer() {
    let fixture = Fixture::new(usize::MAX);
    let canonical = "/src/a.ts";
    let touched = fixture.touch(canonical);
    // A covering subtree event newer than the exact entry, then an exact
    // record at an older generation: the answer keeps the newer evidence.
    let subtree = fixture.touch_subtree("/src");
    assert!(subtree > touched);
    fixture
        .history
        .record_exact(canonical, touched, fixture.current());
    assert_eq!(fixture.last(canonical), subtree);
}

/// Deterministic xorshift for the model test.
fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

#[test]
fn answers_are_monotone_and_never_below_the_true_last_transition() {
    let canonicals: Vec<String> = (0..24)
        .map(|index| format!("/d{}/e{}/f{index}.ts", index % 3, index % 5))
        .collect();
    let prefixes = ["/d0", "/d1/e1", "/d2", "/"];
    for trigger in [1, 3, 8] {
        let fixture = Fixture::new(trigger);
        let readers = fixture.readers();
        let mut leases: Vec<Option<super::CanonicalFreshnessLease>> =
            canonicals.iter().map(|_| None).collect();
        let mut views = Vec::new();
        let mut truth: FxHashMap<&str, u64> = FxHashMap::default();
        let mut previous: FxHashMap<&str, u64> = FxHashMap::default();
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64 ^ trigger as u64;
        for _ in 0..4000 {
            let pick = (next(&mut seed) % canonicals.len() as u64) as usize;
            match next(&mut seed) % 8 {
                0..=2 => {
                    let generation = fixture.touch(&canonicals[pick]);
                    truth.insert(canonicals[pick].as_str(), generation);
                }
                3 => {
                    let prefix = prefixes[(next(&mut seed) % prefixes.len() as u64) as usize];
                    let generation = fixture.touch_subtree(prefix);
                    for canonical in &canonicals {
                        if crate::path_matches_prefix(canonical, prefix) {
                            truth.insert(canonical.as_str(), generation);
                        }
                    }
                }
                4 => leases[pick] = Some(readers.lease_canonical(&canonicals[pick])),
                5 => leases[pick] = None,
                6 => views.push(readers.lease_view(fixture.current())),
                _ => {
                    if !views.is_empty() {
                        let index = (next(&mut seed) % views.len() as u64) as usize;
                        views.swap_remove(index);
                    }
                }
            }
            for canonical in &canonicals {
                let answer = fixture.last(canonical);
                let true_last = truth.get(canonical.as_str()).copied().unwrap_or(0);
                assert!(
                    answer >= true_last,
                    "{canonical} answered {answer} below its true last transition \
                     {true_last} (trigger {trigger})"
                );
                let before = previous.insert(canonical.as_str(), answer).unwrap_or(0);
                assert!(
                    answer >= before,
                    "{canonical} moved backwards {before} -> {answer} (trigger {trigger})"
                );
            }
        }
    }
}

/// Writers, a lease/view thread and readers run together. Every thread
/// starts behind one barrier, so the readers are live before the first
/// record. The writers stop halfway and again at their end until both readers
/// have completed a checked read there, so each reader's checks span the
/// retirements the writers' second half runs. The readers stop only once
/// every writer and the lease thread has been joined. A reader records a
/// violation instead of panicking, so it keeps its barrier appointments and
/// a failure reports rather than hangs.
#[test]
fn concurrent_churn_never_stales_a_leased_untouched_artifact_or_lowers_an_answer() {
    const WRITERS: usize = 3;
    const READERS: usize = 2;
    const RECORDS: usize = 1500;
    let fixture = Arc::new(Fixture::new(8));
    let readers = fixture.readers();
    let pinned_built_at = fixture.current();
    let pinned = readers.lease_canonical("/pinned/x.ts");
    let truth: Arc<Mutex<FxHashMap<String, u64>>> = Arc::default();
    let stop = Arc::new(AtomicBool::new(false));
    let start = Arc::new(Barrier::new(WRITERS + 1 + READERS));
    let midway = Arc::new(Barrier::new(WRITERS + READERS));
    let finished = Arc::new(Barrier::new(WRITERS + READERS));

    let observed: Vec<(Vec<u64>, Vec<String>)> = std::thread::scope(|scope| {
        let mut producers = Vec::new();
        for writer in 0..WRITERS {
            let fixture = Arc::clone(&fixture);
            let truth = Arc::clone(&truth);
            let (start, midway, finished) = (
                Arc::clone(&start),
                Arc::clone(&midway),
                Arc::clone(&finished),
            );
            producers.push(scope.spawn(move || {
                start.wait();
                for index in 0..RECORDS {
                    if index == RECORDS / 2 {
                        midway.wait();
                    }
                    let canonical = format!("/w{writer}/{}.ts", index % 40);
                    // Hold the model lock across the record so the model's
                    // generation for a key is the one the history last saw.
                    let mut truth = truth.lock();
                    let generation = fixture.touch(&canonical);
                    truth.insert(canonical, generation);
                    if index % 97 == 0 {
                        fixture.touch_subtree(&format!("/w{writer}/sub{index}"));
                    }
                }
                finished.wait();
            }));
        }
        {
            let fixture = Arc::clone(&fixture);
            let readers = readers.clone();
            let start = Arc::clone(&start);
            producers.push(scope.spawn(move || {
                start.wait();
                for index in 0..RECORDS {
                    let lease = readers.lease_canonical(&format!("/w0/{}.ts", index % 40));
                    let view = readers.lease_view(fixture.current());
                    drop((lease, view));
                }
            }));
        }
        let consumers: Vec<_> = (0..READERS)
            .map(|reader| {
                let fixture = Arc::clone(&fixture);
                let stop = Arc::clone(&stop);
                let (start, midway, finished) = (
                    Arc::clone(&start),
                    Arc::clone(&midway),
                    Arc::clone(&finished),
                );
                scope.spawn(move || {
                    let mut previous: FxHashMap<String, u64> = FxHashMap::default();
                    let mut floors = Vec::new();
                    let mut violations = Vec::new();
                    let mut handshakes = [&midway, &finished].into_iter();
                    start.wait();
                    loop {
                        let stopping = stop.load(Ordering::Acquire);
                        floors.push(fixture.history.residency().floor);
                        let pinned_answer = fixture.last("/pinned/x.ts");
                        if pinned_answer > pinned_built_at {
                            violations.push(format!(
                                "unrelated concurrent churn made a leased untouched artifact \
                                 stale: {pinned_answer} > {pinned_built_at}"
                            ));
                        }
                        for index in 0..40 {
                            let canonical = format!("/w{}/{index}.ts", reader % WRITERS);
                            let answer = fixture.last(&canonical);
                            let before = previous.insert(canonical.clone(), answer).unwrap_or(0);
                            if answer < before {
                                violations.push(format!(
                                    "{canonical} moved backwards {before} -> {answer}"
                                ));
                            }
                        }
                        if let Some(handshake) = handshakes.next() {
                            handshake.wait();
                        } else if stopping {
                            break;
                        }
                    }
                    (floors, violations)
                })
            })
            .collect();
        for producer in producers {
            producer.join().expect("writer or lease thread");
        }
        stop.store(true, Ordering::Release);
        consumers
            .into_iter()
            .map(|consumer| consumer.join().expect("reader"))
            .collect()
    });

    for (floors, violations) in &observed {
        assert!(violations.is_empty(), "{violations:#?}");
        assert!(
            floors.first() < floors.last(),
            "each reader's checked reads must span a retirement: {:?} .. {:?}",
            floors.first(),
            floors.last()
        );
    }
    for (canonical, generation) in truth.lock().iter() {
        assert!(fixture.last(canonical) >= *generation);
    }
    assert!(fixture.last("/pinned/x.ts") <= pinned_built_at);
    drop(pinned);
    drop(readers);
    let residency = fixture.history.residency();
    assert_eq!(residency.leased_entries, 0);
    assert_eq!(residency.view_leases, 0);
    assert!(
        residency.exact_entries + residency.subtree_entries < 16,
        "unleased churn must have retired: {residency:?}"
    );
}

#[cfg(feature = "semantic-observe")]
#[test]
fn ancestor_lookup_work_is_bounded_by_depth_not_by_recorded_directories() {
    let fixture = Fixture::new(usize::MAX);
    for index in 0..500 {
        fixture.touch_subtree(&format!("/tree/dir{index}"));
    }
    let before = fixture.readers().observe_snapshot().ancestor_probes;
    let _ = fixture.last("/tree/dir7/deep/file.ts");
    let probes = fixture.readers().observe_snapshot().ancestor_probes - before;
    // The canonical itself, `` (root), `/tree`, `/tree/dir7`,
    // `/tree/dir7/deep`.
    assert_eq!(probes, 5);
    assert!(fixture.last("/tree/dir7/deep/file.ts") > 1);
}

#[cfg(feature = "semantic-observe")]
#[test]
fn retiring_a_subtree_visits_only_the_leased_canonicals_under_it() {
    let fixture = Fixture::new(16);
    let readers = fixture.readers();
    let elsewhere: Vec<_> = (0..500)
        .map(|index| readers.lease_canonical(&format!("/other/deep/dir/{index}.ts")))
        .collect();
    let under: Vec<_> = ["/tree/a/x.ts", "/tree/a/deep/y.ts"]
        .into_iter()
        .map(|canonical| readers.lease_canonical(canonical))
        .collect();
    let sibling = readers.lease_canonical("/tree/a-sibling.ts");
    let built_at = fixture.current();

    // A view holds the subtree and the unleased churn in the queue.
    let view = readers.lease_view(built_at);
    fixture.touch_subtree("/tree/a");
    for index in 0..20 {
        fixture.touch(&format!("/loose/{index}.ts"));
    }
    assert_eq!(fixture.history.residency().subtree_entries, 1);

    let before = readers.observe_snapshot().ancestor_probes;
    drop(view);
    let probes = readers.observe_snapshot().ancestor_probes - before;
    assert_eq!(fixture.history.residency().subtree_entries, 0);
    // One range seek for the retired subtree plus one visit per leased
    // canonical under it; the 501 other leased canonicals are never touched.
    assert_eq!(probes, 3);

    assert!(fixture.last("/tree/a/x.ts") > built_at);
    assert!(fixture.last("/tree/a/deep/y.ts") > built_at);
    assert!(fixture.last("/tree/a-sibling.ts") <= built_at);
    assert!(fixture.last("/other/deep/dir/7.ts") <= built_at);
    drop((elsewhere, under, sibling));
}
