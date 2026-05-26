//! Hash-cons memo accessors for the PE4 substitute / evaluate-deferred
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
//! tuple of ids is a complete identity for the cached result. No
//! fact-signature validation is required because the helpers are
//! pure on those inputs.
//!
//! Invalidation: both memos are cleared in lockstep with the family
//! memo and the relation memo on `invalidate_all` (project-content-
//! generation bump). The arena itself is append-only, so a per-
//! canonical content edit does not require clearing these memos —
//! stale entries become unreachable through new queries.

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

    /// Internal: drop both PE4 memos on a workspace-content-generation
    /// bump. Called from `invalidate_all` in `mod.rs`.
    pub(super) fn clear_pe4_memos(&self) {
        self.substitute_memo.clear();
        self.evaluate_deferred_memo.clear();
    }
}
