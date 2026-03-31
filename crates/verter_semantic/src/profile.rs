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

    /// Recommended analysis scope bits for this profile.
    ///
    /// This bridges the transition from `AnalysisScope` bitflags to
    /// lazy semantic queries. The returned value indicates which analysis
    /// passes should be prewarmed for this profile.
    ///
    /// Returns a u32 that can be used with `AnalysisScope::from_bits_truncate()`.
    pub fn recommended_analysis_scope_bits(&self) -> u32 {
        // Bit positions match AnalysisScope in verter_analysis::scope
        const IMPORTS: u32 = 1 << 0;
        const BINDINGS: u32 = 1 << 1;
        const REACTIVITY: u32 = 1 << 2;
        const MACROS: u32 = 1 << 3;
        const MACRO_TYPE_DEPS: u32 = 1 << 4;
        const VUE_API_USAGE: u32 = 1 << 5;
        const EXPORT_SIGNATURES: u32 = 1 << 6;
        const TPL_COMPONENTS: u32 = 1 << 8;
        const TPL_BINDINGS: u32 = 1 << 9;
        const TPL_SLOTS: u32 = 1 << 10;
        const TPL_REFS: u32 = 1 << 11;
        const TPL_EVENTS: u32 = 1 << 12;
        const TPL_CONSTNESS: u32 = 1 << 13;
        const STYLE_CSS: u32 = 1 << 16;
        const STYLE_VBIND: u32 = 1 << 17;
        const STYLE_SCOPED: u32 = 1 << 18;
        const CROSS_PROP_CONST: u32 = 1 << 26;

        let script_base = IMPORTS | BINDINGS | REACTIVITY | MACROS;
        let tpl_base = TPL_COMPONENTS | TPL_BINDINGS | TPL_SLOTS | TPL_REFS | TPL_EVENTS;
        let style_base = STYLE_VBIND | STYLE_SCOPED;

        match self {
            QueryProfile::Build => script_base | MACRO_TYPE_DEPS | EXPORT_SIGNATURES,
            QueryProfile::BuildOptimized => {
                script_base
                    | MACRO_TYPE_DEPS
                    | EXPORT_SIGNATURES
                    | tpl_base
                    | TPL_CONSTNESS
                    | style_base
                    | CROSS_PROP_CONST
            }
            QueryProfile::LspInteractive => script_base | tpl_base | style_base,
            QueryProfile::LspBackground => {
                script_base
                    | MACRO_TYPE_DEPS
                    | VUE_API_USAGE
                    | EXPORT_SIGNATURES
                    | tpl_base
                    | TPL_CONSTNESS
                    | style_base
                    | STYLE_CSS
                    | CROSS_PROP_CONST
            }
            QueryProfile::Lint => {
                script_base | VUE_API_USAGE | tpl_base | TPL_CONSTNESS | style_base | STYLE_CSS
            }
            QueryProfile::Mcp => {
                script_base
                    | MACRO_TYPE_DEPS
                    | VUE_API_USAGE
                    | EXPORT_SIGNATURES
                    | tpl_base
                    | TPL_CONSTNESS
                    | style_base
                    | STYLE_CSS
                    | CROSS_PROP_CONST
            }
            QueryProfile::ComponentMeta => {
                script_base | MACRO_TYPE_DEPS | VUE_API_USAGE | EXPORT_SIGNATURES | tpl_base
            }
        }
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

    #[test]
    fn build_scope_is_minimal() {
        let bits = QueryProfile::Build.recommended_analysis_scope_bits();
        // Positive: has imports, bindings, macros, macro type deps, exports
        assert_ne!(bits & (1 << 0), 0, "should have IMPORTS");
        assert_ne!(bits & (1 << 1), 0, "should have BINDINGS");
        assert_ne!(bits & (1 << 3), 0, "should have MACROS");
        assert_ne!(bits & (1 << 4), 0, "should have MACRO_TYPE_DEPS");
        assert_ne!(bits & (1 << 6), 0, "should have EXPORT_SIGNATURES");
        // Negative: no template or style analysis in basic build
        assert_eq!(bits & (1 << 8), 0, "should NOT have TPL_COMPONENTS");
        assert_eq!(bits & (1 << 16), 0, "should NOT have STYLE_CSS");
    }

    #[test]
    fn lsp_interactive_includes_template() {
        let bits = QueryProfile::LspInteractive.recommended_analysis_scope_bits();
        assert_ne!(bits & (1 << 8), 0, "should have TPL_COMPONENTS");
        assert_ne!(bits & (1 << 9), 0, "should have TPL_BINDINGS");
        // Negative: no cross-file in interactive mode
        assert_eq!(bits & (1 << 26), 0, "should NOT have CROSS_PROP_CONST");
    }

    #[test]
    fn lsp_background_is_most_comprehensive() {
        let bg = QueryProfile::LspBackground.recommended_analysis_scope_bits();
        let interactive = QueryProfile::LspInteractive.recommended_analysis_scope_bits();
        // Background should be a superset of interactive
        assert_eq!(
            bg & interactive,
            interactive,
            "background should include all interactive bits"
        );
        // Plus additional analysis
        assert_ne!(bg & (1 << 16), 0, "should have STYLE_CSS");
        assert_ne!(bg & (1 << 26), 0, "should have CROSS_PROP_CONST");
    }

    #[test]
    fn build_optimized_includes_cross_file() {
        let bits = QueryProfile::BuildOptimized.recommended_analysis_scope_bits();
        assert_ne!(bits & (1 << 26), 0, "should have CROSS_PROP_CONST");
        assert_ne!(bits & (1 << 13), 0, "should have TPL_CONSTNESS");
    }

    #[test]
    fn lint_includes_vue_api_usage() {
        let bits = QueryProfile::Lint.recommended_analysis_scope_bits();
        assert_ne!(bits & (1 << 5), 0, "should have VUE_API_USAGE");
        // Negative: no export signatures (not needed for lint)
        assert_eq!(bits & (1 << 6), 0, "should NOT have EXPORT_SIGNATURES");
    }

    #[test]
    fn mcp_is_comprehensive() {
        let bits = QueryProfile::Mcp.recommended_analysis_scope_bits();
        // MCP needs everything for explanations
        assert_ne!(bits & (1 << 4), 0, "should have MACRO_TYPE_DEPS");
        assert_ne!(bits & (1 << 5), 0, "should have VUE_API_USAGE");
        assert_ne!(bits & (1 << 6), 0, "should have EXPORT_SIGNATURES");
        assert_ne!(bits & (1 << 16), 0, "should have STYLE_CSS");
        assert_ne!(bits & (1 << 26), 0, "should have CROSS_PROP_CONST");
    }

    #[test]
    fn component_meta_includes_macros_and_template() {
        let bits = QueryProfile::ComponentMeta.recommended_analysis_scope_bits();
        assert_ne!(bits & (1 << 3), 0, "should have MACROS");
        assert_ne!(bits & (1 << 4), 0, "should have MACRO_TYPE_DEPS");
        assert_ne!(bits & (1 << 8), 0, "should have TPL_COMPONENTS");
        // Negative: no style CSS analysis needed for meta
        assert_eq!(bits & (1 << 16), 0, "should NOT have STYLE_CSS");
    }

    #[test]
    fn all_profiles_include_imports() {
        for profile in [
            QueryProfile::Build,
            QueryProfile::BuildOptimized,
            QueryProfile::LspInteractive,
            QueryProfile::LspBackground,
            QueryProfile::Lint,
            QueryProfile::Mcp,
            QueryProfile::ComponentMeta,
        ] {
            let bits = profile.recommended_analysis_scope_bits();
            assert_ne!(bits & (1 << 0), 0, "{profile:?} should include IMPORTS");
        }
    }
}
