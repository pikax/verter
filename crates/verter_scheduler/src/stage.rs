//! Stage types for the scheduler pipeline.
//!
//! Files progress through Source → Analysis → Artifact stages.
//! Each stage produces an immutable snapshot committed via ArcSwap.

use std::fmt;
use std::sync::Arc;

use crate::cache_id::SchedulerCacheId;
use crate::dag::Hash16;

/// Scheduler job kind for non-file-staged work. Identifies the
/// independent items an external host/runtime batch carries. The
/// scheduler does NOT fan these out: the host/runtime layer maps a
/// batch of these items and runs the outer fan-out on its own
/// coordinator pool (never the scheduler stage pool), accounting the
/// batch submission through the scheduler's pool-free
/// `account_batch_submission`. The scheduler's stage pool stays free to
/// dispatch each item's cross-file `Source` stage work (the load+parse
/// step the source stage runs under `TaskKind::Load`).
///
/// Currently the only non-staged job kind is `ComponentMeta`; the enum
/// is kept open for future extensions (resolve-named-type adapters,
/// type-expansion workloads, etc.).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum SchedulerJobKind {
    /// Resolve component-meta for a given canonical id. The external
    /// host/runtime layer maps a batch of these job items and fans them
    /// out through its own batch-coordination primitive (running the
    /// outer wait on its coordinator pool, NOT the scheduler stage
    /// pool); the scheduler only accounts for the batch submission. The
    /// per-item closures resolve concurrently while the scheduler's
    /// stage pool stays free to dispatch their cross-file `Source` stage
    /// work (the load+parse step the source stage runs under `TaskKind::Load`).
    ComponentMeta { canonical_id: Arc<str> },
}

/// Owned per-task execution descriptor. The dispatch path constructs a
/// `TaskKind` as the label the worker path needs; it is NOT an identity
/// key. The hashable identities/discriminators are
/// [`WorkNodeIdentity`](crate::dag::WorkNodeIdentity),
/// [`WorkKind`](crate::dag::WorkKind),
/// [`DepKey`](crate::dag::DepKey), [`FileStageKey`](crate::dag::FileStageKey),
/// and [`TargetStage`] — `TaskKind` deliberately does NOT derive
/// `Copy`/`Eq`/`Hash` so it can carry owned execution payloads without ever
/// being mistaken for a cache/queue key.
///
/// The five variants mirror [`WorkKind`](crate::dag::WorkKind) for dispatch
/// labels and pool routing. `Load` is the I/O-bound source-content step and
/// `Parse` is the CPU-bound parse step; the live `FileStage{Source}` DAG node
/// maps to `Load`, and the load+parse work runs in one source-stage execution
/// path. `Artifact` carries `profile_hash` so IDE and SSR jobs for the same
/// file/generation never alias in dedup, cancellation, or wakeups. `CacheNode`
/// carries the stable cache task payload (`cache_id` + `key_hash`); the
/// remaining cache identity (`view_epoch` / `snapshot_pin_id`) lives on
/// [`WorkNodeIdentity::CacheNode`](crate::dag::WorkNodeIdentity) and is handed
/// to the executor directly at dispatch.
#[derive(Clone, PartialEq, Debug)]
pub enum TaskKind {
    /// Load file content from overlay or disk (I/O-bound).
    Load,
    /// Parse loaded file content (CPU-bound).
    Parse,
    /// Run static analysis (imports, bindings, macros, styles).
    Analysis,
    /// Compile to virtual files for a specific profile.
    Artifact { profile_hash: u64 },
    /// Materialise a session-owned cache node (CPU-bound).
    CacheNode {
        /// Opaque session-owned cache identity.
        cache_id: SchedulerCacheId,
        /// Cache key hash for this node.
        key_hash: Hash16,
    },
}

impl fmt::Display for TaskKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskKind::Load => write!(f, "Load"),
            TaskKind::Parse => write!(f, "Parse"),
            TaskKind::Analysis => write!(f, "Analysis"),
            TaskKind::Artifact { profile_hash } => write!(f, "Artifact({profile_hash:016x})"),
            TaskKind::CacheNode { cache_id, key_hash } => {
                write!(f, "CacheNode({:016x},", cache_id.0)?;
                for byte in key_hash {
                    write!(f, "{byte:02x}")?;
                }
                write!(f, ")")
            }
        }
    }
}

/// Scheduling priority tiers. Lower ordinal = higher priority.
///
/// - `Critical` — blocking an interactive user action (hover, completion)
/// - `Interactive` — user-triggered but not blocking (did_open, did_change)
/// - `Background` — background scanner, workspace-wide compilation
/// - `Maintenance` — idle-time housekeeping (eviction, GC)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Priority {
    Critical = 0,
    Interactive = 1,
    Background = 2,
    Maintenance = 3,
}

