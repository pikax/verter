//! Scheduler-side lazy declaration-lowering service.
//!
//! The OXC eval-program parse (`ParsedEvalProgram`) is a `self_cell` over
//! an arena allocator — `!Send`, so it can never enter a host-owned
//! `Send + Sync` cache. This service gives the lazy declaration-body path
//! a retained parse WITHOUT violating that rule: a small set of dedicated
//! worker threads each OWN the parsed snapshots for their key shard, and
//! callers submit pure lowering jobs that borrow the retained AST on the
//! worker, returning OWNED typed IR. The snapshot never crosses a thread
//! boundary; only `Send` job inputs and owned results do.
//!
//! Retention is LEASE-PINNED, not LRU/budget-evicted. A snapshot is
//! retained for a [`SnapshotKey`] — `(canonical, whole_hash,
//! parse_env_hash)`, the file-content generation identity — exactly as
//! long as a live [`SnapshotLease`] holds it. Each owning [`DeclBodyMemo`]
//! acquires ONE lease for its key on first body demand and drops it when
//! the memo (hence its `IndexedReady` artifact) is dropped, so the
//! retained parse lives precisely as long as the live artifact that reads
//! from it — a live artifact can never silently re-parse. A lowering job
//! run while the lease is live reuses the pinned snapshot
//! (`parsed_now == false`); a run with no live lease for the key parses
//! transiently and retains nothing. The key is content-addressed by
//! construction, so a content edit produces a new key (and a fresh memo
//! with a fresh lease) — a superseded snapshot can never answer a
//! new-content demand.
//!
//! Callers BLOCK on the job result (a rendezvous channel — cooperative
//! blocking, no spinning). Jobs must be PURE: no host calls, no service
//! re-entry — a job that submitted a sub-job could deadlock its own
//! worker. A panicking job is caught on the worker and re-raised on the
//! calling thread, so one bad job cannot kill a shard.
//!
//! On `wasm32` there are no worker threads, and `ParsedEvalProgram` is
//! `!Send` (it holds an `Rc<…>` self-cell over an arena). The retained
//! snapshot therefore can never live on a worker, AND it can never be a
//! field of `DeclLoweringService` — the service is held inside the
//! host-owned `Send + Sync` artifact structures, so a non-`Sync`
//! `RefCell<SnapshotShard>` field would poison those bounds (and we do
//! NOT paper over that with `unsafe impl Send`/`Sync`). Instead, on
//! `wasm32` the service is FIELDLESS and the retained shard lives in a
//! wasm-only thread-local (`WASM_DECL_LOWERING_SHARD`). wasm is
//! single-threaded, so one thread-local shard IS the whole retention
//! window. The lowering job runs inline on the calling thread against
//! that thread-local shard — reusing the SAME `SnapshotShard` /
//! `ShardEntry` / lease-pinned retention logic the native workers use.
//! Same-key demands under a live lease reuse the retained parse instead
//! of re-parsing; only the threading/storage substrate differs by
//! platform (native: worker-owned per shard; wasm: single thread-local
//! shard).

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_semantic::analysis::Hash16;

/// Content-generation identity of one retained parse snapshot: the
/// canonical file, its whole-content hash, and the R21 parse-env
/// dimension the parse runs under. Content-addressed by construction —
/// an edit produces a new key, so a stale snapshot can never answer a
/// new-content demand.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SnapshotKey {
    pub canonical: Arc<str>,
    pub whole_hash: Hash16,
    pub parse_env_hash: Hash16,
}

/// One lowering-service call's result: the job's owned value plus
/// whether the service had to parse (vs. serving the retained snapshot).
/// Test-only, like its sole producer [`DeclLoweringService::run`]:
/// production accounts parses at the lease boundary
/// ([`LeaseOutcome::parsed_now`]) and demands bodies through the
/// lease-only `run_leased`.
#[cfg(test)]
pub(crate) struct LoweringOutcome<R> {
    pub value: R,
    /// Whether this `run` had to parse — the reuse/transient
    /// discriminator the `decl_lowering` unit tests assert on to prove
    /// the lease-pinning retention contract.
    pub parsed_now: bool,
}

/// Outcome of acquiring a [`SnapshotLease`]: the lease token (drop it to
/// release the retained snapshot) plus whether acquiring it had to parse
/// (vs. bumping the refcount on an already-retained snapshot).
pub(crate) struct LeaseOutcome {
    pub lease: SnapshotLease,
    pub parsed_now: bool,
}

