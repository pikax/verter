//! Indexed priority queue for scheduling jobs.
//!
//! The [`JobIndex`] is the sole authority for ordering, aging, dedup, and
//! admission policy. It is scheduler-owned — accessed only through the driver.

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use rustc_hash::FxHashMap;

use crate::stage::{Priority, TaskKind};

/// Composite key uniquely identifying a job in the queue.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JobKey {
    pub file_id: String,
    pub generation: u64,
    pub task_kind: TaskKind,
}

/// An entry in the priority queue.
#[derive(Clone, Debug)]
pub struct QueueEntry {
    pub job_key: JobKey,
    pub base_priority: Priority,
    pub enqueue_time: Instant,
    /// Priority inherited from a resolved blocker (for requeued waiters).
    pub unblock_inherited_priority: Option<Priority>,
    /// Optional request context propagated from the parent request that
    /// caused this job to be enqueued. Populated for auto-ingested
    /// dependency jobs so the worker installs the parent's context as
    /// TLS — that way VFS-sink fan-out events for the dep read carry
    /// the parent's `request_id` and land in the audit record's
    /// `vfs_reads`. capture-site audit.
    ///
    /// The dispatch loop prefers this context over
    /// [`crate::node::PendingRequests::winner_context_at_generation`];
    /// both can be `None` for background / maintenance jobs with no
    /// audited caller.
    pub request_context: Option<crate::request_context::OpaqueRequestContext>,
    /// Set to `true` when this entry is stale (newer generation superseded it).
    cancelled: bool,
}

impl QueueEntry {
    /// Create a new queue entry with no propagated request context.
    /// Use [`Self::with_request_context`] for auto-ingested dependency
    /// jobs that need to inherit an audited parent's TLS context.
    pub fn new(
        job_key: JobKey,
        base_priority: Priority,
        enqueue_time: Instant,
        unblock_inherited_priority: Option<Priority>,
    ) -> Self {
        Self {
            job_key,
            base_priority,
            enqueue_time,
            unblock_inherited_priority,
            request_context: None,
            cancelled: false,
        }
    }

    /// Attach a request context to this entry so the dispatching worker
    /// installs it as TLS while running the job.
    #[must_use]
    pub fn with_request_context(
        mut self,
        ctx: Option<crate::request_context::OpaqueRequestContext>,
    ) -> Self {
        self.request_context = ctx;
        self
    }

    /// Compute the effective priority tier for ordering.
    ///
    /// `min(base_priority, unblock_inherited_priority)` — lower ordinal = higher priority.
    pub fn effective_priority(&self) -> Priority {
        match self.unblock_inherited_priority {
            Some(inherited) => std::cmp::min(self.base_priority, inherited),
            None => self.base_priority,
        }
    }

    /// Compute the effective priority after aging rules.
    ///
    /// - Background older than `aging_bg_threshold` → Interactive
    /// - Maintenance older than `aging_maint_threshold` → Background
    /// - Aged entries never reach Critical.
    pub fn effective_priority_aged(&self, now: Instant, config: &AgingConfig) -> Priority {
        let base = self.effective_priority();
        let age = now.duration_since(self.enqueue_time);

        match base {
            Priority::Background if age >= config.background_to_interactive => {
                Priority::Interactive
            }
            Priority::Maintenance if age >= config.maintenance_to_background => {
                Priority::Background
            }
            other => other,
        }
    }
}

/// Configuration for aging thresholds.
#[derive(Clone, Debug)]
pub struct AgingConfig {
    /// Background entries older than this promote to Interactive.
    pub background_to_interactive: std::time::Duration,
    /// Maintenance entries older than this promote to Background.
    pub maintenance_to_background: std::time::Duration,
}

impl Default for AgingConfig {
    fn default() -> Self {
        Self {
            background_to_interactive: std::time::Duration::from_secs(10),
            maintenance_to_background: std::time::Duration::from_secs(30),
        }
    }
}

/// Effective priority key for ordering comparisons.
///
/// Ordered by: (priority tier, enqueue_time, file_id) — this gives
/// strict priority tiers with FIFO within tier and lexicographic tie-breaking.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct EffectiveKey {
    priority: Priority,
    enqueue_time: Instant,
    file_id: String,
    task_kind_discriminant: u8,
}

