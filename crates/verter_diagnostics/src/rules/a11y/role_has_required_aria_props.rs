//! Rule: role-has-required-aria-props
//!
//! Certain ARIA roles require specific ARIA state/property attributes to convey
//! accurate semantic information to assistive technologies. Omitting them makes
//! the element meaningless or misleading to screen readers.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

/// Map of ARIA role → list of required aria-* attributes.
///
/// Based on WAI-ARIA 1.2 required properties.
const ROLE_REQUIRED_PROPS: &[(&str, &[&str])] = &[
    ("checkbox", &["aria-checked"]),
    ("combobox", &["aria-controls", "aria-expanded"]),
    ("heading", &["aria-level"]),
    ("listbox", &["aria-label", "aria-labelledby"]), // at least one
    (
        "meter",
        &["aria-valuenow", "aria-valuemin", "aria-valuemax"],
    ),
    ("option", &["aria-selected"]),
    ("radio", &["aria-checked"]),
    (
        "scrollbar",
        &[
            "aria-controls",
            "aria-valuenow",
            "aria-valuemin",
            "aria-valuemax",
        ],
    ),
    ("separator", &["aria-valuenow"]),
    (
        "slider",
        &["aria-valuenow", "aria-valuemin", "aria-valuemax"],
    ),
    (
        "spinbutton",
        &["aria-valuenow", "aria-valuemin", "aria-valuemax"],
    ),
    ("switch", &["aria-checked"]),
];

/// Roles for which at least one of the listed props must be present.
const ROLE_REQUIRED_ONE_OF: &[(&str, &[&str])] = &[("listbox", &["aria-label", "aria-labelledby"])];

pub struct RoleHasRequiredAriaProps;

impl LintRule for RoleHasRequiredAriaProps {
    fn name(&self) -> &'static str {
        "role-has-required-aria-props"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Accessibility
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        // Find the role attribute
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
        let role = role_value.as_str();

        // Look up required properties for this role
        let Some((_, required_props)) = ROLE_REQUIRED_PROPS.iter().find(|(r, _)| *r == role) else {
            return;
        };

        // Check if it's a "one of" role (like listbox)
        let is_one_of = ROLE_REQUIRED_ONE_OF.iter().any(|(r, _)| *r == role);

        // Collect present aria attributes
        let present: Vec<&str> = el
            .attributes
            .iter()
            .filter(|a| !a.is_dynamic)
            .map(|a| a.name.as_str())
            .collect();

        let missing: Vec<&&str> = if is_one_of {
            // For one-of roles, just check if any of the required props is present
            if required_props.iter().any(|p| present.contains(p)) {
                vec![]
            } else {
                required_props.iter().collect()
            }
        } else {
            required_props
                .iter()
                .filter(|p| !present.contains(*p))
                .collect()
        };

        if missing.is_empty() {
            return;
        }

        let missing_str = missing
            .iter()
            .map(|p| format!("'{}'", p))
            .collect::<Vec<_>>()
            .join(", ");
        ctx.report_with_severity(
            self.name(),
            self.category().as_str(),
            format!(
                "Elements with role='{role}' must have the required ARIA properties: {missing_str}.",
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
        crate::test_support::run_template_elements_rule(RoleHasRequiredAriaProps, elements)
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
    fn checkbox_without_aria_checked_reports() {
        let diags = run(vec![make_el("div", &[("role", "checkbox")])]);
        assert!(
            !diags.is_empty(),
            "checkbox without aria-checked should trigger"
        );
        assert!(diags
            .iter()
            .any(|d| d.rule == "role-has-required-aria-props"));
        assert!(
            !diags.iter().any(|d| d.rule == "aria-role"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn slider_missing_required_props_reports() {
        let diags = run(vec![make_el(
            "div",
            &[("role", "slider"), ("aria-valuenow", "50")],
        )]);
        assert!(
            !diags.is_empty(),
            "slider missing aria-valuemin/max should trigger"
        );
    }

    #[test]
    fn checkbox_with_aria_checked_passes() {
        let diags = run(vec![make_el(
            "div",
            &[("role", "checkbox"), ("aria-checked", "false")],
        )]);
        assert!(diags.is_empty(), "checkbox with aria-checked should pass");
    }

    #[test]
    fn slider_with_all_props_passes() {
        let diags = run(vec![make_el(
            "div",
            &[
                ("role", "slider"),
                ("aria-valuenow", "50"),
                ("aria-valuemin", "0"),
                ("aria-valuemax", "100"),
            ],
        )]);
        assert!(
            diags.is_empty(),
            "slider with all required props should pass"
        );
    }

    #[test]
    fn unknown_role_passes() {
        let diags = run(vec![make_el("div", &[("role", "main")])]);
        assert!(diags.is_empty(), "role with no required props should pass");
    }

    #[test]
    fn switch_missing_aria_checked_reports() {
        let diags = run(vec![make_el("button", &[("role", "switch")])]);
        assert!(
            !diags.is_empty(),
            "switch without aria-checked should trigger"
        );
    }
}
