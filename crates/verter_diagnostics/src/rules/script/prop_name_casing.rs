//! Rule: prop-name-casing
//!
//! Prop definitions should use camelCase naming. Prop names containing hyphens
//! (kebab-case) in the definition are incorrect — Vue expects camelCase in
//! `defineProps` and converts kebab-case usage from templates automatically.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct PropNameCasing;

impl LintRule for PropNameCasing {
    fn name(&self) -> &'static str {
        "prop-name-casing"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for prop in &tpl.prop_definitions {
            if prop.name.contains('-') {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Prop '{}' should use camelCase. Vue automatically converts kebab-case \
                         in templates to camelCase props.",
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
        crate::test_support::run_template_rule(PropNameCasing, template)
    }

    fn make_prop(name: &str) -> AnalyzedPropDefinition {
        AnalyzedPropDefinition {
            name: name.to_string(),
            type_annotation: Some("String".to_string()),
            has_default: false,
            is_required: false,
            is_boolean: false,
            used_in_template: false,
            used_in_script: false,
            span: Span::new(10, 30),
        }
    }

    #[test]
    fn kebab_case_prop_reports() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("my-prop")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "kebab-case prop name should trigger");
        assert!(diags.iter().any(|d| d.rule == "prop-name-casing"));
        assert!(
            diags[0].message.contains("camelCase"),
            "message should mention camelCase"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn camel_case_prop_passes() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("myProp")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "camelCase prop name should pass");
    }

    #[test]
    fn single_word_prop_passes() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("label")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "single word prop name should pass");
    }

    #[test]
    fn multiple_hyphens_reports() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("my-long-prop")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "multi-hyphen prop name should trigger");
        assert!(diags.iter().any(|d| d.rule == "prop-name-casing"));
    }
}
