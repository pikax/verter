//! Condition analysis for prop-based conditional root generic narrowing.
//!
//! Analyzes v-if/v-else-if/v-else condition expressions on root elements
//! to determine if they reference props in simple patterns that can be
//! converted to TypeScript conditional types with generic narrowing.

use rustc_hash::FxHashSet;

/// A single condition analysis result for one v-if/v-else-if branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionNarrowing {
    /// The prop name referenced in the condition (e.g., "foo", "mode").
    pub prop_name: String,
    /// The literal value being compared against.
    /// `None` for bare prop truthiness (`v-if="foo"` / `v-if="!foo"`).
    pub literal: Option<String>,
    /// Whether the condition is negated (`!prop` or `prop !== literal`).
    pub negated: bool,
}

/// A branch in the conditional root chain for narrowing.
#[derive(Debug, Clone)]
pub struct NarrowingBranch {
    /// Comp function offset identifier.
    pub comp_offset: u32,
    /// The narrowing info. `None` for v-else (terminal fallback).
    pub narrowing: Option<ConditionNarrowing>,
}

/// Deduplicated generic parameter derived from condition analysis.
#[derive(Debug, Clone)]
pub struct NarrowingGeneric {
    /// The prop name that becomes a generic type parameter.
    pub prop_name: String,
}

/// Complete narrowing analysis for a conditional root chain.
#[derive(Debug, Clone)]
pub struct ConditionalRootNarrowing {
    /// Ordered branches (one per v-if/v-else-if/v-else).
    pub branches: Vec<NarrowingBranch>,
    /// Deduplicated generics — one per unique prop referenced.
    pub generics: Vec<NarrowingGeneric>,
    /// Whether the chain ends with a v-else.
    /// Used for future enhancements (exhaustiveness checking).
    #[allow(dead_code)]
    pub has_else: bool,
}

/// Error from condition analysis — the condition is too complex.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct NarrowingError {
    pub reason: String,
    pub condition: String,
}

/// Analyze a single condition expression to determine if it references
/// a single prop in a simple, narrowable pattern.
///
/// Returns `Ok(ConditionNarrowing)` for simple patterns, `Err` for complex ones.
pub fn analyze_condition(
    condition: &str,
    prop_names: &FxHashSet<&str>,
) -> Result<ConditionNarrowing, String> {
    let trimmed = condition.trim();

    if trimmed.is_empty() {
        return Err("empty condition".into());
    }

    // Check for function calls: any identifier followed by (
    if trimmed.contains('(') {
        return Err(format!("condition contains function call: {trimmed}"));
    }

    // Check for member access (dots — but not in string literals)
    if contains_member_access(trimmed) {
        return Err(format!("condition contains member access: {trimmed}"));
    }

    // Check for logical operators (&&, ||) — multiple conditions
    if trimmed.contains("&&") || trimmed.contains("||") {
        return Err(format!("condition contains logical operators: {trimmed}"));
    }

    // Check for template literals
    if trimmed.contains('`') {
        return Err(format!("condition contains template literal: {trimmed}"));
    }

    // Try === or !== pattern first
    if let Some(result) = try_comparison(trimmed, "!==", prop_names) {
        return result.map(|mut n| {
            n.negated = true;
            n
        });
    }
    if let Some(result) = try_comparison(trimmed, "===", prop_names) {
        return result;
    }

    // Bare prop or negated bare prop
    let (ident, negated) = if let Some(rest) = trimmed.strip_prefix('!') {
        (rest.trim(), true)
    } else {
        (trimmed, false)
    };

    if !is_valid_identifier(ident) {
        return Err(format!("not a valid identifier: {ident}"));
    }

    if !prop_names.contains(ident) {
        return Err(format!("'{ident}' is not a prop"));
    }

    Ok(ConditionNarrowing {
        prop_name: ident.to_string(),
        literal: None,
        negated,
    })
}

