//! Rule: conditional-root-complex
//!
//! Warns when a root `v-if`/`v-else-if` condition is too complex for generic
//! narrowing. Only active when `conditional_root_narrowing` is enabled in config.
//!
//! Simple patterns that work: `v-if="show"`, `v-if="mode === 'dark'"`.
//! Complex patterns that don't: `v-if="show && x"`, `v-if="items.length"`,
//! `v-if="fn()"`.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};
use rustc_hash::FxHashSet;
use verter_semantic::analysis::types::AnalyzedMacroKind;

pub struct ConditionalRootComplex;

impl LintRule for ConditionalRootComplex {
    fn name(&self) -> &'static str {
        "conditional-root-complex"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        // Only active when conditional root narrowing is enabled
        if !ctx.config().conditional_root_narrowing {
            return;
        }

        let Some(template) = file.template else {
            return;
        };

        // Collect prop names from defineProps
        let prop_names: FxHashSet<&str> = file
            .script
            .iter()
            .flat_map(|s| &s.macros)
            .filter(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .flat_map(|m| &m.prop_fields)
            .map(|f| f.name.as_str())
            .collect();

        if prop_names.is_empty() {
            return;
        }

        // Check root elements with v-if/v-else-if conditions
        for el in &template.elements {
            if el.parent_index.is_some() {
                continue;
            }

            if !el.has_v_if && !el.has_v_else_if {
                continue;
            }

            let Some(condition) = el.v_if_condition.as_deref() else {
                continue;
            };

            if let Some(reason) = check_condition_complexity(condition, &prop_names) {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Root v-if condition '{condition}' is too complex for generic narrowing: {reason}. \
                         Use a simple prop reference (v-if=\"show\") or prop comparison \
                         (v-if=\"mode === 'dark'\") for type-safe root element narrowing."
                    ),
                    el.span.start,
                    el.tag_span_end,
                    self.default_severity(),
                    DiagnosticSpanKind::ElementOpenTag,
                );
            }
        }
    }
}

/// Returns `Some(reason)` if the condition is too complex for narrowing.
fn check_condition_complexity(
    condition: &str,
    prop_names: &FxHashSet<&str>,
) -> Option<&'static str> {
    let trimmed = condition.trim();

    if trimmed.is_empty() {
        return Some("empty condition");
    }

    if trimmed.contains('(') {
        return Some("contains function call");
    }

    if trimmed.contains("&&") || trimmed.contains("||") {
        return Some("contains logical operators");
    }

    if trimmed.contains('`') {
        return Some("contains template literal");
    }

    // Check for member access (dots outside of string literals and numbers)
    if contains_member_access(trimmed) {
        return Some("contains member access");
    }

    // For comparison patterns (=== / !==), check that LHS is a prop
    if let Some(idx) = trimmed.find("!==").or_else(|| trimmed.find("===")) {
        let op_len = 3;
        let lhs = trimmed[..idx].trim();
        if !prop_names.contains(lhs) {
            return Some("left side of comparison is not a prop");
        }
        let rhs = trimmed[idx + op_len..].trim();
        if !is_literal(rhs) {
            return Some("right side of comparison is not a literal");
        }
        return None;
    }

    // Bare identifier (possibly negated)
    let ident = trimmed.strip_prefix('!').unwrap_or(trimmed).trim();

    if !is_valid_identifier(ident) {
        return Some("not a valid identifier");
    }

    if !prop_names.contains(ident) {
        return Some("identifier is not a prop");
    }

    None
}

fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_alphabetic() && first != '_' && first != '$' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

fn is_literal(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    if (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')) {
        return s.len() >= 2;
    }
    if matches!(s, "true" | "false" | "null" | "undefined") {
        return true;
    }
    let num_str = s.strip_prefix('-').unwrap_or(s);
    num_str.parse::<f64>().is_ok()
        && !num_str.is_empty()
        && num_str.chars().next().unwrap().is_ascii_digit()
}

