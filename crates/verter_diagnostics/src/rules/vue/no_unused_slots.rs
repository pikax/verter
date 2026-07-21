//! Rule: no-unused-slots
//!
//! Flags a `defineSlots<{...}>()` member whose outlet never appears in the
//! component's own template and which is never accessed programmatically.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateAnalysisSnapshot;

/// `TemplateAnalysisSnapshot::slot_declarations` is populated FAIL-OPEN by the
/// shared unused-declaration pipeline: members appear only when slot usage
/// could be statically bounded (no dynamic outlet `<slot :name="expr">`, no
/// `useSlots()`, no `$slots` in the template, no expression parse errors).
/// Conditional outlets (`v-if` branches) COUNT as used; implicit template-only
/// slots (no `defineSlots`) produce nothing.
pub struct NoUnusedSlots;

impl LintRule for NoUnusedSlots {
    fn name(&self) -> &'static str {
        "no-unused-slots"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Hint)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for slot in &tpl.slot_declarations {
            if slot.used {
                continue;
            }

            ctx.report_with_tags(
                self.name(),
                self.category().as_str(),
                format!("Slot '{}' is declared but has no outlet.", slot.name),
                slot.span.start,
                slot.span.end,
                self.default_severity(),
                vec![DiagnosticTag::Unnecessary],
                DiagnosticSpanKind::PropDefinition,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_semantic::analysis::template::{AnalyzedSlotDeclaration, TemplateAnalysisSnapshot};
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoUnusedSlots, template)
    }

    fn slot(name: &str, used: bool) -> AnalyzedSlotDeclaration {
        AnalyzedSlotDeclaration {
            name: name.to_string(),
            span: Span::new(10, 10 + name.len() as u32),
            used,
        }
    }

    #[test]
    fn reports_unused_slot_with_unnecessary_tag_on_declared_span() {
        let template = TemplateAnalysisSnapshot {
            slot_declarations: vec![slot("header", false)],
            ..Default::default()
        };
        let diags = run(&template);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "no-unused-slots");
        assert!(diags[0].message.contains("header"));
        assert_eq!(
            diags[0].span,
            Span::new(10, 16),
            "authored declaration span"
        );
        assert_eq!(diags[0].tags, vec![DiagnosticTag::Unnecessary]);
        assert_eq!(diags[0].severity, Severity::Hint);
    }

    #[test]
    fn used_slot_is_silent() {
        let template = TemplateAnalysisSnapshot {
            slot_declarations: vec![slot("header", true)],
            ..Default::default()
        };
        assert!(run(&template).is_empty());
    }

    #[test]
    fn empty_inventory_is_silent() {
        // The fail-open population leaves the inventory EMPTY for dynamic
        // outlets / useSlots() / $slots / parse errors — the rule must then
        // produce nothing.
        assert!(run(&TemplateAnalysisSnapshot::default()).is_empty());
    }
}
