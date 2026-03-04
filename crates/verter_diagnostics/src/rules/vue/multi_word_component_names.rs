//! Rule: multi-word-component-names
//!
//! Requires component names to be multi-word. This prevents conflicts with
//! existing and future HTML elements since all HTML elements are single words.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct MultiWordComponentNames;

/// Returns true if the name is multi-word (contains `-`, `_`, or two+ PascalCase words).
fn is_multi_word(name: &str) -> bool {
    if name.contains('-') || name.contains('_') {
        return true;
    }
    // Count uppercase letters that start a "word" (PascalCase detection)
    // Two consecutive segments: e.g., "TodoList" has T and L → 2 uppercase starts
    let upper_count = name
        .chars()
        .enumerate()
        .filter(|(i, c)| c.is_uppercase() && *i > 0)
        .count();
    upper_count >= 1
        && name
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false)
}

impl LintRule for MultiWordComponentNames {
    fn name(&self) -> &'static str {
        "multi-word-component-names"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if !el.is_component {
            return;
        }

        // Ignore <component> (dynamic component) and <slot>
        if el.tag == "component" || el.tag == "slot" {
            return;
        }

        if !is_multi_word(&el.tag) {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "Component name '{}' should be multi-word. Single-word names conflict with HTML elements.",
                    el.tag
                ),
                el.span.start,
                el.tag_span_end,
                self.default_severity(),
                DiagnosticSpanKind::ElementOpenTag,
            );
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(MultiWordComponentNames)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_component(tag: &str) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
            is_component: true,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![],
            v_for: None,
            v_model: None,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            has_text_content: false,
            has_bare_text: false,
            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 30,
        }
    }

    #[test]
    fn single_word_component_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_component("Todo")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "single-word component should trigger");
        assert!(diags.iter().any(|d| d.rule == "multi-word-component-names"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger no-v-html"
        );
    }

    #[test]
    fn pascal_case_multi_word_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_component("TodoList")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "PascalCase multi-word should pass");
    }

    #[test]
    fn kebab_case_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_component("todo-list")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "kebab-case component should pass");
    }

    #[test]
    fn underscore_case_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_component("todo_list")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "underscore_case component should pass");
    }

    #[test]
    fn native_element_not_flagged() {
        let el = TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![],
            v_for: None,
            v_model: None,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            has_text_content: false,
            has_bare_text: false,
            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 50,
        };
        let template = TemplateAnalysisSnapshot {
            elements: vec![el],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "native elements must not trigger multi-word-component-names"
        );
    }
}
