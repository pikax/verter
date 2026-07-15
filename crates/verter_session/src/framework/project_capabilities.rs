//! Project capability snapshot: the DERIVED capability bits gating
//! candidate language rows.

use std::collections::BTreeSet;

use verter_language::CapabilityId;

use crate::types::Hash16;

/// Immutable snapshot of the project's DERIVED capability bits.
///
/// A capability bit is derived project knowledge (e.g. "this project is
/// an Angular workspace"), never raw configuration bytes: a config edit
/// that flips no bit leaves the snapshot — and therefore its
/// [`hash`](Self::hash) — unchanged, so classification caching keyed on
/// the hash invalidates only on real capability flips.
///
/// Invalidation scoping: the snapshot hash keys CLASSIFICATION caching
/// only. Per-file artifact identity carries the resolved
/// `file_language_id` column on the `FileArtifactStore` key instead, so
/// a gated-row flip invalidates exactly the affected files' artifacts.
/// Nothing capability-shaped enters the global `parse_env_hash`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectCapabilitySnapshot {
    /// Derived capability bits, ordered for hash stability.
    enabled: BTreeSet<CapabilityId>,
}

impl ProjectCapabilitySnapshot {
    /// The empty snapshot (no capability bits derived).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Build a snapshot from derived capability bits.
    pub fn from_capabilities<I: IntoIterator<Item = CapabilityId>>(bits: I) -> Self {
        Self {
            enabled: bits.into_iter().collect(),
        }
    }

    /// Whether a capability bit is derived ON.
    pub fn is_enabled(&self, capability: &CapabilityId) -> bool {
        self.enabled.contains(capability)
    }

    /// Stable hash over the DERIVED capability bits (never raw config
    /// bytes). Two snapshots with the same bit set hash identically
    /// regardless of construction order.
    pub fn hash(&self) -> Hash16 {
        let mut buf = Vec::new();
        for capability in &self.enabled {
            buf.extend_from_slice(capability.as_str().as_bytes());
            buf.push(0);
        }
        crate::hash::hash_16(&buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_over_derived_bits_and_order_independent() {
        let a = ProjectCapabilitySnapshot::from_capabilities([
            CapabilityId::new("alpha"),
            CapabilityId::new("beta"),
        ]);
        let b = ProjectCapabilitySnapshot::from_capabilities([
            CapabilityId::new("beta"),
            CapabilityId::new("alpha"),
        ]);
        assert_eq!(a.hash(), b.hash(), "bit-set hash must be order-independent");

        let c = ProjectCapabilitySnapshot::from_capabilities([CapabilityId::new("alpha")]);
        assert_ne!(a.hash(), c.hash(), "different bit sets must hash apart");
        assert_ne!(
            c.hash(),
            ProjectCapabilitySnapshot::empty().hash(),
            "a derived bit must change the hash"
        );
    }

    #[test]
    fn is_enabled_reads_the_bit_set() {
        let snapshot = ProjectCapabilitySnapshot::from_capabilities([CapabilityId::new("fixture")]);
        assert!(snapshot.is_enabled(&CapabilityId::new("fixture")));
        assert!(!snapshot.is_enabled(&CapabilityId::new("other")));
        assert!(!ProjectCapabilitySnapshot::empty().is_enabled(&CapabilityId::new("fixture")));
    }
}