/// What the caller wants the file to reach.
///
/// This is the caller-facing request target — the hashable stage
/// discriminator. It carries the file-work stages a request can ask for
/// (`Source`, `Analysis`, `Artifact`). The execution-internal `Load` / `Parse`
/// split and the scheduler/cache-internal `CacheNode` work are NOT request
/// targets, so they deliberately have no `TargetStage` arm.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TargetStage {
    /// Caller needs file parsed (source snapshot committed).
    Source,
    /// Caller needs analysis committed.
    Analysis,
    /// Caller needs artifact for a specific compile profile.
    Artifact {
        /// Hash of the [`CompileProfile`] that identifies this artifact variant.
        profile_hash: u64,
    },
}

impl TargetStage {
    /// Returns the minimum `TaskKind` that satisfies this target.
    ///
    /// `Source` requires `Load` — the live `FileStage{Source}` DAG node maps
    /// to `TaskKind::Load`, and the load+parse work runs in one source-stage
    /// execution path.
    pub fn required_task_kind(&self) -> TaskKind {
        match self {
            TargetStage::Source => TaskKind::Load,
            TargetStage::Analysis => TaskKind::Analysis,
            TargetStage::Artifact { profile_hash } => TaskKind::Artifact {
                profile_hash: *profile_hash,
            },
        }
    }

