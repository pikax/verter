//! Rule: vapor/no-non-vapor-components
//!
//! Warns when components with limited Vapor support are used in Vapor mode.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

/// Components that have limited or no Vapor support.
const LIMITED_VAPOR_COMPONENTS: &[&str] = &[
    "KeepAlive",
    "Teleport",
    "RouterView",
    "RouterLink",
    "TransitionGroup",
];

pub struct NoNonVaporComponents;

impl LintRule for NoNonVaporComponents {
    fn name(&self) -> &'static str {
        "vapor/no-non-vapor-components"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Vapor
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if !ctx.config().vapor_mode {
            return;
        }

        if !el.is_component {
            return;
        }

        if LIMITED_VAPOR_COMPONENTS.contains(&el.tag.as_str()) {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "'<{}>' has limited support in Vapor mode. Verify behavior carefully.",
                    el.tag
                ),
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
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run_rule(
        template: &TemplateAnalysisSnapshot,
        vapor: bool,
    ) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoNonVaporComponents)];
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

    fn make_el(tag: &str) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
            is_component: true,
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
        }
    }

    #[test]
    fn keep_alive_in_vapor_warns() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("KeepAlive")],
            ..Default::default()
        };
        let diags = run_rule(&template, true);
        assert!(!diags.is_empty(), "<KeepAlive> in vapor should warn");
        assert!(diags
            .iter()
            .any(|d| d.rule == "vapor/no-non-vapor-components"));
        assert!(
            !diags.iter().any(|d| d.rule == "vapor/no-suspense"),
            "must not trigger no-suspense"
        );
    }

    #[test]
    fn keep_alive_in_vdom_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("KeepAlive")],
            ..Default::default()
        };
        let diags = run_rule(&template, false);
        assert!(
            diags.is_empty(),
            "<KeepAlive> in VDOM should not trigger vapor rule"
        );
    }

    #[test]
    fn my_component_in_vapor_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("MyComponent")],
            ..Default::default()
        };
        let diags = run_rule(&template, true);
        assert!(
            diags.is_empty(),
            "custom component must not trigger no-non-vapor-components"
        );
    }
}