/// A live pin on the retained parse snapshot for one [`SnapshotKey`].
///
/// Holding a lease keeps the worker (native) / thread-local (wasm)
/// retained snapshot alive; dropping it decrements the key's refcount and
/// — at zero — drops the retained `Rc<ParsedEvalProgram>`. The lease is
/// `Send + Sync` (it holds only `Arc<DeclLoweringService>` + the owned
/// key), so it can live in a host-owned `Send + Sync` artifact (the
/// `DeclBodyMemo`).
pub(crate) struct SnapshotLease {
    key: SnapshotKey,
    service: Arc<DeclLoweringService>,
}

impl std::fmt::Debug for SnapshotLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapshotLease")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl Drop for SnapshotLease {
    fn drop(&mut self) {
        self.service.release_key(&self.key);
    }
}

#[cfg(not(target_arch = "wasm32"))]
type WorkerJob = Box<dyn FnOnce(&mut SnapshotShard) + Send>;

#[cfg(target_arch = "wasm32")]
std::thread_local! {
    /// wasm-only retained-parse shard. wasm is single-threaded, so this
    /// one thread-local shard is the entire retention window — it is
    /// deliberately NOT a field of `DeclLoweringService`, because the
    /// service must stay `Send + Sync` for the host-owned artifact
    /// caches and `SnapshotShard` (holding `Rc<ParsedEvalProgram>`) is
    /// neither.
    static WASM_DECL_LOWERING_SHARD: std::cell::RefCell<SnapshotShard> =
        std::cell::RefCell::new(SnapshotShard::new());
}

/// The `!Send` retained-parse state. On native, one worker thread owns
/// one shard; on `wasm32`, a single thread-local shard
/// (`WASM_DECL_LOWERING_SHARD`) holds it — never a service field.
///
/// Retention is lease-pinned: an entry exists exactly while at least one
/// live [`SnapshotLease`] refcounts the key. There is NO count/byte
/// budget and NO eviction — a live artifact's snapshot can never be
/// dropped out from under it.
struct SnapshotShard {
    entries: FxHashMap<SnapshotKey, ShardEntry>,
}

struct ShardEntry {
    /// `None` records a FATAL parse (panicked) — retained (while leased)
    /// so repeated demands against the same broken content do not
    /// re-parse it.
    parsed: Option<std::rc::Rc<crate::ParsedEvalProgram>>,
    /// Live lease count. The entry is removed (and its `Rc` dropped) when
    /// this reaches zero.
    refcount: usize,
}

impl SnapshotShard {
    fn new() -> Self {
        Self {
            entries: FxHashMap::default(),
        }
    }

    /// Pin the snapshot for `key`: parse it if not already retained,
    /// otherwise bump the refcount on the existing entry. Returns
    /// whether this acquisition had to parse.
    fn acquire(
        &mut self,
        key: &SnapshotKey,
        source: &Arc<str>,
        source_type: oxc_span::SourceType,
    ) -> bool {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.refcount += 1;
            return false;
        }
        let parsed =
            crate::ParsedEvalProgram::parse(Arc::clone(source), source_type).map(std::rc::Rc::new);
        self.entries.insert(
            key.clone(),
            ShardEntry {
                parsed,
                refcount: 1,
            },
        );
        true
    }

    /// Release one pin on `key`. At refcount zero the entry — and its
    /// retained `Rc` — is dropped.
    fn release(&mut self, key: &SnapshotKey) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.refcount = entry.refcount.saturating_sub(1);
            if entry.refcount == 0 {
                self.entries.remove(key);
            }
        }
    }

    /// Snapshot to run a job against: reuse the lease-pinned snapshot if
    /// one is retained for `key` (`parsed_now == false`), otherwise parse
    /// transiently and retain NOTHING (retention requires a lease).
    /// Test-only, like its sole caller [`DeclLoweringService::run`] — the
    /// parse-capable path exists only to exercise the retention contract.
    #[cfg(test)]
    fn snapshot_for_run(
        &self,
        key: &SnapshotKey,
        source: &Arc<str>,
        source_type: oxc_span::SourceType,
    ) -> (Option<std::rc::Rc<crate::ParsedEvalProgram>>, bool) {
        if let Some(entry) = self.entries.get(key) {
            return (entry.parsed.clone(), false);
        }
        let parsed =
            crate::ParsedEvalProgram::parse(Arc::clone(source), source_type).map(std::rc::Rc::new);
        (parsed, true)
    }

    /// Snapshot for a LEASE-ONLY run: the retained snapshot for `key` when a
    /// live lease pins it, or `None` on a lease MISS. This method NEVER parses —
    /// it takes no `source` — so a caller that routes through it (span recovery)
    /// is structurally incapable of triggering a transient re-parse. The outer
    /// `Option` distinguishes a lease miss (`None`) from a retained-but-fatal
    /// parse (`Some(None)`).
    fn snapshot_leased(
        &self,
        key: &SnapshotKey,
    ) -> Option<Option<std::rc::Rc<crate::ParsedEvalProgram>>> {
        self.entries.get(key).map(|entry| entry.parsed.clone())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn shard_index(key: &SnapshotKey, worker_count: usize) -> usize {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    key.hash(&mut hasher);
    (hasher.finish() as usize) % worker_count
}

/// The lazy declaration-lowering service. See module docs.
pub(crate) struct DeclLoweringService {
    #[cfg(not(target_arch = "wasm32"))]
    workers: Vec<std::sync::mpsc::Sender<WorkerJob>>,
    // On `wasm32` the service is FIELDLESS: the retained shard lives in
    // the `WASM_DECL_LOWERING_SHARD` thread-local, never here, so the
    // service stays `Send + Sync` without any `unsafe impl`.
}

impl std::fmt::Debug for DeclLoweringService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeclLoweringService")
            .finish_non_exhaustive()
    }
}

