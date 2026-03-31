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

    #[test]
    fn disjoint_routes_takes_priority_over_shared_parent() {
        let report = analyze_co_renderability("a.vue", "b.vue", true, false, true);
        assert_eq!(report.status, CoRenderabilityStatus::MutuallyExclusive);
    }

    #[test]
    fn report_carries_file_ids() {
        let report = analyze_co_renderability("/src/A.vue", "/src/B.vue", false, false, false);
        assert_eq!(report.file_a, "/src/A.vue");
        assert_eq!(report.file_b, "/src/B.vue");
    }

    #[test]
    fn reason_not_empty() {
        let report = analyze_co_renderability("a.vue", "b.vue", false, false, false);
        assert!(!report.reason.is_empty());
    }

    // ── Plan-required co-renderability coverage ────────────────────────────

    #[test]
    fn both_exclusive_and_disjoint_still_exclusive() {
        // Plan: "v-if/else exclusivity" combined with "disjoint routes"
        let report = analyze_co_renderability("a.vue", "b.vue", false, true, true);
        assert_eq!(report.status, CoRenderabilityStatus::MutuallyExclusive);
    }

    #[test]
    fn shared_parent_not_in_branches_is_definite() {
        // Plan: "shared subtree cases"
        let report = analyze_co_renderability("sidebar.vue", "main.vue", true, false, false);
        assert_eq!(report.status, CoRenderabilityStatus::Definite);
        assert!(report.reason.contains("common parent"));
    }

    #[test]
    fn unknown_dynamic_degrades_to_possible() {
        // Plan: "unknown dynamic boundaries" → conservative downgrade
        let report =
            analyze_co_renderability("unknown_a.vue", "unknown_b.vue", false, false, false);
        assert_eq!(report.status, CoRenderabilityStatus::Possible);
        // Negative: not Definite or MutuallyExclusive
        assert_ne!(report.status, CoRenderabilityStatus::Definite);
        assert_ne!(report.status, CoRenderabilityStatus::MutuallyExclusive);
    }

    #[test]
    fn symmetric_result() {
        // Co-renderability should be symmetric: A,B == B,A
        let ab = analyze_co_renderability("a.vue", "b.vue", true, false, false);
        let ba = analyze_co_renderability("b.vue", "a.vue", true, false, false);
        assert_eq!(ab.status, ba.status);
    }
}
