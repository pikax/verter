//! Rule: no-unused-vars
//!
//! Disallow unused scope variables defined by `v-for` or `v-slot`. Variables
//! that are declared but never referenced in the template waste memory and
//! indicate dead code.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct NoUnusedVars;

impl LintRule for NoUnusedVars {
    fn name(&self) -> &'static str {
        "no-unused-vars"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        // Collect all binding occurrence names for quick lookup
        let used_names: std::collections::HashSet<&str> = tpl
            .binding_occurrences
            .iter()
            .map(|b| b.name.as_str())
            .collect();

        // Check v-for variables
        for el in &tpl.elements {
            if let Some(ref v_for) = el.v_for {
                // Check the iterator variable
                if !v_for.variable.starts_with('_') && !used_names.contains(v_for.variable.as_str())
                {
                    ctx.report_with_severity(
                        self.name(),
                        self.category().as_str(),
                        format!(
                            "'v-for' variable '{}' is defined but never used. Prefix with '_' to ignore.",
                            v_for.variable
                        ),
                        v_for.span.start,
                        v_for.span.end,
                        self.default_severity(),
                        DiagnosticSpanKind::Directive,
                    );
                }

                // Check the index variable
                if let Some(ref index) = v_for.index {
                    if !index.starts_with('_') && !used_names.contains(index.as_str()) {
                        ctx.report_with_severity(
                            self.name(),
                            self.category().as_str(),
                            format!(
                                "'v-for' index variable '{}' is defined but never used. Prefix with '_' to ignore.",
                                index
                            ),
                            v_for.span.start,
                            v_for.span.end,
                            self.default_severity(),
                            DiagnosticSpanKind::Directive,
                        );
                    }
                }
            }
        }

        // Check v-slot variables: look for slot directives with expression bindings
        for el in &tpl.elements {
            for dir in &el.directives {
                if dir.name != "slot" {
                    continue;
                }
                if let Some(ref expr) = dir.expression {
                    // Extract variable names from destructuring: { a, b } or just `name`
                    let vars = extract_slot_vars(expr);
                    for var in vars {
                        if !var.starts_with('_') && !used_names.contains(var) {
                            ctx.report_with_severity(
                                self.name(),
                                self.category().as_str(),
                                format!(
                                    "'v-slot' variable '{}' is defined but never used. Prefix with '_' to ignore.",
                                    var
                                ),
                                dir.span.start,
                                dir.span.end,
                                self.default_severity(),
                                DiagnosticSpanKind::Directive,
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Extract variable names from a slot binding expression.
/// Handles simple names and basic destructuring: `{ a, b }`, `{ a: renamed }`, `name`.
fn extract_slot_vars(expr: &str) -> Vec<&str> {
    let trimmed = expr.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        // Destructured: { a, b, c: d }
        let inner = &trimmed[1..trimmed.len() - 1];
        inner
            .split(',')
            .filter_map(|part| {
                let part = part.trim();
                if part.is_empty() {
                    return None;
                }
                // Handle rename: `original: renamed` — the binding is `renamed`
                if let Some((_orig, renamed)) = part.split_once(':') {
                    let r = renamed.trim();
                    if r.is_empty() {
                        None
                    } else {
                        Some(r)
                    }
                } else {
                    Some(part)
                }
            })
            .collect()
    } else if trimmed.starts_with('[') {
        // Array destructuring: [a, b]
        let inner = trimmed
            .strip_prefix('[')
            .unwrap_or(trimmed)
            .strip_suffix(']')
            .unwrap_or(trimmed);
        inner
            .split(',')
            .filter_map(|part| {
                let part = part.trim();
                if part.is_empty() {
                    None
                } else {
                    Some(part)
                }
            })
            .collect()
    } else if !trimmed.is_empty() {
        vec![trimmed]
    } else {
        vec![]
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoUnusedVars)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn unused_v_for_variable_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                v_for: Some(VForDirective {
                    variable: "item".to_string(),
                    index: None,
                    iterable: "items".to_string(),
                    has_key: false,
                    key_expression: None,
                    key_uses_index: false,
                    span: Span::new(5, 30),
                }),
                span: Span::new(0, 50),
                tag_span_end: 35,
                ..Default::default()
            }],
            binding_occurrences: vec![], // no usage of 'item'
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "unused v-for variable should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-unused-vars"));
        assert!(
            diags[0].message.contains("item"),
            "message should mention the variable"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "require-v-for-key"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn used_v_for_variable_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                v_for: Some(VForDirective {
                    variable: "item".to_string(),
                    index: None,
                    iterable: "items".to_string(),
                    has_key: true,
                    key_expression: Some("item.id".to_string()),
                    key_uses_index: false,
                    span: Span::new(5, 30),
                }),
                span: Span::new(0, 50),
                tag_span_end: 35,
                ..Default::default()
            }],
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "item".to_string(),
                span: Span::new(40, 44),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "used v-for variable should pass");
    }

    #[test]
    fn underscore_prefixed_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                v_for: Some(VForDirective {
                    variable: "_item".to_string(),
                    index: Some("_idx".to_string()),
                    iterable: "items".to_string(),
                    has_key: false,
                    key_expression: None,
                    key_uses_index: false,
                    span: Span::new(5, 35),
                }),
                span: Span::new(0, 50),
                tag_span_end: 40,
                ..Default::default()
            }],
            binding_occurrences: vec![],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "underscore-prefixed vars should pass");
    }

    #[test]
    fn unused_v_for_index_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                v_for: Some(VForDirective {
                    variable: "item".to_string(),
                    index: Some("idx".to_string()),
                    iterable: "items".to_string(),
                    has_key: false,
                    key_expression: None,
                    key_uses_index: false,
                    span: Span::new(5, 35),
                }),
                span: Span::new(0, 50),
                tag_span_end: 40,
                ..Default::default()
            }],
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "item".to_string(),
                span: Span::new(42, 46),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "unused v-for index should trigger");
        assert!(
            diags[0].message.contains("idx"),
            "message should mention the index"
        );
    }
}
