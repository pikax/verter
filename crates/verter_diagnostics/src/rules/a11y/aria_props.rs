//! Rule: aria-props
//!
//! Enforce that `aria-*` attributes use valid WAI-ARIA property names.
//! Typos or unknown aria properties have no effect on assistive technologies.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

/// All valid WAI-ARIA 1.2 state and property names (without the `aria-` prefix).
const VALID_ARIA_PROPS: &[&str] = &[
    "activedescendant",
    "atomic",
    "autocomplete",
    "braillelabel",
    "brailleroledescription",
    "busy",
    "checked",
    "colcount",
    "colindex",
    "colindextext",
    "colspan",
    "controls",
    "current",
    "describedby",
    "description",
    "details",
    "disabled",
    "dropeffect",
    "errormessage",
    "expanded",
    "flowto",
    "grabbed",
    "haspopup",
    "hidden",
    "invalid",
    "keyshortcuts",
    "label",
    "labelledby",
    "level",
    "live",
    "modal",
    "multiline",
    "multiselectable",
    "orientation",
    "owns",
    "placeholder",
    "posinset",
    "pressed",
    "readonly",
    "relevant",
    "required",
    "roledescription",
    "rowcount",
    "rowindex",
    "rowindextext",
    "rowspan",
    "selected",
    "setsize",
    "sort",
    "valuemax",
    "valuemin",
    "valuenow",
    "valuetext",
];

pub struct AriaProps;

impl LintRule for AriaProps {
    fn name(&self) -> &'static str {
        "aria-props"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Accessibility
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        for attr in &el.attributes {
            if !attr.name.starts_with("aria-") {
                continue;
            }
            let prop = &attr.name["aria-".len()..];
            if !VALID_ARIA_PROPS.contains(&prop) {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "'{}' is not a valid ARIA property. \
                         Check the WAI-ARIA 1.2 specification for valid `aria-*` attribute names.",
                        attr.name
                    ),
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
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(elements: Vec<TemplateElement>) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(AriaProps)];
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

    fn make_el(tag: &str, aria_attr: &str) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
            attributes: vec![TemplateAttribute {
                name: aria_attr.to_string(),
                value: Some("true".to_string()),
                is_dynamic: false,
                span: Span::new(5, 25),
                name_end: 0,
                value_span: None,
            }],
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn invalid_aria_prop_reports() {
        let diags = run(vec![make_el("div", "aria-labeledby")]); // typo: labeledby vs labelledby
        assert!(!diags.is_empty(), "invalid aria-* property should trigger");
        assert!(diags.iter().any(|d| d.rule == "aria-props"));
        assert!(
            !diags.iter().any(|d| d.rule == "aria-role"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn another_invalid_prop_reports() {
        let diags = run(vec![make_el("button", "aria-disbaled")]); // typo
        assert!(!diags.is_empty(), "aria-disbaled is not valid");
        assert!(diags.iter().any(|d| d.rule == "aria-props"));
    }

    #[test]
    fn valid_aria_label_passes() {
        let diags = run(vec![make_el("div", "aria-label")]);
        assert!(diags.is_empty(), "aria-label is valid");
    }

    #[test]
    fn valid_aria_hidden_passes() {
        let diags = run(vec![make_el("div", "aria-hidden")]);
        assert!(diags.is_empty(), "aria-hidden is valid");
    }

    #[test]
    fn non_aria_attribute_passes() {
        let diags = run(vec![make_el("div", "data-foo")]);
        assert!(diags.is_empty(), "non-aria attribute should not trigger");
    }
}
