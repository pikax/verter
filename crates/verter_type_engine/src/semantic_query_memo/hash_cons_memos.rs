//! Bounded memos for structural substitution and deferred evaluation.
//!
//! Complete substitution results depend on immutable interned nodes and
//! carry construction receipts. Refused or unpaid substitutions never
//! publish. Deferred evaluation can read declarations and export routes,
//! so each retained
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
    receipt: Arc<crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt>,
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
    /// The entry's retention of its delivery's metadata, which its hits
    /// share with their readers.
    _lease_metadata: Option<verter_session_query::retention::shared::CacheHold>,
    /// The root owners the delivery's metadata names, held with it.
    _lease_handles: verter_session_query::retention::owned::HeldHandles,
}

impl SemanticGraphStore {
    construction_ordinal_seam! {
        /// Hash-cons memo lookup for
        /// `substitute_semantic_type_param`. Bumps
        /// `substitute_memo_hits` on hit, `substitute_memo_misses` on
        /// miss.
        #[cfg_attr(
            not(any(test, feature = "test-support")),
            expect(dead_code, reason = "Fixture and compile-fail privacy seam; production substitution reads priced entries.")
        )]
        fn substitute_memo_get(
            &self,
            value_expr: SemanticNodeId,
            parameter_node: SemanticNodeId,
            arg: SemanticNodeId,
        ) -> Option<SemanticNodeId> {
            self.substitute_memo_get_priced(value_expr, parameter_node, arg)
                .map(|(result, _)| result)
        }
    }

    /// The immutable first-writer-wins result and its receipt, read from
    /// one entry: a concurrent eviction misses whole, so the caller
    /// recomputes rather than serve an unpriced node. A hit roots the
    /// result in the active scope and bumps `substitute_memo_hits`; a miss
    /// bumps `substitute_memo_misses`.
    pub(crate) fn substitute_memo_get_priced(
        &self,
        node: SemanticNodeId,
        parameter: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> Option<(
        SemanticNodeId,
        Arc<crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt>,
    )> {
        let hit =
            self.substitute_memo
                .get(&(node, parameter, arg))
                .map(|entry| {
                    let SubstituteMemoEntry {
                        result,
                        receipt,
                        roots,
                        ..
                    } = entry.value();
                    super::node_roots::NodeRootScope::capture(
                        super::node_roots::NodeRoot::from_roots(*result, roots.clone()),
                    );
                    (*result, Arc::clone(receipt))
                });
        let counter = if hit.is_some() {
            &self.stats.substitute_memo_hits
        } else {
            &self.stats.substitute_memo_misses
        };
        counter.fetch_add(1, Ordering::Relaxed);
        hit
    }

    construction_ordinal_seam! {
        /// First-writer-wins publish for `substitute_memo`. Tracks the
        /// inserted key in the FIFO sidecar so the deque can evict the
        /// oldest entry once the retention cap is exceeded; an
        /// `Entry::Occupied` collision (a concurrent publisher already
        /// landed an identical-result write) does NOT push into the
        /// FIFO since the key is already tracked.
        #[cfg_attr(
            not(any(test, feature = "test-support")),
            expect(dead_code, reason = "Fixture and compile-fail privacy seam; production substitution publishes priced receipts.")
        )]
        fn substitute_memo_publish(
            &self,
            value_expr: SemanticNodeId,
            parameter_node: SemanticNodeId,
            arg: SemanticNodeId,
            result: SemanticNodeId,
        ) {
            use crate::project_semantic_dispatch::cost_receipt::{CostIdentity, DemandCostReceipt, LogicalUsage};
            let receipt = DemandCostReceipt::new(
                CostIdentity::of_key((value_expr, parameter_node, arg)),
                LogicalUsage::default(), Vec::new());
            self.substitute_memo_publish_priced(value_expr, parameter_node, arg, result, receipt);
        }
    }

    /// Publish only a complete substitution with the cost of constructing
    /// it. The receipt's reservation follows the same retention account as
    /// the entry and its counted roots.
    pub(crate) fn substitute_memo_publish_priced(
        &self,
        value_expr: SemanticNodeId,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
        result: SemanticNodeId,
        receipt: Arc<crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt>,
    ) {
        if receipt.reserve_retention(self.retention_account()).is_err() {
            return;
        }
        let key = (value_expr, parameter_node, arg);
        match self.substitute_memo.entry(key) {
            Entry::Occupied(_) => {
                // First-writer-wins: another thread already
                // published. No FIFO bookkeeping required.
            }
            Entry::Vacant(slot) => {
                let Ok(roots) = self.root_nodes([value_expr, parameter_node, arg, result]) else {
                    return;
                };
                // Charged like every retained answer; one the account
                // refuses is computed again on demand.
                let mut aliases = verter_session_query::retention::allocation::AliasSet::new();
                let bytes = hash_cons_entry_bytes::<(
                    (SemanticNodeId, SemanticNodeId, SemanticNodeId),
                    SubstituteMemoEntry,
                )>(&roots, &mut aliases);
                // The root owner is an owned allocation, charged by its
                // own cell and held by the entry.
                let handles = aliases.take_handles();
                let verter_session_query::retention::RetentionAdmission::Admitted(retention) =
                    self.retention_account().reserve(
                        verter_session_query::retention::ChargeClass::Retained,
                        bytes,
                    )
                else {
                    return;
                };
                let Ok(retention) = handles.retained_with(retention, self.charge_ctx()) else {
                    return;
                };
                slot.insert(SubstituteMemoEntry {
                    result,
                    receipt,
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
                    walker_diagnostics: super::empty_diagnostics(),
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
        let Ok(roots) = self.root_nodes([node, result]) else {
            return;
        };
        let lease = super::delivery_lease::SemanticDeliveryLease::from_roots(
            roots,
            self.retention_account(),
        );
        // The entry's block, slot and FIFO record and its proof (its
        // delivery's metadata is adopted below): the fact slice with every fact's owned text and the
        // self-root list. A page of a wide proof is claimed into the same
        // reservation and charged once wherever it is shared.
        let (own_bytes, handles) = {
            use verter_session_query::retention::allocation::{
                arc_block_bytes, arc_str_list_bytes, fact_slice_bytes, AliasSet,
            };
            let mut aliases = AliasSet::new();
            let bytes = arc_block_bytes(std::mem::size_of::<DeferredEvaluationMemoEntry>())
                + verter_session_query::retention::ENTRY_OVERHEAD_BYTES
                + fact_slice_bytes(&evidence.0, &mut aliases)
                + arc_str_list_bytes(&evidence.1, &mut aliases);
            // The proof's owned names are charged by their own cells, held
            // by the entry.
            (bytes, aliases.take_handles())
        };
        let retention = match verter_session_query::facts::receipt::reserve_retained_with_evidence(
            self.retention_account(),
            own_bytes,
            &[&evidence.0],
        ) {
            verter_session_query::retention::RetentionAdmission::Admitted(charge) => {
                match handles.retained_with(charge, self.charge_ctx()) {
                    Ok(charge) => charge,
                    Err(_) => return,
                }
            }
            verter_session_query::retention::RetentionAdmission::Refused(_) => return,
        };
        let Ok(lease_metadata) = lease.adopt_metadata() else {
            return;
        };
        let Ok(lease_handles) = lease.hold_metadata_handles(self.charge_ctx()) else {
            return;
        };
        let entry = Arc::new(DeferredEvaluationMemoEntry {
            result,
            receipt,
            evidence,
            lease,
            _retention: retention,
            _lease_metadata: lease_metadata,
            _lease_handles: lease_handles,
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
