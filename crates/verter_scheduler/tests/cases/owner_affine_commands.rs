//! G3: bounded CPU execution and owner-affine commands.
//!
//! Discriminates the sole-owner cutover:
//! - CPU fire-and-forget submit is bounded (unbounded rayon queue is gone)
//! - pool submit is owner-typed (`OwnerCommand<Cpu>` / `OwnerCommand<Io>`)
//! - `DepKey` cannot represent a resource-capacity predecessor
//! - production wait paths do not busy-spin
//!
//! Native-only: owner-affine pools do not exist on wasm.

#![cfg(not(target_arch = "wasm32"))]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use verter_scheduler::dag::{DepKey, FileStageKey, WorkNodeIdentity};
use verter_scheduler::owner_command::OwnerCommand;
use verter_scheduler::pool::{
    SchedulerCpuPool, SchedulerIoPool, SchedulerPoolSubmitError, SchedulerPoolSubmitResult,
};
use verter_scheduler::{Admission, CpuPool};

fn scheduler_src_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn walk_src_code(dir: &Path, buf: &mut String) {
    if !dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            walk_src_code(&path, buf);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if name.ends_with("_tests.rs") {
                continue;
            }
            let raw = fs::read_to_string(&path).expect("read file");
            for line in raw.lines() {
                let code = match line.find("//") {
                    Some(idx) => &line[..idx],
                    None => line,
                };
                buf.push_str(code);
                buf.push('\n');
            }
        }
    }
}

/// G3-AC2: named API/data boundaries are present and usable.
#[test]
fn named_boundaries_are_the_owner_affine_surface() {
    fn _admission_identity(_: Admission<()>) {}
    fn _cpu_pool_identity(_: &CpuPool) {}
    let _ = _admission_identity;
    let _ = _cpu_pool_identity;

    let cpu = SchedulerCpuPool::new(1, 8);
    let (tx, rx) = crossbeam_channel::bounded::<usize>(1);
    let r = cpu.try_submit(OwnerCommand::cpu(Box::new(move || {
        let _ = tx.send(1);
    })));
    assert_eq!(r, Ok(SchedulerPoolSubmitResult::Submitted));
    assert_eq!(rx.recv().unwrap(), 1);

    let io = SchedulerIoPool::new(1, 8);
    let (tx, rx) = crossbeam_channel::bounded::<usize>(1);
    let r = io.try_submit(OwnerCommand::io(Box::new(move || {
        let _ = tx.send(2);
    })));
    assert_eq!(r, Ok(SchedulerPoolSubmitResult::Submitted));
    assert_eq!(rx.recv().unwrap(), 2);
}

/// G3-AC1: saturated CPU transport reports Full instead of growing an
/// unbounded rayon deque.
#[test]
fn cpu_owner_command_submit_is_bounded() {
    let pool = SchedulerCpuPool::new(1, 1);
    let (release_tx, release_rx) = crossbeam_channel::bounded::<()>(0);
    pool.try_submit(OwnerCommand::cpu(Box::new(move || {
        let _ = release_rx.recv();
    })))
    .expect("first submit ok");
    // `new(1, 1)` floors to threads*4 = 4. Submit well past that bound.
    let mut saw_full = false;
    for _ in 0..16 {
        match pool.try_submit(OwnerCommand::cpu(Box::new(|| {}))) {
            Ok(_) => {}
            Err(SchedulerPoolSubmitError::Full) => {
                saw_full = true;
                break;
            }
            Err(SchedulerPoolSubmitError::Closed) => panic!("pool not closed"),
        }
    }
    assert!(saw_full, "bounded CPU transport must report Full");
    let _ = release_tx.send(());
}

/// G3-AC1: DepKey's closed arm set is file/artifact/cache — capacity is
/// not a predecessor.
#[test]
fn dep_key_has_no_resource_capacity_arm() {
    let file = DepKey::from_identity(&WorkNodeIdentity::FileStage {
        canonical: Arc::from("/a.ts"),
        generation: 1,
        stage: FileStageKey::Source,
    });
    let artifact = DepKey::from_identity(&WorkNodeIdentity::Artifact {
        canonical: Arc::from("/a.ts"),
        generation: 1,
        profile_hash: [0; 16],
        content_hash: [0; 16],
    });
    let cache = DepKey::from_identity(&WorkNodeIdentity::CacheNode {
        cache_id: verter_scheduler::cache_id::SchedulerCacheId(1),
        key_hash: [0; 16],
        view_epoch: 0,
        snapshot_pin_id: verter_scheduler::dag::PinId(0),
    });
    // Exhaustive match is the discriminator: a ResourceCapacity arm
    // fails to compile. String labels would not.
    for key in [&file, &artifact, &cache] {
        match key {
            DepKey::FileStage { .. } | DepKey::Artifact { .. } | DepKey::CacheNode { .. } => {}
        }
    }
}

/// G3-AC1: production wait does not busy-spin. `spin_loop` / hint-spin
/// in scheduler src (excluding `*_tests.rs`) is the displaced route.
#[test]
fn scheduler_src_has_no_busy_spin_wait() {
    let mut code = String::new();
    walk_src_code(&scheduler_src_root(), &mut code);
    for needle in ["spin_loop", "hint::spin", "busy_spin"] {
        assert!(
            !code.contains(needle),
            "`{needle}` re-appeared in scheduler production src — waiters must park on a condvar"
        );
    }
}

/// G3-AC3: owner-affine command routing does not own cache/incremental
/// publication; this test only proves the command identity is preserved
/// through submit (one task, one run).
#[test]
fn owner_command_runs_exactly_once() {
    let pool = SchedulerCpuPool::new(1, 8);
    let runs = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = crossbeam_channel::bounded::<()>(1);
    let c = Arc::clone(&runs);
    pool.try_submit(OwnerCommand::cpu(Box::new(move || {
        c.fetch_add(1, Ordering::SeqCst);
        let _ = tx.send(());
    })))
    .expect("submit");
    rx.recv().expect("ran");
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}
