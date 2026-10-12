//! Hash-cons memo accessors for the
//! `substitute_semantic_type_param` and
//! `evaluate_deferred_semantic_node_with_context` caches. Extracted
//! from `mod.rs` so the memo accessors live beside the caches they
//! serve.
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
//! Both memos share completed structural work. Deferred evaluation also
//! reads current declaration and augmentation inputs, so its entries retain
//! fact signatures. Every read validates those facts outside the map lock
//! and replays them into active tracers; a stale candidate is retired only
//! if it is still the candidate that failed validation. Explicit store
//! invalidation remains a retirement path, while ordinary content edits
//! are observed through facts. Results, receipts and signatures share the
//! store's aggregate retention account.

use std::sync::atomic::Ordering;
use std::sync::Arc;

pub(crate) struct DeferredMemoEntry {
    value: SemanticNodeId,
    receipt: Arc<crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt>,
    lease: super::delivery_lease::SemanticDeliveryLease,
    signature: verter_session_query::facts::fact_cache::ReadSetSignature,
    self_roots: Arc<[Arc<str>]>,
    _charge: verter_session_query::retention::RetentionCharge,
}

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
/// bounds key count; the aggregate account separately bounds retained bytes.
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
        let hit =
            self.substitute_memo
                .get(&(value_expr, parameter_node, arg))
                .map(|entry| {
                    let (result, roots) = entry.value();
                    super::node_roots::NodeRootScope::capture(
                        super::node_roots::NodeRoot::from_roots(*result, roots.clone()),
                    );
                    *result
                });
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
                let Ok(roots) =
                    self.root_existing_nodes(vec![value_expr, parameter_node, arg, result])
                else {
                    return;
                };
                slot.insert((result, roots));
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
    /// `evaluate_deferred_semantic_node_with_context`: the evaluated node
    /// and the receipt of the evaluation that produced it, which the reader
    /// replays before using it. Bumps `evaluate_deferred_memo_hits` on hit,
    /// `evaluate_deferred_memo_misses` on miss.
    pub fn evaluate_deferred_memo_get(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        ctx: &dyn crate::resolver_core::fact_validation_port::FactValidation,
    ) -> Option<crate::semantic_query::CacheRead<SemanticNodeId>> {
        let hit = self
            .evaluate_deferred_memo
            .get(&(node, context))
            .map(|entry| Arc::clone(entry.value()))
            .filter(|entry| {
                use crate::fact_signature_helpers::ReadSetSignatureExt as _;
                if entry
                    .signature
                    .validate_with_self_roots(ctx, &entry.self_roots)
                {
                    entry.signature.bubble(ctx);
                    true
                } else {
                    self.evaluate_deferred_memo
                        .remove_if(&(node, context), |_, current| Arc::ptr_eq(current, entry));
                    false
                }
            })
            .map(|entry| crate::semantic_query::CacheRead {
                lease: entry.lease.clone(),
                value: entry.value,
                dep_signature: super::empty_signature(),
                walker_diagnostics: std::sync::Arc::from([]),
                cache_suppress: false,
                result_is_partial: false,
                partial_reasons: crate::semantic_query::PartialReasonSet::empty(),
                receipt: crate::semantic_query::ReadReceipt::Priced(std::sync::Arc::clone(
                    &entry.receipt,
                )),
            });
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
        receipt: std::sync::Arc<crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt>,
        signature: verter_session_query::facts::fact_cache::ReadSetSignature,
    ) {
        // The receipt lives as long as the entry: it is charged to the
        // retention account like every retained receipt, and an entry the
        // account declines is not kept.
        if receipt.reserve_retention(self.retention_account()).is_err() {
            return;
        }
        let key = (node, context);
        match self.evaluate_deferred_memo.entry(key) {
            Entry::Occupied(_) => {
                // First-writer-wins: another thread already
                // published. No FIFO bookkeeping required.
            }
            Entry::Vacant(slot) => {
                let Ok(roots) = self.root_existing_nodes(vec![node, result]) else {
                    return;
                };
                // This node/context memo has no single file owner. Every
                // observed whole-file fact is intentionally a strict root:
                // an untracked declaration or augmenter must reject the
                // evaluated answer rather than validate as a lazy dependency.
                let mut seen = rustc_hash::FxHashSet::default();
                let self_roots: Arc<[Arc<str>]> = signature.entries().filter_map(|fact| match fact {
                    verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash { canonical_id, .. }
                        if seen.insert(canonical_id.as_str()) => Some(Arc::from(canonical_id.as_str())),
                    _ => None,
                }).collect();
                let own_bytes = std::mem::size_of::<DeferredMemoEntry>()
                    + verter_session_query::retention::ENTRY_OVERHEAD_BYTES
                    + signature.facts.len() * 64
                    + self_roots
                        .iter()
                        .map(|root| root.len() + std::mem::size_of::<Arc<str>>())
                        .sum::<usize>();
                let verter_session_query::retention::RetentionAdmission::Admitted(charge) =
                    verter_session_query::facts::receipt::reserve_retained_with_evidence(
                        self.retention_account(),
                        own_bytes,
                        &[&signature.facts],
                    )
                else {
                    return;
                };
                slot.insert(Arc::new(DeferredMemoEntry {
                    value: result,
                    receipt,
                    lease: super::delivery_lease::SemanticDeliveryLease::from_roots(roots),
                    signature,
                    self_roots,
                    _charge: charge,
                }));
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

    /// Admission inspection for tests; production reads always validate facts.
    #[cfg(any(test, feature = "test-support"))]
    pub fn peek_evaluate_deferred_memo_for_tests(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> Option<()> {
        self.evaluate_deferred_memo
            .contains_key(&(node, context))
            .then_some(())
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
