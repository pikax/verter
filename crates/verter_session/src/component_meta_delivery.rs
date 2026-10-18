//! What one component-meta read hands back.
//!
//! A complete fact and a retained refusal both carry the analysis bytes the
//! producer computed. They are not the same delivery: the status says which
//! one the caller received. A refusal is never a successful value.

use std::ops::Deref;

use verter_session_query::analysis::component_meta::ComponentMetaAnalysis;

use crate::component_meta_cached_result::SharedComponentMetaAnalysis;
use verter_type_engine::semantic_query::{FactStatus, ResultCompleteness};

/// Analysis plus the fact status the read actually delivered.
///
/// The analysis is the shared cell the final-result cache keeps (or, for an
/// uncached result, its own cell): a delivery a caller keeps past the cache
/// keeps the analysis charged until its last holder drops.
#[derive(Debug, Clone)]
pub struct ComponentMetaDelivery {
    analysis: SharedComponentMetaAnalysis,
    status: FactStatus,
}

impl ComponentMetaDelivery {
    /// A successful candidate.
    #[must_use]
    pub fn complete(analysis: impl Into<SharedComponentMetaAnalysis>) -> Self {
        Self::deliver(analysis.into(), FactStatus::Complete)
    }

    /// The original refused outcome. `status` is the status that was sealed
    /// with it, not [`FactStatus::Complete`].
    #[must_use]
    pub fn refusal(recovery: impl Into<SharedComponentMetaAnalysis>, status: FactStatus) -> Self {
        Self::deliver(recovery.into(), status)
    }

    /// Deliver `analysis`: charged to its readers unless a cache already
    /// retains it, whose charge it then shares.
    fn deliver(analysis: SharedComponentMetaAnalysis, status: FactStatus) -> Self {
        analysis.pin(verter_session_query::retention::owned::ChargeCtx::new(
            &verter_session_query::retention::SemanticRetentionAccount::process_local(),
        ));
        Self { analysis, status }
    }

    /// Cold delivery. A partial compute is unavailable, including when the
    /// store does not retain it. Only an empty reason set is complete.
    #[must_use]
    pub fn from_completeness(
        analysis: impl Into<SharedComponentMetaAnalysis>,
        completeness: ResultCompleteness,
    ) -> Self {
        match crate::component_meta_result_db::final_component_refusal_status(
            completeness.reasons(),
        ) {
            Some(status) => Self::refusal(analysis, status),
            None => Self::complete(analysis),
        }
    }

    /// The fact status of this delivery.
    #[must_use]
    pub fn status(&self) -> FactStatus {
        self.status
    }

    /// Whether this delivery is a complete fact.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.status.is_complete()
    }

    /// The analysis bytes, borrowed.
    #[must_use]
    pub fn analysis(&self) -> &ComponentMetaAnalysis {
        &self.analysis.0
    }

    /// The shared analysis cell.
    #[must_use]
    pub fn shared_analysis(&self) -> &SharedComponentMetaAnalysis {
        &self.analysis
    }

    /// A plain copy of the analysis bytes, owned by the caller.
    #[must_use]
    pub fn into_analysis(self) -> ComponentMetaAnalysis {
        self.analysis.0.clone()
    }
}

impl Deref for ComponentMetaDelivery {
    type Target = ComponentMetaAnalysis;

    fn deref(&self) -> &ComponentMetaAnalysis {
        &self.analysis.0
    }
}

#[cfg(test)]
#[path = "component_meta_delivery_tests.rs"]
mod tests;
