//! Rule: component-name-in-template-casing
//!
//! Enforces PascalCase for component names in templates. Component elements
//! should use `<MyComponent>` rather than `<my-component>`.

// @ai-generated

use crate::casing::is_pascal_case;
use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct ComponentNameInTemplateCasing;

impl LintRule for ComponentNameInTemplateCasing {
    fn name(&self) -> &'static str {
        "component-name-in-template-casing"
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

        // Skip built-in Vue components (lowercase by convention)
        let builtins = [
            "component",
            "slot",
            "template",
            "transition",
            "transition-group",
            "keep-alive",
            "teleport",
            "suspense",
        ];
        if builtins.contains(&el.tag.as_str()) {
            return;
        }

        if !is_pascal_case(&el.tag) {
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

    use verter_analysis::template::*;
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
    fn kebab_case_component_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("my-component", true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "kebab-case component should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "component-name-in-template-casing"));
        assert!(
            diags[0].message.contains("PascalCase"),
            "message should mention PascalCase"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
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
