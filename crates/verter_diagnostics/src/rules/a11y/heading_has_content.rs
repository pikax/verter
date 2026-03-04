//! Rule: heading-has-content
//!
//! Heading elements (`<h1>` through `<h6>`) must have content. A self-closing
//! heading with no `aria-label` is reported as having no accessible content.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct HeadingHasContent;

impl LintRule for HeadingHasContent {
    fn name(&self) -> &'static str {
        "heading-has-content"
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Accessibility
    }
    fn default_severity(&self) -> Severity {
        Severity::Warning
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
            if el.is_component {
                continue;
            }
            let is_heading = matches!(el.tag.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
            if !is_heading {
                continue;
            }
            let has_aria_label = el.attributes.iter().any(|a| a.name == "aria-label");
            if has_aria_label {
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
                    format!("<{}> elements must have content.", el.tag),
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
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(elements: Vec<TemplateElement>) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(HeadingHasContent)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(
            &TemplateAnalysisSnapshot {
                elements,
                ..Default::default()
            },
            &mut ctx,
        );
        ctx.into_diagnostics()
    }

    fn heading(tag: &str, self_closing: bool, attrs: Vec<(&str, &str)>) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
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
        }
    }

    fn child_element(parent_idx: u32) -> TemplateElement {
        TemplateElement {
            tag: "span".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            parent_tag: Some("h1".to_string()),
            parent_index: Some(parent_idx),
            nesting_depth: 1,
            span: Span::new(5, 25),
            ..Default::default()
        }
    }

    #[test]
    fn self_closing_heading_without_aria_label_reports() {
        let diags = run(vec![heading("h1", true, vec![])]);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("<h1>"));
    }

    #[test]
    fn self_closing_heading_with_aria_label_passes() {
        assert!(run(vec![heading(
            "h2",
            true,
            vec![("aria-label", "Section title")]
        )])
        .is_empty());
    }

    #[test]
    fn non_self_closing_heading_with_children_passes() {
        // <h3><span>text</span></h3> — has child element
        let elems = vec![heading("h3", false, vec![]), child_element(0)];
        assert!(run(elems).is_empty());
    }

    #[test]
    fn non_self_closing_empty_heading_reports() {
        // <h1></h1> — no child elements, should report
        let diags = run(vec![heading("h1", false, vec![])]);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("<h1>"));
    }

    #[test]
    fn heading_with_v_text_passes() {
        let mut h = heading("h1", false, vec![]);
        h.has_v_text = true;
        assert!(run(vec![h]).is_empty());
    }

    #[test]
    fn non_heading_element_passes() {
        assert!(run(vec![heading("div", true, vec![])]).is_empty());
    }

    #[test]
    fn heading_with_v_html_passes() {
        let mut h = heading("h2", false, vec![]);
        h.has_v_html = true;
        assert!(run(vec![h]).is_empty());
    }

    // @ai-generated — Tests that text-only headings are NOT reported as empty.
    #[test]
    fn heading_with_text_content_passes() {
        // <h3>Basic Async Component</h3> — has text content but no child elements
        let mut h = heading("h3", false, vec![]);
        h.has_text_content = true;
        assert!(
            run(vec![h]).is_empty(),
            "heading with text content should NOT report"
        );
    }

    #[test]
    fn heading_with_interpolation_content_passes() {
        // <h3>{{ title }}</h3> — has interpolation content
        let mut h = heading("h1", false, vec![]);
        h.has_text_content = true;
        assert!(
            run(vec![h]).is_empty(),
            "heading with interpolation should NOT report"
        );
    }

    #[test]
    fn heading_with_whitespace_only_reports() {
        // <h3> </h3> — whitespace-only text is NOT meaningful content
        let mut h = heading("h3", false, vec![]);
        h.has_text_content = false; // whitespace-only won't set this flag
        let diags = run(vec![h]);
        assert_eq!(diags.len(), 1, "whitespace-only heading should report");
    }

    #[test]
    fn all_heading_levels_empty_non_self_closing_report() {
        // <h2></h2> through <h6></h6> — all empty non-self-closing headings should report
        for tag in &["h2", "h3", "h4", "h5", "h6"] {
            let diags = run(vec![heading(tag, false, vec![])]);
            assert_eq!(diags.len(), 1, "<{tag}></> should report missing content");
            assert!(
                diags[0].message.contains(&format!("<{tag}>")),
                "diagnostic message should reference the heading tag"
            );
        }
    }
}