    /// Whether this target is satisfied by a completed task kind.
    ///
    /// Source satisfies Source (via Load or Parse), Analysis, and Artifact.
    /// Analysis satisfies Analysis and Artifact.
    /// Artifact{h} satisfies Artifact{h}.
    ///
    /// `Load` and `Parse` both satisfy a `Source` target: the source stage is
    /// the load+parse pair, and either completion label means the source
    /// snapshot is (or will be, on the same node) committed. `CacheNode` work
    /// is scheduler/cache-internal and satisfies no request target.
    pub fn is_satisfied_by(&self, completed: &TaskKind) -> bool {
        match (self, completed) {
            (TargetStage::Source, TaskKind::Load) => true,
            (TargetStage::Source, TaskKind::Parse) => true,
            (TargetStage::Source, TaskKind::Analysis) => true,
            (TargetStage::Source, TaskKind::Artifact { .. }) => true,
            (TargetStage::Analysis, TaskKind::Analysis) => true,
            (TargetStage::Analysis, TaskKind::Artifact { .. }) => true,
            (TargetStage::Artifact { profile_hash: a }, TaskKind::Artifact { profile_hash: b }) => {
                a == b
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SchedulerJobKind::ComponentMeta` identifies a component-meta
    /// job by canonical id. Two jobs for the same canonical id
    /// compare equal; different canonicals stay distinct. Hash
    /// preserves the distinction so `DashMap` / `HashMap` keys work.
    #[test]
    fn scheduler_job_kind_component_meta_identity_by_canonical_id() {
        let a = SchedulerJobKind::ComponentMeta {
            canonical_id: Arc::from("/src/A.vue"),
        };
        let b = SchedulerJobKind::ComponentMeta {
            canonical_id: Arc::from("/src/A.vue"),
        };
        let c = SchedulerJobKind::ComponentMeta {
            canonical_id: Arc::from("/src/B.vue"),
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
        use std::hash::{Hash, Hasher};
        let mut ha = std::collections::hash_map::DefaultHasher::new();
        a.hash(&mut ha);
        let mut hb = std::collections::hash_map::DefaultHasher::new();
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    #[test]
    fn task_kind_display() {
        assert_eq!(TaskKind::Load.to_string(), "Load");
        assert_eq!(TaskKind::Parse.to_string(), "Parse");
        assert_eq!(TaskKind::Analysis.to_string(), "Analysis");
        assert_eq!(
            TaskKind::Artifact {
                profile_hash: 0x1234
            }
            .to_string(),
            "Artifact(0000000000001234)"
        );
        let cache = TaskKind::CacheNode {
            cache_id: SchedulerCacheId(0xab),
            key_hash: [0u8; 16],
        };
        assert_eq!(
            cache.to_string(),
            "CacheNode(00000000000000ab,00000000000000000000000000000000)"
        );
    }

    #[test]
    fn task_kind_equality() {
        assert_eq!(TaskKind::Load, TaskKind::Load);
        assert_ne!(TaskKind::Load, TaskKind::Parse);
        assert_ne!(TaskKind::Load, TaskKind::Analysis);
        assert_eq!(
            TaskKind::Artifact { profile_hash: 1 },
            TaskKind::Artifact { profile_hash: 1 }
        );
        assert_ne!(
            TaskKind::Artifact { profile_hash: 1 },
            TaskKind::Artifact { profile_hash: 2 }
        );
        assert_eq!(
            TaskKind::CacheNode {
                cache_id: SchedulerCacheId(7),
                key_hash: [1u8; 16],
            },
            TaskKind::CacheNode {
                cache_id: SchedulerCacheId(7),
                key_hash: [1u8; 16],
            }
        );
        assert_ne!(
            TaskKind::CacheNode {
                cache_id: SchedulerCacheId(7),
                key_hash: [1u8; 16],
            },
            TaskKind::CacheNode {
                cache_id: SchedulerCacheId(8),
                key_hash: [1u8; 16],
            }
        );
    }

    #[test]
    fn priority_ordering() {
        // Lower ordinal = higher priority = sorts first
        assert!(Priority::Critical < Priority::Interactive);
        assert!(Priority::Interactive < Priority::Background);
        assert!(Priority::Background < Priority::Maintenance);
    }

    #[test]
    fn priority_min_selects_highest() {
        assert_eq!(
            std::cmp::min(Priority::Background, Priority::Critical),
            Priority::Critical
        );
        assert_eq!(
            std::cmp::min(Priority::Interactive, Priority::Maintenance),
            Priority::Interactive
        );
    }

    #[test]
    fn target_stage_required_task_kind() {
        // Source requires Load — the live FileStage{Source} node maps to Load.
        assert_eq!(TargetStage::Source.required_task_kind(), TaskKind::Load);
        assert_eq!(
            TargetStage::Analysis.required_task_kind(),
            TaskKind::Analysis
        );
        assert_eq!(
            TargetStage::Artifact { profile_hash: 42 }.required_task_kind(),
            TaskKind::Artifact { profile_hash: 42 }
        );
    }

    #[test]
    fn target_stage_satisfaction() {
        // Source target — both Load and Parse satisfy it (source = load+parse).
        assert!(TargetStage::Source.is_satisfied_by(&TaskKind::Load));
        assert!(TargetStage::Source.is_satisfied_by(&TaskKind::Parse));
        assert!(TargetStage::Source.is_satisfied_by(&TaskKind::Analysis));
        assert!(TargetStage::Source.is_satisfied_by(&TaskKind::Artifact { profile_hash: 1 }));
        // A scheduler/cache-internal CacheNode never satisfies a request target.
        assert!(!TargetStage::Source.is_satisfied_by(&TaskKind::CacheNode {
            cache_id: SchedulerCacheId(1),
            key_hash: [0u8; 16],
        }));

        // Analysis target
        assert!(!TargetStage::Analysis.is_satisfied_by(&TaskKind::Load));
        assert!(!TargetStage::Analysis.is_satisfied_by(&TaskKind::Parse));
        assert!(TargetStage::Analysis.is_satisfied_by(&TaskKind::Analysis));
        assert!(TargetStage::Analysis.is_satisfied_by(&TaskKind::Artifact { profile_hash: 1 }));

        // Artifact target — profile must match
        let target = TargetStage::Artifact { profile_hash: 42 };
        assert!(!target.is_satisfied_by(&TaskKind::Load));
        assert!(!target.is_satisfied_by(&TaskKind::Parse));
        assert!(!target.is_satisfied_by(&TaskKind::Analysis));
        assert!(target.is_satisfied_by(&TaskKind::Artifact { profile_hash: 42 }));
        assert!(!target.is_satisfied_by(&TaskKind::Artifact { profile_hash: 99 }));
        assert!(!target.is_satisfied_by(&TaskKind::CacheNode {
            cache_id: SchedulerCacheId(1),
            key_hash: [0u8; 16],
        }));
    }

    /// `TargetStage` is the hashable request-target discriminator: equal
    /// targets hash equal so `DashMap` / `HashMap` keys on request targets
    /// work. (`TaskKind` deliberately is NOT `Hash`; the request target is.)
    #[test]
    fn target_stage_is_hashable() {
        use std::collections::HashSet;
        let mut set: HashSet<TargetStage> = HashSet::new();
        set.insert(TargetStage::Source);
        set.insert(TargetStage::Analysis);
        set.insert(TargetStage::Artifact { profile_hash: 1 });
        set.insert(TargetStage::Artifact { profile_hash: 1 });
        // The duplicate Artifact{1} must collapse — 3 distinct targets.
        assert_eq!(set.len(), 3);
        assert!(set.contains(&TargetStage::Source));
        assert!(set.contains(&TargetStage::Artifact { profile_hash: 1 }));
        assert!(!set.contains(&TargetStage::Artifact { profile_hash: 2 }));
    }
}