/// Analyze an entire conditional chain (v-if / v-else-if* / v-else?).
///
/// `conditions` is `(condition_text, comp_offset)` per branch.
/// - v-if and v-else-if have `Some(condition_text)`
/// - v-else has `None`
///
/// Returns `Err` if ANY condition is too complex.
pub fn analyze_conditional_chain(
    conditions: &[(Option<&str>, u32)],
    prop_names: &FxHashSet<&str>,
) -> Result<ConditionalRootNarrowing, Vec<NarrowingError>> {
    let mut branches = Vec::with_capacity(conditions.len());
    let mut errors = Vec::new();
    let mut seen_props: Vec<String> = Vec::new();
    let mut has_else = false;

    for (condition_text, comp_offset) in conditions {
        match condition_text {
            Some(text) => match analyze_condition(text, prop_names) {
                Ok(narrowing) => {
                    if !seen_props.contains(&narrowing.prop_name) {
                        seen_props.push(narrowing.prop_name.clone());
                    }
                    branches.push(NarrowingBranch {
                        comp_offset: *comp_offset,
                        narrowing: Some(narrowing),
                    });
                }
                Err(reason) => {
                    errors.push(NarrowingError {
                        reason,
                        condition: text.to_string(),
                    });
                }
            },
            None => {
                // v-else
                has_else = true;
                branches.push(NarrowingBranch {
                    comp_offset: *comp_offset,
                    narrowing: None,
                });
            }
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    let generics = seen_props
        .into_iter()
        .map(|prop_name| NarrowingGeneric { prop_name })
        .collect();

    Ok(ConditionalRootNarrowing {
        branches,
        generics,
        has_else,
    })
}

// ── Helpers ──────────────────────────────────────────────────────

/// Try to parse `expr === literal` or `expr !== literal` (based on `op`).
fn try_comparison(
    expr: &str,
    op: &str,
    prop_names: &FxHashSet<&str>,
) -> Option<Result<ConditionNarrowing, String>> {
    let idx = expr.find(op)?;
    let lhs = expr[..idx].trim();
    let rhs = expr[idx + op.len()..].trim();

    // LHS must be a valid identifier that's a prop
    if !is_valid_identifier(lhs) {
        return Some(Err(format!(
            "left side of {op} is not a valid identifier: {lhs}"
        )));
    }
    if !prop_names.contains(lhs) {
        return Some(Err(format!("'{lhs}' is not a prop")));
    }

    // RHS must be a literal (string, number, boolean)
    if !is_literal(rhs) {
        return Some(Err(format!("right side of {op} is not a literal: {rhs}")));
    }

    Some(Ok(ConditionNarrowing {
        prop_name: lhs.to_string(),
        literal: Some(rhs.to_string()),
        negated: false,
    }))
}

/// Check if a string is a valid JS identifier (simple check).
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

/// Check if a string is a JS literal (string, number, boolean, null, undefined).
fn is_literal(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // String literal: 'x' or "x"
    if (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')) {
        return s.len() >= 2;
    }
    // Boolean literals
    if s == "true" || s == "false" {
        return true;
    }
    // null / undefined
    if s == "null" || s == "undefined" {
        return true;
    }
    // Number literal (integer or float, possibly negative)
    let num_str = s.strip_prefix('-').unwrap_or(s);
    num_str.parse::<f64>().is_ok()
        && !num_str.is_empty()
        && num_str.chars().next().unwrap().is_ascii_digit()
}

/// Check for member access (dots) outside of string literals.
fn contains_member_access(s: &str) -> bool {
    let mut in_single = false;
    let mut in_double = false;
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'.' if !in_single && !in_double => {
                // Check it's not a number literal like 3.14
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

    fn props<'a>(names: &[&'a str]) -> FxHashSet<&'a str> {
        names.iter().copied().collect()
    }

    // ── analyze_condition ────────────────────────────────────────

    #[test]
    fn bare_prop() {
        let result = analyze_condition("show", &props(&["show"])).unwrap();
        assert_eq!(result.prop_name, "show");
        assert_eq!(result.literal, None);
        assert!(!result.negated);
    }

    #[test]
    fn negated_bare_prop() {
        let result = analyze_condition("!show", &props(&["show"])).unwrap();
        assert_eq!(result.prop_name, "show");
        assert_eq!(result.literal, None);
        assert!(result.negated);
    }

    #[test]
    fn prop_equals_string() {
        let result = analyze_condition("mode === 'dark'", &props(&["mode"])).unwrap();
        assert_eq!(result.prop_name, "mode");
        assert_eq!(result.literal.as_deref(), Some("'dark'"));
        assert!(!result.negated);
    }

    #[test]
    fn prop_not_equals_string() {
        let result = analyze_condition("mode !== 'dark'", &props(&["mode"])).unwrap();
        assert_eq!(result.prop_name, "mode");
        assert_eq!(result.literal.as_deref(), Some("'dark'"));
        assert!(result.negated);
    }

    #[test]
    fn prop_equals_number() {
        let result = analyze_condition("count === 42", &props(&["count"])).unwrap();
        assert_eq!(result.prop_name, "count");
        assert_eq!(result.literal.as_deref(), Some("42"));
        assert!(!result.negated);
    }

    #[test]
    fn prop_equals_boolean() {
        let result = analyze_condition("flag === true", &props(&["flag"])).unwrap();
        assert_eq!(result.prop_name, "flag");
        assert_eq!(result.literal.as_deref(), Some("true"));
        assert!(!result.negated);
    }

    #[test]
    fn not_a_prop() {
        let err = analyze_condition("show", &props(&["msg"])).unwrap_err();
        assert!(err.contains("not a prop"), "got: {err}");
    }

    #[test]
    fn multiple_identifiers_rejected() {
        let err = analyze_condition("show && x", &props(&["show", "x"])).unwrap_err();
        assert!(err.contains("logical operators"), "got: {err}");
    }

    #[test]
    fn member_expression_rejected() {
        let err = analyze_condition("items.length", &props(&["items"])).unwrap_err();
        assert!(err.contains("member access"), "got: {err}");
    }

    #[test]
    fn function_call_rejected() {
        let err = analyze_condition("fn()", &props(&["fn"])).unwrap_err();
        assert!(err.contains("function call"), "got: {err}");
    }

    #[test]
    fn template_literal_rejected() {
        let err = analyze_condition("`test`", &props(&["test"])).unwrap_err();
        assert!(err.contains("template literal"), "got: {err}");
    }

    #[test]
    fn non_prop_in_comparison() {
        let err = analyze_condition("count === 5", &props(&["name"])).unwrap_err();
        assert!(err.contains("not a prop"), "got: {err}");
    }

    #[test]
    fn non_literal_rhs() {
        let err = analyze_condition("mode === variable", &props(&["mode"])).unwrap_err();
        assert!(err.contains("not a literal"), "got: {err}");
    }

    // ── analyze_conditional_chain ────────────────────────────────

    #[test]
    fn chain_simple_two_branch() {
        let p = props(&["show"]);
        let result = analyze_conditional_chain(&[(Some("show"), 100), (None, 200)], &p).unwrap();
        assert_eq!(result.branches.len(), 2);
        assert_eq!(result.generics.len(), 1);
        assert_eq!(result.generics[0].prop_name, "show");
        assert!(result.has_else);
        assert!(result.branches[0].narrowing.is_some());
        assert!(result.branches[1].narrowing.is_none());
    }

    #[test]
    fn chain_multi_prop() {
        let p = props(&["foo", "s"]);
        let result = analyze_conditional_chain(
            &[(Some("foo"), 100), (Some("s === 'bar'"), 200), (None, 300)],
            &p,
        )
        .unwrap();
        assert_eq!(result.generics.len(), 2);
        assert_eq!(result.generics[0].prop_name, "foo");
        assert_eq!(result.generics[1].prop_name, "s");
        assert!(result.has_else);
    }

    #[test]
    fn chain_same_prop_multiple_branches() {
        let p = props(&["m"]);
        let result = analyze_conditional_chain(
            &[
                (Some("m === 'a'"), 100),
                (Some("m === 'b'"), 200),
                (None, 300),
            ],
            &p,
        )
        .unwrap();
        // Deduplicated: only one generic for "m"
        assert_eq!(result.generics.len(), 1);
        assert_eq!(result.generics[0].prop_name, "m");
        assert_eq!(result.branches.len(), 3);
    }

    #[test]
    fn chain_complex_condition_fails() {
        let p = props(&["show", "x"]);
        let errors =
            analyze_conditional_chain(&[(Some("show && x"), 100), (None, 200)], &p).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].reason.contains("logical operators"));
    }

    #[test]
    fn chain_no_else() {
        let p = props(&["show"]);
        let result = analyze_conditional_chain(&[(Some("show"), 100)], &p).unwrap();
        assert!(!result.has_else);
        assert_eq!(result.branches.len(), 1);
    }

    // ── Helper tests ─────────────────────────────────────────────

    #[test]
    fn is_literal_strings() {
        assert!(is_literal("'hello'"));
        assert!(is_literal("\"world\""));
        assert!(!is_literal("'"));
        assert!(!is_literal("hello"));
    }

    #[test]
    fn is_literal_numbers() {
        assert!(is_literal("42"));
        assert!(is_literal("3.14"));
        assert!(is_literal("-1"));
        assert!(!is_literal("abc"));
    }

    #[test]
    fn is_literal_booleans() {
        assert!(is_literal("true"));
        assert!(is_literal("false"));
        assert!(is_literal("null"));
        assert!(is_literal("undefined"));
    }

    #[test]
    fn member_access_outside_strings() {
        assert!(contains_member_access("items.length"));
        assert!(!contains_member_access("mode === 'foo.bar'"));
        assert!(!contains_member_access("3.14"));
    }

    #[test]
    fn valid_identifiers() {
        assert!(is_valid_identifier("show"));
        assert!(is_valid_identifier("_private"));
        assert!(is_valid_identifier("$ref"));
        assert!(!is_valid_identifier("123abc"));
        assert!(!is_valid_identifier(""));
        assert!(!is_valid_identifier("a-b"));
    }
}
