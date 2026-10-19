//! The parse release boundary: it completes only once every participating
//! worker shard has applied every release recorded before it opened.
//!
//! Workers are held by event-controlled jobs (a job that waits on its own
//! channel), so "has not applied yet" is a state the test creates, never a
//! timing it hopes for: no sleeps, no yields, no unrelated work used as
//! readiness.

use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount};
use verter_session_query::source::snapshot::SnapshotKey;

use super::{shard_index, DeclLoweringService, ParseReleaseError, SnapshotLease};

const WORKERS: usize = 4;

fn unbounded() -> Arc<SemanticRetentionAccount> {
    SemanticRetentionAccount::new(RetentionLimits {
        aggregate_ceiling_bytes: usize::MAX,
        active_ceiling_bytes: usize::MAX,
        pin_threshold_bytes: usize::MAX,
        max_entry_bytes: usize::MAX,
    })
}

fn service(account: &Arc<SemanticRetentionAccount>) -> Arc<DeclLoweringService> {
    Arc::new(DeclLoweringService::new_with_account(
        /* lazy = */ false,
        WORKERS,
        Arc::clone(account),
    ))
}

/// One key per worker shard, in shard order, distinct per `prefix`.
fn key_per_shard(prefix: &str) -> Vec<SnapshotKey> {
    let mut keys: Vec<Option<SnapshotKey>> = vec![None; WORKERS];
    let mut n = 0u32;
    while keys.iter().any(Option::is_none) {
        let key = SnapshotKey {
            canonical: Arc::from(format!("/ws/{prefix}{n}.ts")),
            whole_hash: [1u8; 16],
            parse_env_hash: [0u8; 16],
        };
        let shard = shard_index(&key, WORKERS);
        keys[shard].get_or_insert(key);
        n += 1;
    }
    keys.into_iter().flatten().collect()
}

/// Acquire a lease on every key, with the pin each acquisition charged.
fn lease_all(
    service: &Arc<DeclLoweringService>,
    account: &SemanticRetentionAccount,
    keys: &[SnapshotKey],
) -> Vec<(SnapshotLease, usize)> {
    let source: Arc<str> = Arc::from("export type A = { a: string; b: number };\n");
    keys.iter()
        .map(|key| {
            let before = account.snapshot().pinned_bytes;
            let lease = service
                .acquire_lease(key, &source, oxc_span::SourceType::ts())
                .lease;
            let pin = account.snapshot().pinned_bytes - before;
            assert!(pin > 0, "a retained parse is pinned");
            (lease, pin)
        })
        .collect()
}

/// A worker shard held busy by a job that waits for `finish`.
struct Held {
    finish: Sender<()>,
    holder: JoinHandle<Option<()>>,
}

impl Held {
    /// Run a job on `key`'s shard that holds the worker until released, and
    /// return once the worker is executing it.
    fn hold(service: &Arc<DeclLoweringService>, key: &SnapshotKey) -> Self {
        let (started_tx, started_rx) = channel::<()>();
        let (finish, finish_rx) = channel::<()>();
        let holder = {
            let (service, key) = (Arc::clone(service), key.clone());
            std::thread::spawn(move || {
                service.run_leased(&key, move |_| {
                    started_tx.send(()).expect("the test waits for the start");
                    finish_rx.recv().expect("the test releases the worker");
                })
            })
        };
        started_rx
            .recv()
            .expect("the worker starts the holding job");
        Self { finish, holder }
    }

    fn release(self) {
        self.finish.send(()).expect("the holding job waits");
        assert_eq!(self.holder.join().expect("the holder returns"), Some(()));
    }
}

/// Every shard in turn is the last one held: the boundary stays incomplete
/// until that shard runs, whichever shard it is, and each released shard
/// gives back exactly the pins its own releases held.
#[test]
fn the_boundary_completes_only_after_every_shard_applied_its_releases() {
    for last in 0..WORKERS {
        let account = unbounded();
        let service = service(&account);
        let busy = key_per_shard(&format!("busy{last}-"));
        let released = key_per_shard(&format!("released{last}-"));
        let busy_leases = lease_all(&service, &account, &busy);
        let pinned_busy = account.snapshot().pinned_bytes;
        let (released_leases, released_pins): (Vec<_>, Vec<_>) =
            lease_all(&service, &account, &released).into_iter().unzip();
        let pinned_all = account.snapshot().pinned_bytes;

        let held: Vec<Held> = busy.iter().map(|key| Held::hold(&service, key)).collect();
        drop(released_leases);
        let mut boundary = service.release_completion();
        assert_eq!(
            boundary.try_complete(),
            Ok(false),
            "no shard can have applied anything while every worker is held"
        );
        assert_eq!(account.snapshot().pinned_bytes, pinned_all);

        let mut expected = pinned_all;
        let mut last_held = None;
        for (shard, worker) in held.into_iter().enumerate() {
            if shard == last {
                last_held = Some(worker);
                continue;
            }
            worker.release();
            // Queued behind the marker on the same shard: once this answers,
            // the marker has run.
            assert!(service.run_leased(&busy[shard], |_| ()).is_some());
            expected -= released_pins[shard];
            assert_eq!(
                account.snapshot().pinned_bytes,
                expected,
                "shard {shard} applied exactly its own release"
            );
        }
        assert_eq!(
            boundary.try_complete(),
            Ok(false),
            "shard {last} is still held, so the boundary is incomplete"
        );
        assert_eq!(account.snapshot().pinned_bytes, expected);

        last_held.expect("the last shard is held").release();
        boundary.wait().expect("every shard applies its releases");
        assert_eq!(
            account.snapshot().pinned_bytes,
            pinned_busy,
            "every release recorded before the boundary was applied"
        );
        for key in &released {
            assert!(
                service.run_leased(key, |_| ()).is_none(),
                "a released snapshot is gone from its shard"
            );
        }

        drop(busy_leases);
        service
            .release_completion()
            .wait()
            .expect("the busy releases apply");
        assert_eq!(account.snapshot().pinned_bytes, 0);
    }
}

/// A boundary with nothing queued completes at once, repeatedly, and an
/// abandoned boundary leaves the next one unaffected. A service whose
/// workers never spawned holds no lease, so its boundary is complete
/// without spawning them.
#[test]
fn empty_and_repeated_boundaries_complete_at_once() {
    let lazy = DeclLoweringService::new_with_account(true, WORKERS, unbounded());
    assert_eq!(lazy.release_completion().try_complete(), Ok(true));
    assert!(!lazy.workers_spawned(), "a boundary never spawns workers");

    let account = unbounded();
    let service = service(&account);
    for _ in 0..3 {
        service.release_completion().wait().expect("nothing queued");
    }
    drop(service.release_completion());
    let keys = key_per_shard("repeat");
    drop(lease_all(&service, &account, &keys));
    service.release_completion().wait().expect("releases apply");
    service
        .release_completion()
        .wait()
        .expect("a repeated boundary");
    assert_eq!(account.snapshot().pinned_bytes, 0);
}

/// A worker that stops before reaching the marker fails the boundary with
/// its shard named, instead of reporting releases it never applied.
#[test]
fn a_stopped_worker_fails_the_boundary_with_its_shard() {
    let service = service(&unbounded());
    service.workers()[1]
        .jobs
        .send(Box::new(|_| panic!("the worker stops")))
        .expect("the worker is alive");
    assert_eq!(
        service.release_completion().wait(),
        Err(ParseReleaseError::WorkerStopped { shard: 1 })
    );
}
