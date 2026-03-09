//! Rule: no-aria-hidden-on-focusable
//!
//! Warns when `aria-hidden="true"` is placed on a focusable element.
//! Screen readers will still allow keyboard users to focus the element,
//! creating an inconsistent experience (the element is hidden from AT but
//! still reachable via Tab).

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

/// HTML elements that are natively focusable when rendered.
const NATIVELY_FOCUSABLE: &[&str] = &[
    "a", "button", "details", "embed", "iframe", "input", "select", "textarea", "video",
];

pub struct NoAriaHiddenOnFocusable;

impl LintRule for NoAriaHiddenOnFocusable {
    fn name(&self) -> &'static str {
        "no-aria-hidden-on-focusable"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Accessibility
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        // Check for aria-hidden="true" (static, non-dynamic)
        let aria_hidden_attr = el.attributes.iter().find(|a| {
            a.name == "aria-hidden" && !a.is_dynamic && a.value.as_deref() == Some("true")
        });
        let Some(aria_attr) = aria_hidden_attr else {
            return;
        };

        // Is it natively focusable?
        let is_native_focusable = NATIVELY_FOCUSABLE.contains(&el.tag.as_str());

        // Does it have an explicit tabindex (any value, including -1 may still be an issue)?
        // We flag tabindex >= 0 (i.e., not "-1") — negative tabindex removes focus.
        let has_positive_tabindex = el.attributes.iter().any(|a| {
            a.name == "tabindex"
                && !a.is_dynamic
                && a.value
                    .as_deref()
                    .is_some_and(|v| v.parse::<i32>().is_ok_and(|n| n >= 0))
        });

        if is_native_focusable || has_positive_tabindex {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "'aria-hidden=\"true\"' on a focusable element '<{}>'. \
                     Keyboard users can still focus this element even though it is hidden \
                     from assistive technologies. Remove 'aria-hidden' or make the element \
                     non-focusable.",
                    el.tag
                ),
                aria_attr.span.start,
                aria_attr.span.end,
                self.default_severity(),
                DiagnosticSpanKind::Attribute,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(elements: Vec<TemplateElement>) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_elements_rule(NoAriaHiddenOnFocusable, elements)
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
    fn aria_hidden_on_button_reports() {
        let diags = run(vec![make_el("button", &[("aria-hidden", "true")])]);
        assert!(!diags.is_empty(), "aria-hidden on button should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-aria-hidden-on-focusable"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-autofocus"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn aria_hidden_on_input_reports() {
        let diags = run(vec![make_el("input", &[("aria-hidden", "true")])]);
        assert!(!diags.is_empty(), "aria-hidden on input should trigger");
    }

    #[test]
    fn aria_hidden_on_div_with_tabindex_reports() {
        let diags = run(vec![make_el(
            "div",
            &[("aria-hidden", "true"), ("tabindex", "0")],
        )]);
        assert!(
            !diags.is_empty(),
            "aria-hidden on div with tabindex=0 should trigger"
        );
    }

    #[test]
    fn aria_hidden_on_div_passes() {
        let diags = run(vec![make_el("div", &[("aria-hidden", "true")])]);
        assert!(
            diags.is_empty(),
            "aria-hidden on non-focusable div should pass"
        );
    }

    #[test]
    fn aria_hidden_false_on_button_passes() {
        let diags = run(vec![make_el("button", &[("aria-hidden", "false")])]);
        assert!(diags.is_empty(), "aria-hidden=false should not trigger");
    }

    #[test]
    fn aria_hidden_on_div_with_negative_tabindex_passes() {
        let diags = run(vec![make_el(
            "div",
            &[("aria-hidden", "true"), ("tabindex", "-1")],
        )]);
        assert!(diags.is_empty(), "tabindex=-1 removes focus, should pass");
    }
}