impl EffectiveKey {
    fn from_entry(entry: &QueueEntry, now: Instant, config: &AgingConfig) -> Self {
        let priority = entry.effective_priority_aged(now, config);
        Self {
            priority,
            enqueue_time: entry.enqueue_time,
            file_id: entry.job_key.file_id.clone(),
            task_kind_discriminant: match entry.job_key.task_kind {
                TaskKind::Source => 0,
                TaskKind::Analysis => 1,
                TaskKind::Artifact { .. } => 2,
            },
        }
    }
}

/// Scheduler-owned indexed priority structure.
///
/// All ordering, aging, and dedup logic lives here.
/// Request senders are NOT stored here — they live in `FileNode.pending_requests`.
pub struct JobIndex {
    /// Ordered entries. Each entry is a single job.
    entries: Vec<QueueEntry>,
    /// Dedup index: `(file_id, generation, task_kind)` → position in `entries`.
    /// This is an optimization index — entries are the source of truth.
    index: FxHashMap<(String, u64, TaskKind), usize>,
    /// Aging configuration.
    aging_config: AgingConfig,
}

impl JobIndex {
    /// Create a new empty job index.
    pub fn new(aging_config: AgingConfig) -> Self {
        Self {
            entries: Vec::new(),
            index: FxHashMap::default(),
            aging_config,
        }
    }

    /// Number of non-cancelled entries.
    pub fn len(&self) -> usize {
        self.entries.iter().filter(|e| !e.cancelled).count()
    }

    /// Whether the index has no active entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Insert or merge a job entry.
    ///
    /// **Dedup rules:**
    /// - Identical `(file_id, generation, task_kind)` already queued → merge:
    ///   effective priority = min of both, enqueue_time = earlier.
    /// - Newer generation for same `(file_id, task_kind)` → older one cancelled.
    ///
    /// Returns `true` if a new entry was added (not merged into existing).
    pub fn insert(&mut self, entry: QueueEntry) -> bool {
        let key = (
            entry.job_key.file_id.clone(),
            entry.job_key.generation,
            entry.job_key.task_kind,
        );

        // Check for existing entry with same key (merge)
        if let Some(&idx) = self.index.get(&key) {
            if idx < self.entries.len() && !self.entries[idx].cancelled {
                let existing = &mut self.entries[idx];
                // Merge: higher priority wins, earlier enqueue time wins
                existing.base_priority = std::cmp::min(existing.base_priority, entry.base_priority);
                existing.unblock_inherited_priority = match (
                    existing.unblock_inherited_priority,
                    entry.unblock_inherited_priority,
                ) {
                    (Some(a), Some(b)) => Some(std::cmp::min(a, b)),
                    (Some(a), None) => Some(a),
                    (None, Some(b)) => Some(b),
                    (None, None) => None,
                };
                if entry.enqueue_time < existing.enqueue_time {
                    existing.enqueue_time = entry.enqueue_time;
                }
                // Request context is request-scoped and must NOT be
                // dropped on merge. A dependency can be enqueued
                // context-less first (background pre-warm) and then
                // re-ingested with a parent's context — adopting the
                // incoming context lets the dispatching worker install
                // the parent's TLS so VFS-sink events carry the
                // parent's `request_id`. We adopt the incoming context
                // ONLY when the existing entry has none; an already
                // present context is the authoritative attribution and
                // is never overwritten (a later context-less re-insert
                // must not clear it).
                if existing.request_context.is_none() {
                    if let Some(ctx) = entry.request_context {
                        existing.request_context = Some(ctx);
                    }
                }
                return false;
            }
        }

        // Cancel older generation entries for the same (file_id, task_kind)
        self.cancel_older_generations(
            &entry.job_key.file_id,
            entry.job_key.generation,
            &entry.job_key.task_kind,
        );

        // Insert new entry
        let idx = self.entries.len();
        self.index.insert(key, idx);
        self.entries.push(entry);
        true
    }

