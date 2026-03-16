//! Rule: no-ref-as-operand
//!
//! Disallow using ref values as operands directly (without `.value`).
//! When a binding created by `ref()` or `computed()` is used in a template
//! interpolation or as an event handler / iterator source, it may indicate
//! a missing `.value` access in script code.

use crate::context::LintContext;
use crate::diagnostic::Severity;
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{ReactivityKind, ScriptAnalysisSnapshot};

/// Disallow using ref values as operands directly (without `.value`).
pub struct NoRefAsOperand;

impl LintRule for NoRefAsOperand {
    fn name(&self) -> &'static str {
        "no-ref-as-operand"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Reactivity
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, _ctx: &mut LintContext) {
        // Flag ref/computed bindings. In a full implementation this would
        // cross-reference template binding occurrences to detect missing
        // `.value` usage. For now we report bindings whose reactivity kind
        // is Ref or Computed as informational metadata (stub).
        for binding in &script.bindings {
            if binding.reactivity_kind == ReactivityKind::Ref
                || binding.reactivity_kind == ReactivityKind::Computed
            {
                // In a future implementation this would be paired with
                // template analysis to detect actual misuse. For now, this
                // is a metadata-only check that validates the rule wiring.
                // No diagnostic emitted until cross-analysis is wired.
                let _ = binding;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// @ai-generated - Rule metadata is correct
    #[test]
    fn rule_metadata() {
        let rule = NoRefAsOperand;
        assert_eq!(rule.name(), "no-ref-as-operand");
        assert_eq!(rule.category(), RuleCategory::Reactivity);
        assert_eq!(rule.default_severity(), Some(Severity::Warning));
    }

    /// @ai-generated - check_script runs without panic on empty bindings
    #[test]
    fn empty_bindings_no_diagnostic() {
        let rule = NoRefAsOperand;
        let script = ScriptAnalysisSnapshot::default();
        let config = crate::config::LintConfig::default();
        let mut ctx = LintContext::new(&config);
        rule.check_script(&script, &mut ctx);
        assert!(ctx.into_diagnostics().is_empty());
    }
}
