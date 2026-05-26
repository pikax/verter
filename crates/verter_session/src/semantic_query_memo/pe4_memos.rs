//! Hash-cons memo accessors for the substitute / evaluate-deferred
//! caches. Extracted from `mod.rs` to keep the Tier-2 split-target
//! module under the 4000-LOC architecture-guard budget.
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

use super::SemanticGraphStore;
use crate::semantic_query::{ProjectionReductionContext, SemanticNodeId};

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

    /// First-writer-wins publish for `substitute_memo`.
    pub fn substitute_memo_publish(
        &self,
        value_expr: SemanticNodeId,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
        result: SemanticNodeId,
    ) {
        self.substitute_memo
            .entry((value_expr, parameter_node, arg))
            .or_insert(result);
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
    pub fn evaluate_deferred_memo_publish(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        result: SemanticNodeId,
    ) {
        self.evaluate_deferred_memo
            .entry((node, context))
            .or_insert(result);
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
    pub(super) fn clear_pe4_memos(&self) {
        self.substitute_memo.clear();
        self.evaluate_deferred_memo.clear();
    }
}
