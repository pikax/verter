use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateAnalysisSnapshot, TemplateElement};

/// Warns when known client-only components are used without a `<ClientOnly>` wrapper.
/// These components use browser APIs and will fail during SSR.
pub struct RequireClientOnlyWrapper;

/// Well-known components that require client-only rendering.
const CLIENT_ONLY_COMPONENTS: &[&str] = &[
    "Teleport",
    "ClientOnly",
    // Common chart/editor libraries
    "VChart",
    "ECharts",
    "MonacoEditor",
    "CodeMirror",
    "Quill",
    "TipTap",
    // Map components
    "GoogleMap",
    "LeafletMap",
    "MapboxGl",
];

impl RequireClientOnlyWrapper {
    fn is_client_only_component(tag: &str) -> bool {
        // Teleport is handled by Vue SSR, skip it
        if tag == "Teleport" || tag == "teleport" || tag == "ClientOnly" || tag == "client-only" {
            return false;
        }
        CLIENT_ONLY_COMPONENTS
            .iter()
            .any(|&c| c.eq_ignore_ascii_case(tag))
    }

    fn has_client_only_ancestor(el: &TemplateElement, elements: &[TemplateElement]) -> bool {
        let mut parent_idx = el.parent_index;
        while let Some(idx) = parent_idx {
            if let Some(parent) = elements.get(idx as usize) {
                if parent.tag == "ClientOnly"
                    || parent.tag == "client-only"
                    || parent.tag == "LazyClientOnly"
                {
                    return true;
                }
                parent_idx = parent.parent_index;
            } else {
                break;
            }
        }
        false
    }
}

impl LintRule for RequireClientOnlyWrapper {
    fn name(&self) -> &'static str {
        "require-client-only-wrapper"
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

        for el in &tpl.elements {
            if !el.is_component {
                continue;
            }
            if !Self::is_client_only_component(&el.tag) {
                continue;
            }
            if Self::has_client_only_ancestor(el, &tpl.elements) {
                continue;
            }

            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "`<{}>` is a client-only component. Wrap with `<ClientOnly>` for SSR compatibility.",
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
    use crate::test_support::{run_template_rule, run_template_rule_ssr};
    use verter_analysis::template::TemplateAnalysisSnapshot;
    use verter_span::Span;

    fn component(tag: &str, parent_index: Option<u32>) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
            is_component: true,
            span: Span::new(0, 30),
            tag_span_end: tag.len() as u32 + 1,
            parent_index,
            ..Default::default()
        }
    }

    #[test]
    fn no_report_without_ssr_mode() {
        let tpl = TemplateAnalysisSnapshot {
            elements: vec![component("ECharts", None)],
            ..Default::default()
        };
        let diags = run_template_rule(RequireClientOnlyWrapper, &tpl);
        assert!(diags.is_empty());
    }

    #[test]
    fn reports_unwrapped_client_only_component() {
        let tpl = TemplateAnalysisSnapshot {
            elements: vec![component("ECharts", None)],
            ..Default::default()
        };
        let diags = run_template_rule_ssr(RequireClientOnlyWrapper, &tpl);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("ECharts"));
        assert!(diags[0].message.contains("ClientOnly"));
    }

    #[test]
    fn no_report_when_wrapped_in_client_only() {
        let tpl = TemplateAnalysisSnapshot {
            elements: vec![component("ClientOnly", None), {
                let mut el = component("ECharts", Some(0));
                el.parent_index = Some(0);
                el
            }],
            ..Default::default()
        };
        let diags = run_template_rule_ssr(RequireClientOnlyWrapper, &tpl);
        assert!(
            diags.is_empty(),
            "should not report when wrapped in ClientOnly"
        );
    }

    #[test]
    fn ignores_teleport() {
        let tpl = TemplateAnalysisSnapshot {
            elements: vec![component("Teleport", None)],
            ..Default::default()
        };
        let diags = run_template_rule_ssr(RequireClientOnlyWrapper, &tpl);
        assert!(diags.is_empty(), "Teleport is SSR-handled by Vue");
    }

    #[test]
    fn ignores_non_component_elements() {
        let tpl = TemplateAnalysisSnapshot {
            elements: vec![{
                let mut el = component("div", None);
                el.is_component = false;
                el
            }],
            ..Default::default()
        };
        let diags = run_template_rule_ssr(RequireClientOnlyWrapper, &tpl);
        assert!(diags.is_empty());
    }
}
