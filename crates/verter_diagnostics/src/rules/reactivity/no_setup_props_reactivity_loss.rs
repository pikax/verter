//! Rule: no-setup-props-reactivity-loss
//!
//! Disallow destructuring props in `<script setup>`, which causes reactivity
//! loss. Destructured prop bindings become plain values that do not update
//! when the parent re-renders.
//!
//! Bad:
//! ```vue
//! const { msg } = defineProps<{ msg: string }>()
//! ```
//!
//! Good:
//! ```vue
//! const props = defineProps<{ msg: string }>()
//! ```

use crate::context::LintContext;
use crate::diagnostic::Severity;
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::types::ScriptAnalysisSnapshot;

/// Disallow destructuring props in `<script setup>`.
pub struct NoSetupPropsReactivityLoss;

impl LintRule for NoSetupPropsReactivityLoss {
    fn name(&self) -> &'static str {
        "no-setup-props-reactivity-loss"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Reactivity
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, _ctx: &mut LintContext) {
        // Look for defineProps macros. If one exists without a binding name,
        // it was destructured (e.g., `const { msg } = defineProps<...>()`).
        // The analysis snapshot currently stores the binding_name as the
        // identifier on the left side of the assignment. When defineProps is
        // destructured, the analyzer records each destructured name individually
        // rather than a single identifier.
        //
        // For now, this is a stub that validates rule wiring. A full
        // implementation would check macro analysis for destructured patterns.
        for macro_call in &script.macros {
            if macro_call.kind == verter_semantic::analysis::types::AnalyzedMacroKind::DefineProps {
                // Stub: binding_name being None could indicate destructuring
                // in some analysis implementations. Full detection requires
                // AST-level destructuring pattern analysis.
                let _ = macro_call;
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
        let rule = NoSetupPropsReactivityLoss;
        assert_eq!(rule.name(), "no-setup-props-reactivity-loss");
        assert_eq!(rule.category(), RuleCategory::Reactivity);
        assert_eq!(rule.default_severity(), Some(Severity::Warning));
    }

    /// @ai-generated - check_script runs without panic on empty macros
    #[test]
    fn empty_macros_no_diagnostic() {
        let rule = NoSetupPropsReactivityLoss;
        let script = ScriptAnalysisSnapshot::default();
        let config = crate::config::LintConfig::default();
        let mut ctx = LintContext::new(&config);
        rule.check_script(&script, &mut ctx);
        assert!(ctx.into_diagnostics().is_empty());
    }
}
