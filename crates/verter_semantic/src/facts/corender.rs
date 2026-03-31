//! Co-renderability facts.
//!
//! Determines whether two components can be rendered at the same time
//! in the same page/layout, using finite inputs only. No SMT/SAT solver.

use serde::{Deserialize, Serialize};

/// Co-renderability status between two components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CoRenderabilityStatus {
    /// Both components are definitely rendered together (shared subtree).
    Definite,
    /// Both components may be rendered together (shared layout, different routes).
    Possible,
    /// The components are never rendered together (mutually exclusive routes, v-if/v-else).
    MutuallyExclusive,
    /// Cannot determine co-renderability.
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn co_renderability_variants_are_distinct() {
        let all = [
            CoRenderabilityStatus::Definite,
            CoRenderabilityStatus::Possible,
            CoRenderabilityStatus::MutuallyExclusive,
            CoRenderabilityStatus::Unknown,
        ];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
    }

    #[test]
    fn co_renderability_serializes() {
        let status = CoRenderabilityStatus::MutuallyExclusive;
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("MutuallyExclusive"));
        let back: CoRenderabilityStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, back);
    }

    #[test]
    fn all_statuses_round_trip() {
        for status in [
            CoRenderabilityStatus::Definite,
            CoRenderabilityStatus::Possible,
            CoRenderabilityStatus::MutuallyExclusive,
            CoRenderabilityStatus::Unknown,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let back: CoRenderabilityStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, back);
        }
    }

    #[test]
    fn definite_is_not_mutually_exclusive() {
        assert_ne!(
            CoRenderabilityStatus::Definite,
            CoRenderabilityStatus::MutuallyExclusive
        );
    }

    #[test]
    fn possible_is_distinct_from_unknown() {
        assert_ne!(
            CoRenderabilityStatus::Possible,
            CoRenderabilityStatus::Unknown
        );
    }

    #[test]
    fn possible_is_not_definite() {
        assert_ne!(
            CoRenderabilityStatus::Possible,
            CoRenderabilityStatus::Definite
        );
    }

    #[test]
    fn unknown_is_not_exclusive() {
        assert_ne!(
            CoRenderabilityStatus::Unknown,
            CoRenderabilityStatus::MutuallyExclusive
        );
    }
}