fn contains_member_access(s: &str) -> bool {
    let mut in_single = false;
    let mut in_double = false;
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'.' if !in_single && !in_double => {
                if i > 0 && bytes[i - 1].is_ascii_digit() {
                    continue;
                }
                return true;
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_semantic::analysis::template::*;
    use verter_semantic::analysis::types::{
        AnalyzedMacro, AnalyzedMacroKind, AnalyzedPropField, TypeResolutionSource,
    };
    use verter_semantic::analysis::ScriptAnalysisSnapshot;
    use verter_span::Span;

    fn make_script_with_props(prop_names: &[&str]) -> ScriptAnalysisSnapshot {
        ScriptAnalysisSnapshot {
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: Vec::new(),
                binding_name: None,
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: prop_names
                    .iter()
                    .map(|n| AnalyzedPropField {
                        name: n.to_string(),
                        is_optional: false,
                        span: Span::new(0, 0),
                        type_annotation: None,
                        description: None,
                        tags: vec![],
                        resolution_source: TypeResolutionSource::Rust,
                        resolution_error: None,
                    })
                    .collect(),
                emit_fields: vec![],
                slot_fields: vec![],
                default_keys: vec![],
                expose_fields: vec![],
                default_values: Vec::new(),
                resolved_local_types: Vec::new(),
                span: Span::new(0, 50),
            }],
            ..Default::default()
        }
    }

    fn run(
        template: &TemplateAnalysisSnapshot,
        script: &ScriptAnalysisSnapshot,
        narrowing_enabled: bool,
    ) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(ConditionalRootComplex)];
        let visitor = LintVisitor::new(&rules);
        let mut config = LintConfig::default();
        config.conditional_root_narrowing = narrowing_enabled;
        let mut ctx = LintContext::new(&config);
        let file = crate::rules::FileContext {
            template: Some(template),
            script: Some(script),
            styles: &[],
            source: None,
        };
        visitor.visit_file(&file, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn complex_condition_with_setting_on() {
        let script = make_script_with_props(&["show", "x"]);
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                TemplateElement {
                    tag: "div".to_string(),
                    has_v_if: true,
                    v_if_condition: Some("show && x".to_string()),
                    parent_index: None,
                    span: Span::new(0, 40),
                    tag_span_end: 25,
                    content_end: 0,
                    ..Default::default()
                },
                TemplateElement {
                    tag: "span".to_string(),
                    has_v_else: true,
                    parent_index: None,
                    span: Span::new(40, 60),
                    tag_span_end: 50,
                    content_end: 0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let diags = run(&template, &script, true);
        assert!(
            !diags.is_empty(),
            "complex condition should trigger warning"
        );
        assert!(diags[0].rule == "conditional-root-complex");
        assert!(
            diags[0].message.contains("logical operators"),
            "message should explain why: {}",
            diags[0].message
        );
    }

    #[test]
    fn complex_condition_with_setting_off() {
        let script = make_script_with_props(&["show", "x"]);
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                has_v_if: true,
                v_if_condition: Some("show && x".to_string()),
                parent_index: None,
                span: Span::new(0, 40),
                tag_span_end: 25,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template, &script, false);
        assert!(diags.is_empty(), "should not trigger when setting is off");
    }

    #[test]
    fn simple_condition_no_warning() {
        let script = make_script_with_props(&["show"]);
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                TemplateElement {
                    tag: "div".to_string(),
                    has_v_if: true,
                    v_if_condition: Some("show".to_string()),
                    parent_index: None,
                    span: Span::new(0, 40),
                    tag_span_end: 20,
                    content_end: 0,
                    ..Default::default()
                },
                TemplateElement {
                    tag: "span".to_string(),
                    has_v_else: true,
                    parent_index: None,
                    span: Span::new(40, 60),
                    tag_span_end: 50,
                    content_end: 0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let diags = run(&template, &script, true);
        assert!(diags.is_empty(), "simple prop condition should pass");
    }

    #[test]
    fn prop_comparison_no_warning() {
        let script = make_script_with_props(&["mode"]);
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                has_v_if: true,
                v_if_condition: Some("mode === 'dark'".to_string()),
                parent_index: None,
                span: Span::new(0, 40),
                tag_span_end: 30,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template, &script, true);
        assert!(diags.is_empty(), "prop comparison should pass");
    }

    #[test]
    fn function_call_warns() {
        let script = make_script_with_props(&["fn"]);
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                has_v_if: true,
                v_if_condition: Some("fn()".to_string()),
                parent_index: None,
                span: Span::new(0, 40),
                tag_span_end: 25,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template, &script, true);
        assert!(!diags.is_empty(), "function call should warn");
        assert!(diags[0].message.contains("function call"));
    }

    #[test]
    fn member_access_warns() {
        let script = make_script_with_props(&["items"]);
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                has_v_if: true,
                v_if_condition: Some("items.length".to_string()),
                parent_index: None,
                span: Span::new(0, 40),
                tag_span_end: 30,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template, &script, true);
        assert!(!diags.is_empty(), "member access should warn");
        assert!(diags[0].message.contains("member access"));
    }

    #[test]
    fn non_prop_identifier_warns() {
        let script = make_script_with_props(&["otherProp"]);
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                has_v_if: true,
                v_if_condition: Some("localRef".to_string()),
                parent_index: None,
                span: Span::new(0, 40),
                tag_span_end: 25,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template, &script, true);
        assert!(!diags.is_empty(), "non-prop identifier should warn");
        assert!(diags[0].message.contains("not a prop"));
    }

    #[test]
    fn nested_element_not_checked() {
        let script = make_script_with_props(&["show"]);
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                has_v_if: true,
                v_if_condition: Some("show && x".to_string()),
                parent_index: Some(0),
                span: Span::new(10, 40),
                tag_span_end: 25,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template, &script, true);
        assert!(diags.is_empty(), "nested v-if should not be checked");
    }
}
