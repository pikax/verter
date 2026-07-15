//! Rule: vapor/no-suspense
//!
//! Disallows `<Suspense>` in Vapor mode (not supported).

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateElement;

pub struct NoSuspense;

impl LintRule for NoSuspense {
    fn name(&self) -> &'static str {
        "vapor/no-suspense"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Vapor
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if !ctx.config().vapor_mode {
            return;
        }

        if el.tag.eq_ignore_ascii_case("Suspense") {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "'<Suspense>' is not supported in Vapor mode.".to_string(),
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
    use crate::config::{LintConfig, LintPreset};
    use crate::visitor::LintVisitor;
    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run_rule(
        template: &TemplateAnalysisSnapshot,
        vapor: bool,
    ) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoSuspense)];
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

    fn make_el(tag: &str, is_component: bool) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
            is_component,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![],
            v_for: None,
            v_model: None,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            has_text_content: false,
            has_bare_text: false,
            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 30),
            tag_span_end: 30,
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn suspense_in_vapor_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("Suspense", true)],
            ..Default::default()
        };
        let diags = run_rule(&template, true);
        assert!(!diags.is_empty(), "<Suspense> in vapor mode should trigger");
        assert!(diags.iter().any(|d| d.rule == "vapor/no-suspense"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger no-v-html"
        );
    }

    #[test]
    fn suspense_in_vdom_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("Suspense", true)],
            ..Default::default()
        };
        let diags = run_rule(&template, false);
        assert!(
            diags.is_empty(),
            "<Suspense> in VDOM mode should not trigger vapor rule"
        );
    }

    #[test]
    fn transition_in_vapor_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("Transition", true)],
            ..Default::default()
        };
        let diags = run_rule(&template, true);
        assert!(
            diags.is_empty(),
            "<Transition> must not trigger no-suspense"
        );
    }
}
