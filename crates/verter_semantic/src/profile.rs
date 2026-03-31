//! Query profiles — session-level execution policy.
//!
//! Query profiles control prewarming, budgets, and allowed query families.
//! They are **not** semantic identity — they do not change the meaning of
//! a query result. They are hints that the session uses to decide how
//! aggressively to materialize inputs and which background work to schedule.

use serde::{Deserialize, Serialize};

/// Execution profile for a session.
///
/// Controls which semantic/compile queries may be prefetched, which expensive
/// query families are allowed, which latency budget applies, and which
/// background work can be deferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QueryProfile {
    /// Bundler build — all compile queries, no interactive latency requirement.
    Build,
    /// Optimized build — full compile + semantic optimization queries.
    BuildOptimized,
    /// LSP interactive — fast responses, defer expensive cross-file work.
    LspInteractive,
    /// LSP background — workspace scan, full diagnostics, background prewarming.
    LspBackground,
    /// Lint pass — full diagnostics, no compile artifacts needed.
    Lint,
    /// MCP server — full analysis and explanation queries.
    Mcp,
    /// Component metadata extraction.
    ComponentMeta,
}

impl QueryProfile {
    /// Whether this profile allows background materialization of missing inputs.
    pub fn allows_background_materialization(&self) -> bool {
        matches!(
            self,
            QueryProfile::LspBackground
                | QueryProfile::Build
                | QueryProfile::BuildOptimized
                | QueryProfile::Mcp
        )
    }

    /// Whether this profile requires low-latency responses.
    pub fn is_interactive(&self) -> bool {
        matches!(self, QueryProfile::LspInteractive)
    }

    /// Whether this profile allows expensive cross-file queries.
    pub fn allows_cross_file_queries(&self) -> bool {
        !matches!(self, QueryProfile::LspInteractive)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_profile_restricts() {
        let p = QueryProfile::LspInteractive;
        assert!(p.is_interactive());
        assert!(!p.allows_background_materialization());
        assert!(!p.allows_cross_file_queries());
    }

    #[test]
    fn build_profile_allows_everything() {
        let p = QueryProfile::Build;
        assert!(!p.is_interactive());
        assert!(p.allows_background_materialization());
        assert!(p.allows_cross_file_queries());
    }

    #[test]
    fn all_profiles_are_distinct() {
        let profiles = [
            QueryProfile::Build,
            QueryProfile::BuildOptimized,
            QueryProfile::LspInteractive,
            QueryProfile::LspBackground,
            QueryProfile::Lint,
            QueryProfile::Mcp,
            QueryProfile::ComponentMeta,
        ];
        for (i, a) in profiles.iter().enumerate() {
            for (j, b) in profiles.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
    }

    #[test]
    fn profile_serializes() {
        let p = QueryProfile::ComponentMeta;
        let json = serde_json::to_string(&p).unwrap();
        let back: QueryProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }
}
