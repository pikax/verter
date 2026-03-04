//! Rule: max-template-depth
//!
//! Warn when template nesting depth exceeds a configurable threshold
//! (default: 10). Deeply nested templates are harder to maintain and
//! may indicate a need to extract child components.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

/// Default maximum allowed nesting depth.
const DEFAULT_MAX_DEPTH: u16 = 10;

/// Warn when template nesting depth exceeds a threshold.
pub struct MaxTemplateDepth {
    max_depth: u16,
}

impl MaxTemplateDepth {
    /// Create a new rule with the default max depth (10).
    pub fn new() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
        }
    }

    /// Create a new rule with a custom max depth.
    pub fn with_max_depth(max_depth: u16) -> Self {
        Self { max_depth }
    }
}

impl Default for MaxTemplateDepth {
    fn default() -> Self {
        Self::new()
    }
}

impl LintRule for MaxTemplateDepth {
    fn name(&self) -> &'static str {
        "max-template-depth"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Performance
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        if tpl.max_nesting_depth > self.max_depth {
            // Find the deepest element to use its span for the diagnostic.
            let deepest = tpl
                .elements
                .iter()
                .find(|el| el.nesting_depth == tpl.max_nesting_depth);

            let (span_start, span_end) = match deepest {
                Some(el) => (el.span.start, el.span.end),
                // Fallback: report at file start if no element found.
                None => (0, 0),
            };

            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "Template nesting depth ({}) exceeds maximum allowed ({}).",
                    tpl.max_nesting_depth, self.max_depth
                ),
                span_start,
                span_end,
                self.default_severity(),
                DiagnosticSpanKind::FullElement,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run_rule(
        template: &TemplateAnalysisSnapshot,
        max_depth: u16,
    ) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> =
            vec![Box::new(MaxTemplateDepth::with_max_depth(max_depth))];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_element(tag: &str, nesting_depth: u16) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
            is_component: false,
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
            nesting_depth,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 50,
            content_end: 0,
            ..Default::default()
        }
    }

    /// @ai-generated - Rule metadata is correct
    #[test]
    fn rule_metadata() {
        let rule = MaxTemplateDepth::new();
        assert_eq!(rule.name(), "max-template-depth");
        assert_eq!(rule.category(), RuleCategory::Performance);
        assert_eq!(rule.default_severity(), Severity::Warning);
    }

    /// @ai-generated - Depth within limit produces no diagnostic
    #[test]
    fn depth_within_limit_passes() {
        let template = TemplateAnalysisSnapshot {
            max_nesting_depth: 5,
            elements: vec![make_element("div", 5)],
            ..Default::default()
        };
        let diags = run_rule(&template, 10);
        assert!(diags.is_empty());
    }

    /// @ai-generated - Depth equal to limit produces no diagnostic
    #[test]
    fn depth_equal_to_limit_passes() {
        let template = TemplateAnalysisSnapshot {
            max_nesting_depth: 10,
            elements: vec![make_element("div", 10)],
            ..Default::default()
        };
        let diags = run_rule(&template, 10);
        assert!(diags.is_empty());
    }

    /// @ai-generated - Depth exceeding limit reports diagnostic
    #[test]
    fn depth_exceeding_limit_reports() {
        let template = TemplateAnalysisSnapshot {
            max_nesting_depth: 12,
            elements: vec![make_element("div", 1), make_element("span", 12)],
            ..Default::default()
        };
        let diags = run_rule(&template, 10);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "max-template-depth");
        assert!(diags[0].message.contains("12"));
        assert!(diags[0].message.contains("10"));
    }

    /// @ai-generated - Custom max depth works
    #[test]
    fn custom_max_depth() {
        let template = TemplateAnalysisSnapshot {
            max_nesting_depth: 4,
            elements: vec![make_element("div", 4)],
            ..Default::default()
        };
        // Limit of 3 should trigger
        let diags = run_rule(&template, 3);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("4"));
        assert!(diags[0].message.contains("3"));
    }

    /// @ai-generated - Empty template produces no diagnostic
    #[test]
    fn empty_template_passes() {
        let template = TemplateAnalysisSnapshot::default();
        let diags = run_rule(&template, 10);
        assert!(diags.is_empty());
    }
}
