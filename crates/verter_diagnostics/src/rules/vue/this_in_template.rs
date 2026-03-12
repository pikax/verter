//! Rule: this-in-template
//!
//! Disallow `this` in template expressions. In `<script setup>` and the Options API
//! template context, `this` is unnecessary and confusing.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct ThisInTemplate;

impl LintRule for ThisInTemplate {
    fn name(&self) -> &'static str {
        "this-in-template"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for occ in &tpl.binding_occurrences {
            if occ.name.starts_with("this.") || occ.name == "this" {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "Unexpected usage of 'this' in template. Template expressions automatically resolve to the component instance.".to_string(),
                    occ.span.start,
                    occ.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::Interpolation,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(ThisInTemplate, template)
    }

    #[test]
    fn this_prefix_reports() {
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "this.message".to_string(),
                span: Span::new(5, 17),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "this.message should trigger");
        assert!(diags.iter().any(|d| d.rule == "this-in-template"));
        assert!(
            diags[0].message.contains("this"),
            "message should mention 'this'"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn bare_binding_passes() {
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "message".to_string(),
                span: Span::new(5, 12),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "binding without 'this' should pass");
    }

    #[test]
    fn thistle_does_not_trigger() {
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "thistle".to_string(),
                span: Span::new(5, 12),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "'thistle' binding should not trigger");
    }
}
