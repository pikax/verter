//! # deep-composable-tracking
//!
//! Reports composable calls that have hidden side effects via transitive
//! dependencies (lifecycle hooks, watchers, provide/inject buried in the chain).
//!
//! ## Example
//! ```vue
//! <script setup>
//! // useMouse() → useEventListener() → onMounted + onUnmounted
//! const { x, y } = useMouse()
//! </script>
//! ```
//!
//! This rule helps developers understand the full impact of composable calls.
//! Requires deep analysis (FUNC_RETURNS scope) to populate composable chain data.

use crate::context::LintContext;
use crate::cross_file::CrossFileSnapshot;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};

/// Lint rule: report hidden side effects in composable call chains.
pub struct DeepComposableTracking;

impl LintRule for DeepComposableTracking {
    fn name(&self) -> &'static str {
        "deep-composable-tracking"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::CrossFile
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Info)
    }

    fn check_cross_file(&self, snapshot: &CrossFileSnapshot, ctx: &mut LintContext) {
        for chain in &snapshot.composable_chains {
            let mut effects = Vec::new();

            if !chain.lifecycle_hooks.is_empty() {
                effects.push(format!(
                    "lifecycle hooks: {}",
                    chain.lifecycle_hooks.join(", ")
                ));
            }
            if chain.has_watchers {
                effects.push("watchers".to_string());
            }
            if chain.has_provide_inject {
                effects.push("provide/inject".to_string());
            }

            if effects.is_empty() {
                continue;
            }

            let chain_str = chain.chain.join(" → ");
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "`{}()` has hidden side effects via [{}]: {}",
                    chain.composable_name,
                    chain_str,
                    effects.join(", "),
                ),
                chain.span.start,
                chain.span.end,
                self.default_severity(),
                DiagnosticSpanKind::ScriptCallSite,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::cross_file::ComposableChainEntry;

    fn run_rule(snapshot: &CrossFileSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_cross_file_rule(DeepComposableTracking, snapshot)
    }

    #[test]
    fn empty_chains_no_diagnostics() {
        let snapshot = CrossFileSnapshot::default();
        assert!(run_rule(&snapshot).is_empty());
    }

    #[test]
    fn chain_with_lifecycle_reports() {
        let snapshot = CrossFileSnapshot {
            composable_chains: vec![ComposableChainEntry {
                composable_name: "useMouse".to_string(),
                chain: vec!["useMouse".to_string(), "useEventListener".to_string()],
                lifecycle_hooks: vec!["onMounted".to_string(), "onUnmounted".to_string()],
                has_watchers: false,
                has_provide_inject: false,
                span: verter_span::Span::new(10, 30),
            }],
            ..Default::default()
        };
        let diags = run_rule(&snapshot);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "deep-composable-tracking");
        assert!(diags[0].message.contains("useMouse()"));
        assert!(diags[0].message.contains("onMounted"));
    }

    #[test]
    fn chain_with_watchers_reports() {
        let snapshot = CrossFileSnapshot {
            composable_chains: vec![ComposableChainEntry {
                composable_name: "useAutoSave".to_string(),
                chain: vec!["useAutoSave".to_string()],
                lifecycle_hooks: vec![],
                has_watchers: true,
                has_provide_inject: false,
                span: verter_span::Span::new(5, 25),
            }],
            ..Default::default()
        };
        let diags = run_rule(&snapshot);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("watchers"));
    }

    #[test]
    fn chain_with_no_effects_skipped() {
        let snapshot = CrossFileSnapshot {
            composable_chains: vec![ComposableChainEntry {
                composable_name: "useClean".to_string(),
                chain: vec!["useClean".to_string()],
                lifecycle_hooks: vec![],
                has_watchers: false,
                has_provide_inject: false,
                span: verter_span::Span::new(5, 25),
            }],
            ..Default::default()
        };
        assert!(run_rule(&snapshot).is_empty());
    }
}
