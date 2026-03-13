//! Rule: required-slot-has-default
//!
//! Warns when `defineSlots` marks a slot as required (no `?`) but the
//! corresponding `<slot>` element has fallback content. A required slot should
//! always be provided by the parent — having default fallback content is
//! contradictory.
//!
//! ## Bad
//!
//! ```vue
//! <script setup lang="ts">
//! defineSlots<{ header(props: {}): any }>()
//! </script>
//! <template>
//!   <slot name="header">This fallback should not exist</slot>
//! </template>
//! ```
//!
//! ## Good
//!
//! ```vue
//! <script setup lang="ts">
//! defineSlots<{ header(props: {}): any }>()
//! </script>
//! <template>
//!   <slot name="header" />
//! </template>
//! ```

use std::collections::HashSet;

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};
use verter_analysis::types::AnalyzedMacroKind;

pub struct RequiredSlotHasDefault;

impl LintRule for RequiredSlotHasDefault {
    fn name(&self) -> &'static str {
        "required-slot-has-default"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        let script = match file.script {
            Some(s) => s,
            None => return,
        };
        let template = match file.template {
            Some(t) => t,
            None => return,
        };

        // Find defineSlots macro with slot_fields
        let define_slots = script
            .macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineSlots);
        let define_slots = match define_slots {
            Some(m) => m,
            None => return,
        };

        // Build set of required slot names from slot_fields
        let required: HashSet<&str> = define_slots
            .slot_fields
            .iter()
            .filter(|f| f.is_required)
            .map(|f| f.name.as_str())
            .collect();

        if required.is_empty() {
            return;
        }

        // Check template defined_slots with fallback content
        for slot in &template.defined_slots {
            if required.contains(slot.name.as_str()) && slot.has_fallback_content {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Slot '{}' is declared as required in `defineSlots()` but has default fallback content.",
                        slot.name
                    ),
                    slot.span.start,
                    slot.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::ElementOpenTag,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_analysis::template::{DefinedSlot, TemplateAnalysisSnapshot};
    use verter_analysis::types::{AnalysisFlags, ScriptAnalysisSnapshot};
    use verter_analysis::types::{AnalyzedMacro, AnalyzedMacroKind, AnalyzedSlotField};
    use verter_span::Span;

    fn run(file: &FileContext<'_>) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_file_rule(RequiredSlotHasDefault, file)
    }

    fn make_slot(name: &str, has_fallback: bool) -> DefinedSlot {
        DefinedSlot {
            name: name.to_string(),
            has_bindings: false,
            binding_names: vec![],
            binding_expressions: vec![],
            binding_value_spans: vec![],
            has_fallback_content: has_fallback,
            span: Span::new(10, 30),
        }
    }

    fn make_define_slots(slot_fields: Vec<AnalyzedSlotField>) -> AnalyzedMacro {
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineSlots,
            is_type_based: true,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            emit_fields: vec![],
            slot_fields,
            default_keys: vec![],
            expose_fields: vec![],
            default_values: Vec::new(),
            resolved_local_types: Vec::new(),
            span: Span::new(0, 50),
        }
    }

    #[test]
    fn required_slot_with_fallback_reports() {
        let template = TemplateAnalysisSnapshot {
            defined_slots: vec![make_slot("header", true)],
            ..Default::default()
        };
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_define_slots(vec![AnalyzedSlotField {
                name: "header".to_string(),
                is_required: true,
                span: Span::new(20, 26),
                bindings: vec![],
                description: None,
                tags: vec![],
            }])],
            flags: AnalysisFlags::HAS_DEFINE_SLOTS,
            ..Default::default()
        };
        let file = FileContext {
            template: Some(&template),
            script: Some(&script),
            styles: &[],
            source: None,
        };
        let diags = run(&file);
        assert_eq!(diags.len(), 1, "should report one diagnostic: {:?}", diags);
        assert!(
            diags[0].message.contains("header"),
            "should mention slot name"
        );
        assert!(
            diags[0].message.contains("required"),
            "should mention 'required'"
        );
    }

    #[test]
    fn optional_slot_with_fallback_does_not_report() {
        let template = TemplateAnalysisSnapshot {
            defined_slots: vec![make_slot("header", true)],
            ..Default::default()
        };
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_define_slots(vec![AnalyzedSlotField {
                name: "header".to_string(),
                is_required: false,
                span: Span::new(20, 26),
                bindings: vec![],
                description: None,
                tags: vec![],
            }])],
            flags: AnalysisFlags::HAS_DEFINE_SLOTS,
            ..Default::default()
        };
        let file = FileContext {
            template: Some(&template),
            script: Some(&script),
            styles: &[],
            source: None,
        };
        let diags = run(&file);
        assert!(
            diags.is_empty(),
            "optional slot with fallback should not report: {:?}",
            diags
        );
    }

    #[test]
    fn required_slot_without_fallback_does_not_report() {
        let template = TemplateAnalysisSnapshot {
            defined_slots: vec![make_slot("header", false)],
            ..Default::default()
        };
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_define_slots(vec![AnalyzedSlotField {
                name: "header".to_string(),
                is_required: true,
                span: Span::new(20, 26),
                bindings: vec![],
                description: None,
                tags: vec![],
            }])],
            flags: AnalysisFlags::HAS_DEFINE_SLOTS,
            ..Default::default()
        };
        let file = FileContext {
            template: Some(&template),
            script: Some(&script),
            styles: &[],
            source: None,
        };
        let diags = run(&file);
        assert!(
            diags.is_empty(),
            "required slot without fallback should not report: {:?}",
            diags
        );
    }

    #[test]
    fn no_define_slots_does_not_report() {
        let template = TemplateAnalysisSnapshot {
            defined_slots: vec![make_slot("header", true)],
            ..Default::default()
        };
        let script = ScriptAnalysisSnapshot::default();
        let file = FileContext {
            template: Some(&template),
            script: Some(&script),
            styles: &[],
            source: None,
        };
        let diags = run(&file);
        assert!(
            diags.is_empty(),
            "no defineSlots should not report: {:?}",
            diags
        );
    }

    #[test]
    fn multiple_slots_mixed() {
        let template = TemplateAnalysisSnapshot {
            defined_slots: vec![
                make_slot("header", true),   // required + fallback → report
                make_slot("footer", true),   // optional + fallback → no report
                make_slot("sidebar", false), // required + no fallback → no report
            ],
            ..Default::default()
        };
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_define_slots(vec![
                AnalyzedSlotField {
                    name: "header".to_string(),
                    is_required: true,
                    span: Span::new(20, 26),
                    bindings: vec![],
                    description: None,
                    tags: vec![],
                },
                AnalyzedSlotField {
                    name: "footer".to_string(),
                    is_required: false,
                    span: Span::new(30, 36),
                    bindings: vec![],
                    description: None,
                    tags: vec![],
                },
                AnalyzedSlotField {
                    name: "sidebar".to_string(),
                    is_required: true,
                    span: Span::new(40, 47),
                    bindings: vec![],
                    description: None,
                    tags: vec![],
                },
            ])],
            flags: AnalysisFlags::HAS_DEFINE_SLOTS,
            ..Default::default()
        };
        let file = FileContext {
            template: Some(&template),
            script: Some(&script),
            styles: &[],
            source: None,
        };
        let diags = run(&file);
        assert_eq!(
            diags.len(),
            1,
            "should report only for required 'header' with fallback: {:?}",
            diags
        );
        assert!(diags[0].message.contains("header"));
    }
}
