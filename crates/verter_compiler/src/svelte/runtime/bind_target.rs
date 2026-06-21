//! Structural classification of a two-way `bind:` directive's bound target
//! expression from the parsed OXC node (NOT a text scan).

use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, Statement};
use oxc_parser::Parser;
use oxc_span::SourceType;

/// The STRUCTURAL classification of a two-way `bind:` directive's bound target
/// expression, derived from the parsed OXC node — NOT a text scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindTargetKind {
    /// A bare identifier target (`bind:value={name}`) — a REASSIGNMENT of the
    /// bound binding.
    Identifier,
    /// A member target (`bind:value={o.x}` / `bind:value={a[i]}`) — a DEEP MUTATION
    /// of the bound binding's value.
    Member,
}

/// Classify a `bind:` target expression's lvalue shape from the parsed AST.
///
/// Returns `Identifier` for a bare-identifier target (a reassignment), `Member`
/// for a member/computed-member target (a deep mutation), and `None` for a
/// NON-LVALUE (a literal, a call, a binary expression — not a valid bind target;
/// the caller refuses / ignores it rather than mis-classifying). This replaces the
/// `source.contains('.')` text heuristic with a structural decision, so a member
/// access inside a NON-member target (e.g. a default-bearing or parenthesised
/// expression) is classified correctly.
#[must_use]
pub fn classify_bind_target(alloc: &Allocator, source: &str) -> Option<BindTargetKind> {
    let wrapped = format!("({source})");
    let parsed = Parser::new(alloc, alloc.alloc_str(&wrapped), SourceType::tsx()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return None;
    }
    let Some(Statement::ExpressionStatement(stmt)) = parsed.program.body.first() else {
        return None;
    };
    let mut expr = &stmt.expression;
    // Unwrap parentheses + a non-null assertion (`name!`) to reach the lvalue core.
    loop {
        match expr {
            Expression::ParenthesizedExpression(p) => expr = &p.expression,
            Expression::TSNonNullExpression(e) => expr = &e.expression,
            _ => break,
        }
    }
    match expr {
        Expression::Identifier(_) => Some(BindTargetKind::Identifier),
        Expression::StaticMemberExpression(_)
        | Expression::ComputedMemberExpression(_)
        | Expression::PrivateFieldExpression(_) => Some(BindTargetKind::Member),
        // Any other expression is not a valid bind lvalue.
        _ => None,
    }
}

/// Whether a `bind:` target expression is TS-WRAPPED — its lvalue spine carries a
/// TypeScript type operator (`name!` / `name as T` / `name satisfies T` /
/// `<T>name`), possibly under parentheses (`(name!)`). A TS-wrapped target is a
/// distinct official surface: svelte normalizes (strips) the wrapper and emits the
/// clean lvalue setter, but Verter formats the setter from the RAW source, so it
/// would emit an invalid `name! = $$value` / `$.set(name!, $$value)`. The
/// canonical-lvalue-from-TS lowering is a deferral, so a TS-wrapped target fails
/// closed. Only a CLEAN `Identifier` / member lvalue is a supported bind target.
///
/// The decision is STRUCTURAL over the parsed AST (never a `source.contains('!')`
/// text scan): a `!` inside a computed-member index (`a[b!]`) or a string is not a
/// wrapper of the target lvalue itself, and is not flagged here.
// TODO(follow-up): carry a CANONICAL (TS-stripped) bind lvalue from classification
// into the setter emission so a TS-wrapped target lowers like official (strip the
// wrapper, emit the clean `$.set(name, $$value)` / member setter) instead of failing
// closed. Until then a TS-wrapped target is the deferral-ledger refusal.
#[must_use]
pub fn bind_target_is_ts_wrapped(alloc: &Allocator, source: &str) -> bool {
    let wrapped = format!("({source})");
    let parsed = Parser::new(alloc, alloc.alloc_str(&wrapped), SourceType::tsx()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return false;
    }
    let Some(Statement::ExpressionStatement(stmt)) = parsed.program.body.first() else {
        return false;
    };
    let mut expr = &stmt.expression;
    // Walk down through the synthetic parens (and any author parens) to the target
    // expression; a TS operator at ANY level of that spine is a wrapped target.
    loop {
        match expr {
            Expression::ParenthesizedExpression(p) => expr = &p.expression,
            // A TS type operator wrapping the lvalue spine: non-null (`name!`), `as`
            // / `satisfies` (`name as T`), or the prefix type-assertion (`<T>name`).
            Expression::TSNonNullExpression(_)
            | Expression::TSAsExpression(_)
            | Expression::TSSatisfiesExpression(_)
            | Expression::TSTypeAssertion(_) => return true,
            _ => return false,
        }
    }
}

/// The ROOT identifier name of a `bind:` target expression — the leftmost
/// identifier reached by walking down a member / computed-member chain (the
/// object of `obj.x`, `a[i]`, `obj.x.y`, …). Returns the bare identifier's name
/// for an identifier target, the chain's root name for a member target, and
/// `None` when the chain bottoms out at a non-identifier (a call `f().x`, a
/// literal, a `this`-member) — those are not a root the binding table can
/// resolve.
///
/// This lets the bind classifier resolve the SCOPE-AWARE KIND of a member
/// target's root (a `$state` vs a prop vs an import) instead of accepting any
/// member shape blindly: a member rooted at a non-`$state` binding is a distinct
/// (divergent) official surface and must fail closed.
#[must_use]
pub fn bind_target_root_ident(alloc: &Allocator, source: &str) -> Option<String> {
    let wrapped = format!("({source})");
    let parsed = Parser::new(alloc, alloc.alloc_str(&wrapped), SourceType::tsx()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return None;
    }
    let Some(Statement::ExpressionStatement(stmt)) = parsed.program.body.first() else {
        return None;
    };
    let mut node = &stmt.expression;
    loop {
        match node {
            Expression::ParenthesizedExpression(p) => node = &p.expression,
            Expression::TSNonNullExpression(e) => node = &e.expression,
            Expression::StaticMemberExpression(m) => node = &m.object,
            Expression::ComputedMemberExpression(m) => node = &m.object,
            Expression::PrivateFieldExpression(m) => node = &m.object,
            Expression::Identifier(id) => return Some(id.name.to_string()),
            // The chain bottoms out at a non-identifier (a call, a literal, a
            // `this`-member, …) — there is no resolvable root binding.
            _ => return None,
        }
    }
}
