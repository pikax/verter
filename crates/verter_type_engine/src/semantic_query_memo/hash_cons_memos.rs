//! Bounded memos for structural substitution and deferred evaluation.
//!
//! Substitution is a pure function of immutable interned nodes. Deferred
//! evaluation can read declarations and export routes, so each retained
//! answer carries its observed fact signature and strict self roots.
//! Production evaluation reads validate and replay that proof against the
//! caller's view. A changed dependency replaces the stale evaluated answer;
//! an explicit graph invalidation also clears both stores.
//!
//! First insertions enter a bounded FIFO. Replacing an evaluation candidate
//! does not add another FIFO entry. Evaluation receipts, proof pages and
//! entry storage reserve the shared retention account before publication.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use dashmap::mapref::entry::Entry;

use super::SemanticGraphStore;
use crate::fact_signature_helpers::ReadSetSignatureExt;
use crate::project_semantic_dispatch::evaluate::DeferredEvaluationDemand;
use crate::semantic_query::{ProjectionReductionContext, SemanticNodeId};

/// Entry cap for the substitution and deferred-evaluation FIFO stores.
pub(super) const HASH_CONS_MEMO_RETENTION_CAP: usize = 100_000;

/// One substitution answer: the result, the roots owning the key's and
/// result's nodes, and the reservation of the entry's storage.
pub(crate) struct SubstituteMemoEntry {
    result: SemanticNodeId,
    roots: super::node_roots::NodeRootSet,
    _retention: verter_session_query::retention::RetentionCharge,
}

/// The bytes one memo entry keeps: its map slot and FIFO record, its value
/// inline, and the root owner it keeps.
fn hash_cons_entry_bytes<V>(
    roots: &super::node_roots::NodeRootSet,
    aliases: &mut verter_session_query::retention::allocation::AliasSet,
) -> usize {
    std::mem::size_of::<V>()
        + verter_session_query::retention::ENTRY_OVERHEAD_BYTES
        + roots.owned_heap_bytes(aliases)
}

pub(crate) struct DeferredEvaluationMemoEntry {
    result: SemanticNodeId,
    receipt: Arc<crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt>,
    evidence: crate::fact_signature_helpers::StructuralCarrierReadSet,
    lease: super::delivery_lease::SemanticDeliveryLease,
    _retention: verter_session_query::retention::RetentionCharge,
}

