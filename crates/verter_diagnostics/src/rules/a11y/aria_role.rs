//! Rule: aria-role
//!
//! Enforce that `role` attribute values are valid ARIA roles.
//! Reports a warning when an element has a `role` attribute whose value
//! is not in the WAI-ARIA 1.2 role taxonomy.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

/// All valid WAI-ARIA 1.2 roles.
const VALID_ROLES: &[&str] = &[
    "alert",
    "alertdialog",
    "application",
    "article",
    "banner",
    "button",
    "cell",
    "checkbox",
    "columnheader",
    "combobox",
    "complementary",
    "contentinfo",
    "definition",
    "dialog",
    "directory",
    "document",
    "feed",
    "figure",
    "form",
    "grid",
    "gridcell",
    "group",
    "heading",
    "img",
    "link",
    "list",
    "listbox",
    "listitem",
    "log",
    "main",
    "marquee",
    "math",
    "menu",
    "menubar",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "navigation",
    "none",
    "note",
    "option",
    "presentation",
    "progressbar",
    "radio",
    "radiogroup",
    "region",
    "row",
    "rowgroup",
    "rowheader",
    "scrollbar",
    "search",
    "searchbox",
    "separator",
    "slider",
    "spinbutton",
    "status",
    "switch",
    "tab",
    "table",
    "tablist",
    "tabpanel",
    "term",
    "textbox",
    "timer",
    "toolbar",
    "tooltip",
    "tree",
    "treegrid",
    "treeitem",
];

pub struct AriaRole;

impl LintRule for AriaRole {
    fn name(&self) -> &'static str {
        "aria-role"
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Accessibility
    }
    fn default_severity(&self) -> Severity {
        Severity::Warning
    }
    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        for attr in &el.attributes {
            if attr.name == "role" {
                if let Some(ref value) = attr.value {
                    if !VALID_ROLES.contains(&value.as_str()) {
                        ctx.report_with_severity(
                            self.name(),
                            self.category().as_str(),
                            format!("'{}' is not a valid ARIA role.", value),
                            attr.span.start,
                            attr.span.end,
                            self.default_severity(),
                            DiagnosticSpanKind::Attribute,
                        );
                    }
                }
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(AriaRole)];
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

    fn el(tag: &str, attrs: Vec<(&str, Option<&str>)>) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: attrs
                .into_iter()
                .map(|(n, v)| TemplateAttribute {
                    name: n.to_string(),
                    value: v.map(|s| s.to_string()),
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

    #[test]
    fn invalid_role_reports() {
        let diags = run(vec![el("div", vec![("role", Some("foobar"))])]);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("foobar"));
    }

    #[test]
    fn valid_role_passes() {
        assert!(run(vec![el("div", vec![("role", Some("button"))])]).is_empty());
    }

    #[test]
    fn no_role_attribute_passes() {
        assert!(run(vec![el("div", vec![])]).is_empty());
    }
}