    /// Cancel all entries for a file with generation < `new_gen` and matching task kind.
    ///
    /// When inserting a new generation Source job, also cancels Analysis and all
    /// Artifact jobs for older generations.
    fn cancel_older_generations(&mut self, file_id: &str, new_gen: u64, new_kind: &TaskKind) {
        // For Source jobs: cancel ALL older-gen jobs for this file
        // For Analysis/Artifact jobs: cancel only matching kind
        let cancel_all_kinds = matches!(new_kind, TaskKind::Source);

        for entry in self.entries.iter_mut() {
            if entry.cancelled || entry.job_key.file_id != file_id {
                continue;
            }
            if entry.job_key.generation >= new_gen {
                continue;
            }
            if cancel_all_kinds || entry.job_key.task_kind == *new_kind {
                entry.cancelled = true;
                // Remove from dedup index
                let key = (
                    entry.job_key.file_id.clone(),
                    entry.job_key.generation,
                    entry.job_key.task_kind,
                );
                self.index.remove(&key);
            }
        }
    }

    /// Cancel all entries for a file at a specific generation.
    /// Cancel ALL queued jobs for a file (any generation, any task kind).
    /// Used when a file is removed from the scheduler.
    pub fn cancel_file(&mut self, file_id: &str) {
        for entry in self.entries.iter_mut() {
            if !entry.cancelled && entry.job_key.file_id == file_id {
                entry.cancelled = true;
                let key = (
                    entry.job_key.file_id.clone(),
                    entry.job_key.generation,
                    entry.job_key.task_kind,
                );
                self.index.remove(&key);
            }
        }
    }

