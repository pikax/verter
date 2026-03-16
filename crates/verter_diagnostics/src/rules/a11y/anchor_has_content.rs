//! Rule: anchor-has-content
//!
//! Enforce that anchors have content.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct AnchorHasContent;

impl LintRule for AnchorHasContent {
    fn name(&self) -> &'static str {
        "anchor-has-content"
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Accessibility
    }
    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        // Build a set of element indices that have child elements
        let mut has_child_element: rustc_hash::FxHashSet<u32> = rustc_hash::FxHashSet::default();
        for el in &tpl.elements {
            if let Some(parent_idx) = el.parent_index {
                has_child_element.insert(parent_idx);
            }
        }

        for (idx, el) in tpl.elements.iter().enumerate() {
            if el.tag != "a" || el.is_component {
                continue;
            }
            // Check for aria-label or aria-labelledby as alternatives
            let has_label = el
                .attributes
                .iter()
                .any(|a| a.name == "aria-label" || a.name == "aria-labelledby");
            if has_label {
                continue;
            }
            // v-text or v-html provide content
            if el.has_v_text || el.has_v_html {
                continue;
            }
            // Empty if self-closing OR has no child elements and no text/interpolation content
            let is_empty = el.is_self_closing
                || (!has_child_element.contains(&(idx as u32)) && !el.has_text_content);
            if is_empty {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "Anchors must have text content or an aria-label.".to_string(),
                    el.span.start,
                    el.tag_span_end,
                    self.default_severity(),
                    DiagnosticSpanKind::ElementOpenTag,
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

    fn run(elements: Vec<TemplateElement>) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_elements_rule(AnchorHasContent, elements)
    }

    fn anchor(self_closing: bool, attrs: Vec<(&str, &str)>) -> TemplateElement {
        TemplateElement {
            tag: "a".to_string(),
            is_component: false,
            is_self_closing: self_closing,
            namespace: ElementNamespace::Html,
            attributes: attrs
                .into_iter()
                .map(|(n, v)| TemplateAttribute {
                    name: n.to_string(),
                    value: Some(v.to_string()),
                    is_dynamic: false,
                    span: Span::new(0, 10),
                    name_end: 0,
                    value_span: None,
                })
                .collect(),
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
            span: Span::new(0, 30),
            tag_span_end: 30,
            content_end: 0,
            ..Default::default()
        }
    }

    fn child_element(parent_idx: u32) -> TemplateElement {
        TemplateElement {
            tag: "span".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            parent_tag: Some("a".to_string()),
            parent_index: Some(parent_idx),
            nesting_depth: 1,
            span: Span::new(5, 25),
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn self_closing_anchor_reports() {
        assert_eq!(run(vec![anchor(true, vec![("href", "/")])]).len(), 1);
    }

    #[test]
    fn anchor_with_aria_label_passes() {
        assert!(run(vec![anchor(
            true,
            vec![("href", "/"), ("aria-label", "Home")]
        )])
        .is_empty());
    }

    #[test]
    fn non_self_closing_anchor_with_children_passes() {
        // <a href="/"><span>text</span></a> — has child element
        let elems = vec![anchor(false, vec![("href", "/")]), child_element(0)];
        assert!(run(elems).is_empty());
    }

    #[test]
    fn non_self_closing_empty_anchor_reports() {
        // <a href="/"></a> — no child elements, should report
        assert_eq!(run(vec![anchor(false, vec![("href", "/")])]).len(), 1);
    }

    #[test]
    fn anchor_with_v_text_passes() {
        let mut a = anchor(false, vec![("href", "/")]);
        a.has_v_text = true;
        assert!(run(vec![a]).is_empty());
    }

    #[test]
    fn anchor_with_v_html_passes() {
        let mut a = anchor(false, vec![("href", "/")]);
        a.has_v_html = true;
        assert!(run(vec![a]).is_empty());
    }

    #[test]
    fn anchor_with_aria_labelledby_passes() {
        // <a href="/" aria-labelledby="id"></a> — aria-labelledby is an accessible alternative
        assert!(run(vec![anchor(
            false,
            vec![("href", "/"), ("aria-labelledby", "section-title")]
        )])
        .is_empty());
    }

    #[test]
    fn non_self_closing_empty_anchor_without_href_reports() {
        // <a></a> — empty anchor without href still needs content
        assert_eq!(run(vec![anchor(false, vec![])]).len(), 1);
    }
}
