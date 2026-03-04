//! Rule: valid-v-else
//!
//! Ensures `v-else` and `v-else-if` are preceded by a sibling element with
//! `v-if` or `v-else-if`. A stray `v-else` or `v-else-if` is a runtime error.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct ValidVElse;

impl LintRule for ValidVElse {
    fn name(&self) -> &'static str {
        "valid-v-else"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        let elements = &tpl.elements;

        for (i, el) in elements.iter().enumerate() {
            if !el.has_v_else && !el.has_v_else_if {
                continue;
            }

            // Find the previous sibling with the same parent_index (same DOM level)
            // Elements are stored in document order so we scan backwards
            let has_valid_predecessor = elements[..i]
                .iter()
                .rev()
                .find(|prev| {
                    prev.parent_index == el.parent_index && prev.nesting_depth == el.nesting_depth
                })
                .map(|prev| prev.has_v_if || prev.has_v_else_if)
                .unwrap_or(false);

            if !has_valid_predecessor {
                let directive_name = if el.has_v_else { "v-else" } else { "v-else-if" };
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "'{}' must be preceded by a sibling element with 'v-if' or 'v-else-if'.",
                        directive_name
                    ),
                    el.span.start,
                    el.tag_span_end,
                    self.default_severity(),
                    DiagnosticSpanKind::Directive,
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

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(ValidVElse)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_el(
        has_v_if: bool,
        has_v_else: bool,
        has_v_else_if: bool,
        depth: u16,
        parent_index: Option<u32>,
    ) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![],
            v_for: None,
            v_model: None,
            has_v_if,
            has_v_else,
            has_v_else_if,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            has_text_content: false,
            has_bare_text: false,
            has_element_children: false,
            nesting_depth: depth,
            parent_tag: None,
            parent_index,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 50,
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn stray_v_else_reports() {
        // <div v-else> with no preceding v-if sibling
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(false, true, false, 0, None)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "stray v-else should trigger");
        assert!(diags.iter().any(|d| d.rule == "valid-v-else"));
        assert!(
            diags[0].message.contains("v-else"),
            "message should mention v-else"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-if"),
            "must not trigger valid-v-if"
        );
    }

    #[test]
    fn stray_v_else_if_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(false, false, true, 0, None)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "stray v-else-if should trigger");
        assert!(
            diags[0].message.contains("v-else-if"),
            "message should mention v-else-if"
        );
    }

    #[test]
    fn valid_v_if_v_else_chain_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                make_el(true, false, false, 0, None),
                make_el(false, true, false, 0, None),
            ],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "valid v-if + v-else chain should pass");
    }

    #[test]
    fn valid_v_if_v_else_if_v_else_chain_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                make_el(true, false, false, 0, None),
                make_el(false, false, true, 0, None),
                make_el(false, true, false, 0, None),
            ],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "complete v-if/else-if/else chain should pass"
        );
    }
}
