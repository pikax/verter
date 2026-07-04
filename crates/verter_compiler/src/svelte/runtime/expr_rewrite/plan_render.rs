//! Leaf classification + rendering helpers the two-pass rewriter
//! ([`super::plan`]) consumes — the `$state.snapshot` callee matcher, the
//! TS-only-syntax detector, the assignment-operator tables, and the `$$props`
//! member-access / JS-string-literal rendering. Split out of `plan.rs` so the
//! two-pass core stays under the file-size guard; these are pure, self-contained
//! functions with no collector state.

use oxc_ast::ast::{AssignmentOperator, CallExpression, Expression};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;

use super::super::expr::peel_parens;

/// Whether `expr` contains a TS-only wrapper expression (`x!` / `x as T` /
/// `x satisfies T` / `<T>x` / `x<T>`) at ANY depth — the recursive
/// computed-KEY half of the TS-wrapped prop-chain write gate
/// ([`super::plan::BindingOccurrenceCollector::member_lvalue_is_ts_wrapped_prop_chain`]).
/// Structural over the parsed OXC nodes through the exhaustive `walk`
/// traversal (every sub-expression — call arguments, nested members, arrow
/// bodies — is reached; `String(k as any)` is found), never a text scan. The
/// wrapper kinds are exactly the five the chain walk itself peels.
pub(super) fn expression_contains_ts_only_syntax(expr: &Expression<'_>) -> bool {
    struct TsOnlySyntaxDetector {
        found: bool,
    }
    impl<'a> Visit<'a> for TsOnlySyntaxDetector {
        fn visit_expression(&mut self, it: &Expression<'a>) {
            if self.found {
                return;
            }
            if matches!(
                it,
                Expression::TSNonNullExpression(_)
                    | Expression::TSAsExpression(_)
                    | Expression::TSSatisfiesExpression(_)
                    | Expression::TSTypeAssertion(_)
                    | Expression::TSInstantiationExpression(_)
            ) {
                self.found = true;
                return;
            }
            walk::walk_expression(self, it);
        }
    }
    let mut detector = TsOnlySyntaxDetector { found: false };
    detector.visit_expression(expr);
    detector.found
}

/// Whether a call expression's callee is the `$state.snapshot` rune member — a
/// CallExpression whose (paren-peeled) callee is the static `.snapshot` member on
/// the (paren-peeled) bare `$state` identifier. BOTH callee paren positions are
/// transparent (`($state).snapshot(x)`, `($state.snapshot)(x)`, doubled or
/// combined) — official's ESTree AST has no paren nodes, and the rune-scan
/// exemption peels the SAME way, so the scan model and this matcher agree.
/// Returns the span of the WHOLE callee — paren-INCLUSIVE — so replacing it with
/// the `$.snapshot` helper leaves no paren residue (a member-span-only overwrite
/// would emit `($.snapshot)(x)`); on the paren-less spelling the callee span IS
/// the member span. Shadowing (`$state` a local) is the CALLER's check (the
/// collector consults its own local shadow frames). Driven from the typed OXC
/// AST only.
pub(super) fn state_snapshot_callee_span(call: &CallExpression<'_>) -> Option<oxc_span::Span> {
    let Expression::StaticMemberExpression(member) = peel_parens(&call.callee) else {
        return None;
    };
    if member.property.name.as_str() != "snapshot" {
        return None;
    }
    if matches!(peel_parens(&member.object), Expression::Identifier(id) if id.name.as_str() == "$state")
    {
        // Only the WELL-FORMED single-non-spread-arg form is the supported
        // `$.snapshot(<expr>)` rewrite. Official rejects a zero-arg / >=2-arg call
        // (`rune_invalid_arguments_length`) and a spread arg (`rune_invalid_spread`) —
        // oracle-verified against `svelte@5.56.3` at every paren position. The
        // rune-scan gate fails those closed upstream so this rewriter never runs on
        // them; this arity/spread guard is defense-in-depth so the rewriter can NEVER
        // emit a raw `$.snapshot()` / `$.snapshot(a, b)` / `$.snapshot(...o)` even if
        // reached.
        if call.arguments.len() != 1 || call.arguments[0].as_expression().is_none() {
            return None;
        }
        Some(call.callee.span())
    } else {
        None
    }
}

