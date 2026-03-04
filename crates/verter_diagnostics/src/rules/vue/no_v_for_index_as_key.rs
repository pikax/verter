//! Rule: no-v-for-index-as-key
//!
//! Using the v-for index as `:key` is an anti-pattern. It defeats the purpose
//! of virtual DOM diffing since the index changes when items are reordered.
//! Use a stable unique ID from the item instead.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateElement, VForDirective};

pub struct NoVForIndexAsKey;

impl LintRule for NoVForIndexAsKey {
    fn name(&self) -> &'static str {
        "no-v-for-index-as-key"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Performance
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_v_for(&self, vfor: &VForDirective, _el: &TemplateElement, ctx: &mut LintContext) {
        if !vfor.key_uses_index {
            return;
        }
        let index_name = vfor.index.as_deref().unwrap_or("index");
        ctx.report_with_severity(
            self.name(),
            self.category().as_str(),
            format!(
                "Avoid using the v-for index '{index_name}' as ':key'. \
                 Index-based keys break DOM diffing when items are reordered. \
                 Use a stable unique ID from the item instead.",
            ),
            vfor.span.start,
            vfor.span.end,
            self.default_severity(),
            DiagnosticSpanKind::Directive,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoVForIndexAsKey)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_vfor_el(has_key: bool, key_uses_index: bool, index: Option<&str>) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            v_for: Some(VForDirective {
                variable: "item".to_string(),
                index: index.map(|s| s.to_string()),
                iterable: "items".to_string(),
                has_key,
                key_expression: if has_key {
                    Some(if key_uses_index {
                        index.unwrap_or("i").to_string()
                    } else {
                        "item.id".to_string()
                    })
                } else {
                    None
                },
                key_uses_index,
                span: Span::new(5, 35),
            }),
            content_end: 0,
            text_children: Vec::new(),
            ..Default::default()
        }
    }

    #[test]
    fn index_as_key_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_vfor_el(true, true, Some("i"))],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "index as key should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-v-for-index-as-key"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn stable_id_as_key_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_vfor_el(true, false, Some("i"))],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "stable ID key should pass");
    }

    #[test]
    fn no_key_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_vfor_el(false, false, None)],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "no key should pass (different rule catches missing key)"
        );
    }
}
