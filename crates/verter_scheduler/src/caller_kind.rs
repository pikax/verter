//! Caller-kind classification + worker TLS.
//!
//! Every thread that enters the scheduler is classified by a
//! [`CallerKind`] so cooperative-pump callers know what they may do
//! while waiting:
//!
//! - [`CallerKind::Driver`]   — the dedicated scheduler driver thread.
//! - [`CallerKind::CpuWorker`] — a thread owned by the scheduler's
//!   CPU pool. Such threads are running scheduler-owned work; if they
//!   block, only a cooperative pump (or an inline-execute of a ready
//!   dependency) keeps the pipeline alive.
//! - [`CallerKind::IoWorker`]  — a thread owned by the scheduler's
//!   I/O pool. Same constraint.
//! - [`CallerKind::Inline`]    — a thread that owns a sync scheduler
//!   and is responsible for driving stages itself.
//! - [`CallerKind::External`]  — a thread outside the scheduler's
//!   pool. Free to block; cannot drive.
//!
//! The classification is read from a thread-local slot set by the
//! per-pool thread builder when each worker spins up; threads that
//! were never marked default to [`CallerKind::External`].

use std::cell::{Cell, RefCell};

use crate::dag::WorkNodeIdentity;

/// Discriminator describing the thread that entered the scheduler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallerKind {
    /// A thread outside the scheduler's owned pools.
    External,
    /// The single driver thread.
    Driver,
    /// A worker on the scheduler-owned CPU pool.
    CpuWorker,
    /// A worker on the scheduler-owned I/O pool.
    IoWorker,
    /// A thread that owns a sync scheduler and is driving inline.
    Inline,
}

impl CallerKind {
    /// Read the current thread's classification from TLS. Threads
    /// that the scheduler never marked report `External`.
    pub fn current() -> Self {
        CALLER_KIND.with(|slot| slot.get())
    }

    /// Set the current thread's classification. Returns the previous
    /// value so callers can implement scoped overrides (RAII).
    pub(crate) fn set(kind: CallerKind) -> CallerKind {
        CALLER_KIND.with(|slot| slot.replace(kind))
    }
}

thread_local! {
    static CALLER_KIND: Cell<CallerKind> = const { Cell::new(CallerKind::External) };
}

/// RAII guard that restores the prior caller-kind on drop. Used by
/// the inline-drive path so a thread that owns a sync scheduler
/// reports `Inline` for the duration of `wait_or_drive` and reverts
/// when it returns.
pub(crate) struct CallerKindGuard {
    previous: CallerKind,
}

impl CallerKindGuard {
    /// Install `kind` and capture the previous value for restoration.
    pub(crate) fn install(kind: CallerKind) -> Self {
        let previous = CallerKind::set(kind);
        Self { previous }
    }
}

impl Drop for CallerKindGuard {
    fn drop(&mut self) {
        let _ = CallerKind::set(self.previous);
    }
}

thread_local! {
    static ACTIVE_PATH: RefCell<Vec<WorkNodeIdentity>> = const { RefCell::new(Vec::new()) };
}

/// Run `f` while pushing `identity` onto the calling thread's
/// active-path stack — used by stage executors to declare which
/// work they are running so a cooperative pump on the same thread
/// avoids dispatching the same identity back to itself.
///
/// The frame pops on both the normal-return and panic-unwind paths
/// (RAII).
pub(crate) fn with_active_path<R>(identity: WorkNodeIdentity, f: impl FnOnce() -> R) -> R {
    struct PathFrame;
    impl Drop for PathFrame {
        fn drop(&mut self) {
            ACTIVE_PATH.with(|p| {
                let _ = p.borrow_mut().pop();
            });
        }
    }
    ACTIVE_PATH.with(|p| p.borrow_mut().push(identity));
    let _frame = PathFrame;
    f()
}

/// Snapshot the active-path stack into a fresh `Vec`. The clone is
/// intentional — cooperative-pump callers hand the slice to the DAG
/// under its lock, and the TLS borrow cannot survive that
/// boundary.
pub(crate) fn snapshot_active_path() -> Vec<WorkNodeIdentity> {
    ACTIVE_PATH.with(|p| p.borrow().clone())
}

/// Check whether `identity` (or, for request-stage targets, a
/// canonical+stage that matches a file-stage frame at the same
/// stage) is on the calling thread's active path. Used by the
/// cooperative pump's same-path self-await detection.
pub(crate) fn active_path_contains_work(identity: &WorkNodeIdentity) -> bool {
    ACTIVE_PATH.with(|p| p.borrow().contains(identity))
}

