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
//!
//! The classifier also answers a SECOND question on the same pass, for
//! the same reason: is this form a CALL POSITION the substrate has no
//! structural arm for ([`ValueDescent::UnmodeledCall`])? That verdict
//! used to be taken downstream, off the leaf lowering's ANSWER — the
//! content half fails closed only when the shared shallow pass minted an
//! unreduced `ReturnType<callee>` carrier it could recognise. For
//! `new f()`, `` tag`…` ``, `f?.()`, `await f()` and `(k, f())` that pass
//! answers a bare `any` with no carrier in it, so nothing fired and a
//! fabricated `any` published warm and clean under a promise that a call
//! with no structural arm fails closed. Whether an expression is a call
//! position is a property of the FORM, so it is decided here, where the
//! form is.

use oxc_ast::ast::{ChainElement, ConditionalExpression, Expression, ObjectExpression};

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
    /// A CALL POSITION with no structural arm: this form's VALUE is
    /// produced by a call the substrate does not model —
    /// `new f()`, `` tag`…` ``, an optional call chain (`f?.()`), an
    /// awaited call (`await f()`), or a sequence whose last operand is
    /// one of those.
    ///
    /// This variant exists because the fail-closed promise
    /// (`FlowReturnFailure::UnmodeledCallPosition`) has to key on the
    /// EXPRESSION FORM, not on whatever the shared shallow pass happened
    /// to produce for it. The content half's call-carrier gate
    /// (`embeds_call_return_carrier`) only fires when that pass minted an
    /// unreduced `ReturnType<callee>` carrier; for every form here the
    /// pass answers a bare `any` instead, which carries no carrier, so
    /// the gate never fired and a fabricated `any` published warm and
    /// clean. The classifier decides it up front instead.
    ///
    /// The CONTENT half fails closed. The PLANNER half takes the same
    /// disposition as [`Leaf`](Self::Leaf): a call's arguments are not
    /// value providers of the call's value, so there is nothing to
    /// descend into either way — but the sub-expressions are still
    /// VISITED for their evaluation effects. The two halves therefore
    /// stay in agreement about SELECTION while differing about the
    /// VERDICT, which is the one asymmetry this variant introduces.
    ///
    /// The membership question is answered by exactly one function,
    /// [`value_is_unmodeled_call`] — not re-spelled per form in the
    /// match — so the classifier and the content half's residual
    /// type-carrier check cannot disagree about a single form.
    ///
    /// A bare [`Expression::CallExpression`] lands here too, and is
    /// harmless in both halves: the content half owns six structural call
    /// arms (direct, direct-self, on-binding, symbolic, IIFE,
    /// local-function shadow) and takes them BEFORE it consults the
    /// classifier, so a modeled call never reaches this verdict; and for
    /// the planner `UnmodeledCall` and `Leaf` are the same disposition.
    /// If those arms were ever removed, failing closed is the safe
    /// default to fall back to.
    UnmodeledCall,
    /// No value-structural descent: the form answers as a whole, through
    /// the shared shallow-pass leaf lowering.
    ///
    /// A `Leaf` verdict says the form has no value-providing child the
    /// demand plan must reach. It does NOT promise the leaf lowering
    /// MODELS the form: several leaves (`JSXElement`, `MetaProperty`,
    /// `Super`, `await x` over a non-call) answer that pass's fallback
    /// `any`. That fallback is the shallow pass's own coverage question,
    /// decided per expression at lowering time and not statically per
    /// form — the classifier cannot decide it and does not pretend to.
    /// What it DOES decide is the call-position question above, because
    /// that one IS a property of the form.
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
        // THE call-position disposition, delegated to the ONE predicate
        // that answers the question — never re-spelled per form here, so
        // this half and the content half's residual carrier check cannot
        // disagree about a single form, and a change to the predicate is
        // a change to BOTH.
        //
        // The four value-transparent arms above are matched FIRST, so a
        // parenthesised or type-carried call reaches its own disposition
        // (the caller re-enters on the inner expression, or the carrier
        // pins the type) rather than this one.
        expression if value_is_unmodeled_call(expression) => ValueDescent::UnmodeledCall,
        // A call's ARGUMENTS are not value-providers of the call's value,
        // and a nested function / class body is its own frame — planned
        // and lowered under its own skeleton, never through this one.
        //
        // `CallExpression`, `NewExpression`, `TaggedTemplateExpression`
        // and the call-valued `ChainExpression` / `AwaitExpression` /
        // `SequenceExpression` shapes are listed here for exhaustiveness
        // but are consumed by the guarded arm above; what reaches `Leaf`
        // through these names is the NON-call shape of each (`a?.b`,
        // `await promiseVar`, `(a, b)`). A bare `CallExpression` never
        // reaches the CONTENT half's classifier dispatch at all — that
        // half owns six structural call arms and takes them first — and
        // for the PLANNER half `UnmodeledCall` and `Leaf` are the same
        // disposition, so the guard is safe for it either way.
        Expression::Identifier(_)
        | Expression::CallExpression(_)
        | Expression::NewExpression(_)
        | Expression::TaggedTemplateExpression(_)
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
        | Expression::SequenceExpression(_)
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

