//! The request-shared completion tier: the results a request computed and
//! may serve again, shared by every dispatcher of the request.
//!
//! A request runs many dispatchers — one per host operation, and one per
//! semantic task a drive lends its request state to — on one or more
//! threads. A result one of them completed is the answer to its key for
//! every other while the workspace is unedited and the view is the same,
//! so the tier is owned by the request ([`RequestCompletions`] on the
//! [`RequestContext`](crate::request_context::RequestContext)), one tier per
//! graph and store view, and every dispatcher of the request on that graph
//! and view reads and writes the same one. A dispatcher made with no
//! request context installed owns a tier of its own, which the semantic
//! tasks it lends its state to share.
//!
//! The tier holds two kinds of result:
//!
//! - a result the family memo admitted, kept for a claim the memo cannot
//!   serve back to the request's own view (the candidate records facts
//!   newer than the view);
//! - a request-local completion: a nested read of an intermediate the
//!   family memo never admits, because the computation that reads it is
//!   what later requests reach ([`super::completes_request_locally`]), and
//!   the members of a relation component whose root completed that way.
//!
//! Each kept result is served with the receipt its computation sealed, so a
//! reader pays exactly what computing it would have cost, and is served
//! only under the workspace edit clocks it was computed at: an edit moves
//! the clocks and the reader computes again. Every entry holds a `Retained`
//! charge on the process retention account; a refused reservation keeps
//! nothing and the result's delivery stays its only owner. The tier is
//! reclaimed with its owner — the request context, or the dispatcher that
//! minted it.

use std::sync::{Arc, Weak};

use rustc_hash::FxHashMap;
use verter_session_query::facts::fact_cache::ReadSetSignature;
use verter_session_query::facts::store_view::StoreViewCompatToken;
use verter_session_query::retention::{
    ChargeClass, RetentionAdmission, RetentionCharge, ENTRY_OVERHEAD_BYTES,
};

use super::WorkspaceEdit;
use crate::semantic_query::{CacheRead, QueryResult, SemanticQueryKey, SemanticQueryValue};
use crate::semantic_query_memo::{CompletedCarrier, SemanticGraphStore};

type ValueRead = CacheRead<QueryResult<SemanticQueryValue>>;

/// The completion tiers of one request, one per graph and store view
/// ([`RequestCompletionTier`]). Owned by the request context and dropped
/// with it, which releases every result its tiers kept.
#[derive(Default)]
pub struct RequestCompletions {
    tiers: parking_lot::Mutex<Vec<TierSlot>>,
}

struct TierSlot {
    graph: Weak<SemanticGraphStore>,
    view: StoreViewCompatToken,
    tier: Arc<RequestCompletionTier>,
}

impl RequestCompletions {
    /// The request's tier for `graph` under `view`, created on first use.
    pub(crate) fn tier_for(
        &self,
        graph: &Arc<SemanticGraphStore>,
        view: StoreViewCompatToken,
    ) -> Arc<RequestCompletionTier> {
        let mut tiers = self.tiers.lock();
        if let Some(slot) = tiers
            .iter()
            .find(|slot| slot.view == view && std::ptr::eq(slot.graph.as_ptr(), Arc::as_ptr(graph)))
        {
            return Arc::clone(&slot.tier);
        }
        // A graph dropped mid-request leaves its tier unreachable: release it
        // before its address can be taken by another graph.
        tiers.retain(|slot| slot.graph.strong_count() > 0);
        let tier = Arc::new(RequestCompletionTier::default());
        tiers.push(TierSlot {
            graph: Arc::downgrade(graph),
            view,
            tier: Arc::clone(&tier),
        });
        tier
    }
}

impl std::fmt::Debug for RequestCompletions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tiers = self.tiers.lock();
        f.debug_struct("RequestCompletions")
            .field("tiers", &tiers.len())
            .field(
                "results",
                &tiers.iter().map(|slot| slot.tier.len()).sum::<usize>(),
            )
            .finish()
    }
}

/// The results one request completed on one graph under one store view.
#[derive(Default)]
pub(crate) struct RequestCompletionTier {
    completed: parking_lot::Mutex<FxHashMap<SemanticQueryKey, CompletedResult>>,
}