/// Variant of [`active_path_contains_work`] that compares by
/// `(canonical, file-stage)` so a request-level handle (whose
/// generation is not yet known) can still match the active frame.
///
/// This is the fallback path used during the brief race window
/// between `submit_request` (stamps `CompletionTarget::Request`)
/// and `handle_new_request` (overwrites with the concrete
/// `CompletionTarget::Work` identity). A handle observed in
/// `wait_or_drive` BEFORE admission processes the request still
/// carries the `Request` target; this fallback covers it.
///
/// Matching rules by target stage cover the full prerequisite
/// chain — any later-stage request from inside an earlier-stage
/// frame for the same canonical IS the same-file deadlock class:
///
/// - `Source` request matches when the active stack has a
///   `FileStage{Source}` frame for the same canonical.
/// - `Analysis` request matches when the active stack has either
///   a `FileStage{Source}` or `FileStage{Analysis}` frame for the
///   same canonical (a Source executor that submits an Analysis
///   request and waits cannot make progress until Source
///   completes; Analysis admission gates on Source).
/// - `Artifact{ profile_hash }` request matches when the active
///   stack has a `FileStage{Source}` or `FileStage{Analysis}`
///   frame for the same canonical, OR an `Artifact` frame for
///   the same canonical AND the same `profile_hash`. Artifact
///   admission depends on the file's Analysis being complete,
///   which depends on Source; submitting an Artifact request
///   from inside any of those frames IS the same-file deadlock
///   class. Two Artifact frames for the same canonical with
///   DIFFERENT `profile_hash` values are independent work units
///   (they share only the upstream Analysis gate, not the
///   Artifact slot itself), so they MUST NOT collapse into a
///   same-path match. Once admission lands the concrete
///   `Work` identity on the sender's target slot, the more
///   precise [`active_path_contains_work`] takes over.
pub(crate) fn active_path_contains_request(
    canonical: &str,
    target: crate::stage::TargetStage,
) -> bool {
    use crate::dag::{profile_hash_to_bytes, FileStageKey};
    use crate::stage::TargetStage;
    // The set of file-stage frames whose presence on the active
    // path indicates the requested target would deadlock the
    // caller. Each request stage matches its own frame plus every
    // earlier (prerequisite) stage — Analysis matches Source, and
    // Artifact matches Source|Analysis (plus an Artifact frame on
    // the same canonical AND matching `profile_hash`; handled by
    // `artifact_target_profile` below, not in this stage matrix).
    let stage_matches: &[FileStageKey] = match target {
        TargetStage::Source => &[FileStageKey::Source],
        TargetStage::Analysis => &[FileStageKey::Source, FileStageKey::Analysis],
        TargetStage::Artifact { .. } => &[FileStageKey::Source, FileStageKey::Analysis],
    };
    // For Artifact requests an active Artifact frame on the same
    // canonical AND matching `profile_hash` is the self-await
    // class. Two artifact requests for the same canonical but
    // DIFFERENT profiles are independent work units (different
    // profile artefacts share only the upstream Analysis gate, not
    // the Artifact slot itself), so they must NOT collapse into a
    // same-path match.
    let artifact_target_profile = match target {
        TargetStage::Artifact { profile_hash } => Some(profile_hash_to_bytes(profile_hash)),
        _ => None,
    };
    ACTIVE_PATH.with(|p| {
        p.borrow().iter().any(|id| match id {
            WorkNodeIdentity::FileStage {
                canonical: c,
                stage,
                ..
            } => c.as_ref() == canonical && stage_matches.contains(stage),
            WorkNodeIdentity::Artifact {
                canonical: c,
                profile_hash,
                ..
            } => match artifact_target_profile {
                Some(requested) => c.as_ref() == canonical && profile_hash == &requested,
                None => false,
            },
            WorkNodeIdentity::CacheNode { .. } => false,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_thread_reports_external() {
        let h = std::thread::spawn(CallerKind::current);
        assert_eq!(h.join().unwrap(), CallerKind::External);
    }

    #[test]
    fn guard_restores_previous_on_drop() {
        // Force the calling test thread to a known starting value so
        // the assertion is robust against test-runner reuse.
        let _root = CallerKindGuard::install(CallerKind::External);
        assert_eq!(CallerKind::current(), CallerKind::External);
        {
            let _g = CallerKindGuard::install(CallerKind::Driver);
            assert_eq!(CallerKind::current(), CallerKind::Driver);
            {
                let _g2 = CallerKindGuard::install(CallerKind::CpuWorker);
                assert_eq!(CallerKind::current(), CallerKind::CpuWorker);
            }
            assert_eq!(CallerKind::current(), CallerKind::Driver);
        }
        assert_eq!(CallerKind::current(), CallerKind::External);
    }

    #[test]
    fn discriminants_distinct() {
        assert_ne!(CallerKind::External, CallerKind::Driver);
        assert_ne!(CallerKind::Driver, CallerKind::CpuWorker);
        assert_ne!(CallerKind::CpuWorker, CallerKind::IoWorker);
        assert_ne!(CallerKind::IoWorker, CallerKind::Inline);
        assert_ne!(CallerKind::Inline, CallerKind::External);
    }

    #[test]
    fn active_path_push_pop_round_trip() {
        use crate::dag::{FileStageKey, WorkNodeIdentity};
        use std::sync::Arc;
        let id = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/x.vue"),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        // Snapshot is empty before push.
        assert!(
            !active_path_contains_work(&id),
            "fresh thread has empty active path",
        );
        let after = with_active_path(id.clone(), || {
            // Snapshot must include the pushed identity.
            assert!(
                active_path_contains_work(&id),
                "active path must contain the pushed identity",
            );
            snapshot_active_path()
        });
        // After the closure returns the frame must pop.
        assert!(
            !active_path_contains_work(&id),
            "active path must pop after with_active_path returns",
        );
        // The closure-time snapshot retained the push (proof the
        // borrow strategy lets callers extract the path before
        // taking the DAG lock).
        assert_eq!(after.len(), 1);
        assert_eq!(after[0], id);
    }

    #[test]
    fn active_path_contains_request_matches_canonical_and_stage() {
        use crate::dag::{FileStageKey, WorkNodeIdentity};
        use crate::stage::TargetStage;
        use std::sync::Arc;
        let id = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/x.vue"),
            generation: 7,
            stage: FileStageKey::Analysis,
        };
        with_active_path(id, || {
            // Match by canonical + Analysis stage — the request's
            // generation does not need to match the live frame.
            assert!(active_path_contains_request(
                "/x.vue",
                TargetStage::Analysis
            ));
            // Different stage does NOT match (Source target on an
            // Analysis frame).
            assert!(!active_path_contains_request("/x.vue", TargetStage::Source));
            // Different canonical does NOT match.
            assert!(!active_path_contains_request(
                "/y.vue",
                TargetStage::Analysis
            ));
            // Artifact request DOES match same-canonical Analysis
            // frame — the Artifact gates on Analysis, so submitting
            // an Artifact from inside the same file's Analysis
            // executor and waiting IS the same-file deadlock class.
            // The prerequisite-stage match covers the brief race
            // window between `submit_request` (stamps `Request{..}`)
            // and `handle_new_request` (stamps the concrete `Work`
            // identity).
            assert!(active_path_contains_request(
                "/x.vue",
                TargetStage::Artifact { profile_hash: 0 }
            ));
        });
    }

    /// Artifact request against a DIFFERENT canonical's Analysis
    /// frame must NOT match. The prerequisite-stage match fires
    /// only on same-canonical Analysis-then-Artifact paths.
    #[test]
    fn active_path_contains_request_artifact_does_not_match_other_canonical() {
        use crate::dag::{FileStageKey, WorkNodeIdentity};
        use crate::stage::TargetStage;
        use std::sync::Arc;
        let id = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/x.vue"),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        with_active_path(id, || {
            assert!(!active_path_contains_request(
                "/y.vue",
                TargetStage::Artifact { profile_hash: 0 }
            ));
        });
    }

    /// Prerequisite-stage broadening: an Analysis request from
    /// inside a Source frame for the same canonical IS the same-
    /// file deadlock class. Analysis admission gates on Source
    /// completion; a Source executor that submits an Analysis
    /// request for itself and waits would block its own Source
    /// from completing.
    ///
    /// Discriminator: pre-broadening the request fallback matched
    /// Analysis only against an Analysis frame, so a Source
    /// executor calling `submit_request(target=Analysis)` would
    /// hang. Post-broadening the fallback also matches the active
    /// Source frame for the same canonical.
    #[test]
    fn active_path_contains_request_analysis_matches_source_frame() {
        use crate::dag::{FileStageKey, WorkNodeIdentity};
        use crate::stage::TargetStage;
        use std::sync::Arc;
        let id = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/x.vue"),
            generation: 3,
            stage: FileStageKey::Source,
        };
        with_active_path(id, || {
            assert!(
                active_path_contains_request("/x.vue", TargetStage::Analysis),
                "Analysis request must match same-canonical Source frame",
            );
            // Different canonical must NOT match — the broadening
            // is per-canonical only.
            assert!(!active_path_contains_request(
                "/y.vue",
                TargetStage::Analysis
            ));
        });
    }

    /// Prerequisite-stage broadening: an Artifact request from
    /// inside a Source frame for the same canonical IS the same-
    /// file deadlock class. Artifact admission gates on Analysis
    /// → which gates on Source; a Source executor that submits an
    /// Artifact request for itself and waits hangs the chain.
    #[test]
    fn active_path_contains_request_artifact_matches_source_frame() {
        use crate::dag::{FileStageKey, WorkNodeIdentity};
        use crate::stage::TargetStage;
        use std::sync::Arc;
        let id = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/x.vue"),
            generation: 5,
            stage: FileStageKey::Source,
        };
        with_active_path(id, || {
            assert!(
                active_path_contains_request("/x.vue", TargetStage::Artifact { profile_hash: 0 }),
                "Artifact request must match same-canonical Source frame",
            );
        });
    }

    /// Same-canonical + same-profile Artifact frame IS the
    /// self-await class: an Artifact executor that issues an
    /// Artifact request for itself (its own canonical AND its own
    /// profile) and waits would block on its own pending
    /// completion.
    #[test]
    fn active_path_contains_request_artifact_matches_same_profile_only() {
        use crate::dag::{profile_hash_to_bytes, WorkNodeIdentity};
        use crate::stage::TargetStage;
        use std::sync::Arc;
        let profile = 0x42u64;
        let id = WorkNodeIdentity::Artifact {
            canonical: Arc::from("/x.vue"),
            generation: 7,
            profile_hash: profile_hash_to_bytes(profile),
            content_hash: [0u8; 16],
        };
        with_active_path(id, || {
            assert!(
                active_path_contains_request(
                    "/x.vue",
                    TargetStage::Artifact {
                        profile_hash: profile
                    }
                ),
                "Artifact request must match same-canonical + same-profile Artifact frame",
            );
            // Different canonical Artifact frame must NOT match.
            assert!(!active_path_contains_request(
                "/y.vue",
                TargetStage::Artifact {
                    profile_hash: profile
                }
            ));
            // Analysis request does NOT match an Artifact frame —
            // an Artifact frame implies Analysis already completed
            // for this canonical, so submitting Analysis from
            // within an Artifact executor is not the deadlock
            // class.
            assert!(!active_path_contains_request(
                "/x.vue",
                TargetStage::Analysis
            ));
            // Source request does NOT match an Artifact frame —
            // Source already completed too.
            assert!(!active_path_contains_request("/x.vue", TargetStage::Source));
        });
    }

    /// Different-profile Artifact request against an Artifact
    /// frame for the same canonical is INDEPENDENT work — two
    /// distinct profile artefacts share only the upstream Analysis
    /// gate, not the Artifact slot. The same-path match must NOT
    /// fire and the request must proceed normally rather than
    /// being short-circuited to a synthetic Failed.
    ///
    /// Discriminator: without the per-profile comparison, the
    /// active-Artifact arm collapsed all profiles for the same
    /// canonical into a single equivalence class — a benign cross-
    /// profile recompile from inside an Artifact executor would be
    /// converted into a spurious StageFailed. Per-profile gating
    /// keeps the cross-profile case live.
    #[test]
    fn active_path_contains_request_artifact_does_not_match_different_profile() {
        use crate::dag::{profile_hash_to_bytes, WorkNodeIdentity};
        use crate::stage::TargetStage;
        use std::sync::Arc;
        let active_profile = 0x42u64;
        let other_profile = 0x99u64;
        assert_ne!(active_profile, other_profile);
        let id = WorkNodeIdentity::Artifact {
            canonical: Arc::from("/x.vue"),
            generation: 7,
            profile_hash: profile_hash_to_bytes(active_profile),
            content_hash: [0u8; 16],
        };
        with_active_path(id, || {
            assert!(
                !active_path_contains_request(
                    "/x.vue",
                    TargetStage::Artifact {
                        profile_hash: other_profile
                    }
                ),
                "different-profile Artifact request must NOT match same-canonical Artifact frame",
            );
        });
    }
}
