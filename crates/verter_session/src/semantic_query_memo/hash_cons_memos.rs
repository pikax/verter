//! Hash-cons memo accessors for the
//! `substitute_semantic_type_param` and
//! `evaluate_deferred_semantic_node_with_context` caches. Extracted
//! from `mod.rs` to keep the split-target module under the
//! architecture-guard line ceiling.
//!
//! Retention budget: both memos are bounded by
//! [`HASH_CONS_MEMO_RETENTION_CAP`]. Each publish pushes the key
//! into a FIFO sidecar `VecDeque`; once the deque exceeds the cap
//! the oldest entry is popped and removed from the underlying
//! `DashMap`. The cap is intentionally generous so the corpus's
//! hot working set (per-K materialiser loops, repeated `Pick<T, K>`
//! projections across components) fits well within the budget.
//! Eviction is FIFO rather than LRU to keep the bookkeeping
//! lock-free on the hot read path — only publishes pay the FIFO
//! lock cost.
//!
//! Both memos collapse identical structural keys reaching the
//! underlying helpers
//! (`substitute_semantic_type_param`,
//! `evaluate_deferred_semantic_node_with_context`) to one cached
//! result. Reads are lock-free `DashMap::get`; writes use
//! first-writer-wins via the entry API (concurrent publishers for the
//! same key resolve to structurally identical results because both
//! helpers are pure functions of their inputs).
//!
//! Cache scoping: the store is per `(project_identity, parse_env_hash,
//! resolve_env_hash, type_env_hash, lib_env_hash)`. Semantic-node ids
//! inside one store's arena are content-addressed integers, so a
//! tuple of ids is a complete identity for the cached RESULT shape,
//! but the cached id's structural meaning may transitively depend
//! on file content the helpers walked through during their compute
//! (e.g. `TypeOf` evaluation routes through a `ValueRootKey` whose
//! resolved structure depends on the owning file). A canonical-
//! content edit therefore invalidates not only the structural data
//! but any cached `(key → result_id)` mapping that was computed by
//! walking through that canonical.
//!
//! Invalidation: both memos are cleared on `invalidate_all` (project-
//! content-generation bump) AND on every `invalidate_canonical`
//! (per-canonical edit). The per-canonical clear is currently a
//! sledgehammer: any single file edit drops both memos in full
//! rather than tracking a reverse-index from canonical → memo
//! entry. The trade-off is admission correctness over routine
//! warm-hit rate: a stale `(node_id, ctx) → result_id` mapping
//! produced by a now-invalidated cross-file walk would survive
//! under the previous `invalidate_all`-only clear policy and
//! poison every subsequent caller asking for the same key. A
//! future refinement may install a reverse-index (canonical →
//! memo entries) so per-canonical edits invalidate only the memo
//! entries whose compute touched that canonical; the structural
//! plumbing for that is plan-level follow-up work and is not
//! required for correctness.

use std::sync::atomic::Ordering;

use dashmap::mapref::entry::Entry;

use super::SemanticGraphStore;
use crate::semantic_query::{ProjectionReductionContext, SemanticNodeId};

/// FIFO retention cap for the substitute / evaluate-deferred memos.
///
/// Each memo's FIFO sidecar deque is bounded at this size; once
/// exceeded the oldest entry is popped and removed from the
/// underlying `DashMap`. The cap is sized for the empirically
/// observed working set on the bench corpus (per-K materialiser
/// loops, repeated `Pick<T, K>` projections) plus headroom for
/// long-running LSP sessions. 100_000 entries at ~32 bytes each
/// caps each memo at ~3 MB of resident memory.
pub(super) const HASH_CONS_MEMO_RETENTION_CAP: usize = 100_000;

impl SemanticGraphStore {
    /// Hash-cons memo lookup for
    /// `substitute_semantic_type_param`. Bumps
    /// `substitute_memo_hits` on hit, `substitute_memo_misses` on
    /// miss.
    pub fn substitute_memo_get(
        &self,
        value_expr: SemanticNodeId,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        let hit = self
            .substitute_memo
            .get(&(value_expr, parameter_node, arg))
            .map(|entry| *entry.value());
        if hit.is_some() {
            self.stats
                .substitute_memo_hits
                .fetch_add(1, Ordering::Relaxed);
        } else {
            self.stats
                .substitute_memo_misses
                .fetch_add(1, Ordering::Relaxed);
        }
        hit
    }

