//! Condition scope model and guard text generation for TSX type narrowing.
//!
//! Mirrors the TypeScript `TemplateCondition` + `generateConditionText()` pattern
//! from `packages/core/src/v5/process/template/plugins/conditional/conditional.ts`.
//!
//! Each `v-if`/`v-else-if`/`v-else` element in the template creates a [`ConditionScope`].
//! Children inherit accumulated scopes from their ancestors. The accumulated scopes
//! are used to generate type narrowing guards:
//! - **Block guards** (`if(!(condText)) return;`) at the start of nested v-if IIFEs
//! - **Ternary guards** (`!(condText)? undefined :`) in arrow-function prop expressions

/// A condition scope entry for type narrowing.
///
/// Each v-if/v-else-if/v-else in the ancestry creates one.
/// Children inherit the accumulated vec of scopes.
#[derive(Clone, Debug)]
pub struct ConditionScope {
    /// The resolved condition expression (positive — what must be true).
    /// None for v-else (no positive condition, only negations).
    pub positive: Option<String>,
    /// Resolved condition expressions of preceding siblings (negative — must be false).
    /// For v-if: empty (no prior siblings)
    /// For v-else-if: [resolved_cond_of_v_if, resolved_cond_of_prior_else_ifs...]
    /// For v-else: [resolved_cond_of_v_if, all_prior_else_if_conds...]
    pub sibling_negations: Vec<String>,
}

/// Build the combined condition text from accumulated scopes.
///
/// Mirrors TS `generateConditionText()` (conditional.ts:258-304).
///
/// Example for v-else-if "C" after v-if "A", nested inside v-if "P":
///   scopes = [
///     ConditionScope { positive: Some("P"), sibling_negations: [] },  // parent v-if
///     ConditionScope { positive: Some("C"), sibling_negations: ["A"] }, // current v-else-if
///   ]
///   → "!((A)) && (P) && (C)"
pub fn generate_condition_text(scopes: &[ConditionScope]) -> Option<String> {
    let negations: Vec<String> = scopes
        .iter()
        .flat_map(|s| s.sibling_negations.iter())
        .map(|cond| format!("!({})", wrap_if_needed(cond)))
        .collect();

    let positives: Vec<String> = scopes
        .iter()
        .filter_map(|s| s.positive.as_ref())
        .map(|cond| wrap_if_needed(cond).into_owned())
        .collect();

    let parts: Vec<&str> = negations
        .iter()
        .chain(positives.iter())
        .map(|s| s.as_str())
        .collect();

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" && "))
    }
}

/// For block scope: `if(!(condText)) return;`
pub fn build_block_guard(condition_text: &str) -> String {
    format!("if(!({})) return;", condition_text)
}

/// For arrow expression: `!(condText)?undefined:`
pub fn build_ternary_guard(condition_text: &str) -> String {
    format!("!({})?undefined:", condition_text)
}

