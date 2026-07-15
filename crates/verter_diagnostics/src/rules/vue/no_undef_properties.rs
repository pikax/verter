//! Rule: no-undef-properties
//!
//! Detect template bindings that do not match any script binding. Uses the
//! `unresolved_bindings` from template analysis.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateAnalysisSnapshot;

pub struct NoUndefProperties;

impl LintRule for NoUndefProperties {
    fn name(&self) -> &'static str {
        "no-undef-properties"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for binding in &tpl.unresolved_bindings {
            // Skip Vue built-in globals ($refs, $emit, $slots, $attrs, $el, etc.)
            if binding.name.starts_with('$') {
                continue;
            }

            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "'{}' is used in the template but not defined in the script block.",
                    binding.name
                ),
                binding.span.start,
                binding.span.end,
                self.default_severity(),
                DiagnosticSpanKind::Interpolation,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoUndefProperties, template)
    }

    #[test]
    fn unresolved_binding_reports() {
        let template = TemplateAnalysisSnapshot {
            unresolved_bindings: vec![UnresolvedBinding {
                name: "unknownVar".to_string(),
                span: Span::new(15, 25),
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "unresolved binding should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-undef-properties"));
        assert!(
            diags[0].message.contains("unknownVar"),
            "message should mention the binding name"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn no_unresolved_passes() {
        let template = TemplateAnalysisSnapshot {
            unresolved_bindings: vec![],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "no unresolved bindings should pass");
    }

    #[test]
    fn vue_builtin_globals_pass() {
        let template = TemplateAnalysisSnapshot {
            unresolved_bindings: vec![
                UnresolvedBinding {
                    name: "$refs".to_string(),
                    span: Span::new(10, 15),
                },
                UnresolvedBinding {
                    name: "$emit".to_string(),
                    span: Span::new(20, 25),
                },
                UnresolvedBinding {
                    name: "$slots".to_string(),
                    span: Span::new(30, 36),
                },
            ],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "Vue built-in globals ($refs, $emit, $slots) should pass"
        );
    }
}
