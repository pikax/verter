//! Rule: component-name-in-template-casing
//!
//! Enforces PascalCase for component names in templates. Kebab-case is
//! accepted when the corresponding PascalCase component is a known import
//! (e.g., `<my-component>` is valid if `MyComponent` is imported).

// @ai-generated

use crate::casing::{is_pascal_case, kebab_to_pascal_case};
use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateAnalysisSnapshot;

pub struct ComponentNameInTemplateCasing;

const BUILTINS: &[&str] = &[
    "component",
    "slot",
    "template",
    "transition",
    "transition-group",
    "keep-alive",
    "teleport",
    "suspense",
];

impl LintRule for ComponentNameInTemplateCasing {
    fn name(&self) -> &'static str {
        "component-name-in-template-casing"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        // Build set of known component names (PascalCase-normalized).
        let known_components: std::collections::HashSet<&str> =
            tpl.components.iter().map(|c| c.name.as_str()).collect();

        for el in &tpl.elements {
            if !el.is_component {
                continue;
            }

            if BUILTINS.contains(&el.tag.as_str()) {
                continue;
            }

            if is_pascal_case(&el.tag) {
                continue;
            }

            // Kebab-case: accept if PascalCase form matches a known component.
            let pascal = kebab_to_pascal_case(&el.tag);
            if known_components.contains(pascal.as_str()) {
                continue;
            }

            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!("Component '{}' should use PascalCase in templates.", el.tag),
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

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(ComponentNameInTemplateCasing, template)
    }

    fn make_el(tag: &str, is_component: bool) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
            is_component,
            span: Span::new(0, 20),
            tag_span_end: 20,
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn kebab_case_without_import_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("my-component", true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "kebab-case without matching import should trigger"
        );
        assert!(diags
            .iter()
            .any(|d| d.rule == "component-name-in-template-casing"));
        assert!(
            diags[0].message.contains("PascalCase"),
            "message should mention PascalCase"
        );
    }

    #[test]
    fn kebab_case_with_matching_import_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("my-component", true)],
            components: vec![TemplateComponentUsage {
                name: "MyComponent".to_string(),
                import_source: None,
                is_dynamic: false,
                props: vec![],
                has_spread: false,
                slots_used: vec![],
                static_classes: vec![],
                has_dynamic_class: false,
                dynamic_classes: vec![],
                span: Span::new(0, 20),
                v_models: vec![],
                bindings: vec![],
                events: vec![],
            }],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "kebab-case with matching import should pass: {:?}",
            diags
        );
    }

    #[test]
    fn pascal_case_component_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("MyComponent", true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "PascalCase component should pass");
    }

    #[test]
    fn html_element_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("div", false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "native HTML element should pass");
    }

    #[test]
    fn builtin_component_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("transition", true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "built-in component should pass");
    }

    #[test]
    fn lowercase_component_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("mycomponent", true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "lowercase component name should trigger");
    }
}
