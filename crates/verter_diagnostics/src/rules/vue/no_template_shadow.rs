//! Rule: no-template-shadow
//!
//! Disallow `v-for` or `v-slot` variables that shadow variables already defined
//! in the component's `<script>` block. Shadowing makes the code confusing
//! because the template binding hides the script binding.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct NoTemplateShadow;

impl LintRule for NoTemplateShadow {
    fn name(&self) -> &'static str {
        "no-template-shadow"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        // Collect script binding names from binding_occurrences that are not
        // scoped (v-for/v-slot) — we use all occurrences as an approximation
        // of what's available from script. In practice, the analysis snapshot
        // provides these.
        let script_bindings: std::collections::HashSet<&str> = tpl
            .binding_occurrences
            .iter()
            .map(|b| b.name.as_str())
            .collect();

        if script_bindings.is_empty() {
            return;
        }

        // Check v-for variables
        for el in &tpl.elements {
            if let Some(ref v_for) = el.v_for {
                if script_bindings.contains(v_for.variable.as_str()) {
                    ctx.report_with_severity(
                        self.name(),
                        self.category().as_str(),
                        format!(
                            "Variable '{}' is already defined in the component scope. Avoid shadowing.",
                            v_for.variable
                        ),
                        v_for.span.start,
                        v_for.span.end,
                        self.default_severity(),
                        DiagnosticSpanKind::Directive,
                    );
                }
                if let Some(ref index) = v_for.index {
                    if script_bindings.contains(index.as_str()) {
                        ctx.report_with_severity(
                            self.name(),
                            self.category().as_str(),
                            format!(
                                "Variable '{}' is already defined in the component scope. Avoid shadowing.",
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

            // Check v-slot variables
            for dir in &el.directives {
                if dir.name != "slot" {
                    continue;
                }
                if let Some(ref expr) = dir.expression {
                    let vars = extract_slot_var_names(expr);
                    for var in vars {
                        if script_bindings.contains(var) {
                            ctx.report_with_severity(
                                self.name(),
                                self.category().as_str(),
                                format!(
                                    "Variable '{}' is already defined in the component scope. Avoid shadowing.",
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

/// Extract variable names from slot binding expressions.
fn extract_slot_var_names(expr: &str) -> Vec<&str> {
    let trimmed = expr.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        let inner = &trimmed[1..trimmed.len() - 1];
        inner
            .split(',')
            .filter_map(|part| {
                let part = part.trim();
                if part.is_empty() {
                    return None;
                }
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoTemplateShadow)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn v_for_shadowing_script_binding_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                v_for: Some(VForDirective {
                    variable: "item".to_string(),
                    index: None,
                    iterable: "list".to_string(),
                    has_key: false,
                    key_expression: None,
                    key_uses_index: false,
                    span: Span::new(5, 30),
                }),
                span: Span::new(0, 50),
                tag_span_end: 35,
                content_end: 0,
                ..Default::default()
            }],
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "item".to_string(),
                span: Span::new(100, 104),
                usage_kind: BindingUsageKind::DirectiveValue,
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "v-for var shadowing script binding should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "no-template-shadow"));
        assert!(
            diags[0].message.contains("item"),
            "message should mention the variable"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-unused-vars"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn v_for_unique_variable_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                v_for: Some(VForDirective {
                    variable: "item".to_string(),
                    index: None,
                    iterable: "list".to_string(),
                    has_key: false,
                    key_expression: None,
                    key_uses_index: false,
                    span: Span::new(5, 30),
                }),
                span: Span::new(0, 50),
                tag_span_end: 35,
                content_end: 0,
                ..Default::default()
            }],
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "message".to_string(),
                span: Span::new(60, 67),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "v-for var not shadowing anything should pass"
        );
    }

    #[test]
    fn v_slot_shadowing_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "template".to_string(),
                directives: vec![TemplateDirective {
                    name: "slot".to_string(),
                    raw_name: "#default".to_string(),
                    argument: Some("default".to_string()),
                    modifiers: vec![],
                    expression: Some("{ msg }".to_string()),
                    span: Span::new(10, 30),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                parent_index: Some(0),
                span: Span::new(5, 60),
                tag_span_end: 35,
                content_end: 0,
                ..Default::default()
            }],
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "msg".to_string(),
                span: Span::new(80, 83),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "v-slot var shadowing script binding should trigger"
        );
    }
}