/// Whether `expression`'s VALUE is produced by a CALL the flow substrate
/// has no structural arm for — the predicate
/// [`ValueDescent::UnmodeledCall`] is decided by, and the same predicate
/// the content half consults before it may answer a leaf `any`.
///
/// A bare `CallExpression` counts as unmodeled HERE even though the
/// content half owns structural arms for one: those arms only ever see a
/// call in an expression position the half itself reached, and every
/// position this predicate is asked about is one they did not.
///
/// Value-transparent wrappers (parentheses, the five TYPE carriers) are
/// followed through: `f()!` is still a call position. That is safe for
/// the type carriers precisely because their leaf answer normally PINS
/// the type (`f() as T` is `T`, never `any`), so the predicate only
/// decides the case where the carrier answered nothing.
#[must_use]
pub fn value_is_unmodeled_call(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::CallExpression(_)
        | Expression::NewExpression(_)
        | Expression::TaggedTemplateExpression(_) => true,
        Expression::ParenthesizedExpression(paren) => value_is_unmodeled_call(&paren.expression),
        Expression::TSAsExpression(inner) => value_is_unmodeled_call(&inner.expression),
        Expression::TSSatisfiesExpression(inner) => value_is_unmodeled_call(&inner.expression),
        Expression::TSNonNullExpression(inner) => value_is_unmodeled_call(&inner.expression),
        Expression::TSTypeAssertion(inner) => value_is_unmodeled_call(&inner.expression),
        Expression::TSInstantiationExpression(inner) => value_is_unmodeled_call(&inner.expression),
        Expression::ChainExpression(chain) => chain_is_call_valued(&chain.expression),
        Expression::AwaitExpression(inner) => value_is_unmodeled_call(&inner.argument),
        Expression::SequenceExpression(sequence) => sequence
            .expressions
            .last()
            .is_some_and(value_is_unmodeled_call),
        // A conditional's arms are each their own lowered position (the
        // content half unions them), so the join is not itself a call
        // position; an object / array / literal / identifier / nested
        // function value never is.
        Expression::ConditionalExpression(_)
        | Expression::Identifier(_)
        | Expression::ObjectExpression(_)
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
        | Expression::BinaryExpression(_)
        | Expression::ImportExpression(_)
        | Expression::LogicalExpression(_)
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
        | Expression::PrivateFieldExpression(_) => false,
    }
}

/// Whether an optional-chain element takes its value from a CALL
/// (`f?.()`, `a?.b()`, `a.b?.()`) as opposed to a pure member read
/// (`a?.b`, `a?.[k]`).
fn chain_is_call_valued(element: &ChainElement<'_>) -> bool {
    match element {
        ChainElement::CallExpression(_) => true,
        ChainElement::TSNonNullExpression(inner) => value_is_unmodeled_call(&inner.expression),
        ChainElement::ComputedMemberExpression(_)
        | ChainElement::StaticMemberExpression(_)
        | ChainElement::PrivateFieldExpression(_) => false,
    }
}
