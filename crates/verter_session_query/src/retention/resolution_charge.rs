//! The retention charge a resolution publication holds against the process-local retention account.

/// The host's aggregate retention account, as the workspace sees it.
///
/// Dependency-neutral: the host adapts its own account to this, so the
/// workspace charges resident resolution state to the same ceiling every
/// other host store charges, without depending on the host.
pub trait ResolutionRetentionAccount: Send + Sync {
    /// Reserve `bytes` for a retained, evictable entry. `None` refuses the
    /// reservation: the caller serves its value without retaining it.
    fn reserve_retained(&self, bytes: usize) -> Option<ResolutionRetentionCharge>;

    /// Reserve the entry and claim its reachable evidence pages together.
    /// Byte-only test accounts cannot transfer page charges, so they refuse
    /// signatures that need that capability rather than retain pinned pages.
    fn reserve_retained_with_evidence(
        &self,
        bytes: usize,
        facts: &[crate::facts::fact_cache::FactVersionRef],
    ) -> Option<ResolutionRetentionCharge> {
        if crate::facts::receipt::has_unclaimed_evidence_pages(facts) {
            return None;
        }
        self.reserve_retained(bytes)
    }
}

/// One admitted reservation. Dropping it releases the bytes — once — which
/// is why an entry keeps its charge beside its payload.
pub struct ResolutionRetentionCharge {
    _charge: Box<dyn std::any::Any + Send + Sync>,
}

impl ResolutionRetentionCharge {
    /// Wrap the host account's own RAII charge.
    #[must_use]
    pub fn new(charge: impl std::any::Any + Send + Sync) -> Self {
        Self {
            _charge: Box::new(charge),
        }
    }
}

impl std::fmt::Debug for ResolutionRetentionCharge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ResolutionRetentionCharge")
    }
}
