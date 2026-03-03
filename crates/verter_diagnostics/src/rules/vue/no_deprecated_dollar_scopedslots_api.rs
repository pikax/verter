//! Rule: no-deprecated-dollar-scopedslots-api
//!
//! `$scopedSlots` was removed in Vue 3. All slots are now unified under
//! `$slots` as functions. Detect `$scopedSlots` in template binding occurrences.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct NoDeprecatedDollarScopedslotsApi;

impl LintRule for NoDeprecatedDollarScopedslotsApi {
    fn name(&self) -> &'static str {
        "no-deprecated-dollar-scopedslots-api"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for occ in &tpl.binding_occurrences {
            if occ.name == "$scopedSlots" {
                ctx.report_with_tags(
                    self.name(),
                    self.category().as_str(),
                    "'$scopedSlots' has been removed in Vue 3. Use '$slots' instead — all slots are now functions.".to_string(),
                    occ.span.start,
                    occ.span.end,
                    self.default_severity(),
                    vec![DiagnosticTag::Deprecated],
                    DiagnosticSpanKind::Interpolation,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoDeprecatedDollarScopedslotsApi)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn dollar_scopedslots_reports() {
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "$scopedSlots".to_string(),
                span: Span::new(10, 22),
                usage_kind: BindingUsageKind::DirectiveValue,
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "$scopedSlots should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-deprecated-dollar-scopedslots-api"));
        assert!(
            diags[0].tags.contains(&DiagnosticTag::Deprecated),
            "should have Deprecated tag"
        );
        assert!(
            diags[0].message.contains("$slots"),
            "message should suggest $slots"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn dollar_slots_passes() {
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "$slots".to_string(),
                span: Span::new(10, 16),
                usage_kind: BindingUsageKind::DirectiveValue,
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "$slots should pass");
    }
}
