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
