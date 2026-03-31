//! Rule: no-reserved-component-names
//!
//! Disallows registering components that use Vue's built-in component names
//! (Transition, TransitionGroup, KeepAlive, Teleport, Suspense) or popular
//! router component names (RouterView, RouterLink) unless they are imported
//! from Vue or vue-router.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateElement;

/// Vue built-in component names that shouldn't be overridden.
const RESERVED_NAMES: &[&str] = &[
    "Transition",
    "TransitionGroup",
    "KeepAlive",
    "Teleport",
    "Suspense",
    "RouterView",
    "RouterLink",
];

/// Allowed import sources for reserved names.
fn is_allowed_source(source: &str) -> bool {
    source == "vue"
        || source == "vue-router"
        || source.starts_with("vue/")
        || source.starts_with("vue-router/")
}

pub struct NoReservedComponentNames;

impl LintRule for NoReservedComponentNames {
    fn name(&self) -> &'static str {
        "no-reserved-component-names"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if !el.is_component {
            return;
        }

        if !RESERVED_NAMES.iter().any(|&name| name == el.tag) {
            return;
        }

        // Check the component list for this element's usage from template analysis snapshot.
        // We look at whether the import source is known to be from vue/vue-router.
        // Since TemplateElement doesn't carry import_source directly, we use a conservative
        // approach: warn unless the element's tag matches exactly a Vue built-in used without
        // a local import (the analysis snapshot's components list carries import_source).
        // For this rule, we report on the element span directly.
        ctx.report_with_severity(
            self.name(),
            self.category().as_str(),
            format!(
                "Component name '{}' is reserved by Vue. Rename your component to avoid conflicts.",
                el.tag
            ),
            el.span.start,
            el.tag_span_end,
            self.default_severity(),
            DiagnosticSpanKind::ElementOpenTag,
        );
    }

    fn check_template(
        &self,
        tpl: &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
        ctx: &mut LintContext,
    ) {
        // Use the template's components list to check import sources.
        // This gives us accurate import_source information.
        for comp in &tpl.components {
            if !RESERVED_NAMES.iter().any(|&name| name == comp.name) {
                continue;
            }

            // If the import source is Vue or vue-router, it's fine
            if let Some(src) = &comp.import_source {
                if is_allowed_source(src) {
                    continue;
                }
            }

            // Report at the component span
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "Component name '{}' is reserved by Vue. Rename your component or import it from 'vue'/'vue-router'.",
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

#[cfg(test)]
mod tests {
    use super::*;

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoReservedComponentNames, template)
    }

    fn make_component_usage(name: &str, import_source: Option<&str>) -> TemplateComponentUsage {
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
            span: Span::new(0, 30),
        }
    }

    #[test]
    fn reserved_name_without_import_reports() {
        // <Transition> without importing from 'vue'
        let template = TemplateAnalysisSnapshot {
            components: vec![make_component_usage("Transition", None)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "Transition without vue import should trigger"
        );
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-reserved-component-names"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger no-v-html"
        );
    }

    #[test]
    fn reserved_name_with_non_vue_import_reports() {
        // <Transition> imported from a local file
        let template = TemplateAnalysisSnapshot {
            components: vec![make_component_usage(
                "Transition",
                Some("./MyTransition.vue"),
            )],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "Transition from local file should trigger"
        );
    }

    #[test]
    fn reserved_name_from_vue_passes() {
        // <Transition> imported from 'vue' is allowed
        let template = TemplateAnalysisSnapshot {
            components: vec![make_component_usage("Transition", Some("vue"))],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "Transition from 'vue' should pass");
    }

    #[test]
    fn router_link_from_vue_router_passes() {
        let template = TemplateAnalysisSnapshot {
            components: vec![make_component_usage("RouterLink", Some("vue-router"))],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "RouterLink from 'vue-router' should pass");
    }

    #[test]
    fn non_reserved_component_name_passes() {
        let template = TemplateAnalysisSnapshot {
            components: vec![make_component_usage("MyComponent", None)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "custom component name should pass");
    }
}
