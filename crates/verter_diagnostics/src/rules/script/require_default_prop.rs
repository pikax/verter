//! Rule: require-default-prop
//!
//! Non-required props should have a default value. Without a default, the prop
//! is implicitly `undefined` when not passed, which may cause runtime errors.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateAnalysisSnapshot;

pub struct RequireDefaultProp;

impl LintRule for RequireDefaultProp {
    fn name(&self) -> &'static str {
        "require-default-prop"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for prop in &tpl.prop_definitions {
            // Boolean props are implicitly `false` when absent — no default needed
            if prop.is_boolean {
                continue;
            }
            // Required props don't need a default
            if prop.is_required {
                continue;
            }
            // Already has a default
            if prop.has_default {
                continue;
            }
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "Prop '{}' is not required and has no default value. \
                     Add a default value or mark it as required.",
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

#[cfg(test)]
mod tests {
    use super::*;

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(RequireDefaultProp, template)
    }

    fn make_prop(
        name: &str,
        is_required: bool,
        has_default: bool,
        is_boolean: bool,
    ) -> AnalyzedPropDefinition {
        AnalyzedPropDefinition {
            name: name.to_string(),
            type_annotation: Some("String".to_string()),
            has_default,
            is_required,
            is_boolean,
            used_in_template: false,
            used_in_script: false,
            span: Span::new(10, 30),
        }
    }

    #[test]
    fn optional_prop_without_default_reports() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("label", false, false, false)],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "optional prop without default should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "require-default-prop"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn required_prop_passes() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("label", true, false, false)],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "required prop should pass");
    }

    #[test]
    fn optional_with_default_passes() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("label", false, true, false)],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "optional prop with default should pass");
    }

    #[test]
    fn boolean_prop_passes() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("disabled", false, false, true)],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "boolean prop should pass (implicitly false)"
        );
    }
}
