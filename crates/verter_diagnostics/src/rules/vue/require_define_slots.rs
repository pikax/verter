//! Rule: require-define-slots
//!
//! When `$slots` is used in the template but no `defineSlots()` macro is present
//! in script setup, report a warning. `defineSlots()` declares slot types for
//! better type safety and IDE support.
//!
//! ## Bad
//!
//! ```vue
//! <script setup lang="ts">
//! // no defineSlots()
//! </script>
//! <template>
//!   <div>{{ $slots.default }}</div>
//! </template>
//! ```
//!
//! ## Good
//!
//! ```vue
//! <script setup lang="ts">
//! defineSlots<{ default(props: { msg: string }): any }>()
//! </script>
//! <template>
//!   <div>{{ $slots.default }}</div>
//! </template>
//! ```

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};
use verter_analysis::types::{AnalysisFlags, AnalyzedMacroKind};

pub struct RequireDefineSlots;

impl LintRule for RequireDefineSlots {
    fn name(&self) -> &'static str {
        "require-define-slots"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        let template = match file.template {
            Some(t) => t,
            None => return,
        };

        // Check if $slots is referenced in template
        let slots_occ = template
            .binding_occurrences
            .iter()
            .find(|occ| occ.name == "$slots");
        let slots_occ = match slots_occ {
            Some(occ) => occ,
            None => return,
        };

        // Check if defineSlots() is present in script
        if let Some(script) = file.script {
            if script.flags.contains(AnalysisFlags::HAS_DEFINE_SLOTS) {
                return;
            }
            // Also check macros directly as a fallback
            if script
                .macros
                .iter()
                .any(|m| m.kind == AnalyzedMacroKind::DefineSlots)
            {
                return;
            }
        }

        ctx.report_with_severity(
            self.name(),
            self.category().as_str(),
            "Component uses `$slots` in template but does not call `defineSlots()`. \
             Add `defineSlots()` to declare slot types."
                .to_string(),
            slots_occ.span.start,
            slots_occ.span.end,
            self.default_severity(),
            DiagnosticSpanKind::ScriptCallSite,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::rules::FileContext;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_analysis::types::*;
    use verter_span::Span;

    fn run_file(
        script: Option<&ScriptAnalysisSnapshot>,
        template: &TemplateAnalysisSnapshot,
    ) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(RequireDefineSlots)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        let file = FileContext {
            template: Some(template),
            script,
            styles: &[],
            source: None,
        };
        visitor.visit_file(&file, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn slots_usage_without_define_slots_reports() {
        let script = ScriptAnalysisSnapshot {
            flags: AnalysisFlags::empty(),
            ..Default::default()
        };
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "$slots".to_string(),
                span: Span::new(50, 56),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run_file(Some(&script), &template);
        assert!(
            !diags.is_empty(),
            "should report when $slots used without defineSlots()"
        );
        assert!(diags.iter().any(|d| d.rule == "require-define-slots"));
        assert!(
            diags[0].message.contains("defineSlots()"),
            "message should mention defineSlots()"
        );
        // Negative: should not contain unrelated rules
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn slots_usage_with_define_slots_passes() {
        let script = ScriptAnalysisSnapshot {
            flags: AnalysisFlags::HAS_DEFINE_SLOTS,
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineSlots,
                is_type_based: true,
                type_references: vec![],
                binding_name: None,
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: vec![],
                span: Span::new(10, 30),
            }],
            ..Default::default()
        };
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "$slots".to_string(),
                span: Span::new(50, 56),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run_file(Some(&script), &template);
        assert!(
            diags.is_empty(),
            "should pass when defineSlots() is present"
        );
    }

    #[test]
    fn no_slots_usage_passes() {
        let script = ScriptAnalysisSnapshot {
            flags: AnalysisFlags::empty(),
            ..Default::default()
        };
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "count".to_string(),
                span: Span::new(50, 55),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run_file(Some(&script), &template);
        assert!(diags.is_empty(), "should pass when $slots is not used");
    }

    #[test]
    fn no_script_with_slots_usage_reports() {
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "$slots".to_string(),
                span: Span::new(50, 56),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run_file(None, &template);
        assert!(
            !diags.is_empty(),
            "should report when $slots used and no script block exists"
        );
    }

    #[test]
    fn no_template_passes() {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(RequireDefineSlots)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        let file = FileContext {
            template: None,
            script: None,
            styles: &[],
            source: None,
        };
        visitor.visit_file(&file, &mut ctx);
        let diags = ctx.into_diagnostics();
        assert!(diags.is_empty(), "should pass when there is no template");
    }
}
