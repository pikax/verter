//! Rule: no-duplicate-attributes
//!
//! Disallow duplication of attributes on elements.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use rustc_hash::FxHashSet;
use verter_semantic::analysis::template::TemplateElement;

/// Disallow duplicate attributes on elements.
pub struct NoDuplicateAttributes;

impl LintRule for NoDuplicateAttributes {
    fn name(&self) -> &'static str {
        "no-duplicate-attributes"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        let mut seen = FxHashSet::default();
        for attr in &el.attributes {
            if !seen.insert(&attr.name) {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!("Duplicate attribute '{}'.", attr.name),
                    attr.span.start,
                    attr.span.end,
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

    use verter_semantic::analysis::template::*;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoDuplicateAttributes, template)
    }

    fn make_attr(name: &str) -> TemplateAttribute {
        TemplateAttribute {
            name: name.to_string(),
            value: Some("val".to_string()),
            is_dynamic: false,
            span: verter_span::Span::new(0, 10),
            name_end: 0,
            value_span: None,
        }
    }

    fn make_element(attrs: Vec<TemplateAttribute>) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: attrs,
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
            span: verter_span::Span::new(0, 50),
            tag_span_end: 50,
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn duplicate_attributes_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element(vec![make_attr("class"), make_attr("class")])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("class"));
    }

    #[test]
    fn unique_attributes_pass() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element(vec![make_attr("class"), make_attr("id")])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty());
    }
}
