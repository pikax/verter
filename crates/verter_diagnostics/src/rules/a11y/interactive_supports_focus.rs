//! Rule: interactive-supports-focus
//!
//! Elements with interactive ARIA roles (button, link, checkbox, etc.) must either
//! be natively focusable or have a non-negative `tabindex` attribute. Without
//! keyboard focusability, keyboard-only users cannot interact with these elements.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

/// ARIA roles that represent interactive widgets.
const INTERACTIVE_ROLES: &[&str] = &[
    "button",
    "checkbox",
    "gridcell",
    "link",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "option",
    "radio",
    "searchbox",
    "slider",
    "spinbutton",
    "switch",
    "tab",
    "textbox",
    "treeitem",
];

/// HTML elements that are natively focusable.
const NATIVELY_FOCUSABLE: &[&str] = &[
    "a", "button", "details", "embed", "iframe", "input", "select", "textarea",
];

pub struct InteractiveSupportsFocus;

impl LintRule for InteractiveSupportsFocus {
    fn name(&self) -> &'static str {
        "interactive-supports-focus"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Accessibility
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        // Find an interactive role attribute
        let role_attr = el
            .attributes
            .iter()
            .find(|a| a.name == "role" && !a.is_dynamic);
        let Some(role_attr) = role_attr else {
            return;
        };
        let Some(role_value) = &role_attr.value else {
            return;
        };
        if !INTERACTIVE_ROLES.contains(&role_value.as_str()) {
            return;
        }

        // Natively focusable tags are fine
        if NATIVELY_FOCUSABLE.contains(&el.tag.as_str()) {
            return;
        }

        // Check for tabindex >= 0 (allows keyboard focus)
        let has_focusable_tabindex = el.attributes.iter().any(|a| {
            a.name == "tabindex"
                && !a.is_dynamic
                && a.value
                    .as_deref()
                    .is_some_and(|v| v.parse::<i32>().is_ok_and(|n| n >= 0))
        });

        if has_focusable_tabindex {
            return;
        }

        ctx.report_with_severity(
            self.name(),
            self.category().as_str(),
            format!(
                "Element with role='{}' must be focusable. Add 'tabindex=\"0\"' or use a natively \
                 focusable element like '<button>' instead.",
                role_value
            ),
            role_attr.span.start,
            role_attr.span.end,
            self.default_severity(),
            DiagnosticSpanKind::Attribute,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(elements: Vec<TemplateElement>) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_elements_rule(InteractiveSupportsFocus, elements)
    }

    fn make_el(tag: &str, attrs: &[(&str, &str)]) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
            attributes: attrs
                .iter()
                .map(|(name, value)| TemplateAttribute {
                    name: name.to_string(),
                    value: Some(value.to_string()),
                    is_dynamic: false,
                    span: Span::new(5, 25),
                    name_end: 0,
                    value_span: None,
                })
                .collect(),
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn div_with_button_role_without_tabindex_reports() {
        let diags = run(vec![make_el("div", &[("role", "button")])]);
        assert!(
            !diags.is_empty(),
            "div with role=button and no tabindex should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "interactive-supports-focus"));
        assert!(
            !diags.iter().any(|d| d.rule == "aria-role"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn span_with_link_role_without_tabindex_reports() {
        let diags = run(vec![make_el("span", &[("role", "link")])]);
        assert!(
            !diags.is_empty(),
            "span with role=link and no tabindex should trigger"
        );
    }

    #[test]
    fn div_with_button_role_and_tabindex_passes() {
        let diags = run(vec![make_el(
            "div",
            &[("role", "button"), ("tabindex", "0")],
        )]);
        assert!(
            diags.is_empty(),
            "div with role=button and tabindex=0 should pass"
        );
    }

    #[test]
    fn native_button_with_role_passes() {
        let diags = run(vec![make_el("button", &[("role", "button")])]);
        assert!(diags.is_empty(), "native button is focusable, should pass");
    }

    #[test]
    fn native_input_with_checkbox_role_passes() {
        let diags = run(vec![make_el("input", &[("role", "checkbox")])]);
        assert!(diags.is_empty(), "native input is focusable, should pass");
    }

    #[test]
    fn non_interactive_role_passes() {
        let diags = run(vec![make_el("div", &[("role", "banner")])]);
        assert!(diags.is_empty(), "non-interactive role should not trigger");
    }
}
