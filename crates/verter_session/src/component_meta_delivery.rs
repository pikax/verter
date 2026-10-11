//! What one component-meta read hands back.
//!
//! A complete fact and a retained refusal both carry the analysis bytes the
//! producer computed. They are not the same delivery: the status says which
//! one the caller received. A refusal is never a successful value.

use std::ops::Deref;

use verter_session_query::analysis::component_meta::ComponentMetaAnalysis;
use verter_type_engine::semantic_query::{FactStatus, ResultCompleteness};

/// Analysis plus the fact status the read actually delivered.
#[derive(Debug, Clone)]
pub struct ComponentMetaDelivery {
    analysis: ComponentMetaAnalysis,
    status: FactStatus,
}

impl ComponentMetaDelivery {
    /// A successful candidate.
    #[must_use]
    pub fn complete(analysis: ComponentMetaAnalysis) -> Self {
        Self {
            analysis,
            status: FactStatus::Complete,
        }
    }

    /// The original refused outcome. `status` is the status that was sealed
    /// with it, not [`FactStatus::Complete`].
    #[must_use]
    pub fn refusal(recovery: ComponentMetaAnalysis, status: FactStatus) -> Self {
        Self {
            analysis: recovery,
            status,
        }
    }

    /// Cold delivery. A partial compute is unavailable, including when the
    /// store does not retain it. Only an empty reason set is complete.
    #[must_use]
    pub fn from_completeness(
        analysis: ComponentMetaAnalysis,
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
        &self.analysis
    }

    /// The analysis bytes, by value.
    #[must_use]
    pub fn into_analysis(self) -> ComponentMetaAnalysis {
        self.analysis
    }
}

impl Deref for ComponentMetaDelivery {
    type Target = ComponentMetaAnalysis;

    fn deref(&self) -> &ComponentMetaAnalysis {
        &self.analysis
    }
}
