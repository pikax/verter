use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::{TemplateDirective, TemplateElement};

/// Suggests using `v-if` instead of `v-show` during SSR. `v-show` renders the
/// element with `display: none` causing a visible flash on hydration.
pub struct NoVShowPreferVIf;

impl LintRule for NoVShowPreferVIf {
    fn name(&self) -> &'static str {
        "no-v-show-prefer-v-if"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Ssr
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Hint)
    }

    fn check_directive(
        &self,
        dir: &TemplateDirective,
        _el: &TemplateElement,
        ctx: &mut LintContext,
    ) {
        if !ctx.config().ssr_mode {
            return;
        }

        if dir.name == "show" {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Prefer `v-if` over `v-show` in SSR. `v-show` renders with `display: none`, causing a flash before hydration removes it.".to_string(),
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
    use crate::config::LintConfig;
    use crate::context::LintContext;
    use crate::diagnostic::LintDiagnostic;
    use crate::visitor::LintVisitor;
    use verter_semantic::analysis::template::{
        TemplateAnalysisSnapshot, TemplateDirective, TemplateElement,
    };
    use verter_span::Span;

    fn run_ssr(elements: Vec<TemplateElement>) -> Vec<LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoVShowPreferVIf)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig {
            ssr_mode: true,
            ..Default::default()
        };
        let mut ctx = LintContext::new(&config);
        let tpl = TemplateAnalysisSnapshot {
            elements,
            ..Default::default()
        };
        visitor.visit_template(&tpl, &mut ctx);
        ctx.into_diagnostics()
    }

    fn run_no_ssr(elements: Vec<TemplateElement>) -> Vec<LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoVShowPreferVIf)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        let tpl = TemplateAnalysisSnapshot {
            elements,
            ..Default::default()
        };
        visitor.visit_template(&tpl, &mut ctx);
        ctx.into_diagnostics()
    }

    fn el_with_v_show() -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            directives: vec![TemplateDirective {
                name: "show".to_string(),
                raw_name: "v-show".to_string(),
                argument: None,
                modifiers: vec![],
                expression: Some("visible".to_string()),
                span: Span::new(5, 25),
                name_end: 11,
                arg_span: None,
                expression_span: None,
                modifier_spans: vec![],
            }],
            span: Span::new(0, 50),
            ..Default::default()
        }
    }

    fn el_with_v_if() -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            has_v_if: true,
            directives: vec![],
            span: Span::new(0, 50),
            ..Default::default()
        }
    }

    #[test]
    fn no_report_without_ssr_mode() {
        let diags = run_no_ssr(vec![el_with_v_show()]);
        assert!(diags.is_empty());
    }

    #[test]
    fn reports_v_show_in_ssr() {
        let diags = run_ssr(vec![el_with_v_show()]);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("v-show"));
        assert!(diags[0].message.contains("v-if"));
    }

    #[test]
    fn ignores_v_if() {
        let diags = run_ssr(vec![el_with_v_if()]);
        assert!(diags.is_empty(), "v-if is fine for SSR");
    }
}
