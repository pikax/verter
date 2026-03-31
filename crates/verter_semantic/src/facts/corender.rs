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
    fn all_statuses_round_trip_through_json() {
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
    fn mutually_exclusive_serializes_as_expected_string() {
        let json = serde_json::to_string(&CoRenderabilityStatus::MutuallyExclusive).unwrap();
        // Verify the exact serialized form consumers will see
        assert!(json.contains("MutuallyExclusive"));
    }
}