    /// Clear all entries and reset the index.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
    }

    pub fn cancel_generation(&mut self, file_id: &str, generation: u64) {
        for entry in self.entries.iter_mut() {
            if !entry.cancelled
                && entry.job_key.file_id == file_id
                && entry.job_key.generation == generation
            {
                entry.cancelled = true;
                let key = (
                    entry.job_key.file_id.clone(),
                    entry.job_key.generation,
                    entry.job_key.task_kind,
                );
                self.index.remove(&key);
            }
        }
    }

    /// Upgrade the priority of a specific job.
    ///
    /// Returns `true` if the job was found and upgraded.
    pub fn upgrade_priority(
        &mut self,
        file_id: &str,
        generation: u64,
        task_kind: TaskKind,
        new_priority: Priority,
    ) -> bool {
        let key = (file_id.to_string(), generation, task_kind);
        if let Some(&idx) = self.index.get(&key) {
            if idx < self.entries.len() && !self.entries[idx].cancelled {
                let entry = &mut self.entries[idx];
                entry.base_priority = std::cmp::min(entry.base_priority, new_priority);
                return true;
            }
        }
        false
    }

    /// Dequeue the highest-priority non-cancelled entry.
    ///
    /// Applies aging at dequeue time. Skips cancelled entries (compaction).
    pub fn dequeue(&mut self) -> Option<QueueEntry> {
        if self.entries.is_empty() {
            return None;
        }

        let now = Instant::now();
        let mut best_idx: Option<usize> = None;
        let mut best_key: Option<EffectiveKey> = None;

        for (idx, entry) in self.entries.iter().enumerate() {
            if entry.cancelled {
                continue;
            }
            let key = EffectiveKey::from_entry(entry, now, &self.aging_config);
            if best_key.as_ref().is_none_or(|bk| key < *bk) {
                best_idx = Some(idx);
                best_key = Some(key);
            }
        }

        if let Some(idx) = best_idx {
            let entry = self.entries[idx].clone();
            self.entries[idx].cancelled = true;
            let key = (
                entry.job_key.file_id.clone(),
                entry.job_key.generation,
                entry.job_key.task_kind,
            );
            self.index.remove(&key);

            // Compact if more than half are cancelled
            if self.entries.len() > 16
                && self.entries.iter().filter(|e| e.cancelled).count() * 2 > self.entries.len()
            {
                self.compact();
            }

            Some(entry)
        } else {
            // All entries cancelled — compact
            self.entries.clear();
            self.index.clear();
            None
        }
    }

    /// Remove cancelled entries and rebuild index.
    fn compact(&mut self) {
        self.entries.retain(|e| !e.cancelled);
        self.index.clear();
        for (idx, entry) in self.entries.iter().enumerate() {
            let key = (
                entry.job_key.file_id.clone(),
                entry.job_key.generation,
                entry.job_key.task_kind,
            );
            self.index.insert(key, idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(file: &str, gen: u64, kind: TaskKind, priority: Priority) -> QueueEntry {
        QueueEntry {
            job_key: JobKey {
                file_id: file.to_string(),
                generation: gen,
                task_kind: kind,
            },
            base_priority: priority,
            enqueue_time: Instant::now(),
            unblock_inherited_priority: None,
            request_context: None,
            cancelled: false,
        }
    }

    fn make_entry_at(
        file: &str,
        gen: u64,
        kind: TaskKind,
        priority: Priority,
        time: Instant,
    ) -> QueueEntry {
        QueueEntry {
            job_key: JobKey {
                file_id: file.to_string(),
                generation: gen,
                task_kind: kind,
            },
            base_priority: priority,
            enqueue_time: time,
            unblock_inherited_priority: None,
            request_context: None,
            cancelled: false,
        }
    }

    // ── Priority Ordering ──

    #[test]
    fn dequeue_returns_highest_priority_first() {
        let mut idx = JobIndex::new(AgingConfig::default());

        idx.insert(make_entry(
            "low.vue",
            1,
            TaskKind::Source,
            Priority::Background,
        ));
        idx.insert(make_entry(
            "high.vue",
            1,
            TaskKind::Source,
            Priority::Critical,
        ));
        idx.insert(make_entry(
            "mid.vue",
            1,
            TaskKind::Source,
            Priority::Interactive,
        ));

        let first = idx.dequeue().unwrap();
        assert_eq!(first.job_key.file_id, "high.vue");
        assert_eq!(first.base_priority, Priority::Critical);

        let second = idx.dequeue().unwrap();
        assert_eq!(second.job_key.file_id, "mid.vue");

        let third = idx.dequeue().unwrap();
        assert_eq!(third.job_key.file_id, "low.vue");

        assert!(idx.dequeue().is_none());
    }

    // ── FIFO Within Tier ──

    #[test]
    fn dequeue_fifo_within_same_priority() {
        let mut idx = JobIndex::new(AgingConfig::default());
        let base = Instant::now();

        // Same priority, different enqueue times
        idx.insert(make_entry_at(
            "first.vue",
            1,
            TaskKind::Source,
            Priority::Interactive,
            base,
        ));
        idx.insert(make_entry_at(
            "second.vue",
            1,
            TaskKind::Source,
            Priority::Interactive,
            base + std::time::Duration::from_millis(1),
        ));
        idx.insert(make_entry_at(
            "third.vue",
            1,
            TaskKind::Source,
            Priority::Interactive,
            base + std::time::Duration::from_millis(2),
        ));

        assert_eq!(idx.dequeue().unwrap().job_key.file_id, "first.vue");
        assert_eq!(idx.dequeue().unwrap().job_key.file_id, "second.vue");
        assert_eq!(idx.dequeue().unwrap().job_key.file_id, "third.vue");
    }

    // ── Dedup: Identical Key Merge ──

    #[test]
    fn insert_merges_identical_keys() {
        let mut idx = JobIndex::new(AgingConfig::default());
        let base = Instant::now();

        // First insert
        let added = idx.insert(make_entry_at(
            "a.vue",
            1,
            TaskKind::Source,
            Priority::Background,
            base,
        ));
        assert!(added);

        // Same key, higher priority — should merge, not add
        let added = idx.insert(make_entry_at(
            "a.vue",
            1,
            TaskKind::Source,
            Priority::Critical,
            base + std::time::Duration::from_millis(10),
        ));
        assert!(!added);

        // Only one entry
        assert_eq!(idx.len(), 1);

        // Dequeue should have merged priority (Critical, the higher one)
        let entry = idx.dequeue().unwrap();
        assert_eq!(entry.base_priority, Priority::Critical);
        // Enqueue time should be the earlier one
        assert_eq!(entry.enqueue_time, base);
    }

    // ── Dedup: Newer Generation Cancels Older ──

    #[test]
    fn newer_generation_source_cancels_all_older_jobs() {
        let mut idx = JobIndex::new(AgingConfig::default());

        // Old generation jobs
        idx.insert(make_entry(
            "a.vue",
            1,
            TaskKind::Source,
            Priority::Interactive,
        ));
        idx.insert(make_entry(
            "a.vue",
            1,
            TaskKind::Analysis,
            Priority::Interactive,
        ));
        idx.insert(make_entry(
            "a.vue",
            1,
            TaskKind::Artifact { profile_hash: 0 },
            Priority::Interactive,
        ));

        assert_eq!(idx.len(), 3);

        // New generation Source — should cancel all gen=1 jobs
        idx.insert(make_entry(
            "a.vue",
            2,
            TaskKind::Source,
            Priority::Interactive,
        ));

        // Only the new Source job remains active
        assert_eq!(idx.len(), 1);

        let entry = idx.dequeue().unwrap();
        assert_eq!(entry.job_key.generation, 2);
        assert_eq!(entry.job_key.task_kind, TaskKind::Source);
    }

    #[test]
    fn newer_generation_analysis_cancels_only_analysis() {
        let mut idx = JobIndex::new(AgingConfig::default());

        idx.insert(make_entry(
            "a.vue",
            1,
            TaskKind::Source,
            Priority::Interactive,
        ));
        idx.insert(make_entry(
            "a.vue",
            1,
            TaskKind::Analysis,
            Priority::Interactive,
        ));

        // New gen Analysis — cancels old Analysis but NOT old Source
        idx.insert(make_entry(
            "a.vue",
            2,
            TaskKind::Analysis,
            Priority::Interactive,
        ));

        // Source gen=1 + Analysis gen=2
        assert_eq!(idx.len(), 2);
    }

    // ── Priority Upgrade ──

    #[test]
    fn upgrade_priority() {
        let mut idx = JobIndex::new(AgingConfig::default());

        idx.insert(make_entry(
            "a.vue",
            1,
            TaskKind::Source,
            Priority::Background,
        ));
        assert!(idx.upgrade_priority("a.vue", 1, TaskKind::Source, Priority::Critical));

        let entry = idx.dequeue().unwrap();
        assert_eq!(entry.base_priority, Priority::Critical);
    }

    #[test]
    fn upgrade_nonexistent_returns_false() {
        let mut idx = JobIndex::new(AgingConfig::default());
        assert!(!idx.upgrade_priority("missing.vue", 1, TaskKind::Source, Priority::Critical));
    }

    // ── Cancel Generation ──

    #[test]
    fn cancel_generation_removes_all_tasks_for_gen() {
        let mut idx = JobIndex::new(AgingConfig::default());

        idx.insert(make_entry(
            "a.vue",
            1,
            TaskKind::Source,
            Priority::Interactive,
        ));
        idx.insert(make_entry(
            "a.vue",
            1,
            TaskKind::Analysis,
            Priority::Interactive,
        ));
        idx.insert(make_entry(
            "b.vue",
            1,
            TaskKind::Source,
            Priority::Interactive,
        ));

        idx.cancel_generation("a.vue", 1);

        // Only b.vue remains
        assert_eq!(idx.len(), 1);
        let entry = idx.dequeue().unwrap();
        assert_eq!(entry.job_key.file_id, "b.vue");
    }

    // ── Aging ──

    #[test]
    fn aging_promotes_background_to_interactive() {
        let config = AgingConfig {
            background_to_interactive: std::time::Duration::from_millis(50),
            maintenance_to_background: std::time::Duration::from_secs(30),
        };

        let old_time = Instant::now() - std::time::Duration::from_millis(100);
        let entry = make_entry_at(
            "old.vue",
            1,
            TaskKind::Source,
            Priority::Background,
            old_time,
        );

        // Should be promoted to Interactive by aging
        let aged_priority = entry.effective_priority_aged(Instant::now(), &config);
        assert_eq!(aged_priority, Priority::Interactive);
    }

    #[test]
    fn aging_promotes_maintenance_to_background() {
        let config = AgingConfig {
            background_to_interactive: std::time::Duration::from_secs(10),
            maintenance_to_background: std::time::Duration::from_millis(50),
        };

        let old_time = Instant::now() - std::time::Duration::from_millis(100);
        let entry = make_entry_at(
            "old.vue",
            1,
            TaskKind::Source,
            Priority::Maintenance,
            old_time,
        );

        let aged_priority = entry.effective_priority_aged(Instant::now(), &config);
        assert_eq!(aged_priority, Priority::Background);
    }

    #[test]
    fn aging_never_promotes_to_critical() {
        let config = AgingConfig {
            background_to_interactive: std::time::Duration::from_millis(1),
            maintenance_to_background: std::time::Duration::from_millis(1),
        };

        let very_old = Instant::now() - std::time::Duration::from_millis(100);
        let entry = make_entry_at(
            "old.vue",
            1,
            TaskKind::Source,
            Priority::Background,
            very_old,
        );

        // Promotes to Interactive at most, never to Critical
        let aged = entry.effective_priority_aged(Instant::now(), &config);
        assert_eq!(aged, Priority::Interactive);
        assert_ne!(aged, Priority::Critical);
    }

    #[test]
    fn aging_does_not_affect_fresh_entries() {
        let config = AgingConfig::default(); // 10s threshold
        let fresh = Instant::now();
        let entry = make_entry_at(
            "fresh.vue",
            1,
            TaskKind::Source,
            Priority::Background,
            fresh,
        );

        let aged = entry.effective_priority_aged(Instant::now(), &config);
        assert_eq!(aged, Priority::Background);
    }

    // ── Unblock Inherited Priority ──

    #[test]
    fn unblock_inherited_priority_affects_effective() {
        let mut entry = make_entry("a.vue", 1, TaskKind::Source, Priority::Background);
        entry.unblock_inherited_priority = Some(Priority::Critical);

        // Min(Background, Critical) = Critical
        assert_eq!(entry.effective_priority(), Priority::Critical);
    }

    #[test]
    fn unblock_inherited_priority_integrates_with_dequeue() {
        let mut idx = JobIndex::new(AgingConfig::default());

        // A has base Background but inherited Critical
        let mut a = make_entry("a.vue", 1, TaskKind::Source, Priority::Background);
        a.unblock_inherited_priority = Some(Priority::Critical);
        idx.insert(a);

        // B has base Interactive (no inheritance)
        idx.insert(make_entry(
            "b.vue",
            1,
            TaskKind::Source,
            Priority::Interactive,
        ));

        // A should dequeue first due to inherited Critical
        let first = idx.dequeue().unwrap();
        assert_eq!(first.job_key.file_id, "a.vue");
    }

    // ── Tie-breaking ──

    #[test]
    fn tie_breaking_by_file_id() {
        let mut idx = JobIndex::new(AgingConfig::default());
        let t = Instant::now();

        // Same priority, same time — tie-break by file_id lexicographic
        idx.insert(make_entry_at(
            "z.vue",
            1,
            TaskKind::Source,
            Priority::Interactive,
            t,
        ));
        idx.insert(make_entry_at(
            "a.vue",
            1,
            TaskKind::Source,
            Priority::Interactive,
            t,
        ));
        idx.insert(make_entry_at(
            "m.vue",
            1,
            TaskKind::Source,
            Priority::Interactive,
            t,
        ));

        assert_eq!(idx.dequeue().unwrap().job_key.file_id, "a.vue");
        assert_eq!(idx.dequeue().unwrap().job_key.file_id, "m.vue");
        assert_eq!(idx.dequeue().unwrap().job_key.file_id, "z.vue");
    }

    // ── Empty Queue ──

    #[test]
    fn dequeue_empty_returns_none() {
        let mut idx = JobIndex::new(AgingConfig::default());
        assert!(idx.dequeue().is_none());
    }

    #[test]
    fn len_and_is_empty() {
        let mut idx = JobIndex::new(AgingConfig::default());
        assert!(idx.is_empty());
        assert_eq!(idx.len(), 0);

        idx.insert(make_entry(
            "a.vue",
            1,
            TaskKind::Source,
            Priority::Interactive,
        ));
        assert!(!idx.is_empty());
        assert_eq!(idx.len(), 1);

        idx.dequeue();
        assert!(idx.is_empty());
    }

    // ── Compaction ──

    #[test]
    fn compaction_after_many_cancellations() {
        let mut idx = JobIndex::new(AgingConfig::default());

        // Insert many entries then cancel most
        for i in 0..20 {
            idx.insert(make_entry(
                &format!("file{i}.vue"),
                1,
                TaskKind::Source,
                Priority::Background,
            ));
        }

        // Cancel half by inserting newer generations
        for i in 0..15 {
            idx.insert(make_entry(
                &format!("file{i}.vue"),
                2,
                TaskKind::Source,
                Priority::Background,
            ));
        }

        // Dequeue should still work correctly
        let mut count = 0;
        while idx.dequeue().is_some() {
            count += 1;
        }
        // 5 old (not cancelled) + 15 new = 20
        assert_eq!(count, 20);
    }

    // ── Dedup merge: request_context must never be dropped ──

    use std::sync::Arc;

    use crate::request_context::{
        CacheEventKind, OpaqueContextGuard, OpaqueRequestContext, RequestContextLike, TlsUninstall,
    };

    /// Minimal `RequestContextLike` fake carrying only a request id —
    /// enough to assert which context survives a queue merge.
    struct FakeCtx(u64);
    impl RequestContextLike for FakeCtx {
        fn request_id(&self) -> u64 {
            self.0
        }
        fn capture_enabled(&self) -> bool {
            false
        }
        fn on_dedup_joiner(&self, _c: Arc<str>, _w: u64, _a: bool) {}
        fn record_cache_event(&self, _event: CacheEventKind) {}
        fn install_tls(self: Arc<Self>) -> Box<dyn TlsUninstall + Send> {
            let guard = OpaqueContextGuard::install(OpaqueRequestContext(
                self as Arc<dyn RequestContextLike>,
            ));
            Box::new(FakeGuard(guard))
        }
    }
    struct FakeGuard(#[allow(dead_code)] OpaqueContextGuard);
    impl TlsUninstall for FakeGuard {
        fn uninstall(self: Box<Self>) {}
    }

    fn opaque_ctx(id: u64) -> OpaqueRequestContext {
        OpaqueRequestContext(Arc::new(FakeCtx(id)) as Arc<dyn RequestContextLike>)
    }

    #[test]
    fn merge_adopts_incoming_context_when_existing_is_none() {
        // (a) Insert key K context-LESS first, then re-insert K WITH a
        // parent context. The merged entry must ADOPT the parent
        // context. Pre-fix the merge ignored `entry.request_context`,
        // so the dequeued entry's context stayed `None` and this
        // `request_id() == 7` assertion failed (it would panic on the
        // `.expect` of a `None`).
        let mut idx = JobIndex::new(AgingConfig::default());

        let added = idx.insert(make_entry(
            "dep.ts",
            1,
            TaskKind::Source,
            Priority::Background,
        ));
        assert!(added, "first context-less insert must add a new entry");

        let with_ctx = make_entry("dep.ts", 1, TaskKind::Source, Priority::Background)
            .with_request_context(Some(opaque_ctx(7)));
        let added = idx.insert(with_ctx);
        assert!(!added, "same-key re-insert must merge, not add");
        assert_eq!(idx.len(), 1, "merge must keep a single entry");

        let entry = idx.dequeue().expect("one merged entry must be present");
        let ctx = entry
            .request_context
            .as_ref()
            .expect("merge must adopt the incoming parent context, not drop it");
        assert_eq!(
            ctx.0.request_id(),
            7,
            "merged entry must carry the parent request id from the second insert",
        );
    }

    #[test]
    fn merge_keeps_existing_context_when_incoming_is_none() {
        // (b) Insert key K WITH a context first, then re-insert K
        // context-LESS. The merged entry must KEEP the original
        // context — a later context-less re-insert must never clear an
        // already-present attribution. Pre-fix the merge ignored
        // request_context entirely; because the EXISTING entry already
        // held the context this branch happened to pass on the buggy
        // tree, so this test alone is not discriminating — its purpose
        // is to lock the "never overwrite a present Some" half of the
        // contract that the fix must not regress. The discriminating
        // half is `merge_adopts_incoming_context_when_existing_is_none`.
        let mut idx = JobIndex::new(AgingConfig::default());

        let with_ctx = make_entry("dep.ts", 1, TaskKind::Source, Priority::Background)
            .with_request_context(Some(opaque_ctx(42)));
        let added = idx.insert(with_ctx);
        assert!(added, "first insert with context must add a new entry");

        let added = idx.insert(make_entry(
            "dep.ts",
            1,
            TaskKind::Source,
            Priority::Background,
        ));
        assert!(!added, "context-less re-insert must merge, not add");

        let entry = idx.dequeue().expect("one merged entry must be present");
        let ctx = entry
            .request_context
            .as_ref()
            .expect("merge must keep the original context");
        assert_eq!(
            ctx.0.request_id(),
            42,
            "a later context-less insert must not clear the original parent context",
        );
    }
}