/// Wraps expression in parentheses for safe composition in compound conditions.
/// Already-wrapped expressions (balanced outer parens) are returned as-is.
///
/// Mirrors TS `wrapIfNeeded()` at conditional.ts:313-336.
/// All expressions are wrapped to ensure correct precedence in negation
/// and conjunction contexts: `!(expr)`, `(expr) && (other)`.
fn wrap_if_needed(expr: &str) -> std::borrow::Cow<'_, str> {
    if expr.starts_with('(') && expr.ends_with(')') {
        // Check if outer parens are balanced (wrap the entire expression)
        let mut depth = 0i32;
        let bytes = expr.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'(' {
                depth += 1;
            }
            if b == b')' {
                depth -= 1;
            }
            // If depth hits 0 before the last char, the outer parens don't wrap everything
            if depth == 0 && i < bytes.len() - 1 {
                return std::borrow::Cow::Owned(format!("({})", expr));
            }
        }
        return std::borrow::Cow::Borrowed(expr);
    }
    std::borrow::Cow::Owned(format!("({})", expr))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── wrap_if_needed ──────────────────────────────────────────

    #[test]
    fn wrap_simple_ident_gets_wrapped() {
        assert_eq!(wrap_if_needed("x").as_ref(), "(x)");
    }

    #[test]
    fn wrap_expr_with_and_gets_wrapped() {
        assert_eq!(wrap_if_needed("a && b").as_ref(), "(a && b)");
    }

    #[test]
    fn wrap_expr_with_or_gets_wrapped() {
        assert_eq!(wrap_if_needed("a || b").as_ref(), "(a || b)");
    }

    #[test]
    fn wrap_already_wrapped_stays() {
        assert_eq!(
            wrap_if_needed("(typeof test === 'string')").as_ref(),
            "(typeof test === 'string')"
        );
    }

    #[test]
    fn wrap_split_parens_gets_wrapped() {
        // (a) && (b) — outer parens close at position 2, not the end
        assert_eq!(wrap_if_needed("(a) && (b)").as_ref(), "((a) && (b))");
    }

    #[test]
    fn wrap_typeof_gets_wrapped() {
        assert_eq!(
            wrap_if_needed("typeof test === 'string'").as_ref(),
            "(typeof test === 'string')"
        );
    }

    // ── generate_condition_text ──────────────────────────────────

    #[test]
    fn condition_text_simple_v_if() {
        let scopes = vec![ConditionScope {
            positive: Some("show".into()),
            sibling_negations: vec![],
        }];
        assert_eq!(generate_condition_text(&scopes).unwrap(), "(show)");
    }

    #[test]
    fn condition_text_v_else_if_with_negation() {
        let scopes = vec![ConditionScope {
            positive: Some("typeof test === 'string'".into()),
            sibling_negations: vec!["typeof test === 'object'".into()],
        }];
        assert_eq!(
            generate_condition_text(&scopes).unwrap(),
            "!((typeof test === 'object')) && (typeof test === 'string')"
        );
    }

    #[test]
    fn condition_text_v_else_negates_all() {
        let scopes = vec![ConditionScope {
            positive: None,
            sibling_negations: vec![
                "typeof test === 'string'".into(),
                "typeof test === 'number'".into(),
            ],
        }];
        assert_eq!(
            generate_condition_text(&scopes).unwrap(),
            "!((typeof test === 'string')) && !((typeof test === 'number'))"
        );
    }

    #[test]
    fn condition_text_nested_v_if_combines_parent_and_own() {
        let scopes = vec![
            ConditionScope {
                positive: Some("typeof test === 'string'".into()),
                sibling_negations: vec![],
            },
            ConditionScope {
                positive: Some("test === 'app'".into()),
                sibling_negations: vec![],
            },
        ];
        assert_eq!(
            generate_condition_text(&scopes).unwrap(),
            "(typeof test === 'string') && (test === 'app')"
        );
    }

    #[test]
    fn condition_text_nested_with_sibling_negations() {
        // v-else-if "C" after v-if "A", nested inside v-if "P"
        let scopes = vec![
            ConditionScope {
                positive: Some("P".into()),
                sibling_negations: vec![],
            },
            ConditionScope {
                positive: Some("C".into()),
                sibling_negations: vec!["A".into()],
            },
        ];
        assert_eq!(
            generate_condition_text(&scopes).unwrap(),
            "!((A)) && (P) && (C)"
        );
    }

    #[test]
    fn condition_text_complex_chain() {
        // Full chain: v-if "A" → v-else-if "B" → v-else-if "C" → v-else
        // For v-else: negations = [A, B, C], positive = None
        let scopes = vec![ConditionScope {
            positive: None,
            sibling_negations: vec!["A".into(), "B".into(), "C".into()],
        }];
        assert_eq!(
            generate_condition_text(&scopes).unwrap(),
            "!((A)) && !((B)) && !((C))"
        );
    }

    #[test]
    fn condition_text_empty_scopes_returns_none() {
        assert!(generate_condition_text(&[]).is_none());
    }

    #[test]
    fn condition_text_and_or_get_wrapped() {
        let scopes = vec![ConditionScope {
            positive: Some("a && b".into()),
            sibling_negations: vec!["c || d".into()],
        }];
        assert_eq!(
            generate_condition_text(&scopes).unwrap(),
            "!((c || d)) && (a && b)"
        );
    }

    // ── build_block_guard ────────────────────────────────────────

    #[test]
    fn block_guard_simple() {
        assert_eq!(build_block_guard("show"), "if(!(show)) return;");
    }

    #[test]
    fn block_guard_complex() {
        assert_eq!(build_block_guard("!(A) && B"), "if(!(!(A) && B)) return;");
    }

    // ── build_ternary_guard ──────────────────────────────────────

    #[test]
    fn ternary_guard_simple() {
        assert_eq!(
            build_ternary_guard("typeof test === 'string'"),
            "!(typeof test === 'string')?undefined:"
        );
    }

    #[test]
    fn ternary_guard_complex() {
        assert_eq!(
            build_ternary_guard("!((typeof test === 'object')) && (typeof test === 'string')"),
            "!(!((typeof test === 'object')) && (typeof test === 'string'))?undefined:"
        );
    }
}
