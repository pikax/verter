//! Condition scope model and guard text generation for TSX type narrowing.
//!
//! Mirrors the TypeScript `TemplateCondition` + `generateConditionText()` pattern
//! from Verter's retired TypeScript transformer. The Rust implementation is
//! now the sole production owner and the behavioral tests below are authoritative.
//!
//! Each `v-if`/`v-else-if`/`v-else` element in the template narrows the code
//! nested inside it. TypeScript drops property-access narrowing (`__props.kind`)
//! at every function boundary, so each closure the template emits — a nested
//! `v-if` IIFE or a callback prop — repeats the narrowing as a guard:
//! - **Block guards** (`if(!(condText)) return;`) at the start of nested v-if IIFEs
//! - **Ternary guards** (`!(condText)? undefined :`) in arrow-function prop expressions
//!
//! The IDE template emitter builds guards from [`resolve_chain`] and
//! [`GuardScope`]: every chain member's condition is resolved once, a member
//! reads its predecessors as a prefix of the chain's shared terms, nested scopes
//! link to their enclosing scope instead of copying it, and one guard repeats at
//! most [`MAX_GUARD_TERMS`] terms. Guard construction therefore visits a bounded
//! number of predecessors and the generated guard bytes grow linearly with the
//! number of branches and callbacks, never quadratically.

use std::cell::OnceCell;
use std::rc::Rc;

/// Most condition terms a single narrowing guard repeats.
///
/// A guard keeps its terms innermost-first — positive conditions of the
/// element and its enclosing scopes before predecessor negations, nearest
/// predecessor first — and drops the farthest terms beyond this bound. A guard
/// within the bound repeats every term, exactly as the full narrowing chain.
pub const MAX_GUARD_TERMS: usize = 16;

/// One member of a `v-if` / `v-else-if` / `v-else` chain, in chain order.
#[derive(Debug)]
pub enum ChainBranch {
    /// `v-if` / `v-else-if` with its resolved condition expression.
    Condition(String),
    /// `v-if` / `v-else-if` without a condition value: narrows nothing and
    /// adds no negation for later members.
    Unconditioned,
    /// `v-else`: narrows by the negation of every preceding condition.
    Else,
}

/// A chain member's narrowing terms: a prefix of its chain's shared resolved
/// conditions (the predecessors, negated) plus its own positive condition.
#[derive(Clone, Debug)]
pub struct ChainMember {
    /// Wrapped conditions of the whole chain, shared by every member.
    terms: Rc<[Rc<str>]>,
    /// `terms[..negations]` precede this member.
    negations: usize,
    /// Index of this member's own condition in `terms`.
    positive: Option<usize>,
}

impl ChainMember {
    fn positive(&self) -> Option<&str> {
        self.positive.map(|i| &*self.terms[i])
    }

    fn negations(&self) -> &[Rc<str>] {
        &self.terms[..self.negations]
    }
}

/// Resolve a chain's member conditions once into shared narrowing terms.
///
/// Returns one entry per branch, in order: `None` for an unconditioned
/// `v-if`/`v-else-if` (it builds no narrowing scope), `Some` otherwise. Each
/// member shares the chain's term storage, so building every member is linear
/// in the chain length.
pub fn resolve_chain(branches: impl IntoIterator<Item = ChainBranch>) -> Vec<Option<ChainMember>> {
    let branches: Vec<ChainBranch> = branches.into_iter().collect();
    let mut terms: Vec<Rc<str>> = Vec::with_capacity(branches.len());
    // (negations, positive) per branch; `None` when the branch builds no scope.
    let mut shapes: Vec<Option<(usize, Option<usize>)>> = Vec::with_capacity(branches.len());
    for branch in branches {
        match branch {
            ChainBranch::Condition(condition) => {
                shapes.push(Some((terms.len(), Some(terms.len()))));
                terms.push(Rc::from(wrap_if_needed(&condition).as_ref()));
            }
            ChainBranch::Unconditioned => shapes.push(None),
            ChainBranch::Else => shapes.push(Some((terms.len(), None))),
        }
    }
    let terms: Rc<[Rc<str>]> = terms.into();
    shapes
        .into_iter()
        .map(|shape| {
            shape.map(|(negations, positive)| ChainMember {
                terms: Rc::clone(&terms),
                negations,
                positive,
            })
        })
        .collect()
}

