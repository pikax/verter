//! Stage types for the scheduler pipeline.
//!
//! Files progress through Source → Analysis → Artifact stages.
//! Each stage produces an immutable snapshot committed via ArcSwap.

use std::fmt;
use std::sync::Arc;

/// Scheduler job kind for non-file-staged work. Lets the session layer
/// route component-meta requests through the scheduler as independent
/// jobs so N component-meta queries in Batch mode fan out onto the
/// Rayon pool instead of executing synchronously in caller order.
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
    /// stage pool stays free to dispatch their cross-file `Load`/`Parse`.
    ComponentMeta { canonical_id: Arc<str> },
}

/// Internal work discriminant. Artifact carries `profile_hash` so IDE and SSR
/// jobs for the same file/generation never alias in dedup, cancellation, or wakeups.
///
/// Adapted into the DAG's typed identity at admission time via
/// `WorkNodeIdentity::{FileStage, Artifact}` — there is no flat
/// `(file_id, generation, TaskKind)` queue key in the final state.
#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub enum TaskKind {
    /// Load and parse file content.
    Source,
    /// Run static analysis (imports, bindings, macros, styles).
    Analysis,
    /// Compile to virtual files for a specific profile.
    Artifact { profile_hash: u64 },
}

impl fmt::Display for TaskKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskKind::Source => write!(f, "Source"),
            TaskKind::Analysis => write!(f, "Analysis"),
            TaskKind::Artifact { profile_hash } => write!(f, "Artifact({profile_hash:016x})"),
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
#[derive(Clone, Debug, PartialEq, Eq)]
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
    pub fn required_task_kind(&self) -> TaskKind {
        match self {
            TargetStage::Source => TaskKind::Source,
            TargetStage::Analysis => TaskKind::Analysis,
            TargetStage::Artifact { profile_hash } => TaskKind::Artifact {
                profile_hash: *profile_hash,
            },
        }
    }

    /// Whether this target is satisfied by a completed task kind.
    ///
    /// Source satisfies Source.
    /// Analysis satisfies Source and Analysis.
    /// Artifact{h} satisfies Source, Analysis, and Artifact{h}.
    pub fn is_satisfied_by(&self, completed: &TaskKind) -> bool {
        match (self, completed) {
            (TargetStage::Source, TaskKind::Source) => true,
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
        assert_eq!(TaskKind::Source.to_string(), "Source");
        assert_eq!(TaskKind::Analysis.to_string(), "Analysis");
        assert_eq!(
            TaskKind::Artifact {
                profile_hash: 0x1234
            }
            .to_string(),
            "Artifact(0000000000001234)"
        );
    }

    #[test]
    fn task_kind_equality() {
        assert_eq!(TaskKind::Source, TaskKind::Source);
        assert_ne!(TaskKind::Source, TaskKind::Analysis);
        assert_eq!(
            TaskKind::Artifact { profile_hash: 1 },
            TaskKind::Artifact { profile_hash: 1 }
        );
        assert_ne!(
            TaskKind::Artifact { profile_hash: 1 },
            TaskKind::Artifact { profile_hash: 2 }
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
        assert_eq!(TargetStage::Source.required_task_kind(), TaskKind::Source);
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
        // Source target
        assert!(TargetStage::Source.is_satisfied_by(&TaskKind::Source));
        assert!(TargetStage::Source.is_satisfied_by(&TaskKind::Analysis));
        assert!(TargetStage::Source.is_satisfied_by(&TaskKind::Artifact { profile_hash: 1 }));

        // Analysis target
        assert!(!TargetStage::Analysis.is_satisfied_by(&TaskKind::Source));
        assert!(TargetStage::Analysis.is_satisfied_by(&TaskKind::Analysis));
        assert!(TargetStage::Analysis.is_satisfied_by(&TaskKind::Artifact { profile_hash: 1 }));

        // Artifact target — profile must match
        let target = TargetStage::Artifact { profile_hash: 42 };
        assert!(!target.is_satisfied_by(&TaskKind::Source));
        assert!(!target.is_satisfied_by(&TaskKind::Analysis));
        assert!(target.is_satisfied_by(&TaskKind::Artifact { profile_hash: 42 }));
        assert!(!target.is_satisfied_by(&TaskKind::Artifact { profile_hash: 99 }));
    }
}
