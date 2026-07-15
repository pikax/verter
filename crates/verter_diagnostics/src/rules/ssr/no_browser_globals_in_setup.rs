use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateAnalysisSnapshot;

/// Warns about browser globals (`window`, `document`, `navigator`, `localStorage`)
/// used in template expressions. These are undefined on the server and will cause
/// hydration mismatches or runtime errors during SSR.
pub struct NoBrowserGlobalsInSetup;

const BROWSER_GLOBALS: &[&str] = &[
    "window",
    "document",
    "navigator",
    "localStorage",
    "sessionStorage",
    "location",
    "history",
    "screen",
    "matchMedia",
    "requestAnimationFrame",
    "cancelAnimationFrame",
    "IntersectionObserver",
    "ResizeObserver",
    "MutationObserver",
];

impl LintRule for NoBrowserGlobalsInSetup {
    fn name(&self) -> &'static str {
        "no-browser-globals-in-setup"
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
            if BROWSER_GLOBALS.contains(&binding.name.as_str()) {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "`{}` is not available during SSR and may cause a hydration mismatch or runtime error.",
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
            unresolved_bindings: vec![unresolved("window")],
            ..Default::default()
        };
        let diags = run_template_rule(NoBrowserGlobalsInSetup, &tpl);
        assert!(diags.is_empty(), "should not report without ssr_mode");
    }

    #[test]
    fn reports_window_in_ssr_mode() {
        let tpl = TemplateAnalysisSnapshot {
            unresolved_bindings: vec![unresolved("window")],
            ..Default::default()
        };
        let diags = run_template_rule_ssr(NoBrowserGlobalsInSetup, &tpl);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("window"));
    }

    #[test]
    fn reports_local_storage() {
        let tpl = TemplateAnalysisSnapshot {
            unresolved_bindings: vec![unresolved("localStorage")],
            ..Default::default()
        };
        let diags = run_template_rule_ssr(NoBrowserGlobalsInSetup, &tpl);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn ignores_non_browser_globals() {
        let tpl = TemplateAnalysisSnapshot {
            unresolved_bindings: vec![unresolved("console"), unresolved("someVar")],
            ..Default::default()
        };
        let diags = run_template_rule_ssr(NoBrowserGlobalsInSetup, &tpl);
        assert!(diags.is_empty(), "console is not browser-only");
    }
}
