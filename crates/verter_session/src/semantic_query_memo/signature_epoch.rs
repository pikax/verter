//! The family memo's side of signature-kernel epoch replacement.
//!
//! Warm `SignaturesOfType` and `ReadSignatureResult` values carry kernel
//! handles of the epoch they were built in, and a `ReadSignatureResult`
//! family key names them too. Once the kernel store replaces its epoch
//! those handles are retired: a pinned read rejects them as stale. A
//! retired-epoch value is therefore a MISS, never an answer. The warm gate
//! here refuses it and the caller recomputes against the current epoch; a
//! cross-thread joiner forks away from an in-flight build that published
//! one; and a consumer that pinned its view after a replacement re-reads
//! once. With that in place a replacement needs no memo clear. What remains
//! is retention hygiene: dropping the families and candidates no lookup can
//! reach again.
//!
//! The edit path calls
//! [`SemanticGraphStore::compact_signature_store_if_over_cap`] once per
//! content change, so the kernel tables stay bounded by
//! [`EPOCH_RECORD_CAP`](crate::signature_kernel::EPOCH_RECORD_CAP) however
//! long an edited project stays open.

use super::*;
use crate::signature_kernel::GraphEpoch;

/// The kernel epoch whose handles a memo value carries; `None` for a value
/// with no kernel handle (every other value tag, and the empty set).
fn value_kernel_epoch(result: &QueryResult<SemanticQueryValue>) -> Option<GraphEpoch> {
    match result {
        QueryResult::Value(SemanticQueryValue::SignatureSet(value)) => value.set.epoch(),
        QueryResult::Value(SemanticQueryValue::SignatureResult(value)) => {
            Some(value.result.epoch())
        }
        _ => None,
    }
}

impl FamilyKey {
    /// Whether this family's KEY names a kernel handle of an epoch other
    /// than `current`. Only `ReadSignatureResult` keys name handles (its
    /// descriptor and call substitution); such a family can never be
    /// looked up again once its epoch is retired, because every live
    /// request builds its key from current-epoch handles.
    pub(super) fn names_retired_kernel_epoch(&self, current: GraphEpoch) -> bool {
        match self {
            FamilyKey::ReadSignatureResult { key } => {
                key.descriptor.epoch() != current || key.call_substitution.epoch() != current
            }
            _ => false,
        }
    }

    /// Whether this family's values can carry kernel handles.
    fn bears_kernel_handles(&self) -> bool {
        matches!(
            self,
            FamilyKey::SignaturesOfType { .. } | FamilyKey::ReadSignatureResult { .. }
        )
    }
}

impl SemanticGraphStore {
    /// Whether `result` carries kernel handles of a retired epoch. Only the
    /// kernel-bearing value tags read the current epoch; every other value
    /// answers from its discriminant.
    #[inline]
    pub(super) fn names_retired_kernel_epoch(
        &self,
        result: &QueryResult<SemanticQueryValue>,
    ) -> bool {
        value_kernel_epoch(result).is_some_and(|epoch| epoch != self.signatures.epoch())
    }

    /// The warm gates a candidate passes before it serves, cheapest first:
    /// its recorded materialised set dominates the request (§3.4), it
    /// names no retired kernel epoch, and its fact rail validates against
    /// the caller's view. A candidate failing any gate is skipped without
    /// bubbling, and the caller recomputes.
    #[inline]
    pub(super) fn warm_candidate_serves(
        &self,
        entry: &MemoEntry,
        requested: &MaterializedPoint,
        ctx: &dyn crate::resolver_core::ResolverContext,
    ) -> bool {
        cached_satisfies(&entry.satisfied_projection, requested)
            && !self.names_retired_kernel_epoch(&entry.result)
            && entry.validate(ctx)
    }

    /// Replace the signature kernel's epoch once its interners hold more
    /// than its record cap
    /// ([`EPOCH_RECORD_CAP`](crate::signature_kernel::EPOCH_RECORD_CAP)), and
    /// drop the memo families and candidates the retired epoch leaves
    /// unreachable. Returns the new epoch, or `None` while the store is under
    /// the cap.
    ///
    /// Safe at any point where the caller holds no memo lock: warm lookups
    /// refuse retired-epoch values, joiners fork away from an in-flight
    /// build that publishes one, and a consumer whose pin lands after the
    /// replacement re-reads once. Pinned readers and retained results keep
    /// their own epoch alive until they are released.
    pub fn compact_signature_store_if_over_cap(&self) -> Option<GraphEpoch> {
        self.compact_signature_store_over(self.signatures.record_cap())
    }

    /// [`Self::compact_signature_store_if_over_cap`] against an explicit
    /// record cap.
    pub(crate) fn compact_signature_store_over(&self, cap: usize) -> Option<GraphEpoch> {
        let epoch = self.signatures.replace_epoch_if_over(cap)?;
        self.evict_retired_kernel_candidates(epoch);
        Some(epoch)
    }

    /// Remove every kernel-bearing candidate that names an epoch other than
    /// `current` — in its family key or in its value — and every family
    /// left empty. Runs under one `entries` hold, like every multi-member
    /// mutation of the family memo, so `memo_budget` and
    /// `canonical_to_entries` move with `entries`. Returns the number of
    /// candidates removed.
    fn evict_retired_kernel_candidates(&self, current: GraphEpoch) -> usize {
        let mut removed = 0usize;
        let mut entries = self.entries_lock_diagnosed();
        entries.retain(|family, slots| {
            if !family.bears_kernel_handles() {
                return true;
            }
            let retired_key = family.names_retired_kernel_epoch(current);
            let mut evicted: Vec<MemoEntry> = Vec::new();
            slots.retain_candidates_in_slot_mut(ModeSlot::Single, |entry| {
                let live = !retired_key
                    && value_kernel_epoch(&entry.result).is_none_or(|epoch| epoch == current);
                if !live {
                    evicted.push(entry.clone());
                }
                live
            });
            for entry in &evicted {
                reverse_index::drain_candidate_reverse_index_registrations(
                    &self.canonical_to_entries,
                    family,
                    ModeSlot::Single,
                    entry,
                );
            }
            removed += evicted.len();
            if slots.populated_count() > 0 {
                true
            } else {
                // Sound under the held `entries` lock: no admission of
                // `family` can race this key-wide forget.
                self.memo_budget.forget_key_under_exclusive_lock(family);
                false
            }
        });
        removed
    }
}

#[cfg(test)]
#[path = "signature_epoch_tests.rs"]
mod tests;
