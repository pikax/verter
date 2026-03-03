//! Rule: no-deprecated-destroyed-lifecycle
//!
//! The `destroyed` and `beforeDestroy` lifecycle hooks were renamed in Vue 3
//! to `unmounted` and `beforeUnmount`. Detect bindings with the old names.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::ScriptAnalysisSnapshot;

pub struct NoDeprecatedDestroyedLifecycle;

impl LintRule for NoDeprecatedDestroyedLifecycle {
    fn name(&self) -> &'static str {
        "no-deprecated-destroyed-lifecycle"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for binding in &script.bindings {
            let replacement = match binding.name.as_str() {
                "destroyed" => "unmounted",
                "beforeDestroy" => "beforeUnmount",
                _ => continue,
            };
            ctx.report_with_tags(
                self.name(),
                self.category().as_str(),
                format!(
                    "The '{}' lifecycle hook is deprecated in Vue 3. Use '{}' instead.",
                    binding.name, replacement
                ),
                binding.span.start,
                binding.span.end,
                self.default_severity(),
                vec![DiagnosticTag::Deprecated],
                DiagnosticSpanKind::ScriptCallSite,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::types::*;
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoDeprecatedDestroyedLifecycle)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_script(script, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn destroyed_binding_reports() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![AnalyzedBinding {
                name: "destroyed".to_string(),
                kind: AnalyzedBindingKind::Function,
                is_reactive: false,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span: Span::new(10, 19),
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(!diags.is_empty(), "'destroyed' binding should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-deprecated-destroyed-lifecycle"));
        assert!(
            diags[0].message.contains("unmounted"),
            "message should suggest 'unmounted'"
        );
        assert!(
            diags[0].tags.contains(&DiagnosticTag::Deprecated),
            "should have Deprecated tag"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn before_destroy_binding_reports() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![AnalyzedBinding {
                name: "beforeDestroy".to_string(),
                kind: AnalyzedBindingKind::Function,
                is_reactive: false,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span: Span::new(10, 23),
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(!diags.is_empty(), "'beforeDestroy' binding should trigger");
        assert!(
            diags[0].message.contains("beforeUnmount"),
            "message should suggest 'beforeUnmount'"
        );
    }

    #[test]
    fn on_unmounted_passes() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![AnalyzedBinding {
                name: "onUnmounted".to_string(),
                kind: AnalyzedBindingKind::Function,
                is_reactive: false,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span: Span::new(10, 21),
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "'onUnmounted' should pass");
    }
}
