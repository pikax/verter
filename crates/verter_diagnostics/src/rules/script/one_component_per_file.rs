//! Rule: one-component-per-file
//!
//! Each `.vue` file should contain only one component definition. Having multiple
//! `defineProps` or `defineOptions` macros in a single file indicates multiple
//! component definitions, which should be split into separate files.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{AnalyzedMacroKind, ScriptAnalysisSnapshot};

pub struct OneComponentPerFile;

impl LintRule for OneComponentPerFile {
    fn name(&self) -> &'static str {
        "one-component-per-file"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        // Count defineProps macros (including withDefaults wrapping defineProps)
        let props_count = script
            .macros
            .iter()
            .filter(|m| {
                m.kind == AnalyzedMacroKind::DefineProps
                    || m.kind == AnalyzedMacroKind::WithDefaults
            })
            .count();

        if props_count > 1 {
            // Report on the second occurrence
            let second = script
                .macros
                .iter()
                .filter(|m| {
                    m.kind == AnalyzedMacroKind::DefineProps
                        || m.kind == AnalyzedMacroKind::WithDefaults
                })
                .nth(1)
                .unwrap();
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Only one component definition per file. Found multiple `defineProps` calls."
                    .to_string(),
                second.span.start,
                second.span.end,
                self.default_severity(),
                DiagnosticSpanKind::ScriptCallSite,
            );
        }

        // Count defineOptions macros
        let options_count = script
            .macros
            .iter()
            .filter(|m| m.kind == AnalyzedMacroKind::DefineOptions)
            .count();

        if options_count > 1 {
            let second = script
                .macros
                .iter()
                .filter(|m| m.kind == AnalyzedMacroKind::DefineOptions)
                .nth(1)
                .unwrap();
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Only one component definition per file. Found multiple `defineOptions` calls."
                    .to_string(),
                second.span.start,
                second.span.end,
                self.default_severity(),
                DiagnosticSpanKind::ScriptCallSite,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::types::*;
    use verter_span::Span;

    fn run_script(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(OneComponentPerFile)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_script(script, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_macro(kind: AnalyzedMacroKind, start: u32, end: u32) -> AnalyzedMacro {
        AnalyzedMacro {
            kind,
            is_type_based: false,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            span: Span::new(start, end),
        }
    }

    #[test]
    fn multiple_define_props_reports() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![
                make_macro(AnalyzedMacroKind::DefineProps, 10, 30),
                make_macro(AnalyzedMacroKind::DefineProps, 50, 70),
            ],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(!diags.is_empty(), "multiple defineProps should trigger");
        assert!(diags.iter().any(|d| d.rule == "one-component-per-file"));
        assert!(
            diags[0].message.contains("defineProps"),
            "message should mention defineProps"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn single_define_props_passes() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_macro(AnalyzedMacroKind::DefineProps, 10, 30)],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(diags.is_empty(), "single defineProps should pass");
    }

    #[test]
    fn multiple_define_options_reports() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![
                make_macro(AnalyzedMacroKind::DefineOptions, 10, 40),
                make_macro(AnalyzedMacroKind::DefineOptions, 50, 80),
            ],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(!diags.is_empty(), "multiple defineOptions should trigger");
        assert!(diags.iter().any(|d| d.rule == "one-component-per-file"));
    }

    #[test]
    fn props_and_emits_passes() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![
                make_macro(AnalyzedMacroKind::DefineProps, 10, 30),
                make_macro(AnalyzedMacroKind::DefineEmits, 40, 60),
            ],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(
            diags.is_empty(),
            "defineProps + defineEmits in one file should pass"
        );
    }

    #[test]
    fn no_macros_passes() {
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_script(&script);
        assert!(diags.is_empty(), "no macros should pass");
    }
}
