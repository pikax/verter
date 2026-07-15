use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateAnalysisSnapshot;

/// Warns about timer/event-listener calls (`setInterval`, `setTimeout`,
/// `addEventListener`) detected in template expressions during SSR.
/// These leak on the server because there's no component teardown.
pub struct NoSideEffectsInSetupForSsr;

const SIDE_EFFECT_APIS: &[&str] = &[
    "setInterval",
    "setTimeout",
    "addEventListener",
    "removeEventListener",
    "requestIdleCallback",
    "queueMicrotask",
];

impl LintRule for NoSideEffectsInSetupForSsr {
    fn name(&self) -> &'static str {
        "no-side-effects-in-setup-for-ssr"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Ssr
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        if !ctx.config().ssr_mode {
            return;
        }

        for binding in &tpl.unresolved_bindings {
            if SIDE_EFFECT_APIS.contains(&binding.name.as_str()) {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "`{}` causes resource leaks on the server. Move to `onMounted()` with cleanup in `onUnmounted()`.",
                        binding.name
                    ),
                    binding.span.start,
                    binding.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::Interpolation,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{run_template_rule, run_template_rule_ssr};
    use verter_semantic::analysis::template::{TemplateAnalysisSnapshot, UnresolvedBinding};
    use verter_span::Span;

    fn unresolved(name: &str) -> UnresolvedBinding {
        UnresolvedBinding {
            name: name.to_string(),
            span: Span::new(10, 10 + name.len() as u32),
        }
    }

    #[test]
    fn no_report_without_ssr_mode() {
        let tpl = TemplateAnalysisSnapshot {
            unresolved_bindings: vec![unresolved("setInterval")],
            ..Default::default()
        };
        let diags = run_template_rule(NoSideEffectsInSetupForSsr, &tpl);
        assert!(diags.is_empty());
    }

    #[test]
    fn reports_set_interval_in_ssr() {
        let tpl = TemplateAnalysisSnapshot {
            unresolved_bindings: vec![unresolved("setInterval")],
            ..Default::default()
        };
        let diags = run_template_rule_ssr(NoSideEffectsInSetupForSsr, &tpl);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("setInterval"));
    }

    #[test]
    fn reports_add_event_listener() {
        let tpl = TemplateAnalysisSnapshot {
            unresolved_bindings: vec![unresolved("addEventListener")],
            ..Default::default()
        };
        let diags = run_template_rule_ssr(NoSideEffectsInSetupForSsr, &tpl);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn ignores_safe_globals() {
        let tpl = TemplateAnalysisSnapshot {
            unresolved_bindings: vec![unresolved("console"), unresolved("JSON")],
            ..Default::default()
        };
        let diags = run_template_rule_ssr(NoSideEffectsInSetupForSsr, &tpl);
        assert!(diags.is_empty());
    }
}
