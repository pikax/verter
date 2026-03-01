//! Rule: vapor/no-vue-lifecycle-events
//!
//! Disallows using Vue lifecycle hook names as event listeners in Vapor mode.
//! E.g., `@mounted="fn"` — lifecycle hooks are not events in Vapor.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

const LIFECYCLE_NAMES: &[&str] = &[
    "mounted",
    "unmounted",
    "updated",
    "beforeMount",
    "beforeUnmount",
    "beforeUpdate",
    "activated",
    "deactivated",
    "errorCaptured",
    "before-mount",
    "before-unmount",
    "before-update",
    "error-captured",
];

pub struct NoVueLifecycleEvents;

impl LintRule for NoVueLifecycleEvents {
    fn name(&self) -> &'static str {
        "vapor/no-vue-lifecycle-events"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Vapor
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_directive(
        &self,
        dir: &TemplateDirective,
        _el: &TemplateElement,
        ctx: &mut LintContext,
    ) {
        if !ctx.config().vapor_mode {
            return;
        }

        if dir.name != "on" {
            return;
        }

        let Some(arg) = &dir.argument else {
            return;
        };

        if LIFECYCLE_NAMES.contains(&arg.as_str()) {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "'@{}' uses a Vue lifecycle hook name as an event. Lifecycle hooks are not DOM events in Vapor mode.",
                    arg
                ),
                dir.span.start,
                dir.span.end,
                self.default_severity(),
                DiagnosticSpanKind::Directive,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LintConfig, LintPreset};
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run_rule(
        template: &TemplateAnalysisSnapshot,
        vapor: bool,
    ) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoVueLifecycleEvents)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig {
            vapor_mode: vapor,
            preset: LintPreset::Recommended,
            ..Default::default()
        };
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_el_with_event(event: &str) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![TemplateDirective {
                name: "on".to_string(),
                raw_name: format!("@{}", event),
                argument: Some(event.to_string()),
                modifiers: vec![],
                expression: Some("fn".to_string()),
                span: Span::new(5, 20),
            }],
            v_for: None,
            v_model: None,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            has_text_content: false,
            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 50,
        }
    }

    #[test]
    fn mounted_event_in_vapor_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_event("mounted")],
            ..Default::default()
        };
        let diags = run_rule(&template, true);
        assert!(!diags.is_empty(), "@mounted in vapor should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "vapor/no-vue-lifecycle-events"));
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-on"),
            "must not trigger valid-v-on"
        );
    }

    #[test]
    fn mounted_event_in_vdom_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_event("mounted")],
            ..Default::default()
        };
        let diags = run_rule(&template, false);
        assert!(
            diags.is_empty(),
            "@mounted in VDOM should not trigger vapor rule"
        );
    }

    #[test]
    fn click_event_in_vapor_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_event("click")],
            ..Default::default()
        };
        let diags = run_rule(&template, true);
        assert!(
            diags.is_empty(),
            "@click in vapor must not trigger lifecycle rule"
        );
    }
}