/// The narrowing in effect at a template position: a persistent list of the
/// chain members enclosing it, innermost first.
///
/// Cloning and [`GuardScope::enter`] are O(1). The guard text of a scope is
/// rendered at most once and shared by every element and callback inside it.
#[derive(Clone, Debug, Default)]
pub struct GuardScope(Option<Rc<GuardFrame>>);

#[derive(Debug)]
struct GuardFrame {
    parent: GuardScope,
    member: ChainMember,
    guard: OnceCell<Option<Rc<str>>>,
}

impl GuardScope {
    /// The scope inside `member`, nested in `self`. A member that contributes
    /// no term (a `v-else` without predecessors) narrows nothing and returns
    /// `self` unchanged.
    pub fn enter(&self, member: ChainMember) -> GuardScope {
        if member.positive.is_none() && member.negations == 0 {
            return self.clone();
        }
        GuardScope(Some(Rc::new(GuardFrame {
            parent: self.clone(),
            member,
            guard: OnceCell::new(),
        })))
    }

    /// The combined condition text a closure inside this scope repeats, or
    /// `None` outside any condition.
    ///
    /// Example for `v-else-if="C"` after `v-if="A"`, nested inside `v-if="P"`:
    /// `!((A)) && (P) && (C)` — negations first, then positives, each group
    /// ordered outermost scope first and in chain order.
    pub fn guard_text(&self) -> Option<Rc<str>> {
        let frame = self.0.as_deref()?;
        frame.guard.get_or_init(|| render_guard(frame)).clone()
    }
}

/// Render the bounded guard of `innermost`.
///
/// Every frame carries at least one term, so the [`MAX_GUARD_TERMS`] innermost
/// frames hold enough terms to fill the guard; farther frames are never visited.
fn render_guard(innermost: &GuardFrame) -> Option<Rc<str>> {
    // Candidates innermost-first: positives by frame, negations by frame with
    // the nearest predecessor first.
    let mut positives: Vec<&str> = Vec::new();
    let mut negations: Vec<&str> = Vec::new();
    let mut visits = 0usize;
    let mut frame = Some(innermost);
    let mut frames = 0usize;
    while let Some(current) = frame {
        if frames == MAX_GUARD_TERMS {
            break;
        }
        frames += 1;
        if let Some(positive) = current.member.positive() {
            positives.push(positive);
            visits += 1;
        }
        for negation in current
            .member
            .negations()
            .iter()
            .rev()
            .take(MAX_GUARD_TERMS)
        {
            negations.push(negation);
            visits += 1;
        }
        frame = current.parent.0.as_deref();
    }
    record_guard_work(visits);

    positives.truncate(MAX_GUARD_TERMS);
    negations.truncate(MAX_GUARD_TERMS - positives.len());
    if positives.is_empty() && negations.is_empty() {
        return None;
    }

    let mut text = String::new();
    for negation in negations.iter().rev() {
        if !text.is_empty() {
            text.push_str(" && ");
        }
        text.push_str("!(");
        text.push_str(negation);
        text.push(')');
    }
    for positive in positives.iter().rev() {
        if !text.is_empty() {
            text.push_str(" && ");
        }
        text.push_str(positive);
    }
    Some(text.into())
}

#[cfg(any(test, feature = "semantic-observe"))]
thread_local! {
    /// Condition terms visited while rendering narrowing guards.
    static GUARD_WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(any(test, feature = "semantic-observe"))]
#[inline]
fn record_guard_work(units: usize) {
    GUARD_WORK.with(|work| work.set(work.get() + units));
}

#[cfg(not(any(test, feature = "semantic-observe")))]
#[inline(always)]
fn record_guard_work(_units: usize) {}

/// Read and reset the per-thread guard-construction work counter.
#[cfg(any(test, feature = "semantic-observe"))]
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "measurement builds read it through their own harnesses"
    )
)]
pub fn take_guard_work() -> usize {
    GUARD_WORK.with(|work| work.replace(0))
}

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