/// The base operator of a compound assignment (`+=` → `+`, `*=` → `*`, …).
pub(super) fn compound_base_operator(op: AssignmentOperator) -> &'static str {
    match op {
        AssignmentOperator::Addition => "+",
        AssignmentOperator::Subtraction => "-",
        AssignmentOperator::Multiplication => "*",
        AssignmentOperator::Division => "/",
        AssignmentOperator::Remainder => "%",
        AssignmentOperator::Exponential => "**",
        AssignmentOperator::ShiftLeft => "<<",
        AssignmentOperator::ShiftRight => ">>",
        AssignmentOperator::ShiftRightZeroFill => ">>>",
        AssignmentOperator::BitwiseOR => "|",
        AssignmentOperator::BitwiseXOR => "^",
        AssignmentOperator::BitwiseAnd => "&",
        AssignmentOperator::LogicalOr => "||",
        AssignmentOperator::LogicalAnd => "&&",
        AssignmentOperator::LogicalNullish => "??",
        AssignmentOperator::Assign => "=",
    }
}

/// The span of a bare `<ident>.KEY` member that is the WHOLE right-hand side of an
/// assignment, or `None` when the RHS is not that shape. Paren-transparent on both
/// the RHS and its object (official ESTree has no paren node), and IDENT-object-gated
/// so a chained `rest.y.z` RHS — whose object is itself a member — is EXCLUDED (its
/// root member still de-localizes). The caller records the span into
/// `member_assign_rhs_verbatim_spans` so the rest / whole-object member disposition
/// keeps it verbatim (the oracle's coarse Assignment-child guard).
pub(super) fn bare_member_rhs_verbatim_span(rhs: &Expression<'_>) -> Option<oxc_span::Span> {
    if let Expression::StaticMemberExpression(m) = peel_parens(rhs) {
        if matches!(peel_parens(&m.object), Expression::Identifier(_)) {
            return Some(m.span);
        }
    }
    None
}

/// Whether an assignment operator is NON-COERCIVE — the official
/// `is_non_coercive_operator` set (`=`, `||=`, `&&=`, `??=`). Only these gate the
/// proxy `, true` on a reassignment; a coercive compound (`+=`, `*=`, `<<=`, …)
/// always evaluates to a primitive and never proxies.
pub(super) fn is_non_coercive_operator(op: AssignmentOperator) -> bool {
    matches!(
        op,
        AssignmentOperator::Assign
            | AssignmentOperator::LogicalOr
            | AssignmentOperator::LogicalAnd
            | AssignmentOperator::LogicalNullish
    )
}

/// Build the `$$props` member access for a no-default prop's SOURCE key. An
/// identifier-safe key reads via DOTTED access (`$$props.foo`); a key that is not
/// a valid JS identifier (`foo-bar`, a numeric key, a key with quotes) reads via
/// BRACKET access with a properly-escaped JS string literal (`$$props['foo-bar']`).
pub(super) fn props_member_access(source_key: &str) -> String {
    if is_js_identifier(source_key) {
        format!("$$props.{source_key}")
    } else {
        format!("$$props[{}]", js_string_literal(source_key))
    }
}

/// Whether `name` is a valid plain JS identifier (so a `$$props.<name>` dotted
/// access is valid). A `$state`-style `$`/`_`-prefixed name qualifies; a `foo-bar`
/// / numeric-leading / empty name does not (it requires bracket access).
fn is_js_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Render `value` as a single-quoted JS string literal, escaping backslash, the
/// single-quote delimiter, and the line terminators — so an arbitrary destructure
/// key (`foo-bar`, `it's`, a key with a newline) interpolates into emitted JS
/// SAFELY (no broken `'<key>'`).
pub(super) fn js_string_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(c),
        }
    }
    out.push('\'');
    out
}
