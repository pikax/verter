//! Rule: require-prop-types
//!
//! Requires type annotations on prop definitions.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct RequirePropTypes;

impl LintRule for RequirePropTypes {
    fn name(&self) -> &'static str {
        "require-prop-types"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for prop in &tpl.prop_definitions {
            if prop.type_annotation.is_none() {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!("Prop '{}' should have a type annotation.", prop.name),
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
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(RequirePropTypes)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_prop(name: &str, type_annotation: Option<&str>) -> AnalyzedPropDefinition {
        AnalyzedPropDefinition {
            name: name.to_string(),
            type_annotation: type_annotation.map(|s| s.to_string()),
            has_default: false,
            is_required: false,
            is_boolean: false,
            used_in_template: false,
            used_in_script: false,
            span: Span::new(10, 15),
        }
    }

    #[test]
    fn prop_without_type_reports() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("title", None)],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "prop without type should trigger");
        assert!(diags[0].rule == "require-prop-types");
        assert!(
            !diags.iter().any(|d| d.rule == "require-default-prop"),
            "must not trigger default prop rule"
        );
    }

    #[test]
    fn prop_with_type_passes() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("title", Some("string"))],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "prop with type should pass");
    }
}