    /// First-writer-wins publish for `substitute_memo`. Tracks the
    /// inserted key in the FIFO sidecar so the deque can evict the
    /// oldest entry once the retention cap is exceeded; an
    /// `Entry::Occupied` collision (a concurrent publisher already
    /// landed an identical-result write) does NOT push into the
    /// FIFO since the key is already tracked.
    pub fn substitute_memo_publish(
        &self,
        value_expr: SemanticNodeId,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
        result: SemanticNodeId,
    ) {
        let key = (value_expr, parameter_node, arg);
        match self.substitute_memo.entry(key) {
            Entry::Occupied(_) => {
                // First-writer-wins: another thread already
                // published. No FIFO bookkeeping required.
            }
            Entry::Vacant(slot) => {
                slot.insert(result);
                let mut fifo = self.substitute_memo_fifo.lock();
                fifo.push_back(key);
                while fifo.len() > HASH_CONS_MEMO_RETENTION_CAP {
                    if let Some(victim) = fifo.pop_front() {
                        // Drop the FIFO lock BEFORE touching
                        // `substitute_memo` to avoid blocking
                        // concurrent publishers while DashMap's
                        // shard lock is taken; re-acquire on the
                        // next loop iteration if we still need
                        // to evict.
                        drop(fifo);
                        self.substitute_memo.remove(&victim);
                        fifo = self.substitute_memo_fifo.lock();
                    } else {
                        break;
                    }
                }
            }
        }
    }

    /// Hash-cons memo lookup for
    /// `evaluate_deferred_semantic_node_with_context`. Bumps
    /// `evaluate_deferred_memo_hits` on hit,
    /// `evaluate_deferred_memo_misses` on miss.
    pub fn evaluate_deferred_memo_get(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> Option<SemanticNodeId> {
        let hit = self
            .evaluate_deferred_memo
            .get(&(node, context))
            .map(|entry| *entry.value());
        if hit.is_some() {
            self.stats
                .evaluate_deferred_memo_hits
                .fetch_add(1, Ordering::Relaxed);
        } else {
            self.stats
                .evaluate_deferred_memo_misses
                .fetch_add(1, Ordering::Relaxed);
        }
        hit
    }

    /// First-writer-wins publish for `evaluate_deferred_memo`.
    /// Tracks the inserted key in the FIFO sidecar; an
    /// `Entry::Occupied` collision does NOT push (the key is
    /// already tracked).
    pub fn evaluate_deferred_memo_publish(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        result: SemanticNodeId,
    ) {
        let key = (node, context);
        match self.evaluate_deferred_memo.entry(key) {
            Entry::Occupied(_) => {
                // First-writer-wins: another thread already
                // published. No FIFO bookkeeping required.
            }
            Entry::Vacant(slot) => {
                slot.insert(result);
                let mut fifo = self.evaluate_deferred_memo_fifo.lock();
                fifo.push_back(key);
                while fifo.len() > HASH_CONS_MEMO_RETENTION_CAP {
                    if let Some(victim) = fifo.pop_front() {
                        drop(fifo);
                        self.evaluate_deferred_memo.remove(&victim);
                        fifo = self.evaluate_deferred_memo_fifo.lock();
                    } else {
                        break;
                    }
                }
            }
        }
    }

    /// Internal: drop both hash-cons memos on a workspace-content-
    /// generation bump (called from `invalidate_all`) and on every
    /// per-canonical edit (called from `invalidate_canonical`).
    ///
    /// The per-canonical clear is currently a sledgehammer: any single
    /// file edit drops both memos in full rather than walking a
    /// reverse-index from canonical → memo entry to remove only the
    /// affected entries. This is the correctness-first choice — a
    /// stale `(node_id, ctx) → result_id` mapping computed by walking
    /// through now-invalidated content would otherwise survive the
    /// edit and poison every subsequent caller for the same key.
    pub(super) fn clear_hash_cons_memos(&self) {
        self.substitute_memo.clear();
        self.substitute_memo_fifo.lock().clear();
        self.evaluate_deferred_memo.clear();
        self.evaluate_deferred_memo_fifo.lock().clear();
    }
}
