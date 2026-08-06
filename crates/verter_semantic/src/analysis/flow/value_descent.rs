//! [`value_descent`] — THE classifier of one expression's
//! VALUE-STRUCTURAL sub-expressions, and the single authority BOTH halves
//! of the flow substrate dispatch on.
//!
//! The substrate has two halves that must agree about which expression
//! forms have sub-expressions whose values contribute to the enclosing
//! value:
//!
//! - the PLANNER half — the skeleton opens a tracked expression site per
//!   such sub-expression ([`SkeletonBuilder::open_site`](super)), the
//!   graph emits the value-provider edges between them, and the demand
//!   planner reaches them as graph reachability;
//! - the CONTENT half — `flow_slice_content`'s `lower_expr` descends into
//!   the same sub-expressions and lowers each one, gating an object
//!   member value on whether the plan VALUE-selected it.
//!
//! Applied per form, that is a rule applied to two of three: a
//! conditional expression's branches were a descent the CONTENT half
//! performed and the PLANNER half did not, so an object literal in a
//! branch lowered with every member value on the typed
//! `SliceExpr::Elided` carrier — the planner/content mismatch that
//! carrier exists to make visible — and the whole return failed closed.
//! One half was extended; the other was not.
//!
//! So the classification is ONE function with ONE exhaustive match over
//! `Expression`. Neither half carries a wildcard over `Expression` any
//! more: a new variant does not compile until it is dispositioned HERE,
//! and both halves inherit that disposition in the same change.
//!
//! The two directions are NOT symmetric in consequence, which is why
//! [`ValueDescent::TypeCarrier`] is a NAMED variant rather than two
//! independent unwrap helpers. A site the planner opens and the content
//! half leaf-lowers is over-selection: harmless. A form the content half
//! descends into and the planner does not is under-selection: an
//! `Elided` value at a position the evaluator must read, i.e. a
//! fail-closed whole result. The enum states which forms take which
//! disposition on each side, once.

use oxc_ast::ast::{ConditionalExpression, Expression, ObjectExpression};

/// The value-structural disposition of one expression.
///
/// "Value-structural" means: does this form have SUB-EXPRESSIONS whose
/// values contribute to this expression's value, such that a demand for
/// this value must reach them? A call's arguments do not (the call's
/// value is its callee's return); a nested function's body does not (it
/// is its own frame, planned and lowered under its own skeleton).
#[derive(Debug)]
pub enum ValueDescent<'a, 'ast> {
    /// Value-transparent in EVERY sense: parentheses. Both halves
    /// re-enter on the inner expression, so neither can see a node the
    /// other does not.
    Transparent(&'a Expression<'ast>),
    /// A TYPE carrier (`x as T`, `x satisfies T`, `x!`, `<T>x`, `f<T>`).
    ///
    /// Value-transparent for STRUCTURE — the planner descends, so
    /// `({ a: 1 } as const).a` still reaches the member and a projection
    /// demand into a carrier-wrapped literal is not lost — but NOT for
    /// CONTENT: the carrier decides the published type (`x as const`
    /// pins what a bare literal would widen), so the content half lowers
    /// the whole carrier as one leaf and never reads the members the
    /// planner selected. That is over-selection, which is safe by the
    /// asymmetry above.
    TypeCarrier(&'a Expression<'ast>),
    /// An object literal: its member VALUES are the descent, each one a
    /// value provider of exactly the key it provisions.
    Object(&'a ObjectExpression<'ast>),
    /// A branch JOIN: EVERY arm provides the whole value, so a demand
    /// for the join's value (or for a projection under it) is a demand
    /// for each arm's. The test's value is never consumed by either
    /// half — only its evaluation effects are.
    Branches(&'a ConditionalExpression<'ast>),
    /// No value-structural descent: the form answers as a whole.
    Leaf,
}

/// The value-structural disposition of `expression` — ONE step. A caller
/// that must reach a non-transparent form re-enters on the inner
/// expression of [`ValueDescent::Transparent`] /
/// [`ValueDescent::TypeCarrier`], applying its OWN rule for the carrier
/// case.
///
/// The match is exhaustive with no wildcard: that is the whole point.
#[must_use]
pub fn value_descent<'a, 'ast>(expression: &'a Expression<'ast>) -> ValueDescent<'a, 'ast> {
    match expression {
        Expression::ParenthesizedExpression(paren) => ValueDescent::Transparent(&paren.expression),
        Expression::TSAsExpression(inner) => ValueDescent::TypeCarrier(&inner.expression),
        Expression::TSSatisfiesExpression(inner) => ValueDescent::TypeCarrier(&inner.expression),
        Expression::TSNonNullExpression(inner) => ValueDescent::TypeCarrier(&inner.expression),
        Expression::TSTypeAssertion(inner) => ValueDescent::TypeCarrier(&inner.expression),
        Expression::TSInstantiationExpression(inner) => {
            ValueDescent::TypeCarrier(&inner.expression)
        }
        Expression::ObjectExpression(object) => ValueDescent::Object(object),
        Expression::ConditionalExpression(conditional) => ValueDescent::Branches(conditional),
        // A call's ARGUMENTS are not value-providers of the call's value,
        // and a nested function / class body is its own frame — planned
        // and lowered under its own skeleton, never through this one.
        Expression::Identifier(_)
        | Expression::CallExpression(_)
        | Expression::FunctionExpression(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::ClassExpression(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::TemplateLiteral(_)
        | Expression::MetaProperty(_)
        | Expression::Super(_)
        | Expression::ArrayExpression(_)
        | Expression::AssignmentExpression(_)
        | Expression::AwaitExpression(_)
        | Expression::BinaryExpression(_)
        | Expression::ChainExpression(_)
        | Expression::ImportExpression(_)
        | Expression::LogicalExpression(_)
        | Expression::NewExpression(_)
        | Expression::SequenceExpression(_)
        | Expression::TaggedTemplateExpression(_)
        | Expression::ThisExpression(_)
        | Expression::UnaryExpression(_)
        | Expression::UpdateExpression(_)
        | Expression::YieldExpression(_)
        | Expression::PrivateInExpression(_)
        | Expression::JSXElement(_)
        | Expression::JSXFragment(_)
        | Expression::V8IntrinsicExpression(_)
        | Expression::ComputedMemberExpression(_)
        | Expression::StaticMemberExpression(_)
        | Expression::PrivateFieldExpression(_) => ValueDescent::Leaf,
    }
}