/// One kept result: the read (with the receipt its computation sealed),
/// the carrier its readers replay with the self-roots it completed under,
/// the edit clocks it was computed under, and its share of the retention
/// charge it was kept under — the result's own, or the one charge its whole
/// relation component shares.
pub(super) struct CompletedResult {
    pub(super) read: ValueRead,
    pub(super) completed: CompletedCarrier,
    pub(super) completed_at: WorkspaceEdit,
    _retention: Arc<RetentionCharge>,
}

impl CompletedResult {
    /// The bytes a kept `read` is charged: its entry, its key and its
    /// walker diagnostics.
    pub(super) fn footprint(read: &ValueRead) -> usize {
        std::mem::size_of::<SemanticQueryKey>()
            + std::mem::size_of::<Self>()
            + ENTRY_OVERHEAD_BYTES
            + read.walker_diagnostics.len()
                * std::mem::size_of::<crate::project_semantic_dispatch::walk::ShallowDiagnostic>()
    }
}

impl RequestCompletionTier {
    /// The read and carrier kept for `key`, when it was computed under
    /// `edit` and `serves` accepts its value.
    pub(super) fn completed(
        &self,
        key: &SemanticQueryKey,
        edit: WorkspaceEdit,
        serves: impl FnOnce(&QueryResult<SemanticQueryValue>) -> bool,
    ) -> Option<(ValueRead, ReadSetSignature)> {
        let (read, carrier) = {
            let completed = self.completed.lock();
            let result = completed.get(key)?;
            if result.completed_at != edit {
                return None;
            }
            (result.read.clone(), result.completed.carrier.clone())
        };
        serves(&read.value).then_some((read, carrier))
    }

    /// Keep `read` for `key`, computed under `completed_at`, when the
    /// retention account admits it. A result kept again replaces the
    /// earlier one: both answer the key, and the later one is current.
    pub(super) fn keep(
        &self,
        graph: &SemanticGraphStore,
        key: &SemanticQueryKey,
        read: &ValueRead,
        completed: CompletedCarrier,
        completed_at: WorkspaceEdit,
    ) {
        let RetentionAdmission::Admitted(retention) = graph
            .retention_account()
            .reserve(ChargeClass::Retained, CompletedResult::footprint(read))
        else {
            return;
        };
        self.completed.lock().insert(
            key.clone(),
            CompletedResult {
                read: read.clone(),
                completed,
                completed_at,
                _retention: Arc::new(retention),
            },
        );
    }

    /// Keep every result of one completed component under ONE charge for
    /// the whole component, all or nothing: a component kept in part would
    /// serve some members while the others compute again against them.
    /// Returns whether the component was kept.
    pub(super) fn keep_component(
        &self,
        graph: &SemanticGraphStore,
        members: Vec<(SemanticQueryKey, ValueRead)>,
        completed: &CompletedCarrier,
        completed_at: WorkspaceEdit,
    ) -> bool {
        if members.is_empty() {
            return true;
        }
        let bytes = members
            .iter()
            .map(|(_, read)| CompletedResult::footprint(read))
            .sum();
        let RetentionAdmission::Admitted(retention) = graph
            .retention_account()
            .reserve(ChargeClass::Retained, bytes)
        else {
            return false;
        };
        let retention = Arc::new(retention);
        let mut results = self.completed.lock();
        for (key, read) in members {
            results.insert(
                key,
                CompletedResult {
                    read,
                    completed: completed.clone(),
                    completed_at,
                    _retention: Arc::clone(&retention),
                },
            );
        }
        true
    }

    /// How many results the tier keeps.
    pub(crate) fn len(&self) -> usize {
        self.completed.lock().len()
    }

    /// The self-roots the result kept for `key` completed under.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn self_roots_for_tests(&self, key: &SemanticQueryKey) -> Option<Arc<[Arc<str>]>> {
        self.completed
            .lock()
            .get(key)
            .map(|result| Arc::clone(&result.completed.self_roots))
    }
}

#[cfg(test)]
#[path = "request_completions_tests.rs"]
mod tests;