impl SemanticGraphStore {
    construction_ordinal_seam! {
        /// Hash-cons memo lookup for
        /// `substitute_semantic_type_param`. Bumps
        /// `substitute_memo_hits` on hit, `substitute_memo_misses` on
        /// miss.
        fn substitute_memo_get(
            &self,
            value_expr: SemanticNodeId,
            parameter_node: SemanticNodeId,
            arg: SemanticNodeId,
        ) -> Option<SemanticNodeId> {
            let hit =
                self.substitute_memo
                    .get(&(value_expr, parameter_node, arg))
                    .map(|entry| {
                        let SubstituteMemoEntry { result, roots, .. } = entry.value();
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
    }

    construction_ordinal_seam! {
        /// First-writer-wins publish for `substitute_memo`. Tracks the
        /// inserted key in the FIFO sidecar so the deque can evict the
        /// oldest entry once the retention cap is exceeded; an
        /// `Entry::Occupied` collision (a concurrent publisher already
        /// landed an identical-result write) does NOT push into the
        /// FIFO since the key is already tracked.
        fn substitute_memo_publish(
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
                    // Charged like every retained answer; one the account
                    // refuses is computed again on demand.
                    let bytes = hash_cons_entry_bytes::<(
                        (SemanticNodeId, SemanticNodeId, SemanticNodeId),
                        SubstituteMemoEntry,
                    )>(
                        &roots,
                        &mut verter_session_query::retention::allocation::AliasSet::new(),
                    );
                    let verter_session_query::retention::RetentionAdmission::Admitted(retention) =
                        self.retention_account().reserve(
                            verter_session_query::retention::ChargeClass::Retained,
                            bytes,
                        )
                    else {
                        return;
                    };
                    slot.insert(SubstituteMemoEntry {
                        result,
                        roots,
                        _retention: retention,
                    });
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
    }

    /// A deferred operator can read declarations and export routes. Its
    /// interned input is immutable, but its evaluated answer needs the
    /// complete read proof to validate against this request's view.
    pub(crate) fn evaluate_deferred_memo_get(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        demand: DeferredEvaluationDemand,
        ctx: &dyn crate::resolver_core::fact_validation_port::FactValidation,
    ) -> Option<crate::semantic_query::CacheRead<SemanticNodeId>> {
        self.read_deferred_evaluation(node, context, demand, |entry| {
            verter_session_query::facts::fact_cache::ReadSetSignature::new(Arc::clone(
                &entry.evidence.0,
            ))
            .validate_with_self_roots(ctx, &entry.evidence.1)
        })
    }

    fn read_deferred_evaluation(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        demand: DeferredEvaluationDemand,
        validate: impl FnOnce(&DeferredEvaluationMemoEntry) -> bool,
    ) -> Option<crate::semantic_query::CacheRead<SemanticNodeId>> {
        let entry = self
            .evaluate_deferred_memo
            .get(&(node, context, demand))
            .map(|entry| Arc::clone(entry.value()));
        let hit = entry
            .filter(|entry| {
                if validate(entry) {
                    true
                } else {
                    self.evaluate_deferred_memo
                        .remove_if(&(node, context, demand), |_, current| {
                            Arc::ptr_eq(current, entry)
                        });
                    false
                }
            })
            .map(|entry| {
                crate::fact_tracing::tracing::observe_fan_out_canonical(&entry.evidence.0);
                crate::semantic_query::CacheRead {
                    lease: entry.lease.clone(),
                    value: entry.result,
                    dep_signature: super::empty_signature(),
                    walker_diagnostics: Arc::from([]),
                    cache_suppress: false,
                    fact_status: crate::semantic_query::FactStatus::Complete,
                    receipt: crate::semantic_query::ReadReceipt::Priced(Arc::clone(&entry.receipt)),
                }
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

    /// Publish the complete evaluation and the facts it actually read.
    /// A new view replaces a stale candidate under the content-free key.
    pub(crate) fn evaluate_deferred_memo_publish(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        demand: DeferredEvaluationDemand,
        result: SemanticNodeId,
        receipt: Arc<crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt>,
        evidence: crate::fact_signature_helpers::StructuralCarrierReadSet,
    ) {
        if receipt.reserve_retention(self.retention_account()).is_err() {
            return;
        }
        let Ok(roots) = self.root_existing_nodes(vec![node, result]) else {
            return;
        };
        let lease = super::delivery_lease::SemanticDeliveryLease::from_roots(roots);
        // The entry's block, slot and FIFO record, its delivery metadata, and
        // its proof: the fact slice with every fact's owned text and the
        // self-root list. A page of a wide proof is claimed into the same
        // reservation and charged once wherever it is shared.
        let own_bytes = {
            use verter_session_query::retention::allocation::{
                arc_block_bytes, arc_str_list_bytes, fact_slice_bytes, AliasSet,
            };
            let mut aliases = AliasSet::new();
            arc_block_bytes(std::mem::size_of::<DeferredEvaluationMemoEntry>())
                + verter_session_query::retention::ENTRY_OVERHEAD_BYTES
                + lease.owned_heap_bytes(&mut aliases)
                + fact_slice_bytes(&evidence.0, &mut aliases)
                + arc_str_list_bytes(&evidence.1, &mut aliases)
        };
        let retention = match verter_session_query::facts::receipt::reserve_retained_with_evidence(
            self.retention_account(),
            own_bytes,
            &[&evidence.0],
        ) {
            verter_session_query::retention::RetentionAdmission::Admitted(charge) => charge,
            verter_session_query::retention::RetentionAdmission::Refused(_) => return,
        };
        let entry = Arc::new(DeferredEvaluationMemoEntry {
            result,
            receipt,
            evidence,
            lease,
            _retention: retention,
        });
        let key = (node, context, demand);
        match self.evaluate_deferred_memo.entry(key) {
            Entry::Occupied(mut slot) => {
                slot.insert(entry);
            }
            Entry::Vacant(slot) => {
                slot.insert(entry);
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

    /// Inspect admission under the shell demand; selected values have a
    /// separate identity and cannot establish shell admission.
    #[cfg(any(test, feature = "test-support"))]
    pub fn peek_evaluate_deferred_memo_for_tests(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> Option<()> {
        self.evaluate_deferred_memo
            .contains_key(&(node, context, DeferredEvaluationDemand::Shell))
            .then_some(())
    }

    /// Inspect admission of a selected value, independently of its shell.
    #[cfg(any(test, feature = "test-support"))]
    pub fn peek_selected_value_memo_for_tests(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> Option<()> {
        self.evaluate_deferred_memo
            .contains_key(&(node, context, DeferredEvaluationDemand::SelectedValue))
            .then_some(())
    }

    /// Release both stores during explicit graph invalidation. Ordinary
    /// evaluation reads establish freshness through their retained facts;
    /// correctness does not depend on this cleanup running after an edit.
    pub(super) fn clear_hash_cons_memos(&self) {
        self.structural_domains.clear();
        self.structural_domains_fifo.lock().clear();
        self.binder_dependencies.clear();
        self.binder_dependencies_fifo.lock().clear();
        self.substitute_memo.clear();
        self.substitute_memo_fifo.lock().clear();
        self.evaluate_deferred_memo.clear();
        self.evaluate_deferred_memo_fifo.lock().clear();
    }
}
