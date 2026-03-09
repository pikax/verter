//! Rule: no-required-prop-with-default
//!
//! Disallows props that are both required and have a default value.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct NoRequiredPropWithDefault;

impl LintRule for NoRequiredPropWithDefault {
    fn name(&self) -> &'static str {
        "no-required-prop-with-default"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for prop in &tpl.prop_definitions {
            if prop.is_required && prop.has_default {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Prop '{}' is required but also has a default value. Remove `required: true` or the default.",
                        prop.name
                    ),
                    prop.span.start,
                    prop.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::PropDefinition,
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
        crate::test_support::run_template_rule(NoRequiredPropWithDefault, template)
    }

    fn make_prop(name: &str, is_required: bool, has_default: bool) -> AnalyzedPropDefinition {
        AnalyzedPropDefinition {
            name: name.to_string(),
            type_annotation: None,
            has_default,
            is_required,
            is_boolean: false,
            used_in_template: false,
            used_in_script: false,
            span: Span::new(10, 30),
        }
    }

    #[test]
    fn required_with_default_reports() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("title", true, true)],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "required prop with default should trigger"
        );
        assert!(diags[0].rule == "no-required-prop-with-default");
        assert!(
            !diags.iter().any(|d| d.rule == "no-reserved-props"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn required_without_default_passes() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("title", true, false)],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "required without default should pass");
    }

    #[test]
    fn optional_with_default_passes() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("title", false, true)],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "optional with default should pass");
    }
}
