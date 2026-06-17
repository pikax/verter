//! Rule: no-undef-components
//!
//! Reports components used in the template that are not imported or registered.
//! Checks `TemplateComponentUsage` where `import_source` is `None`.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateAnalysisSnapshot;

/// Vue built-in components that don't need imports.
const BUILTIN_COMPONENTS: &[&str] = &[
    "Component",
    "Transition",
    "TransitionGroup",
    "KeepAlive",
    "Teleport",
    "Suspense",
    "Slot",
    // Lowercase aliases
    "component",
    "transition",
    "transition-group",
    "keep-alive",
    "teleport",
    "suspense",
    "slot",
];

pub struct NoUndefComponents;

impl LintRule for NoUndefComponents {
    fn name(&self) -> &'static str {
        "no-undef-components"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for comp in &tpl.components {
            // Skip dynamic components
            if comp.is_dynamic {
                continue;
            }

            // Skip built-in Vue components
            if BUILTIN_COMPONENTS.contains(&comp.name.as_str()) {
                continue;
            }

            // If import_source is None, the component is unresolved
            if comp.import_source.is_none() {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Component '{}' is used in the template but not imported or registered.",
                        comp.name
                    ),
                    comp.span.start,
                    comp.span.end,
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

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoUndefComponents, template)
    }

    fn make_component(name: &str, import_source: Option<&str>) -> TemplateComponentUsage {
        TemplateComponentUsage {
            name: name.to_string(),
            import_source: import_source.map(|s| s.to_string()),
            is_dynamic: false,
            props: vec![],
            has_spread: false,
            slots_used: vec![],
            static_classes: vec![],
            has_dynamic_class: false,
            dynamic_classes: vec![],
            v_models: vec![],
            bindings: vec![],
            events: vec![],
            span: Span::new(0, 20),
        }
    }

    #[test]
    fn unresolved_component_reports() {
        let template = TemplateAnalysisSnapshot {
            components: vec![make_component("MyDialog", None)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "unresolved component should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-undef-components"));
        assert!(
            diags[0].message.contains("MyDialog"),
            "message should mention component name"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn resolved_component_passes() {
        let template = TemplateAnalysisSnapshot {
            components: vec![make_component("MyDialog", Some("./MyDialog.vue"))],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "resolved component should pass");
    }

    #[test]
    fn builtin_component_passes() {
        let template = TemplateAnalysisSnapshot {
            components: vec![make_component("Transition", None)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "built-in component should pass");
    }

    #[test]
    fn dynamic_component_passes() {
        let template = TemplateAnalysisSnapshot {
            components: vec![TemplateComponentUsage {
                name: "Unknown".to_string(),
                import_source: None,
                is_dynamic: true,
                props: vec![],
                has_spread: false,
                slots_used: vec![],
                static_classes: vec![],
                has_dynamic_class: false,
                dynamic_classes: vec![],
                v_models: vec![],
                bindings: vec![],
                events: vec![],
                span: Span::new(0, 20),
            }],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "dynamic component should pass");
    }

    #[test]
    fn no_components_passes() {
        let template = TemplateAnalysisSnapshot::default();
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "no components should pass");
    }
}
