//! Rule: slot-name-casing
//!
//! Slot names from `defined_slots` should use kebab-case. PascalCase or
//! camelCase slot names are inconsistent with HTML conventions.

// @ai-generated

use crate::casing::{has_uppercase, to_kebab_case};
use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct SlotNameCasing;

impl LintRule for SlotNameCasing {
    fn name(&self) -> &'static str {
        "slot-name-casing"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for slot in &tpl.defined_slots {
            // "default" is always fine
            if slot.name == "default" {
                continue;
            }

            if has_uppercase(&slot.name) {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Slot name '{}' should use kebab-case. \
                         Rename to '{}'.",
                        slot.name,
                        to_kebab_case(&slot.name)
                    ),
                    slot.span.start,
                    slot.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::Attribute,
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
        crate::test_support::run_template_rule(SlotNameCasing, template)
    }

    fn make_slot(name: &str) -> DefinedSlot {
        DefinedSlot {
            name: name.to_string(),
            has_bindings: false,
            binding_names: vec![],
            binding_expressions: vec![],
            binding_value_spans: vec![],
            span: Span::new(10, 30),
        }
    }

    #[test]
    fn camel_case_slot_reports() {
        let template = TemplateAnalysisSnapshot {
            defined_slots: vec![make_slot("headerContent")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "camelCase slot name should trigger");
        assert!(diags.iter().any(|d| d.rule == "slot-name-casing"));
        assert!(
            diags[0].message.contains("header-content"),
            "message should suggest kebab-case"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn kebab_case_slot_passes() {
        let template = TemplateAnalysisSnapshot {
            defined_slots: vec![make_slot("header-content")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "kebab-case slot name should pass");
    }

    #[test]
    fn default_slot_passes() {
        let template = TemplateAnalysisSnapshot {
            defined_slots: vec![make_slot("default")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "'default' slot should always pass");
    }

    #[test]
    fn pascal_case_slot_reports() {
        let template = TemplateAnalysisSnapshot {
            defined_slots: vec![make_slot("HeaderContent")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "PascalCase slot name should trigger");
    }

    #[test]
    fn to_kebab_case_helper() {
        assert_eq!(to_kebab_case("headerContent"), "header-content");
        assert_eq!(to_kebab_case("HeaderContent"), "header-content");
        assert_eq!(to_kebab_case("header"), "header");
    }
}
