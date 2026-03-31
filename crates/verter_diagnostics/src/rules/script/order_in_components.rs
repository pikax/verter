//! Rule: order-in-components
//!
//! Enforces a consistent order for Options API options. When using the Options API
//! (e.g., `defineComponent()`), options should follow a standard order:
//! name, components, props, data, computed, methods, watch, lifecycle hooks.
//!
//! This is a simplified stub that checks for bindings named after common Options API
//! properties and warns when they appear out of order.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::Severity;
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::types::ScriptAnalysisSnapshot;

pub struct OrderInComponents;

/// Canonical option order for Options API properties.
#[allow(dead_code)]
const OPTION_ORDER: &[&str] = &[
    "name",
    "components",
    "directives",
    "props",
    "emits",
    "setup",
    "data",
    "computed",
    "watch",
    "methods",
    // lifecycle hooks
    "beforeCreate",
    "created",
    "beforeMount",
    "mounted",
    "beforeUpdate",
    "updated",
    "beforeUnmount",
    "unmounted",
    "render",
];

impl LintRule for OrderInComponents {
    fn name(&self) -> &'static str {
        "order-in-components"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_script(&self, _script: &ScriptAnalysisSnapshot, _ctx: &mut LintContext) {
        // Stub: Options API ordering requires deeper AST analysis of the
        // defineComponent() object literal, which is not yet available in
        // ScriptAnalysisSnapshot. This rule is registered for future implementation.
        //
        // When implemented, it will:
        // 1. Find bindings that are option names (name, components, props, etc.)
        // 2. Check their span order matches OPTION_ORDER
        // 3. Report out-of-order options
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;

    fn run_script(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(OrderInComponents)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_script(script, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn empty_script_passes() {
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_script(&script);
        assert!(diags.is_empty(), "empty script should pass");
    }

    #[test]
    fn stub_does_not_trigger() {
        // Until fully implemented, the stub should never emit diagnostics
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_script(&script);
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
        assert!(
            diags.is_empty(),
            "stub rule should not emit any diagnostics"
        );
    }

    #[test]
    fn option_order_is_correct() {
        // Verify the constant is well-formed
        assert_eq!(OPTION_ORDER[0], "name");
        assert!(OPTION_ORDER.contains(&"components"));
        assert!(OPTION_ORDER.contains(&"mounted"));
        assert!(!OPTION_ORDER.contains(&"unknown"));
    }
}
