//! A resolved ambient (global `.d.ts`) symbol lookup hit.
//!
//! Plain data — the ambient registry, registration, and lookup storage
//! stay host-owned; this is only the value type a lookup hands back.

use std::sync::Arc;

use super::project_stable_key::ProjectStableKey;

/// One ambient-symbol registry hit: which project's ambient lib set
/// declared the symbol, and where it lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmbientSymbolHit {
    pub project: ProjectStableKey,
    pub canonical_id: Arc<str>,
    pub virtual_id: Arc<str>,
    pub lib_order: u32,
}

#[cfg(test)]
#[path = "ambient_symbol_hit_tests.rs"]
mod tests;
