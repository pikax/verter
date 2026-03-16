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

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        // Collect all binding occurrence names for quick lookup.
        // Include both resolved (script) and unresolved bindings.
        let used_names: std::collections::HashSet<&str> = tpl
            .binding_occurrences
            .iter()
            .map(|b| b.name.as_str())
            .chain(tpl.unresolved_bindings.iter().map(|b| b.name.as_str()))
            .collect();

        // Check v-for variables
        for (el_idx, el) in tpl.elements.iter().enumerate() {
            if let Some(ref v_for) = el.v_for {
                // Check the iterator variable
                if !v_for.variable.starts_with('_')
                    && !used_names.contains(v_for.variable.as_str())
                    && !is_var_used_in_subtree(&v_for.variable, el_idx, &v_for.key_expression, tpl)
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
                    if !index.starts_with('_')
                        && !used_names.contains(index.as_str())
                        && !is_var_used_in_subtree(index, el_idx, &v_for.key_expression, tpl)
                    {
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
        for (el_idx, el) in tpl.elements.iter().enumerate() {
            for dir in &el.directives {
                if dir.name != "slot" {
                    continue;
                }
                if let Some(ref expr) = dir.expression {
                    // Extract variable names from destructuring: { a, b } or just `name`
                    let vars = extract_slot_vars(expr);
                    for var in vars {
                        if !var.starts_with('_')
                            && !used_names.contains(var)
                            && !is_var_used_in_subtree(var, el_idx, &None, tpl)
                        {
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

/// Check if a v-for scoped variable is used in directive expressions, dynamic
/// attribute values, or key expression within the v-for element's subtree.
///
/// V-for scoped variables (like `item`, `index`) are marked `ignore` by OXC's
/// binding analysis (meaning "don't prefix with `_ctx.`"), so they're filtered
/// out of `binding_occurrences`. This function directly scans the subtree's
/// directive expressions and dynamic attribute values as a fallback.
fn is_var_used_in_subtree(
    var_name: &str,
    v_for_el_idx: usize,
    key_expression: &Option<String>,
    tpl: &TemplateAnalysisSnapshot,
) -> bool {
    // Check key expression
    if let Some(ref key_expr) = key_expression {
        if contains_identifier(key_expr, var_name) {
            return true;
        }
    }

    // Check all descendant elements (including the v-for element itself)
    for (idx, el) in tpl.elements.iter().enumerate() {
        if idx != v_for_el_idx && !is_descendant_of(idx, v_for_el_idx, tpl) {
            continue;
        }

        // Check directive expressions
        for dir in &el.directives {
            if let Some(ref expr) = dir.expression {
                if contains_identifier(expr, var_name) {
                    return true;
                }
            }
        }

        // Check dynamic attribute values
        for attr in &el.attributes {
            if attr.is_dynamic {
                if let Some(ref val) = attr.value {
                    if contains_identifier(val, var_name) {
                        return true;
                    }
                }
            }
        }

        // Check v-if condition
        if let Some(ref cond) = el.v_if_condition {
            if contains_identifier(cond, var_name) {
                return true;
            }
        }

        // Check text interpolation children
        for seg in &el.text_children {
            if let verter_analysis::template::TemplateTextSegment::Interpolation { .. } = seg {
                // We don't have the expression text in the interpolation segment,
                // but interpolation bindings would appear in binding_occurrences
                // (checked in the main loop via used_names)
            }
        }
    }

    false
}

/// Check if `text` contains `name` as a standalone identifier (not as part of a longer word).
fn contains_identifier(text: &str, name: &str) -> bool {
    for (i, _) in text.match_indices(name) {
        let before_ok = i == 0 || {
            let b = text.as_bytes()[i - 1];
            !b.is_ascii_alphanumeric() && b != b'_' && b != b'$'
        };
        let after = i + name.len();
        let after_ok = after >= text.len() || {
            let b = text.as_bytes()[after];
            !b.is_ascii_alphanumeric() && b != b'_' && b != b'$'
        };
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

/// Check if element at `idx` is a descendant of element at `ancestor_idx`.
fn is_descendant_of(idx: usize, ancestor_idx: usize, tpl: &TemplateAnalysisSnapshot) -> bool {
    let mut current = idx;
    loop {
        if let Some(parent) = tpl.elements[current].parent_index {
            let parent_idx = parent as usize;
            if parent_idx == ancestor_idx {
                return true;
            }
            if parent_idx >= current {
                return false; // prevent infinite loop
            }
            current = parent_idx;
        } else {
            return false;
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

    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoUnusedVars, template)
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
                content_end: 0,
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
                content_end: 0,
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
                content_end: 0,
                ..Default::default()
            }],
            binding_occurrences: vec![],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "underscore-prefixed vars should pass");
    }

    #[test]
    fn v_for_var_used_in_child_directive_passes() {
        // FP1: v-for variables used in child component props (e.g., :text="action.text")
        // should NOT be reported as unused. OXC marks them `ignore` (for _ctx. prefix)
        // so they don't appear in binding_occurrences. The rule checks directive
        // expressions and dynamic attribute values on descendant elements.
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                TemplateElement {
                    tag: "div".to_string(),
                    v_for: Some(VForDirective {
                        variable: "action".to_string(),
                        index: Some("index".to_string()),
                        iterable: "actions".to_string(),
                        has_key: true,
                        key_expression: Some("index".to_string()),
                        key_uses_index: true,
                        span: Span::new(5, 40),
                    }),
                    span: Span::new(0, 100),
                    tag_span_end: 45,
                    content_end: 0,
                    ..Default::default()
                },
                TemplateElement {
                    tag: "ChildComp".to_string(),
                    is_component: true,
                    parent_index: Some(0),
                    attributes: vec![TemplateAttribute {
                        name: "text".to_string(),
                        value: Some("action.text".to_string()),
                        is_dynamic: true,
                        span: Span::new(55, 75),
                        name_end: 60,
                        value_span: Some(Span::new(61, 72)),
                    }],
                    directives: vec![TemplateDirective {
                        name: "on".to_string(),
                        raw_name: "@click".to_string(),
                        argument: Some("click".to_string()),
                        modifiers: vec![],
                        expression: Some("onClickItem(action, index)".to_string()),
                        span: Span::new(76, 95),
                        name_end: 82,
                        arg_span: None,
                        expression_span: None,
                        modifier_spans: vec![],
                    }],
                    span: Span::new(50, 90),
                    tag_span_end: 75,
                    content_end: 0,
                    ..Default::default()
                },
            ],
            binding_occurrences: vec![], // NOT in binding_occurrences (v-for scoped, ignore=true)
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.iter().any(|d| d.message.contains("'action'")),
            "v-for variable 'action' used in child attrs/directives should NOT be reported unused"
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("'index'")),
            "v-for index 'index' used in child directive expression should NOT be reported unused"
        );
    }

    #[test]
    fn v_slot_var_used_in_child_shorthand_passes() {
        // V-slot variables like { char, index } are used via same-name shorthand
        // attributes on children (e.g., :char :index). OXC marks them ignore=true
        // so they don't appear in binding_occurrences. The rule scans the subtree.
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                TemplateElement {
                    tag: "MyComp".to_string(),
                    is_component: true,
                    directives: vec![TemplateDirective {
                        name: "slot".to_string(),
                        raw_name: "v-slot".to_string(),
                        argument: None,
                        modifiers: vec![],
                        expression: Some("{ char, index }".to_string()),
                        span: Span::new(10, 40),
                        name_end: 16,
                        arg_span: None,
                        expression_span: None,
                        modifier_spans: vec![],
                    }],
                    span: Span::new(0, 100),
                    tag_span_end: 45,
                    content_end: 0,
                    ..Default::default()
                },
                TemplateElement {
                    tag: "slot".to_string(),
                    parent_index: Some(0),
                    attributes: vec![
                        TemplateAttribute {
                            name: "char".to_string(),
                            value: Some("char".to_string()),
                            is_dynamic: true,
                            span: Span::new(50, 60),
                            name_end: 54,
                            value_span: Some(Span::new(55, 59)),
                        },
                        TemplateAttribute {
                            name: "index".to_string(),
                            value: Some("index".to_string()),
                            is_dynamic: true,
                            span: Span::new(61, 73),
                            name_end: 66,
                            value_span: Some(Span::new(67, 72)),
                        },
                    ],
                    span: Span::new(50, 80),
                    tag_span_end: 75,
                    content_end: 0,
                    ..Default::default()
                },
            ],
            binding_occurrences: vec![],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.iter().any(|d| d.message.contains("'char'")),
            "v-slot var 'char' used in child shorthand should NOT be reported unused"
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("'index'")),
            "v-slot var 'index' used in child shorthand should NOT be reported unused"
        );
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
                content_end: 0,
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
