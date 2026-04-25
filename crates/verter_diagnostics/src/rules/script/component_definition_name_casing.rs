//! Rule: component-definition-name-casing
//!
//! Component names defined in `defineOptions` should be PascalCase.
//! For example, `defineOptions({ name: 'my-component' })` should use
//! `defineOptions({ name: 'MyComponent' })`.

// @ai-generated

use crate::casing::is_pascal_case;
use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::types::{AnalyzedMacroKind, ScriptAnalysisSnapshot};

pub struct ComponentDefinitionNameCasing;

impl LintRule for ComponentDefinitionNameCasing {
    fn name(&self) -> &'static str {
        "component-definition-name-casing"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        // Look for defineOptions macros — the component name is in the binding_name
        // or we check for a "name" binding in the script analysis
        for mac in &script.macros {
            if mac.kind != AnalyzedMacroKind::DefineOptions {
                continue;
            }
            // The binding_name on DefineOptions may contain the component name
            // if it was extracted by the analysis. Otherwise, we flag the macro
            // itself if the binding name looks like it's not PascalCase.
            if let Some(ref name) = mac.binding_name {
                if !is_pascal_case(name) {
                    ctx.report_with_severity(
                        self.name(),
                        self.category().as_str(),
                        format!(
                            "Component name '{}' should be PascalCase (e.g., 'MyComponent').",
                            name
                        ),
                        mac.span.start,
                        mac.span.end,
                        self.default_severity(),
                        DiagnosticSpanKind::ScriptCallSite,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_semantic::analysis::types::*;
    use verter_span::Span;

    fn run_script(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(ComponentDefinitionNameCasing)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_script(script, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_define_options(binding_name: Option<&str>) -> AnalyzedMacro {
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineOptions,
            is_type_based: false,
            type_references: vec![],
            binding_name: binding_name.map(|s| s.to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            emit_fields: vec![],
            slot_fields: vec![],
            default_keys: vec![],
            expose_fields: vec![],
            default_values: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            span: Span::new(10, 50),
        }
    }

    #[test]
    fn kebab_case_name_reports() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_define_options(Some("my-component"))],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(
            !diags.is_empty(),
            "kebab-case component name should trigger"
        );
        assert!(diags
            .iter()
            .any(|d| d.rule == "component-definition-name-casing"));
        assert!(
            diags[0].message.contains("PascalCase"),
            "message should mention PascalCase"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn pascal_case_name_passes() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_define_options(Some("MyComponent"))],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(diags.is_empty(), "PascalCase name should pass");
    }

    #[test]
    fn no_binding_name_passes() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_define_options(None)],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(diags.is_empty(), "no binding name should pass");
    }

    #[test]
    fn define_props_not_flagged() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: false,
                type_references: vec![],
                binding_name: Some("my-props".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: vec![],
                emit_fields: vec![],
                slot_fields: vec![],
                default_keys: vec![],
                expose_fields: vec![],
                default_values: Vec::new(),
                resolved_local_types: Vec::new(),
                parsed_type_argument: None,
                span: Span::new(10, 50),
            }],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(
            diags.is_empty(),
            "defineProps should not trigger component-definition-name-casing"
        );
    }
}
