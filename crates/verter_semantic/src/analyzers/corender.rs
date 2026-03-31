//! Co-renderability analyzer.
//!
//! Determines whether two components can be rendered simultaneously.
//! Uses finite inputs only — no SMT/SAT solver. When uncertain,
//! degrades conservatively to `Possible` or `Unknown`.

use serde::{Deserialize, Serialize};

use crate::facts::corender::CoRenderabilityStatus;

/// Co-renderability report between two components.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoRenderabilityReport {
    pub file_a: String,
    pub file_b: String,
    pub status: CoRenderabilityStatus,
    pub reason: String,
}

/// Determine co-renderability from structural information.
///
/// Finite inputs checked:
/// - Same parent (shared subtree) → Definite
/// - Disjoint route trees → MutuallyExclusive
/// - v-if/v-else branches → MutuallyExclusive
/// - Otherwise → Possible or Unknown
pub fn analyze_co_renderability(
    file_a: &str,
    file_b: &str,
    share_parent: bool,
    in_exclusive_branches: bool,
    in_disjoint_routes: bool,
) -> CoRenderabilityReport {
    let (status, reason) = if file_a == file_b {
        (
            CoRenderabilityStatus::Definite,
            "same component".to_string(),
        )
    } else if in_exclusive_branches {
        (
            CoRenderabilityStatus::MutuallyExclusive,
            "in mutually exclusive v-if/v-else branches".to_string(),
        )
    } else if in_disjoint_routes {
        (
            CoRenderabilityStatus::MutuallyExclusive,
            "in disjoint route trees".to_string(),
        )
    } else if share_parent {
        (
            CoRenderabilityStatus::Definite,
            "share a common parent in the render tree".to_string(),
        )
    } else {
        (
            CoRenderabilityStatus::Possible,
            "no definitive structural information available".to_string(),
        )
    };

    CoRenderabilityReport {
        file_a: file_a.to_string(),
        file_b: file_b.to_string(),
        status,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_component_is_definite() {
        let report = analyze_co_renderability("a.vue", "a.vue", false, false, false);
        assert_eq!(report.status, CoRenderabilityStatus::Definite);
    }

    #[test]
    fn shared_parent_is_definite() {
        let report = analyze_co_renderability("a.vue", "b.vue", true, false, false);
        assert_eq!(report.status, CoRenderabilityStatus::Definite);
        assert!(report.reason.contains("common parent"));
    }

    #[test]
    fn exclusive_branches_is_mutually_exclusive() {
        let report = analyze_co_renderability("a.vue", "b.vue", false, true, false);
        assert_eq!(report.status, CoRenderabilityStatus::MutuallyExclusive);
    }

    #[test]
    fn disjoint_routes_is_mutually_exclusive() {
        let report = analyze_co_renderability("a.vue", "b.vue", false, false, true);
        assert_eq!(report.status, CoRenderabilityStatus::MutuallyExclusive);
    }

    #[test]
    fn no_info_is_possible() {
        let report = analyze_co_renderability("a.vue", "b.vue", false, false, false);
        assert_eq!(report.status, CoRenderabilityStatus::Possible);
    }

    #[test]
    fn exclusive_branches_takes_priority_over_shared_parent() {
        // If both share a parent AND are in exclusive branches, exclusivity wins
        let report = analyze_co_renderability("a.vue", "b.vue", true, true, false);
        assert_eq!(report.status, CoRenderabilityStatus::MutuallyExclusive);
    }
}