impl DeclLoweringService {
    pub(crate) fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let worker_count = std::thread::available_parallelism()
                .map(|n| (n.get() / 4).clamp(1, 4))
                .unwrap_or(2);
            Self::with_workers(worker_count)
        }
        #[cfg(target_arch = "wasm32")]
        {
            // Fieldless on wasm — retention lives in the
            // `WASM_DECL_LOWERING_SHARD` thread-local.
            Self {}
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn with_workers(worker_count: usize) -> Self {
        let worker_count = worker_count.max(1);
        let mut workers = Vec::with_capacity(worker_count);
        for index in 0..worker_count {
            let (tx, rx) = std::sync::mpsc::channel::<WorkerJob>();
            // 8 MiB stack, matching the host CPU pool: lowering
            // recursion over deeply nested type bodies must not
            // regress stack capacity vs. the former inline path.
            std::thread::Builder::new()
                .name(format!("verter-decl-lower-{index}"))
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    let mut shard = SnapshotShard::new();
                    while let Ok(job) = rx.recv() {
                        job(&mut shard);
                    }
                })
                .expect("failed to spawn decl-lowering worker");
            workers.push(tx);
        }
        Self { workers }
    }

    /// Test-only single-worker constructor: forces every key onto one
    /// shard so retention tests are deterministic regardless of host
    /// parallelism.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn new_single_worker() -> Self {
        Self::with_workers(1)
    }

    /// Acquire a [`SnapshotLease`] pinning the retained parse for `key`.
    /// While the returned lease is live, [`Self::run_leased`] calls for
    /// `key` serve the retained snapshot (and MISS, never parse, without
    /// one).
    pub(crate) fn acquire_lease(
        self: &Arc<Self>,
        key: &SnapshotKey,
        source: &Arc<str>,
        source_type: oxc_span::SourceType,
    ) -> LeaseOutcome {
        #[cfg(not(target_arch = "wasm32"))]
        let parsed_now = {
            let shard_index = shard_index(key, self.workers.len());
            let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
            let key_for_job = key.clone();
            let source = Arc::clone(source);
            let job: WorkerJob = Box::new(move |shard| {
                let parsed_now = shard.acquire(&key_for_job, &source, source_type);
                let _ = result_tx.send(parsed_now);
            });
            self.workers[shard_index]
                .send(job)
                .expect("decl-lowering worker channel must outlive the service");
            result_rx
                .recv()
                .expect("decl-lowering worker must answer every acquire")
        };
        #[cfg(target_arch = "wasm32")]
        let parsed_now = WASM_DECL_LOWERING_SHARD
            .with(|cell| cell.borrow_mut().acquire(key, source, source_type));

        LeaseOutcome {
            lease: SnapshotLease {
                key: key.clone(),
                service: Arc::clone(self),
            },
            parsed_now,
        }
    }

    /// Release one pin on `key`. Fire-and-forget: dropping a lease must
    /// not block, and a worker that has already shut down (service
    /// teardown) simply drops the release.
    fn release_key(&self, key: &SnapshotKey) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let shard_index = shard_index(key, self.workers.len());
            let key_for_job = key.clone();
            let job: WorkerJob = Box::new(move |shard| shard.release(&key_for_job));
            // Ignore a send error: the only way the channel is closed is
            // the worker (and its shard, including this key's entry) is
            // already gone.
            let _ = self.workers[shard_index].send(job);
        }
        #[cfg(target_arch = "wasm32")]
        {
            WASM_DECL_LOWERING_SHARD.with(|cell| cell.borrow_mut().release(key));
        }
    }

    /// Run `job` against the (lease-retained or freshly parsed) eval
    /// program for `key`, blocking until the owned result is back. `job`
    /// receives `None` when the parse is fatal (panicked). When a live
    /// lease pins `key`, the retained snapshot is reused
    /// (`parsed_now == false`); otherwise the parse is transient and
    /// retained nowhere.
    ///
    /// **`job` MUST be a PURE lowering closure**: it borrows the retained
    /// AST and returns OWNED typed IR, and it must NOT re-enter this
    /// service (`run` / `acquire_lease` / `release`) nor call back into the
    /// host. Re-entry is a deadlock/panic hazard on BOTH platforms:
    /// - native — a job runs ON the worker thread that owns its shard;
    ///   submitting a sub-job to the same shard from inside the job blocks
    ///   that worker on a rendezvous it can never service (self-deadlock);
    /// - `wasm32` — the job runs inline while a shared
    ///   `WASM_DECL_LOWERING_SHARD.borrow()` is held, so a re-entrant
    ///   `acquire_lease` / `release` (`borrow_mut`) panics the `RefCell`.
    ///
    /// The whole lazy-body design upholds this: `DeclBodyMemo` acquires its
    /// lease BEFORE the run and the lowering arms only build typed IR, so a
    /// job never reaches back into the service.
    ///
    /// PARSE-CAPABLE by design — and therefore TEST-ONLY: `DeclBodyMemo`
    /// routes every production demand through the lease-only
    /// [`Self::run_leased`], and gating this method `#[cfg(test)]` makes
    /// the transient-parse hole structurally un-reopenable in production
    /// builds. The unit tests below exercise the transient/retention
    /// contract through it (a same-key run under a live lease reuses the
    /// retained snapshot; without one it parses transiently and retains
    /// nothing).
    #[cfg(test)]
    pub(crate) fn run<R, F>(
        &self,
        key: &SnapshotKey,
        source: &Arc<str>,
        source_type: oxc_span::SourceType,
        job: F,
    ) -> LoweringOutcome<R>
    where
        F: FnOnce(Option<&crate::ParsedEvalProgram>) -> R + Send + 'static,
        R: Send + 'static,
    {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::panic::AssertUnwindSafe;

            let shard_index = shard_index(key, self.workers.len());
            let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
            let key = key.clone();
            let source = Arc::clone(source);
            let worker_job: WorkerJob = Box::new(move |shard| {
                let (parsed, parsed_now) = shard.snapshot_for_run(&key, &source, source_type);
                // Catch a job panic on the worker and re-raise it on the
                // caller — the worker thread (and its retained shard)
                // must survive a bad job.
                let value = std::panic::catch_unwind(AssertUnwindSafe(|| job(parsed.as_deref())));
                let _ = result_tx.send(value.map(|value| LoweringOutcome { value, parsed_now }));
            });
            self.workers[shard_index]
                .send(worker_job)
                .expect("decl-lowering worker channel must outlive the service");
            match result_rx
                .recv()
                .expect("decl-lowering worker must answer every job")
            {
                Ok(outcome) => outcome,
                Err(panic_payload) => std::panic::resume_unwind(panic_payload),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            // Single-threaded platform: no worker threads, and the
            // `Rc` parse is `!Send`, so the retained shard lives in the
            // `WASM_DECL_LOWERING_SHARD` thread-local (NOT a service
            // field). Same lease-pinned retention logic as the native
            // workers — a same-key run under a live lease reuses the
            // retained snapshot (`parsed_now == false`) via
            // `snapshot_for_run` instead of re-parsing. Both `parsed_now`
            // and `value` come from the real shard hit/miss, never a
            // hardcoded fresh-parse flag.
            WASM_DECL_LOWERING_SHARD.with(|cell| {
                let shard = cell.borrow();
                let (parsed, parsed_now) = shard.snapshot_for_run(key, source, source_type);
                let value = job(parsed.as_deref());
                LoweringOutcome { value, parsed_now }
            })
        }
    }

    /// Test-only out-of-band release of the retained snapshot pin for
    /// `key`. Used to BREAK the lease invariant (the memo still holds its
    /// `SnapshotLease`, but the worker-side retained entry is gone) and
    /// prove the memo's demanded-lowering path fails CLOSED instead of
    /// transiently re-parsing.
    #[cfg(test)]
    pub(crate) fn release_retained_snapshot_for_test(&self, key: &SnapshotKey) {
        self.release_key(key);
    }

    /// Run `job` against the LEASE-RETAINED eval program for `key`, WITHOUT ever
    /// parsing. On a lease MISS (no retained snapshot) returns `None` and `job`
    /// does NOT run — this path is structurally incapable of a transient parse,
    /// so a caller on it (span recovery, the decl-body memo's demanded
    /// lowering) can never violate the retained-parse invariant. When a
    /// retained snapshot exists, `job` runs with `Some(&program)`
    /// (or `None` for a fatal parse) and the result is `Some(job_result)`.
    ///
    /// Unlike the test-only parse-capable `run`, this takes NO `source` — a lease miss cannot be
    /// papered over by re-parsing. `job` MUST still be a PURE lowering closure
    /// (no host / service re-entry), per the same worker-purity contract.
    pub(crate) fn run_leased<R, F>(&self, key: &SnapshotKey, job: F) -> Option<R>
    where
        F: FnOnce(Option<&crate::ParsedEvalProgram>) -> R + Send + 'static,
        R: Send + 'static,
    {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::panic::AssertUnwindSafe;

            let shard_index = shard_index(key, self.workers.len());
            let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
            let key = key.clone();
            let worker_job: WorkerJob = Box::new(move |shard| {
                // `None` = lease miss (job does not run, nothing is parsed);
                // `Some(catch_unwind(..))` = the retained snapshot ran the job.
                let result = shard.snapshot_leased(&key).map(|parsed| {
                    std::panic::catch_unwind(AssertUnwindSafe(|| job(parsed.as_deref())))
                });
                let _ = result_tx.send(result);
            });
            self.workers[shard_index]
                .send(worker_job)
                .expect("decl-lowering worker channel must outlive the service");
            match result_rx
                .recv()
                .expect("decl-lowering worker must answer every job")
            {
                None => None,
                Some(Ok(value)) => Some(value),
                Some(Err(panic_payload)) => std::panic::resume_unwind(panic_payload),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            WASM_DECL_LOWERING_SHARD.with(|cell| {
                let shard = cell.borrow();
                shard
                    .snapshot_leased(key)
                    .map(|parsed| job(parsed.as_deref()))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(canonical: &str, hash_byte: u8) -> SnapshotKey {
        SnapshotKey {
            canonical: Arc::from(canonical),
            whole_hash: [hash_byte; 16],
            parse_env_hash: [0u8; 16],
        }
    }

    /// Two jobs against the SAME key under a LIVE LEASE share one
    /// retained parse: the second call must observe `parsed_now == false`
    /// and the same program content. A regressed per-call parse would
    /// report `parsed_now == true` twice.
    #[test]
    fn leased_key_jobs_share_one_retained_parse() {
        let service = Arc::new(DeclLoweringService::new());
        let source: Arc<str> = Arc::from("type A = { a: 1 };\ntype B = { b: 2 };\n");
        let k = key("/ws/a.ts", 1);
        let st = oxc_span::SourceType::ts();

        let lease = service.acquire_lease(&k, &source, st);
        assert!(lease.parsed_now, "acquiring the lease parses once");

        let first = service.run(&k, &source, st, |program| {
            program
                .expect("parse must succeed")
                .borrow_dependent()
                .body
                .len()
        });
        assert!(
            !first.parsed_now,
            "a run under a live lease reuses the retained snapshot"
        );
        assert_eq!(first.value, 2);

        let second = service.run(&k, &source, st, |program| {
            program
                .expect("parse must succeed")
                .borrow_dependent()
                .body
                .len()
        });
        assert!(
            !second.parsed_now,
            "warm call must reuse the retained snapshot — NOT re-parse"
        );
        assert_eq!(second.value, 2);

        drop(lease.lease);
    }

    /// `run_leased` NEVER parses: on a lease miss it returns `None` (the job
    /// does not run), under a live lease it runs against the retained snapshot,
    /// and after the lease drops it misses again. A regressed implementation
    /// that re-parsed on a miss would return `Some(true)` on the first/last
    /// probe and fail this test.
    #[test]
    fn run_leased_never_parses_and_misses_without_a_live_lease() {
        let service = Arc::new(DeclLoweringService::new());
        let source: Arc<str> = Arc::from("type A = { a: 1 };\n");
        let k = key("/ws/leased-only.ts", 1);
        let st = oxc_span::SourceType::ts();

        // No live lease → miss (NOT a transient parse).
        let miss = service.run_leased(&k, |program| program.is_some());
        assert!(
            miss.is_none(),
            "run_leased must MISS (never parse) with no live lease"
        );

        // A live lease pins the snapshot → run_leased runs the job against it.
        let lease = service.acquire_lease(&k, &source, st);
        assert!(lease.parsed_now, "acquiring the lease parses once");
        let hit = service.run_leased(&k, |program| {
            program
                .expect("the retained snapshot must be present under a live lease")
                .borrow_dependent()
                .body
                .len()
        });
        assert_eq!(
            hit,
            Some(1),
            "run_leased runs against the retained snapshot"
        );

        // After the lease drops, retention is released → miss again, no reparse.
        drop(lease.lease);
        let after = service.run_leased(&k, |program| program.is_some());
        assert!(
            after.is_none(),
            "run_leased misses after the lease drops — it never re-parses"
        );
    }

    /// An UN-LEASED run parses transiently and retains nothing: a second
    /// un-leased run of the same key parses again. Retention requires a
    /// lease.
    #[test]
    fn unleased_run_is_transient() {
        let service = Arc::new(DeclLoweringService::new());
        let source: Arc<str> = Arc::from("type A = 1;\n");
        let k = key("/ws/unleased.ts", 9);
        let st = oxc_span::SourceType::ts();

        let first = service.run(&k, &source, st, |p| p.is_some());
        let second = service.run(&k, &source, st, |p| p.is_some());
        assert!(first.parsed_now && first.value);
        assert!(
            second.parsed_now,
            "with no live lease, each run parses transiently — retention \
             requires a lease"
        );
    }

    /// A live lease pins the snapshot across MANY distinct other keys —
    /// there is no count/byte budget and no eviction, so a re-run of the
    /// leased key still reuses its retained parse. A reintroduced
    /// LRU/budget cap would evict the leased key and force a re-parse.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn live_lease_pins_snapshot_across_many_other_keys() {
        // Single worker so every key shares one shard — deterministic
        // regardless of host parallelism.
        let service = Arc::new(DeclLoweringService::new_single_worker());
        let st = oxc_span::SourceType::ts();
        let k1 = key("/ws/k1.ts", 1);
        let src1: Arc<str> = Arc::from("type A = { a: 1 };\n");

        let lease1 = service.acquire_lease(&k1, &src1, st);
        assert!(lease1.parsed_now, "leasing k1 parses it once");

        // Hold leases on 16 other distinct keys (more than any plausible
        // former per-worker snapshot cap).
        let mut others = Vec::new();
        for i in 0..16u8 {
            let k = key(&format!("/ws/other{i}.ts"), i.wrapping_add(50));
            let src: Arc<str> = Arc::from(format!("type T{i} = {{ v: {i} }};\n"));
            others.push(service.acquire_lease(&k, &src, st));
        }

        // k1 is still pinned by its live lease — re-running it reuses the
        // retained snapshot.
        let outcome = service.run(&k1, &src1, st, |program| program.is_some());
        assert!(
            !outcome.parsed_now,
            "a live lease must pin k1's snapshot across 16 other keys — \
             no LRU/budget eviction is allowed"
        );
        assert!(outcome.value);

        drop(lease1.lease);
        drop(others);
    }

    /// The retained snapshot for a live-leased key is released EXCLUSIVELY
    /// by dropping its own lease (the RAII memo-drop). No production
    /// eviction, release, or budget path exists: a heavy battery of
    /// other-key traffic — acquire-and-DROP many other keys (the release
    /// path runs for each) interleaved with leased runs of those keys (the
    /// access path) — leaves the leased key pinned across ALL of it, and
    /// ONLY dropping its lease releases it. A production eviction / release
    /// / budget path wired into any of that other-key acquire / release /
    /// access traffic would drop the leased snapshot and force a re-parse,
    /// failing this test. This is the companion to
    /// `live_lease_pins_snapshot_across_many_other_keys` (which pins the
    /// budget/eviction-on-acquire axis): together they close the eviction,
    /// access, and release axes of the lease-liveness invariant.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn retained_snapshot_release_is_lease_drop_only() {
        // Single worker so every key shares one shard and job order is
        // deterministic regardless of host parallelism.
        let service = Arc::new(DeclLoweringService::new_single_worker());
        let st = oxc_span::SourceType::ts();
        let k1 = key("/ws/pinned.ts", 1);
        let src1: Arc<str> = Arc::from("type Pinned = { a: 1 };\n");

        let lease1 = service.acquire_lease(&k1, &src1, st);
        assert!(lease1.parsed_now, "leasing k1 parses it once");

        // Churn 32 other keys through the FULL acquire -> access -> release
        // lifecycle each — every hook a production eviction / release /
        // budget path could attach to. Each other lease drops before the
        // next is acquired, so the release path fires 32 times while k1
        // stays leased.
        for i in 0..32u8 {
            let k = key(&format!("/ws/churn{i}.ts"), i.wrapping_add(80));
            let src: Arc<str> = Arc::from(format!("type C{i} = {{ v: {i} }};\n"));
            let other = service.acquire_lease(&k, &src, st);
            // Access the just-leased other key (the access path), then drop
            // its lease (the release path). Neither must touch k1's entry.
            let hit = service.run_leased(&k, |program| program.is_some());
            assert_eq!(hit, Some(true), "the just-leased other key is retained");
            drop(other.lease);
        }

        // k1 is STILL pinned by its live lease across all that release and
        // access traffic — no eviction, release, or budget path released it.
        let warm = service.run(&k1, &src1, st, |program| program.is_some());
        assert!(
            !warm.parsed_now,
            "the leased snapshot must survive all other-key release/access \
             traffic — release is reachable ONLY by dropping k1's own lease"
        );
        assert!(warm.value);

        // Dropping k1's lease is the SOLE release path: the next run
        // re-parses fresh.
        drop(lease1.lease);
        let after = service.run(&k1, &src1, st, |program| program.is_some());
        assert!(
            after.parsed_now,
            "dropping the last lease is the sole release path — the next run \
             re-parses"
        );
        assert!(after.value);
    }

    /// Dropping the LAST lease releases the retained snapshot: a later
    /// run of the same key parses fresh again.
    #[test]
    fn dropping_last_lease_releases_snapshot() {
        let service = Arc::new(DeclLoweringService::new());
        let source: Arc<str> = Arc::from("type A = 1;\n");
        let k = key("/ws/release.ts", 7);
        let st = oxc_span::SourceType::ts();

        let lease = service.acquire_lease(&k, &source, st);
        assert!(lease.parsed_now);
        let warm = service.run(&k, &source, st, |p| p.is_some());
        assert!(!warm.parsed_now, "leased run reuses the snapshot");

        drop(lease.lease);

        // After the lease drops, the next run parses transiently again
        // (the retained snapshot was released).
        let after = service.run(&k, &source, st, |p| p.is_some());
        assert!(
            after.parsed_now,
            "dropping the last lease releases the retained snapshot — the \
             next run parses fresh"
        );
    }

    /// A moved R21 parse-env dimension is a distinct retention key: a
    /// snapshot leased under one parse env can never answer a demand
    /// made under another, even over identical content bytes.
    #[test]
    fn moved_parse_env_key_forces_fresh_parse() {
        let service = Arc::new(DeclLoweringService::new());
        let source: Arc<str> = Arc::from("type A = { a: 1 };\n");
        let st = oxc_span::SourceType::ts();
        let mut env_a = key("/ws/a.ts", 1);
        let mut env_b = env_a.clone();
        env_a.parse_env_hash = [1u8; 16];
        env_b.parse_env_hash = [2u8; 16];

        let lease_a = service.acquire_lease(&env_a, &source, st);
        assert!(lease_a.parsed_now);
        let warm_a = service.run(&env_a, &source, st, |program| program.is_some());
        assert!(!warm_a.parsed_now && warm_a.value);

        // A different parse env under the SAME content bytes is a
        // different key — no lease pins it, so it parses fresh.
        let moved = service.run(&env_b, &source, st, |program| program.is_some());
        assert!(
            moved.parsed_now,
            "identical content under a MOVED parse env must parse fresh — \
             the env is part of the retention identity"
        );
        assert!(moved.value);

        drop(lease_a.lease);
    }

    /// Distinct keys (a content edit) get distinct parses — the old
    /// content's snapshot never answers the new content's demand.
    #[test]
    fn content_edit_key_change_forces_fresh_parse() {
        let service = Arc::new(DeclLoweringService::new());
        let st = oxc_span::SourceType::ts();
        let old_source: Arc<str> = Arc::from("type A = { old: 1 };\n");
        let new_source: Arc<str> = Arc::from("type A = { edited: 2 };\n");

        let old_lease = service.acquire_lease(&key("/ws/a.ts", 1), &old_source, st);
        assert!(old_lease.parsed_now);
        let outcome = service.run(&key("/ws/a.ts", 2), &new_source, st, |program| {
            program
                .expect("parse must succeed")
                .source_str()
                .contains("edited")
        });
        assert!(outcome.parsed_now, "a new whole_hash must parse fresh");
        assert!(outcome.value, "the new snapshot must carry the NEW content");

        drop(old_lease.lease);
    }

    /// A fatal (panicking) parse is served as `None` and RETAINED while
    /// leased — a second demand against the same broken content does not
    /// re-parse.
    #[test]
    fn fatal_parse_is_retained_as_none() {
        let service = Arc::new(DeclLoweringService::new());
        // Unterminated template literal panics the parser.
        let source: Arc<str> = Arc::from("const a = `unterminated\n");
        let k = key("/ws/broken.ts", 3);
        let st = oxc_span::SourceType::ts();

        let lease = service.acquire_lease(&k, &source, st);
        assert!(lease.parsed_now);
        let first = service.run(&k, &source, st, |program| program.is_none());
        let second = service.run(&k, &source, st, |program| program.is_none());
        assert!(first.value, "fatal parse must surface as None");
        assert!(second.value);
        assert!(
            !first.parsed_now && !second.parsed_now,
            "the fatal outcome is retained while leased — no re-parse of \
             broken content"
        );

        drop(lease.lease);
    }

    /// A panicking JOB is re-raised on the caller and the worker (and
    /// its retained shard) survives: the next job on the same leased key
    /// still sees the retained snapshot.
    #[test]
    fn job_panic_reraises_on_caller_and_worker_survives() {
        let service = Arc::new(DeclLoweringService::new());
        let source: Arc<str> = Arc::from("type A = 1;\n");
        let k = key("/ws/a.ts", 4);
        let st = oxc_span::SourceType::ts();

        let lease = service.acquire_lease(&k, &source, st);
        assert!(lease.parsed_now);
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            service.run(&k, &source, st, |_| -> () { panic!("job bug") });
        }));
        assert!(panicked.is_err(), "the job panic must reach the caller");

        let after = service.run(&k, &source, st, |program| program.is_some());
        assert!(after.value);
        assert!(
            !after.parsed_now,
            "the shard must survive the panicking job with its retention intact"
        );

        drop(lease.lease);
    }

    /// Concurrent jobs on different keys all complete (no deadlock,
    /// no lost rendezvous) — the blocking-caller contract. Native-only:
    /// `wasm32` has no worker threads and runs jobs inline.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn concurrent_demands_complete() {
        let service = Arc::new(DeclLoweringService::new());
        let mut handles = Vec::new();
        for i in 0..16u8 {
            let service = Arc::clone(&service);
            handles.push(std::thread::spawn(move || {
                let source: Arc<str> = Arc::from(format!("type T{i} = {{ v: {i} }};\n"));
                let outcome = service.run(
                    &key(&format!("/ws/f{i}.ts"), i),
                    &source,
                    oxc_span::SourceType::ts(),
                    |program| program.is_some(),
                );
                outcome.value
            }));
        }
        for handle in handles {
            assert!(handle.join().expect("no panics"));
        }
    }
}
