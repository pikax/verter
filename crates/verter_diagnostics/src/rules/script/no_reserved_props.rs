//! Rule: no-reserved-props
//!
//! Disallows using reserved prop names like `key`, `ref`, `is`, `slot`, `slot-scope`, `scope`.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

const RESERVED_PROPS: &[&str] = &["key", "ref", "is", "slot", "slot-scope", "scope"];

pub struct NoReservedProps;

impl LintRule for NoReservedProps {
    fn name(&self) -> &'static str {
        "no-reserved-props"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for prop in &tpl.prop_definitions {
            if RESERVED_PROPS.contains(&prop.name.as_str()) {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "'{}' is a reserved Vue prop name and cannot be used in defineProps.",
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
        crate::test_support::run_template_rule(NoReservedProps, template)
    }

    fn make_prop(name: &str) -> AnalyzedPropDefinition {
        AnalyzedPropDefinition {
            name: name.to_string(),
            type_annotation: None,
            has_default: false,
            is_required: false,
            is_boolean: false,
            used_in_template: false,
            used_in_script: false,
            span: Span::new(10, 13),
        }
    }

    #[test]
    fn reserved_prop_reports() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("key")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "reserved prop 'key' should trigger");
        assert!(diags[0].rule == "no-reserved-props");
        assert!(
            !diags.iter().any(|d| d.rule == "no-reserved-keys"),
            "must not trigger keys rule"
        );
    }

    #[test]
    fn normal_prop_passes() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("title")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "normal prop should pass");
    }
}
