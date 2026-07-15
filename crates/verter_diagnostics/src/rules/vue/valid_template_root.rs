//! Rule: valid-template-root
//!
//! Template must have at least one root element. An empty `<template></template>`
//! block is likely a mistake.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateAnalysisSnapshot;

pub struct ValidTemplateRoot;

impl LintRule for ValidTemplateRoot {
    fn name(&self) -> &'static str {
        "valid-template-root"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        if tpl.elements.is_empty() {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "The template requires at least one root element.".to_string(),
                0,
                0,
                self.default_severity(),
                DiagnosticSpanKind::FileLevel,
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
        crate::test_support::run_template_rule(ValidTemplateRoot, template)
    }

    #[test]
    fn empty_template_reports() {
        let template = TemplateAnalysisSnapshot::default();
        let diags = run(&template);
        assert!(!diags.is_empty(), "empty template should trigger");
        assert!(diags.iter().any(|d| d.rule == "valid-template-root"));
        assert!(
            diags[0].message.contains("root element"),
            "message should mention root element"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-unused-components"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn template_with_element_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                span: Span::new(0, 10),
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "template with root element should pass");
    }
}
