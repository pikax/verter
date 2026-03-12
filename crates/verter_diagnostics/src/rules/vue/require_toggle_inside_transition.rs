//! Rule: require-toggle-inside-transition
//!
//! Children of `<Transition>` or `<transition>` must have `v-if` or `v-show`,
//! otherwise the transition has no trigger and never activates.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct RequireToggleInsideTransition;

/// Check if a tag is a Transition component.
fn is_transition(tag: &str) -> bool {
    tag == "Transition" || tag == "transition"
}

impl LintRule for RequireToggleInsideTransition {
    fn name(&self) -> &'static str {
        "require-toggle-inside-transition"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        // Check if this element's parent is a Transition
        let parent_is_transition = el.parent_tag.as_deref().map(is_transition).unwrap_or(false);

        if !parent_is_transition {
            return;
        }

        // Skip component children — they may manage their own visibility
        if el.is_component {
            return;
        }

        // The child should have v-if or v-show
        if !el.has_v_if && !el.has_v_show {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Children of '<Transition>' must have 'v-if' or 'v-show' to trigger the transition."
                    .to_string(),
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

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(RequireToggleInsideTransition, template)
    }

    #[test]
    fn transition_child_without_toggle_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                TemplateElement {
                    tag: "Transition".to_string(),
                    is_component: true,
                    span: Span::new(0, 60),
                    tag_span_end: 12,
                    content_end: 0,
                    ..Default::default()
                },
                TemplateElement {
                    tag: "div".to_string(),
                    parent_tag: Some("Transition".to_string()),
                    parent_index: Some(0),
                    span: Span::new(13, 50),
                    tag_span_end: 18,
                    content_end: 0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "Transition child without v-if/v-show should trigger"
        );
        assert!(diags
            .iter()
            .any(|d| d.rule == "require-toggle-inside-transition"));
        assert!(
            diags[0].message.contains("Transition"),
            "message should mention Transition"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-show"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn transition_child_with_v_if_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                TemplateElement {
                    tag: "Transition".to_string(),
                    is_component: true,
                    span: Span::new(0, 60),
                    tag_span_end: 12,
                    content_end: 0,
                    ..Default::default()
                },
                TemplateElement {
                    tag: "div".to_string(),
                    has_v_if: true,
                    parent_tag: Some("Transition".to_string()),
                    parent_index: Some(0),
                    span: Span::new(13, 50),
                    tag_span_end: 30,
                    content_end: 0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "Transition child with v-if should pass");
    }

    #[test]
    fn transition_child_with_v_show_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                TemplateElement {
                    tag: "transition".to_string(),
                    is_component: true,
                    span: Span::new(0, 60),
                    tag_span_end: 12,
                    content_end: 0,
                    ..Default::default()
                },
                TemplateElement {
                    tag: "p".to_string(),
                    has_v_show: true,
                    parent_tag: Some("transition".to_string()),
                    parent_index: Some(0),
                    span: Span::new(13, 50),
                    tag_span_end: 28,
                    content_end: 0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "transition child with v-show should pass");
    }

    #[test]
    fn non_transition_parent_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                parent_tag: Some("section".to_string()),
                parent_index: Some(0),
                span: Span::new(10, 30),
                tag_span_end: 15,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "element in non-Transition parent should pass"
        );
    }
}
