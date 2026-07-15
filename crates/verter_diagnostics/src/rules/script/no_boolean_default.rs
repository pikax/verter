//! Rule: no-boolean-default
//!
//! Boolean props should not have default values. A boolean prop defaults to
//! `false` automatically, so specifying `default: false` is redundant, and
//! `default: true` is a design smell.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateAnalysisSnapshot;

pub struct NoBooleanDefault;

impl LintRule for NoBooleanDefault {
    fn name(&self) -> &'static str {
        "no-boolean-default"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for prop in &tpl.prop_definitions {
            if prop.is_boolean && prop.has_default {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Boolean prop '{}' should not have a default value. Boolean props default to `false` automatically.",
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

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoBooleanDefault, template)
    }

    fn make_prop(name: &str, is_boolean: bool, has_default: bool) -> AnalyzedPropDefinition {
        AnalyzedPropDefinition {
            name: name.to_string(),
            type_annotation: if is_boolean {
                Some("boolean".to_string())
            } else {
                Some("string".to_string())
            },
            has_default,
            is_required: false,
            is_boolean,
            used_in_template: false,
            used_in_script: false,
            span: Span::new(10, 30),
        }
    }

    #[test]
    fn boolean_with_default_reports() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("disabled", true, true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "boolean prop with default should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "no-boolean-default"));
        assert!(
            diags[0].message.contains("disabled"),
            "message should mention prop name"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn boolean_without_default_passes() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("disabled", true, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "boolean prop without default should pass");
    }

    #[test]
    fn non_boolean_with_default_passes() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("title", false, true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "non-boolean prop with default should pass"
        );
    }

    #[test]
    fn no_props_passes() {
        let template = TemplateAnalysisSnapshot::default();
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "no props should pass");
    }
}
