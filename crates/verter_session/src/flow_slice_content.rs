//! Demand-sliced flow content — the OWNED, arena-free content lowering
//! of exactly one planned flow slice.
//!
//! The [`FunctionProgramIndex`](verter_semantic::analysis::function_program::FunctionProgramIndex)
//! is the eager STRUCTURAL inventory (identities + locators, no lowered
//! types), and the flow-slice substrate (`verter_semantic::analysis::flow`)
//! plans the demanded slice as graph reachability and lowers it into the
//! content-free `FlowSliceIR`. THIS module is the content half: on the
//! cold path of one flow evaluation it reborrows the retained parse
//! snapshot ONCE (through the memo's lease-only run, exactly like every
//! other body product) and lowers ONLY the slice-selected expression
//! content into owned typed IR with a block/if control-flow tree.
//! Content OUTSIDE the selection never lowers: an unselected binding
//! initializer is omitted, an unselected object member value and any
//! unselected root expression ride the typed [`SliceExpr::Elided`]
//! carrier, and `if`-test / expression-statement positions carry no
//! content at all (the evaluator never consumes their values).
//!
//! Control semantics: sequential region evaluation (a terminal return or
//! throw ends the region; statements after it are unreachable and
//! dropped), an `if` whose arms both terminate cannot fall through, blocks
//! nest, return-free loop/labeled constructs are fall-through transparent,
//! and return-bearing loop/labeled constructs, `switch`, `try`, `with`,
//! jumps, and module-level statements are UNSUPPORTED — typed,
//! fail-closed: the region is produced up to the first
//! [`SliceStatement::Unsupported`] marker and the marker propagates to
//! the root so the evaluator degrades the whole result.
//!
//! Expression content lowers through the ONE shared shallow-pass
//! per-expression lowering (`infer_declaration_expression_type`); the
//! flow-only differences are explicit IR carriers: parameter references
//! become [`SliceExpr::Param`], simple local bindings become
//! [`SliceExpr::Local`] (reaching definitions resolved by the
//! evaluator), and EVERY call form rides the single
//! [`SliceExpr::Call`] carrier over the closed [`SliceCall`] vocabulary:
//! a bare-identifier call resolves through ONE lexical binding authority
//! — a hoisted nested function declaration of the same name is
//! [`SliceCall::LocalFunctionShadow`] (fail-closed), a parameter or
//! in-scope local is [`SliceCall::OnBinding`], a call to the function
//! itself is [`SliceCall::DirectSelf`], an index-exact direct call is
//! [`SliceCall::Direct`] — and any other call rides the symbolic
//! `ReturnType<typeof …>` carrier (or `any` for an unrepresentable
//! callee) as [`SliceCall::Symbolic`].

use std::sync::Arc;

use oxc_ast::ast::{
    BindingPattern, Expression, FormalParameters, ObjectPropertyKind, Program, PropertyKey,
    PropertyKind, Statement, VariableDeclarationKind,
};
use oxc_span::GetSpan;
use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::flow::flow_ir::{FlowExprRole, FlowSliceIR};
use verter_semantic::analysis::flow::{
    build_function_body_skeleton, FrameSpan, FunctionBodySkeleton, FunctionBodySource, NameMeaning,
    SkeletonBindingKind,
};
use verter_semantic::analysis::function_program::{
    inventory_statement_list, resolve_function_node, FunctionControlRegion, FunctionNode,
    FunctionProgramEntry,
};
use verter_semantic::analysis::type_eval_build::{
    embeds_call_return_carrier, infer_declaration_expression_type, TopLevelLiteralPolicy,
};
use verter_type_expr::{PrimitiveName, TypeExpr};
use verter_type_expr_oxc::lower_ts_type;

/// The demand selection one content lowering serves: the value-selected
/// expression spans and the value-selected slot declaration spans of ONE
/// lowered flow slice. Derived from the content-free `FlowSliceIR` — the
/// plan is the sole authority for what lowers; this carrier only
/// transports the selection into the lease-only run.
#[derive(Debug, Clone)]
pub(crate) struct FlowSliceSelection {
    /// Spans of the slice's VALUE-selected expression records, in the
    /// frame's own coordinates — the only coordinates the plan speaks.
    value_spans: FxHashSet<FrameSpan>,
    /// Binding-identifier spans of the slice's VALUE-selected slots —
    /// DECLARATION-precise identity, so a shadowed same-named sibling
    /// declarator the plan kept out never lowers (name identity would
    /// re-conflate what the plan's lexical resolution separated).
    value_slot_spans: FxHashSet<FrameSpan>,
}

impl FlowSliceSelection {
    /// The selection of one lowered slice.
    pub(crate) fn from_slice_ir(ir: &FlowSliceIR) -> Self {
        Self {
            value_spans: ir
                .exprs
                .iter()
                .filter(|expr| expr.role == FlowExprRole::Value)
                .map(|expr| expr.span)
                .collect(),
            value_slot_spans: ir
                .slots
                .iter()
                .filter(|slot| slot.value_selected)
                .map(|slot| slot.span)
                .collect(),
        }
    }

    fn value_span(&self, span: FrameSpan) -> bool {
        self.value_spans.contains(&span)
    }

    fn value_slot_span(&self, span: FrameSpan) -> bool {
        self.value_slot_spans.contains(&span)
    }
}

/// The OWNED content of one demanded slice: lowered parameters, the root
/// body region with slice-gated expression content, and the region's
/// reachability result.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceContent {
    /// Formal parameters in source order (rest parameter last).
    pub params: Arc<[SliceParam]>,
    /// The function's OWN type parameters (the root signature's binders —
    /// the evaluator lowers parameters and body leaves under them, never
    /// under an outer same-name resolution).
    pub type_parameters: Arc<[SliceTypeParam]>,
    /// The ENCLOSING declaration's type parameters — today exactly the
    /// class clause a member body sits inside (`class C<T> { m(x: T) }`).
    /// They bind throughout the member's signature and body but appear in
    /// no clause of the member itself, so the evaluator seeds the root
    /// binder environment from them before composing the function's own.
    /// Empty for every other function position.
    pub enclosing_type_parameters: Arc<[SliceTypeParam]>,
    /// The root region (the function body statement list). An
    /// expression-bodied arrow lowers to a single `return` of the
    /// expression.
    pub body: SliceRegion,
    /// Whether execution can reach past the body without a `return`.
    pub can_fall_through: bool,
    /// A budget edge one SELECTED leaf's expression lowering hit (the
    /// expression itself degrades to `any`, the whole evaluation fails
    /// with the typed budget reason). Unselected content never lowers,
    /// so it can never charge this edge.
    pub budget_failure: Option<verter_type_expr::facts::InferenceUnavailableReason>,
}

/// One formal parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceParam {
    /// The binding name (`None` for a destructured parameter).
    pub name: Option<Arc<str>>,
    /// Whether the parameter is optional (`?`).
    pub optional: bool,
    /// Whether this is the rest parameter.
    pub rest: bool,
    /// The authored TS annotation lowered through `lower_ts_type`, else
    /// the default initializer's inferred type, else `any` — always
    /// through the frame gate for the signature's scope.
    pub ty: GatedType,
}

/// One sequential statement list with its reachability result.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceRegion {
    /// The reachable statements, in source order. Statements after a
    /// terminal path (return / throw / unsupported construct) are
    /// unreachable and dropped.
    pub statements: Arc<[SliceStatement]>,
    /// Whether execution can reach past this region without a `return`.
    pub can_fall_through: bool,
}

/// One statement of the slice content.
#[derive(Debug, Clone, PartialEq)]
pub enum SliceStatement {
    /// A `return` (bare `return;` carries no argument).
    Return {
        /// The lowered return argument, when present.
        argument: Option<SliceExpr>,
        /// Whether the argument is a FRESH literal source (a bare
        /// literal expression with no const assertion). tsc widens a
        /// fresh literal return only when it is the function's SOLE
        /// return contributor; a multi-contributor join keeps every
        /// literal, so the widening decision belongs to the join, not
        /// to this position.
        widening_literal: bool,
    },
    /// An `if` statement. No guard narrowing: each arm is its own region;
    /// the test's value is never consumed by the evaluator, so no test
    /// content is carried.
    If {
        /// The consequent region.
        consequent: Box<SliceRegion>,
        /// The alternate region, when an `else` exists.
        alternate: Option<Box<SliceRegion>>,
    },
    /// A nested block, as its own region.
    Block(SliceRegion),
    /// A `const` / `let` / `var` declarator with an identifier binding.
    Binding {
        /// The binding name.
        name: Arc<str>,
        /// The declaration kind.
        kind: SliceBindingKind,
        /// The lowered initializer, when present.
        init: Option<SliceExpr>,
        /// The authored TS annotation lowered through the same shallow
        /// pass a [`SliceExpr::Type`] leaf carries, when the declarator
        /// annotates one. The annotation is the binding's DECLARED type:
        /// an initializer-less declarator seeds from it, and an
        /// annotated `const` publishes it instead of its initializer's
        /// pinned literal.
        ///
        /// A declarator annotation is a BODY position: this frame's
        /// body-local type declarations ARE in scope in it, so it always
        /// carries the frame gate's verdict.
        declared: Option<GatedType>,
        /// Whether the binding carries a WIDENING literal type: an
        /// unannotated `const` whose initializer is a bare literal
        /// expression with no const assertion (`const b = 1`). Reads of
        /// such a binding widen to the literal's primitive at
        /// return-object member positions and at the return join —
        /// `1 as const` / an annotated `const b: 1` stay non-widening
        /// and preserve the literal.
        widening_literal: bool,
    },
    /// A return-free loop or labeled construct: effectful but fall-through
    /// transparent.
    TransparentLoop,
    /// An unsupported construct (return-bearing loop/labeled, `switch`,
    /// `try`, `with`, a `break`/`continue` jump, a module-level
    /// statement). The whole function is unsupported: the region is
    /// produced up to this marker and the evaluator degrades the whole
    /// result.
    Unsupported(SliceUnsupported),
}

/// The kind of one local binding declarator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceBindingKind {
    /// A `const` (or `using`) declarator.
    Const,
    /// A `let` declarator.
    Let,
    /// A `var` declarator.
    Var,
}

/// A leaf `TypeExpr` that has PASSED the frame gate.
///
/// The field and the constructor are MODULE-private, so a
/// [`SliceExpr::Type`] cannot be minted anywhere else: every leaf
/// answer reaches this carrier through [`Lowerer::lower_leaf`], which
/// routes it through [`Lowerer::leaf_type`]'s gate verdict first. The
/// only other channel is [`GatedLeaf::map_ty`], which rewrites the
/// lowered type while PRESERVING the verdict that was already reached.
///
/// This is the same confinement [`GatedType`] applies to signature
/// positions, at the body-leaf position: "produce a `TypeExpr` in slice
/// content without deciding what the frame does to it" is inexpressible
/// rather than merely discouraged.
#[derive(Debug, Clone, PartialEq)]
pub struct GatedLeaf(TypeExpr);

impl GatedLeaf {
    /// The lowered leaf type.
    #[must_use]
    pub fn ty(&self) -> &TypeExpr {
        &self.0
    }

    /// Rewrite the lowered type PRESERVING the gate verdict.
    ///
    /// The one caller widens a non-`as const` object-literal member's
    /// value, which cannot introduce a name the gate has not already
    /// seen — widening only ever replaces a literal with its primitive.
    fn map_ty(self, f: impl FnOnce(TypeExpr) -> TypeExpr) -> Self {
        Self(f(self.0))
    }
}

/// One expression of the slice content.
#[derive(Debug, Clone, PartialEq)]
pub enum SliceExpr {
    /// A fully lowered leaf: literals, arrays, objects (spread members ride
    /// as `ObjectMember::Spread` for later delegation to the object-spread
    /// projection), templates, `typeof` paths, `as` / `satisfies` /
    /// parenthesized results — the shared shallow-pass per-expression
    /// lowering, through the frame gate.
    Type(GatedLeaf),
    /// A leaf answer that names one or more bindings THIS FRAME owns —
    /// the root-identifier gate's carrier.
    ///
    /// The shared shallow-pass leaf lowering has no frame: it resolves
    /// every name in FILE OWNER SCOPE. So the leaf's `typeof CBait.s` /
    /// `ReturnType<typeof obj.m>` / `{ ...base }` answers are only
    /// correct while no owner-scope declaration ANSWERS those names — the
    /// moment one does, the published value is a different symbol's,
    /// cleanly and warm. The gate cannot decide that in the lowerer (the
    /// content half is arena-only and never sees the owner scope), so it
    /// wraps the answer it produced together with the frame-owned names
    /// it found; the evaluator — which resolves through the one shared
    /// resolver — fails closed exactly when the owner scope would answer
    /// one of them, and otherwise evaluates the wrapped leaf unchanged.
    FrameShadowed {
        /// The leaf carrier the gate wrapped ([`SliceExpr::Type`] or
        /// [`SliceCall::Symbolic`]).
        inner: Box<SliceExpr>,
        /// Frame-owned names the answer references, by name space.
        shadowed: Arc<[FrameShadowedName]>,
    },
    /// A parameter reference, substituted by the evaluator.
    Param {
        /// The parameter's ordinal in source order (rest last).
        ordinal: u32,
    },
    /// A local binding reference; its reaching definition is resolved by
    /// the evaluator. Covers BOTH a same-frame local and a binding an
    /// ENCLOSING frame declares (read from inside a nested function
    /// value) — the two differ only in `captured`, so every consumer that
    /// reasons about "a read of a local binding" (the widening-literal
    /// widen, the freshness classification) covers both by construction
    /// rather than by remembering to name a second carrier.
    Local {
        /// The binding name.
        name: Arc<str>,
        /// The ordinal of a parameter this binding REDECLARES (a hoisted
        /// `var` of the same name). The evaluator falls back to it when
        /// the declarator's reaching definition is not bound yet — a
        /// redeclaring `var` never erases the parameter's value. Always
        /// `None` for a captured read: a capture never redeclares one of
        /// THIS frame's parameters.
        param: Option<u32>,
        /// Whether the binding belongs to an ENCLOSING frame. The
        /// evaluator answers a capture from the snapshot of the enclosing
        /// layers the nested frame was seeded with; a capture the snapshot
        /// does not carry (the demand slice selected no definition for it)
        /// fails CLOSED — never the implicit-`any` a same-frame unbound
        /// read takes, and never a file-scope resolution of the same name.
        captured: bool,
    },
    /// An object-literal return evaluated STRUCTURALLY: every member value
    /// is a flow expression (parameter / local references substitute). Only
    /// plain string-keyed `Init` properties lower this way — spreads,
    /// computed keys, and method / accessor members keep the whole-literal
    /// leaf lowering.
    Object {
        /// The members in source order.
        members: Arc<[SliceObjectMember]>,
    },
    /// A nested function VALUE (a function / arrow expression or an
    /// object-literal method in any expression position): its parameters
    /// and OWNED body region, lowered inline — the evaluator answers its
    /// body-derived return through the same flow evaluation, never a body
    /// scan and never a leaf fallback.
    NestedFunctionValue {
        /// The nested function's formal parameters (rest last).
        params: Arc<[SliceParam]>,
        /// The nested function's own type parameters (the signature's own
        /// binders — carried so the composed signature keeps `<T>`).
        type_parameters: Arc<[SliceTypeParam]>,
        /// The nested function's body region (an expression-bodied arrow
        /// lowers to a single `return` of the expression).
        body: SliceRegion,
        /// Whether execution can reach past the nested body without a
        /// `return`.
        can_fall_through: bool,
    },
    /// EVERY call form — the one carrier through which a CALLEE's return
    /// can become this frame's value.
    ///
    /// Calls are grouped behind a single variant, over the closed
    /// [`SliceCall`] vocabulary, precisely so the evaluator has ONE call
    /// arm: its call sink is typed
    /// [`CallValue`](crate::project_semantic_dispatch::flow_return_callee::CallValue),
    /// whose constructors all decide what happens to the callee's own
    /// type-parameter clause. A new call form is added HERE, and the
    /// evaluator's exhaustive match then forces that decision at the new
    /// arm rather than leaving "hand the callee's return back verbatim"
    /// available as the path of least resistance.
    ///
    /// The [`SliceCallSite`] rides on the variant rather than inside
    /// [`SliceCall`] because it is what EVERY form needs and no form
    /// owns: the callee's clause resolves against the CALL, not against
    /// the way the callee was reached.
    Call(SliceCall, SliceCallSite),
    /// An expression the leaf lowering cannot represent (its `any`
    /// fallback), including a call with an unrepresentable callee.
    Any,
    /// A read (or call) of a name the frame's lexical authority resolves
    /// to a FUNCTION-LOCAL binding this content half does not model: a
    /// destructuring-pattern element, a local `class` / `enum` /
    /// `namespace` / `import =`, or a `catch` parameter.
    ///
    /// The name is RESOLVED, not free — falling back to the shared leaf
    /// lowering would resolve it in FILE OWNER SCOPE and silently bind an
    /// unrelated module-scope (or cross-file imported) value of the same
    /// name, cleanly and warm. Fails closed at the evaluator instead.
    UnmodeledBinding,
    /// A VALUE UNION of lowered arms — a conditional expression's two
    /// branches.
    ///
    /// The arms are lowered flow expressions, not a leaf answer, which is
    /// the whole point: a call in a ternary arm is a CALL, and rides
    /// [`SliceExpr::Call`] to the evaluator's one call sink exactly as
    /// the `if` / `return` twin's does. Folding the ternary through the
    /// shared shallow-pass leaf lowering instead published the callee's
    /// UNREDUCED return carrier — binders and overload group intact.
    Union {
        /// The branch values in source order.
        arms: Arc<[SliceExpr]>,
    },
    /// An expression whose leaf answer EMBEDS an unreduced call-return
    /// carrier: a call reached through a form with no structural arm.
    ///
    /// The shared shallow pass has no frame and no resolver, so a CALL it
    /// meets answers as `ReturnType<callee>` with nothing instantiated.
    /// Publishing that as this frame's value hands out the callee's own
    /// type-parameter binders and skips its overload group, warm. There
    /// is no honest value here, so the evaluator fails closed.
    UnreducedCallValue,
    /// Content the demand slice did NOT select: never lowered, never
    /// evaluable. Observing an elided value is a planner/content mismatch
    /// and fails closed at the evaluator — it is never a fabricated
    /// `any` and never a silently widened sibling.
    Elided,
}

/// The CLOSED vocabulary of call forms — every way a callee's return can
/// become the value of an expression in a flow frame.
///
/// One enum rather than six sibling [`SliceExpr`] variants because the
/// evaluator's call sink is a single typed value: each arm has to say
/// what happens to the CALLEE's own type-parameter clause before the
/// callee's return can be this frame's answer. Splitting the forms back
/// across `SliceExpr` would restore the per-arm drift this grouping
/// exists to prevent (two of the arms below silently lost the rule while
/// their siblings kept it).
/// The CALL-SITE facts a callee's type-parameter clause resolves
/// against.
///
/// TypeScript resolves a call's type arguments in one order: explicit
/// type arguments, else inference from the supplied arguments, else the
/// declared defaults. This substrate cannot yet do the first two
/// (`U6.CALL_RESOLVE`), and `unknown` is its recorded interim for both.
/// But "the default applies" is a statement about the other two having
/// produced NOTHING, so it is not expressible without knowing whether
/// they COULD have produced something — which is exactly what these
/// bits say. The argument TYPES are deliberately absent: deciding what
/// inference would produce is the work being deferred, and a substrate
/// that guessed would publish a confident wrong answer instead of the
/// honest interim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliceCallSite {
    /// The number of arguments written before any spread element.
    fixed_argument_count: u32,
    /// Whether the call SPREADS (`f(...xs)`), which makes its arity
    /// unbounded: every parameter ordinal must then be treated as
    /// supplied, because assuming otherwise would take a declared
    /// default at a position inference could reach.
    spreads_arguments: bool,
    /// Whether the call authored explicit type arguments (`f<string>()`).
    /// Resolving them is `U6.CALL_RESOLVE`'s; until then their presence
    /// means the DECLARED DEFAULT is definitely not the answer.
    has_explicit_type_arguments: bool,
}

impl SliceCallSite {
    /// The call-site facts of one authored call expression.
    #[must_use]
    pub fn new(
        fixed_argument_count: u32,
        spreads_arguments: bool,
        has_explicit_type_arguments: bool,
    ) -> Self {
        Self {
            fixed_argument_count,
            spreads_arguments,
            has_explicit_type_arguments,
        }
    }

    /// Whether the call supplies an argument at `ordinal` — the ONLY
    /// question inference asks of a call site here. A spreading call
    /// answers yes for every ordinal.
    #[must_use]
    pub fn supplies_parameter_ordinal(self, ordinal: u32) -> bool {
        self.spreads_arguments || ordinal < self.fixed_argument_count
    }

    /// Whether the call authored explicit type arguments.
    #[must_use]
    pub fn has_explicit_type_arguments(self) -> bool {
        self.has_explicit_type_arguments
    }
}

/// The [`SliceCallSite`] of one authored call expression.
fn call_site(call: &oxc_ast::ast::CallExpression<'_>) -> SliceCallSite {
    let fixed = call
        .arguments
        .iter()
        .take_while(|argument| !matches!(argument, oxc_ast::ast::Argument::SpreadElement(_)))
        .count();
    SliceCallSite::new(
        fixed as u32,
        fixed != call.arguments.len(),
        call.type_arguments.is_some(),
    )
}

#[derive(Debug, Clone, PartialEq)]
pub enum SliceCall {
    /// A direct call on a nested function value (an IIFE) — the call's
    /// value is the nested function's evaluated return.
    Nested(Box<SliceExpr>),
    /// A call on a parameter or in-scope local binding of function type —
    /// the call's value is the binding's signature return (a shadowed
    /// name is never a flow obligation edge).
    OnBinding {
        /// The parameter ordinal (when the callee is a parameter).
        param: Option<u32>,
        /// The binding name.
        name: Arc<str>,
        /// Whether the callee is a CAPTURED enclosing binding (the same
        /// axis [`SliceExpr::Local`] carries): an unbound capture fails
        /// closed instead of taking the implicit-`any` call.
        captured: bool,
    },
    /// A bare-identifier call to a name a hoisted nested function
    /// declaration binds in this function. The nested declaration shadows
    /// every outer same-name callee (function declarations hoist over
    /// parameters, locals, and file-level bindings); exact recovery of the
    /// nested declaration's own return is not implemented, so the
    /// evaluator FAILS CLOSED, never binding the outer callee.
    LocalFunctionShadow,
    /// A bare-identifier call to the function itself — a direct same-slot
    /// recursion hold.
    DirectSelf,
    /// A bare-identifier call whose target the per-file function index
    /// resolves EXACTLY (a same-file served function position) — a Flow
    /// obligation edge to that target.
    Direct(verter_semantic::analysis::function_program::FunctionProgramKey),
    /// A call lowered to the symbolic `ReturnType<typeof …>` carrier.
    Symbolic(TypeExpr),
}

/// One name an answer references that the frame's LEXICAL AUTHORITY
/// owns, with the name MEANING it was referenced in. The evaluator
/// probes the owner scope for exactly that meaning: a value name through
/// `typeof name`, a type or namespace name through a bare `name`
/// reference (the head of a qualified reference is a scope lookup in
/// either meaning — the meaning selects which LOCAL declarations shadow
/// it, which is decided on the frame side).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FrameShadowedName {
    /// The root of a `typeof name…` path — a VALUE binding.
    Value(Arc<str>),
    /// The head of a BARE named type reference — a TYPE binding.
    Type(Arc<str>),
    /// The head of a QUALIFIED (`N.B`) named type reference — a
    /// NAMESPACE binding. A local `class N` shadows the `Type` question
    /// but not this one; a local `namespace N` shadows this one but not
    /// `Type`.
    Namespace(Arc<str>),
}

/// One `TypeExpr` minted INSIDE slice content, carrying the frame gate's
/// verdict: the frame-owned names its answer references.
///
/// The shared shallow-pass lowering has no frame — it resolves every
/// name it meets in FILE-OWNER SCOPE — so any answer produced inside a
/// function body position is wrong whenever the frame binds one of the
/// names it references. Both fields are PRIVATE and both constructors
/// live in this module, so "produce a `TypeExpr` in slice content
/// without deciding what the frame does to it" is inexpressible at every
/// call site rather than merely discouraged: a new producer must pick
/// [`Lowerer::gate`] or the explicitly-named
/// [`GatedType::root_signature`].
#[derive(Debug, Clone, PartialEq)]
pub struct GatedType {
    ty: TypeExpr,
    shadowed: Arc<[FrameShadowedName]>,
}

impl GatedType {
    /// The ROOT function's OWN signature — its parameter list, its
    /// type-parameter clause, and its parameter defaults.
    ///
    /// Deliberately UNGATED, and the only such constructor.
    /// `checker.ts::resolveName` discards a Type-meaning hit in a
    /// function's own `locals` whenever `lastLocation !== location.body`
    /// (mirrored on the value side by `useOuterVariableScopeInParameter`),
    /// so a function's body-local declarations are in scope only inside
    /// its own body — never in its own parameter list, type-parameter
    /// clause, or parameter defaults. Gating these positions against the
    /// frame would fail closed on `function f(p: Info) { class Info {} }`,
    /// where `Info` is the OUTER one and the owner-scope answer is the
    /// correct one.
    #[must_use]
    pub fn root_signature(ty: TypeExpr) -> Self {
        Self {
            ty,
            shadowed: Arc::from(Vec::new().into_boxed_slice()),
        }
    }

    /// The lowered type.
    #[must_use]
    pub fn ty(&self) -> &TypeExpr {
        &self.ty
    }

    /// The frame-owned names the answer references. Empty means every
    /// name is genuinely free in the frame this type was produced in.
    #[must_use]
    pub fn shadowed(&self) -> &[FrameShadowedName] {
        &self.shadowed
    }

    /// WIDEN an existing answer's frame verdict with more shadow
    /// entries.
    ///
    /// NOT a mint and not a third constructor: the answer was already
    /// produced by one of the two above, and this only records
    /// additional frame-owned names it references — the signature's own
    /// PARAMETER LIST inventory, and a default initializer's
    /// reference-chain root. Private to this module, so the mint surface
    /// stays exactly two entrances.
    fn add_shadowed(&mut self, extra: impl IntoIterator<Item = FrameShadowedName>) {
        let mut shadowed = self.shadowed.to_vec();
        let before = shadowed.len();
        for entry in extra {
            if !shadowed.contains(&entry) {
                shadowed.push(entry);
            }
        }
        if shadowed.len() != before {
            self.shadowed = Arc::from(shadowed.into_boxed_slice());
        }
    }
}

/// One type parameter of a function value.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceTypeParam {
    /// The parameter name.
    pub name: Arc<str>,
    /// The lowered constraint, when authored.
    pub constraint: Option<GatedType>,
    /// The lowered default, when authored.
    pub default: Option<GatedType>,
}

/// Which signature a [`lower_params`] / [`lower_slice_type_params`] call
/// is lowering, and therefore which frame — if any — its answers are
/// gated against.
///
/// NOT defaultable: a new call site must choose, because the two arms
/// differ in exactly the way `resolveName` does and picking the wrong
/// one is either a silent wrong answer (Root where Nested was needed) or
/// a spurious fail-closed (the reverse).
enum SignatureScope<'a, 'b> {
    /// The indexed function's OWN signature: body-local declarations are
    /// NOT in scope here, so the owner-scope answer is the right one.
    Root,
    /// A NESTED function value's signature, which sits INSIDE the
    /// enclosing frame's body and therefore sees that frame's
    /// body-locals.
    Nested {
        /// The ENCLOSING frame's lexical authority.
        gate: &'a Lowerer<'b>,
        /// The nested function value's own position — the region the
        /// enclosing frame resolves names at.
        at: oxc_span::Span,
        /// The nested function's OWN type parameters: they bind inside
        /// its signature and the evaluator's binder environment carries
        /// them, so they are the answer's binders, not references into
        /// the enclosing frame.
        binders: &'a [Arc<str>],
    },
}

/// One member of a structurally lowered object-literal return.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceObjectMember {
    /// The static string key.
    pub key: Arc<str>,
    /// The member value.
    pub value: SliceExpr,
    /// The authored method / accessor kind (`None` for a plain property).
    pub method_kind: Option<verter_type_expr::ObjectMethodKind>,
    /// The authored member spans (declaration / name) — they keep two
    /// same-shaped return objects at distinct source sites distinct at
    /// interning.
    pub spans: verter_type_expr::MemberSpans,
}

/// The unsupported-construct classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceUnsupported {
    /// A return-bearing loop.
    Loop,
    /// A `switch` statement.
    Switch,
    /// A `try` statement.
    Try,
    /// A return-bearing labeled statement.
    Labeled,
    /// A `break` / `continue` jump of the current function's statement
    /// list.
    Jump,
    /// A `with` statement.
    With,
    /// A module-level statement inside the body.
    ModuleDeclaration,
}

/// One function node's own type-parameter NAMES, read syntactically.
///
/// The names are needed BEFORE the clause is lowered: they are the
/// binders every parameter annotation of the same signature lowers
/// under, so [`lower_params`] must already know them.
fn slice_type_param_names(node: &FunctionNode<'_>) -> Vec<Arc<str>> {
    node.type_parameters()
        .map(|declaration| {
            declaration
                .params
                .iter()
                .map(|param| Arc::from(param.name.name.as_str()))
                .collect()
        })
        .unwrap_or_default()
}

/// Lower one type parameter clause (name, lowered constraint, lowered
/// default) — shared by the root content, every nested function value,
/// and the enclosing class clause a member body sits inside.
///
/// A clause binds its OWN siblings, so constraints and defaults gate
/// under the WHOLE clause, not "the preceding siblings": TypeScript
/// accepts a forward sibling reference in a constraint
/// (`<U extends V, V>` type-checks and still constrains through `V`), so
/// a preceding-only inventory is wrong for exactly that shape. The
/// evaluator's binder environment mirrors this — it interns the whole
/// clause first, then lowers the constraints and defaults under it.
fn lower_type_param_clause(
    declaration: Option<&oxc_ast::ast::TSTypeParameterDeclaration<'_>>,
    source: &str,
    scope: &SignatureScope<'_, '_>,
) -> Vec<SliceTypeParam> {
    let binders = scope.param_binders();
    declaration
        .map(|declaration| {
            declaration
                .params
                .iter()
                .map(|param| SliceTypeParam {
                    name: Arc::from(param.name.name.as_str()),
                    constraint: param
                        .constraint
                        .as_ref()
                        .map(|constraint| scope.gate(lower_ts_type(constraint, source), binders)),
                    default: param
                        .default
                        .as_ref()
                        .map(|default| scope.gate(lower_ts_type(default, source), binders)),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One function node's own type parameter clause.
fn lower_slice_type_params(
    node: &FunctionNode<'_>,
    source: &str,
    scope: &SignatureScope<'_, '_>,
) -> Vec<SliceTypeParam> {
    lower_type_param_clause(node.type_parameters(), source, scope)
}

impl SignatureScope<'_, '_> {
    /// Gate one signature-position answer for this scope.
    fn gate(&self, ty: TypeExpr, binders: &[Arc<str>]) -> GatedType {
        match self {
            SignatureScope::Root => GatedType::root_signature(ty),
            SignatureScope::Nested { gate, at, .. } => gate.gate(ty, *at, binders),
        }
    }

    /// The binders a PARAMETER annotation of this signature lowers
    /// under: the signature's own type-parameter clause.
    fn param_binders(&self) -> &[Arc<str>] {
        match self {
            SignatureScope::Root => &[],
            SignatureScope::Nested { binders, .. } => binders,
        }
    }

    /// Gate one parameter DEFAULT-INITIALIZER answer.
    ///
    /// A default initializer is an EXPRESSION evaluated in the scope the
    /// signature sits in, and the shared shallow pass resolves its names
    /// in owner scope. The answer's own names are gated as usual, but
    /// the initializer's REFERENCE-CHAIN ROOT must be checked too: a
    /// widened or primitive answer (`p = C` ⇒ `string`) carries no name
    /// at all while still having read THROUGH a binding the answer never
    /// names, so the answer-name half alone cannot see it.
    ///
    /// Two inventories answer that question and BOTH arms consult both.
    /// The enclosing FRAME's bindings apply only to a nested signature
    /// (a root signature does not see its own body-locals). This
    /// signature's PRECEDING PARAMETERS apply to either arm — they are
    /// not `locals`, and TS2373 makes "preceding" exact.
    fn gate_param_default(
        &self,
        ty: TypeExpr,
        initializer: &Expression<'_>,
        binders: &[Arc<str>],
        parameters: &[(Arc<str>, verter_span::Span)],
    ) -> GatedType {
        let mut gated = match self {
            SignatureScope::Root => GatedType::root_signature(ty),
            SignatureScope::Nested { gate, at, .. } => {
                let mut gated = gate.gate(ty, *at, binders);
                if let Some(root) = chain_root_identifier(initializer) {
                    if !matches!(
                        gate.resolve_name(root.name.as_str(), root.span),
                        NameBinding::Free
                    ) {
                        gated.add_shadowed([FrameShadowedName::Value(Arc::from(
                            root.name.as_str(),
                        ))]);
                    }
                }
                gated
            }
        };
        if let Some(root) = chain_root_identifier(initializer) {
            let limit = initializer.span().start;
            if parameters
                .iter()
                .any(|(name, span)| name.as_ref() == root.name.as_str() && span.end <= limit)
            {
                gated.add_shadowed([FrameShadowedName::Value(Arc::from(root.name.as_str()))]);
            }
        }
        gated
    }
}

/// Lower ONE indexed function's own type-parameter clause against the
/// retained parse snapshot — the callee-side half of the call-site
/// clause rule.
///
/// A CALLER instantiating a generic callee's clause needs the declared
/// DEFAULTS, not just the names: an argument-free call to
/// `f<T = number>()` is `number`, and substituting `unknown` there
/// publishes a type the callee's own declaration rules out. The names
/// ride the shallow function-program index (a syntactic fact); the
/// default TYPES are a body lowering, so they come from here, through
/// the same lease-only retained-snapshot run every other body product
/// uses — and only for the clauses the index says have one.
///
/// The clause is lowered in [`SignatureScope::Root`], the same scope the
/// function's own signature lowers in: a function's body-local
/// declarations are not in scope in its own type-parameter clause.
/// Returns `None` on a locator miss (a typed miss, never a panic).
pub(crate) fn build_function_type_param_clause(
    program: &Program<'_>,
    source: &str,
    entry: &FunctionProgramEntry,
) -> Option<Vec<SliceTypeParam>> {
    let resolved = resolve_function_node(program, &entry.locator)?;
    Some(lower_slice_type_params(
        &resolved.node,
        source,
        &SignatureScope::Root,
    ))
}

/// Build the slice content for one indexed function entry against the
/// retained parse snapshot, lowering ONLY `selection`-selected expression
/// content. Runs inside the memo's lease-only job: pure, owned output, no
/// host re-entry. Returns `None` on any locator miss (a typed miss, never
/// a panic).
pub(crate) fn build_flow_slice_content(
    program: &Program<'_>,
    source: &str,
    entry: &FunctionProgramEntry,
    selection: &FlowSliceSelection,
    skeleton: &FunctionBodySkeleton,
) -> Option<SliceContent> {
    let resolved = resolve_function_node(program, &entry.locator)?;
    let node = resolved.node;
    let self_name = resolved.self_name;
    // The ROOT function's OWN signature resolves in the OUTER scope: its
    // body-local declarations are not in scope in its parameter list,
    // its type-parameter clause, or its parameter defaults.
    //
    // The ENCLOSING declaration's clause joins the function's own: a
    // class member sits inside `class C<T>`, whose binders are in scope
    // throughout the member's signature and body but never appear in the
    // member's own clause. Without them the class binder reads as a free
    // name and resolves in owner scope.
    let enclosing_type_parameters = lower_type_param_clause(
        resolved.enclosing_type_parameters,
        source,
        &SignatureScope::Root,
    );
    let type_param_names = slice_type_param_names(&node);
    let anchor = node_span(&node).start;
    let params = lower_params(
        node.params(),
        source,
        &SignatureScope::Root,
        skeleton,
        anchor,
    );
    let type_parameters = lower_slice_type_params(&node, source, &SignatureScope::Root);
    let body = node.body()?;
    // The enclosing clause is deliberately NOT part of this frame's
    // binder inventory. TS2300 protects a function's own clause from a
    // same-named body-local, but a CLASS binder and a member body's
    // local are different scopes: `class C<T> { m() { class T {} … } }`
    // is legal and the method's local WINS — so the class clause could
    // only ever enter behind this frame's own lexical authority, where
    // it is indistinguishable from a name nothing claims. In TYPE
    // meaning "a binder answers" and "nothing here answers" have the
    // same verdict (the composed binder environment supplies the answer
    // either way), and in NAMESPACE meaning recording it would be
    // strictly WRONG: `resolveName` skips a type parameter for a
    // qualified head, so `class C<QY> { m() { … as QY.Inner } }`
    // resolves to a module `namespace QY` (checker-verified), which
    // marking it frame-bound would fail closed on. The class clause
    // therefore reaches the answer through the EVALUATOR's binder
    // environment only.
    let captures = CaptureScope::default();
    let mut lowerer = Lowerer {
        source,
        anchor,
        selection: Some(selection),
        params: &params,
        type_param_names: &type_param_names,
        self_name: self_name.as_deref(),
        skeleton,
        captures: &captures,
        control: Arc::clone(&entry.control),
        direct_calls: &entry.direct_calls,
        budget_failure: None,
    };
    let region = if node.is_expression_body() {
        // An expression-bodied arrow's body is one synthesized expression
        // statement; it lowers to a single `return` of the expression (the
        // expression cannot fall through).
        let statement = body.statements.first()?;
        let Statement::ExpressionStatement(expression) = statement else {
            return None;
        };
        let widening_literal = expr_is_bare_literal(&expression.expression);
        let argument = if lowerer.value_span_selected(expression.expression.span()) {
            lowerer.lower_expr(&expression.expression, ExprMode::Return)
        } else {
            SliceExpr::Elided
        };
        SliceRegion {
            statements: Arc::from([SliceStatement::Return {
                argument: Some(argument),
                widening_literal,
            }]),
            can_fall_through: false,
        }
    } else {
        lowerer.lower_region(&body.statements).region
    };
    let budget_failure = lowerer.budget_failure;
    Some(SliceContent {
        can_fall_through: region.can_fall_through,
        params: Arc::from(params.into_boxed_slice()),
        type_parameters: Arc::from(type_parameters.into_boxed_slice()),
        enclosing_type_parameters: Arc::from(enclosing_type_parameters.into_boxed_slice()),
        body: region,
        budget_failure,
    })
}

/// The authored span of one nested function value — the position its
/// capture scope resolves at.
fn node_span(node: &FunctionNode<'_>) -> oxc_span::Span {
    match node {
        FunctionNode::Function(func) => func.span,
        FunctionNode::Arrow(arrow) => arrow.span,
    }
}

/// Unwrap a parenthesized expression (the IIFE callee shape).
fn unwrap_parenthesized<'a>(expression: &'a Expression<'a>) -> &'a Expression<'a> {
    match expression {
        Expression::ParenthesizedExpression(paren) => unwrap_parenthesized(&paren.expression),
        inner => inner,
    }
}

/// Unwrap the wrappers that are TRANSPARENT to literal freshness: a
/// parenthesis, and `satisfies`. `x satisfies T` checks `x` against `T`
/// and evaluates to `x`'s own type unchanged — including its freshness —
/// so `return 1 satisfies number` is `number`, exactly like `return 1`.
///
/// A type ASSERTION is not on this list and must never be added: `1 as 1`
/// PINS to `1` even though the asserted type is the literal's own
/// (tsgo 7.0.0-dev.20260526.1: `(): 1`).
fn unwrap_freshness_transparent<'a>(expression: &'a Expression<'a>) -> &'a Expression<'a> {
    match expression {
        Expression::ParenthesizedExpression(paren) => {
            unwrap_freshness_transparent(&paren.expression)
        }
        Expression::TSSatisfiesExpression(satisfies) => {
            unwrap_freshness_transparent(&satisfies.expression)
        }
        inner => inner,
    }
}

/// The ROOT IDENTIFIER of an expression's REFERENCE CHAIN — the binding
/// whose value the whole chain reads from: `a` for `a`, `a.b`, `a["b"]`,
/// `a.#b`, `a?.b`, `a.b()`, `new a()`, `` a`…` ``, and each of those
/// through a parenthesis or a TS wrapper (`as` / `satisfies` / `!` /
/// explicit instantiation).
///
/// `None` for every expression that is not a reference chain (a literal,
/// an assignment, an operator expression, an object / array literal, a
/// function value, `this`): those read no single binding, so there is no
/// root for the frame's lexical authority to classify.
fn chain_root_identifier<'a>(
    expr: &'a Expression<'a>,
) -> Option<&'a oxc_ast::ast::IdentifierReference<'a>> {
    match expr {
        Expression::Identifier(identifier) => Some(identifier),
        Expression::ParenthesizedExpression(paren) => chain_root_identifier(&paren.expression),
        Expression::TSAsExpression(ts_as) => chain_root_identifier(&ts_as.expression),
        Expression::TSSatisfiesExpression(satisfies) => {
            chain_root_identifier(&satisfies.expression)
        }
        Expression::TSNonNullExpression(non_null) => chain_root_identifier(&non_null.expression),
        Expression::TSInstantiationExpression(instantiation) => {
            chain_root_identifier(&instantiation.expression)
        }
        Expression::StaticMemberExpression(member) => chain_root_identifier(&member.object),
        Expression::ComputedMemberExpression(member) => chain_root_identifier(&member.object),
        Expression::PrivateFieldExpression(member) => chain_root_identifier(&member.object),
        Expression::CallExpression(call) => chain_root_identifier(&call.callee),
        Expression::NewExpression(new) => chain_root_identifier(&new.callee),
        Expression::TaggedTemplateExpression(tagged) => chain_root_identifier(&tagged.tag),
        Expression::ChainExpression(chain) => chain_element_root_identifier(&chain.expression),
        _ => None,
    }
}

/// [`chain_root_identifier`] for the optional-chain element carrier.
fn chain_element_root_identifier<'a>(
    element: &'a oxc_ast::ast::ChainElement<'a>,
) -> Option<&'a oxc_ast::ast::IdentifierReference<'a>> {
    match element {
        oxc_ast::ast::ChainElement::CallExpression(call) => chain_root_identifier(&call.callee),
        oxc_ast::ast::ChainElement::TSNonNullExpression(non_null) => {
            chain_root_identifier(&non_null.expression)
        }
        oxc_ast::ast::ChainElement::StaticMemberExpression(member) => {
            chain_root_identifier(&member.object)
        }
        oxc_ast::ast::ChainElement::ComputedMemberExpression(member) => {
            chain_root_identifier(&member.object)
        }
        oxc_ast::ast::ChainElement::PrivateFieldExpression(member) => {
            chain_root_identifier(&member.object)
        }
    }
}

/// Whether an initializer is a BARE literal expression — a fresh
/// (widening) literal source: a string / numeric / boolean literal or a
/// substitution-free template, seen through the freshness-transparent
/// wrappers. A const assertion (`1 as const`), a type assertion
/// (`1 as 1`), or any other expression shape is NOT bare — its literal is
/// pinned or derived, never widening.
fn expr_is_bare_literal(expression: &Expression<'_>) -> bool {
    match unwrap_freshness_transparent(expression) {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_) => true,
        Expression::TemplateLiteral(template) => template.expressions.is_empty(),
        _ => false,
    }
}

/// Whether `statement` declares a function-scoped (`var`) binding, read
/// from the SAME single inventory walk the index uses (nested function
/// bodies are never entered, so a `var` inside a nested function value
/// belongs to that frame, not this one).
fn declares_var(statement: &Statement<'_>) -> bool {
    !inventory_statement_list(std::slice::from_ref(statement))
        .var_names
        .is_empty()
}

/// The names THIS signature's parameter list binds, paired with their
/// binding-identifier spans, read from the frame's own
/// [`FunctionBodySkeleton`] — the SAME single lexical authority every
/// other classification in this module routes through. A DESTRUCTURED
/// element is inventoried exactly like a plain binding identifier: the
/// checker resolves `typeof a` in `f({ a }: { a: number }, b: typeof a)`
/// to the destructured element.
fn signature_parameter_bindings(
    skeleton: &FunctionBodySkeleton,
    anchor: u32,
) -> Vec<(Arc<str>, verter_span::Span)> {
    skeleton
        .bindings
        .iter()
        .filter(|binding| binding.kind == SkeletonBindingKind::Param)
        .map(|binding| {
            (
                Arc::from(skeleton.name(binding.name)),
                // The skeleton is anchor-relative; the offsets these spans
                // are compared against (a default initializer's start) are
                // live and absolute, so this is the crossing OUT — the
                // only one, and it has to name the anchor to happen.
                binding.span.to_absolute(anchor),
            )
        })
        .collect()
}

/// Rebase a LIVE source span onto a function's own anchor.
fn rebase_span(anchor: u32, span: oxc_span::Span) -> FrameSpan {
    FrameSpan::rebase(anchor, span.into())
}

/// The parameter names one signature answer references — the
/// PARAMETER-LIST half of the frame gate.
///
/// `visible_before` is the byte offset a DEFAULT INITIALIZER starts at,
/// or `None` for an ANNOTATION. An annotation sees the WHOLE parameter
/// list (`f(a: number, p: X, b: typeof p)` binds the parameter `p`
/// regardless of order); a default initializer sees only the PRECEDING
/// parameters, because TS2373 rejects a forward reference outright and a
/// later same-named parameter must not mask the outer declaration the
/// initializer genuinely reads.
fn parameter_list_shadowed(
    ty: &TypeExpr,
    parameters: &[(Arc<str>, verter_span::Span)],
    visible_before: Option<u32>,
) -> Vec<FrameShadowedName> {
    let names = verter_type_expr::referenced_names(ty);
    let mut out: Vec<FrameShadowedName> = Vec::new();
    for root in &names.value_roots {
        let bound = parameters.iter().any(|(name, span)| {
            name.as_ref() == root.as_str() && visible_before.is_none_or(|limit| span.end <= limit)
        });
        if bound {
            let entry = FrameShadowedName::Value(Arc::from(root.as_str()));
            if !out.contains(&entry) {
                out.push(entry);
            }
        }
    }
    out
}

/// Lower the formal parameters: binding name, optional/rest flags, and the
/// parameter type — the authored TS annotation through `lower_ts_type`,
/// else the default initializer's inferred type, else `any`.
///
/// A signature's OWN parameter list is a shadowing inventory of THAT
/// signature, in the ROOT arm exactly as much as the nested one.
/// `resolveName`'s root rule discards a hit in the function's own
/// `locals`; a FORMAL PARAMETER is not in `locals`, so `typeof p` in a
/// sibling annotation and a preceding parameter named in a default
/// initializer both bind the PARAMETER — never an outer declaration of
/// the same name. Resolving those positively needs intra-signature
/// forward-reference resolution, so recording them here is what makes
/// the answer fail CLOSED instead of publishing an unrelated
/// module-scope symbol's type cleanly and warm.
fn lower_params(
    params: &FormalParameters<'_>,
    source: &str,
    scope: &SignatureScope<'_, '_>,
    skeleton: &FunctionBodySkeleton,
    anchor: u32,
) -> Vec<SliceParam> {
    let binders = scope.param_binders();
    let parameter_bindings = signature_parameter_bindings(skeleton, anchor);
    let mut out = Vec::with_capacity(params.items.len() + usize::from(params.rest.is_some()));
    for param in &params.items {
        let name = match &param.pattern {
            BindingPattern::BindingIdentifier(id) => Some(Arc::from(id.name.as_str())),
            _ => None,
        };
        let (mut ty, visible_before) =
            match (param.type_annotation.as_ref(), param.initializer.as_ref()) {
                (Some(annotation), _) => (
                    scope.gate(lower_ts_type(&annotation.type_annotation, source), binders),
                    None,
                ),
                (None, Some(initializer)) => (
                    scope.gate_param_default(
                        infer_declaration_expression_type(
                            initializer,
                            source,
                            TopLevelLiteralPolicy::Widen,
                        )
                        .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any)),
                        initializer,
                        binders,
                        &parameter_bindings,
                    ),
                    Some(initializer.span().start),
                ),
                (None, None) => (
                    GatedType::root_signature(TypeExpr::Primitive(PrimitiveName::Any)),
                    None,
                ),
            };
        let extra = parameter_list_shadowed(ty.ty(), &parameter_bindings, visible_before);
        ty.add_shadowed(extra);
        // An optional (`?`) parameter is `T | undefined` inside the body; a
        // defaulted parameter always has a value. The union rides the
        // SAME gate verdict: adding `undefined` names nothing new.
        let ty = if param.optional && param.initializer.is_none() {
            GatedType {
                ty: TypeExpr::union(vec![ty.ty, TypeExpr::Primitive(PrimitiveName::Undefined)]),
                shadowed: ty.shadowed,
            }
        } else {
            ty
        };
        out.push(SliceParam {
            name,
            optional: param.optional || param.initializer.is_some(),
            rest: false,
            ty,
        });
    }
    if let Some(rest) = &params.rest {
        let name = match &rest.rest.argument {
            BindingPattern::BindingIdentifier(id) => Some(Arc::from(id.name.as_str())),
            _ => None,
        };
        let mut ty = match rest.type_annotation.as_ref() {
            Some(annotation) => {
                scope.gate(lower_ts_type(&annotation.type_annotation, source), binders)
            }
            None => GatedType::root_signature(TypeExpr::Primitive(PrimitiveName::Any)),
        };
        let extra = parameter_list_shadowed(ty.ty(), &parameter_bindings, None);
        ty.add_shadowed(extra);
        out.push(SliceParam {
            name,
            optional: false,
            rest: true,
            ty,
        });
    }
    out
}

/// The expression-lowering position, selecting the shared shallow-pass
/// entry's literal policy.
#[derive(Clone, Copy)]
enum ExprMode {
    /// Return-argument position (including an expression-bodied arrow's
    /// synthesized return): the literal is PRESERVED here — tsc widens a
    /// fresh literal return only when it is the sole contributor, so the
    /// return join owns that decision.
    Return,
    /// Binding-initializer position. `preserve_literal` is the
    /// declarator's policy: a `const` keeps its initializer's literal,
    /// `let` / `var` widen it, and an ANNOTATED declarator keeps it
    /// because the declared type governs the outcome (the initializer
    /// only selects a constituent).
    BindingInit {
        /// Whether the initializer's fresh literal survives lowering.
        preserve_literal: bool,
    },
}

/// The region lowering result: the region plus whether any nested lowering
/// hit an unsupported construct (the marker is in the tree; the flag
/// propagates so the root region stops at the same point).
struct LoweredRegion {
    region: SliceRegion,
    hit_unsupported: bool,
}

/// How the frame's lexical authority classifies one identifier.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NameBinding {
    /// No binding in this frame or any enclosing one: a module- /
    /// outer-scope reference the shared leaf lowering resolves.
    Free,
    /// A simple formal parameter of THIS frame.
    Param(u32),
    /// A modelable local declarator (`const` / `let` / `var` / `using`),
    /// carrying the ordinal of a parameter it REDECLARES (a hoisted
    /// `var` sharing a parameter's slot).
    Local(Option<u32>),
    /// A modelable binding an ENCLOSING frame declares (a closure
    /// capture), read by name from the evaluator's seeded snapshot.
    Captured,
    /// A hoisted nested function declaration of this frame binds the
    /// name; it shadows every outer same-name declaration.
    NestedFunction,
    /// A resolved function-local binding this content half cannot model.
    Unmodeled,
}

/// A name an enclosing frame binds, as the ENCLOSING frame's lexical
/// authority classified it at the nested function value's own position.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CapturedBinding {
    /// A modelable enclosing parameter / local: the evaluator seeds the
    /// nested frame with the captured reaching definition BY NAME.
    Local,
    /// An enclosing binding the content half cannot model.
    Unmodeled,
}

/// The names a nested function value captures from its ENCLOSING frames,
/// resolved once at the position the function value itself occupies.
/// Empty for the root frame.
#[derive(Default)]
struct CaptureScope {
    names: FxHashMap<Arc<str>, CapturedBinding>,
    /// The captured names an enclosing frame binds in TYPE meaning — a
    /// captured `class` / `enum` / `type` / `interface` / `import =`.
    ///
    /// A SEPARATE inventory, not a projection of `names`:
    /// [`CapturedBinding`] collapses every kind into one modelability
    /// bit, so a captured `class` and a captured `const` are
    /// indistinguishable there. They are opposites in type space — the
    /// class shadows an outer type alias, the `const` is invisible to it.
    type_names: FxHashSet<Arc<str>>,
    /// The captured names an enclosing frame binds in NAMESPACE meaning
    /// — a captured `enum` / `namespace` / `import =`.
    ///
    /// A THIRD inventory, because the two type-space meanings do not
    /// nest: a captured `class N` owns `N` but not `N.B`, a captured
    /// `namespace N` owns `N.B` but not `N`. Collapsing them makes one
    /// of the two answers wrong in every frame that captures either.
    namespace_names: FxHashSet<Arc<str>>,
    /// The TYPE-PARAMETER names an ENCLOSING frame BINDS, accumulated
    /// across every enclosing frame.
    ///
    /// A FOURTH inventory, deliberately not folded into `type_names`: a
    /// type parameter is not a scope lookup at all in TYPE meaning — the
    /// composed binder environment interns it, so a captured same-named
    /// `class` does not shadow it and reporting it frame-bound is a
    /// spurious fail-closed. In NAMESPACE meaning the binder still WINS
    /// lexically but denotes no namespace, so `T.B` is unresolvable and
    /// reporting it frame-bound IS the fail-closed answer. Recording a
    /// binder as a `type_name` collapses those two opposite verdicts
    /// into one.
    binder_names: FxHashSet<Arc<str>>,
}

impl CaptureScope {
    fn lookup(&self, name: &str) -> NameBinding {
        match self.names.get(name) {
            // A captured binding is read BY NAME from the evaluator's
            // seeded snapshot: no parameter ordinal applies (ordinals
            // index the NESTED frame's own signature).
            Some(CapturedBinding::Local) => NameBinding::Captured,
            Some(CapturedBinding::Unmodeled) => NameBinding::Unmodeled,
            None => NameBinding::Free,
        }
    }
}

/// The statement/expression lowering state: the demand selection (root
/// frame only — nested function values lower ungated, their bodies are
/// beyond slice granularity), the shared leaf-lowering entry, the
/// function's parameters (for [`SliceExpr::Param`] ordinals), its
/// bare-identifier self name (for [`SliceCall::DirectSelf`]), and the
/// frame's LEXICAL AUTHORITY — the same
/// [`FunctionBodySkeleton`] the demand plan resolves against, so a
/// planned edge and a lowered read can never disagree about which slot a
/// name denotes.
struct Lowerer<'a> {
    source: &'a str,
    /// The function's own start offset — the anchor the frame's
    /// [`FunctionBodySkeleton`] (and the plan derived from it) stores
    /// every span relative to.
    ///
    /// Those artifacts are content-addressed and carry no absolute
    /// source position, so a LIVE position is rebased onto this anchor
    /// before it is compared against, or looked up in, either of them.
    anchor: u32,
    /// The demand selection gating content lowering (`None` inside a
    /// nested function value — its whole body is one selected value).
    selection: Option<&'a FlowSliceSelection>,
    params: &'a [SliceParam],
    /// This frame's OWN type-parameter names. They are TYPE-meaning
    /// binders of the frame: the evaluator's binder environment interns
    /// them, so a body answer naming one resolves to the binder and must
    /// NOT be reported as shadowed by a captured same-named `class`.
    type_param_names: &'a [Arc<str>],
    self_name: Option<&'a str>,
    /// THE lexical binding authority of this frame: the arena-free
    /// skeleton of the function body being lowered (the served
    /// function's own skeleton at the root; the nested value's own
    /// skeleton inside a function expression). Every identifier
    /// classification routes through
    /// [`FunctionBodySkeleton::bindings_of_name_in_scope`] — there is no
    /// second inventory.
    skeleton: &'a FunctionBodySkeleton,
    /// The ENCLOSING frames' bindings visible at this function value's
    /// position (empty at the root).
    captures: &'a CaptureScope,
    /// The function's control-region skeleton (the index's for a served
    /// function; computed by the same single inventory walk for a nested
    /// function value's body) — the authoritative `has_return` source for
    /// loop / labeled transparency.
    control: Arc<[FunctionControlRegion]>,
    /// The function's exact direct local call targets (from the per-file
    /// function index), keyed by call span.
    direct_calls: &'a [verter_semantic::analysis::function_program::FunctionDirectCall],
    /// The first budget edge a SELECTED leaf's expression lowering hit.
    budget_failure: Option<verter_type_expr::facts::InferenceUnavailableReason>,
}

impl Lowerer<'_> {
    /// Rebase a LIVE source span onto this frame's anchor.
    ///
    /// The two coordinate systems are different TYPES
    /// ([`FrameSpan`] vs. [`verter_span::Span`]), so this is not the only
    /// crossing by convention — it is the only crossing on this side that
    /// TYPECHECKS, and comparing a live position against a stored one
    /// without it does not compile. (The inverse crossing —
    /// [`FrameSpan::to_absolute`] — has exactly one caller, the parameter
    /// inventory that compares stored binding positions against live
    /// default-initializer offsets.)
    fn rebase(&self, span: oxc_span::Span) -> FrameSpan {
        rebase_span(self.anchor, span)
    }

    /// Whether a root content position is value-selected by the demand
    /// slice. Ungated (nested function value) frames select everything.
    fn value_span_selected(&self, span: oxc_span::Span) -> bool {
        self.selection
            .is_none_or(|selection| selection.value_span(self.rebase(span)))
    }

    /// Whether a binding slot (identified by its binding-identifier
    /// span) is value-selected by the demand slice.
    fn slot_selected(&self, span: oxc_span::Span) -> bool {
        self.selection
            .is_none_or(|selection| selection.value_slot_span(self.rebase(span)))
    }

    fn param_ordinal(&self, name: &str) -> Option<u32> {
        self.params
            .iter()
            .position(|param| param.name.as_deref() == Some(name))
            .map(|ordinal| ordinal as u32)
    }

    /// Classify one identifier occurrence through the frame's LEXICAL
    /// AUTHORITY: the skeleton resolves `name`, evaluated at `span`, to
    /// the binding(s) of the nearest enclosing region (unioned with the
    /// hoisting kinds at function scope). A name the skeleton does not
    /// bind falls through to the enclosing frames' capture scope and,
    /// failing that, is genuinely FREE.
    fn resolve_name(&self, name: &str, span: oxc_span::Span) -> NameBinding {
        let Some(name_id) = self.skeleton.name_id(name) else {
            return self.captures.lookup(name);
        };
        let region = self.skeleton.innermost_region_containing(self.rebase(span));
        let bindings = self.skeleton.bindings_of_name_in_scope(name_id, region);
        if bindings.is_empty() {
            return self.captures.lookup(name);
        }
        self.classify_bindings(name, &bindings)
    }

    /// Whether this frame binds `name` in `meaning` at `span`.
    ///
    /// The TYPE-space twin of [`Self::resolve_name`], over the SAME
    /// [`FunctionBodySkeleton`] authority through its meaning-filtered
    /// entry ([`FunctionBodySkeleton::declares_meaning_in_scope`]) — a
    /// SEPARATE region-chain walk, not a kind filter over the value
    /// lookup's answer. A local binding that declares a VALUE only is
    /// TRANSPARENT here at every hop: `const Info = 1` leaves `x as Info`
    /// naming whatever encloses it — an outer `class Info {}` of the same
    /// frame, or failing that the module type alias — so the lookup falls
    /// through to the enclosing frames' captured names exactly as a
    /// completely unbound name does.
    ///
    /// A TYPE PARAMETER is not a scope lookup at all in TYPE meaning —
    /// the composed binder environment interns it — so a same-named
    /// `class` does not shadow it and reporting it frame-bound is a
    /// spurious fail-closed. In NAMESPACE meaning the binder still WINS
    /// lexically but denotes no namespace, so `T.B` is unresolvable and
    /// reporting it frame-bound IS the fail-closed answer.
    ///
    /// FOUR binder inventories feed that rule, and they are consulted in
    /// NESTING ORDER rather than as one union, because the nearest
    /// declaration wins and a binder and a local of the same name can
    /// genuinely coexist — across frames AND within one frame:
    ///
    /// 1. `binders` — the clause the answer is lowered under, when that
    ///    clause is STRICTLY NEARER than this frame's region chain (a
    ///    NESTED signature's own type parameters, which bind inside a
    ///    signature that merely SITS in this frame). Nothing can be
    ///    nearer, so this short-circuits.
    /// 2. This frame's own lexical declarations, at the reference's
    ///    region.
    /// 3. This frame's OWN type-parameter clause — SAME level as the
    ///    region chain in step 2, and therefore BEHIND it.
    /// 4. The ENCLOSING frames', through [`CaptureScope`].
    ///
    /// Steps 2 and 3 are the reason `binders` and `type_param_names` are
    /// separate parameters rather than one union: they express two
    /// different lexical distances, and a BODY position must pass an
    /// EMPTY `binders` so this frame's clause is consulted at step 3.
    ///
    /// TS2300 constrains only a BODY-level collision of ONE frame:
    /// `function f<T>() { class T {} }` is a duplicate identifier, but
    /// `function f<T>() { { class T {}; … } }` is LEGAL and the
    /// BLOCK-scoped class WINS for everything the block encloses — as
    /// does a `class T` in a nested frame (`function f<T>() { return ()
    /// => { class T {}; … } }`), while a nearer `<T>` shadows an outer
    /// frame's `class T`. All three directions are checker-verified.
    /// [`Lowerer::capture_scope_for`] keeps the captured inventories
    /// disjoint per name, so step 4 needs no nesting order of its own.
    fn name_is_frame_bound(
        &self,
        name: &str,
        span: oxc_span::Span,
        meaning: NameMeaning,
        binders: &[Arc<str>],
    ) -> bool {
        if binders.iter().any(|binder| binder.as_ref() == name) {
            return meaning == NameMeaning::Namespace;
        }
        if let Some(name_id) = self.skeleton.name_id(name) {
            let region = self.skeleton.innermost_region_containing(self.rebase(span));
            if self
                .skeleton
                .declares_meaning_in_scope(name_id, region, meaning)
            {
                return true;
            }
        }
        if self
            .type_param_names
            .iter()
            .any(|binder| binder.as_ref() == name)
            || self.captures.binder_names.contains(name)
        {
            return meaning == NameMeaning::Namespace;
        }
        match meaning {
            NameMeaning::Type => self.captures.type_names.contains(name),
            NameMeaning::Namespace => self.captures.namespace_names.contains(name),
        }
    }

    /// THE gated constructor: lower-then-gate one answer produced at
    /// `span` inside this frame, under `binders`.
    ///
    /// Together with [`GatedType::root_signature`] this is the whole
    /// mint surface for a slice-content type, so an entrance that
    /// forgets the frame does not compile.
    fn gate(&self, ty: TypeExpr, span: oxc_span::Span, binders: &[Arc<str>]) -> GatedType {
        let shadowed = self.answer_names_frame_bound(&ty, span, binders);
        GatedType {
            ty,
            shadowed: Arc::from(shadowed.into_boxed_slice()),
        }
    }

    /// Classify one RESOLVED binding set. A set carrying any binding this
    /// content half cannot model classifies as [`NameBinding::Unmodeled`]
    /// — never as the modelable sibling and never as a free name.
    fn classify_bindings(
        &self,
        name: &str,
        bindings: &[verter_semantic::analysis::flow::SkeletonBindingId],
    ) -> NameBinding {
        let mut unmodeled = false;
        let mut nested_function = false;
        let mut modelable_local = false;
        let mut param: Option<u32> = None;
        for id in bindings {
            match self.skeleton.binding(*id).kind {
                SkeletonBindingKind::Param => {
                    // A destructured parameter has no whole-slot value
                    // carrier (`lower_params` records no name for it).
                    match (
                        self.skeleton.binding(*id).destructured,
                        self.param_ordinal(name),
                    ) {
                        (false, Some(ordinal)) => param = Some(ordinal),
                        _ => unmodeled = true,
                    }
                }
                SkeletonBindingKind::Const
                | SkeletonBindingKind::Let
                | SkeletonBindingKind::Var => {
                    // A destructuring-pattern element has no whole-slot
                    // `Binding` statement (the content lowering emits one
                    // only for a plain binding identifier).
                    if self.skeleton.binding(*id).destructured {
                        unmodeled = true;
                    } else {
                        modelable_local = true;
                    }
                }
                SkeletonBindingKind::NestedFunction => nested_function = true,
                SkeletonBindingKind::Class
                | SkeletonBindingKind::CatchParam
                | SkeletonBindingKind::Enum
                | SkeletonBindingKind::Namespace
                | SkeletonBindingKind::ImportEquals => unmodeled = true,
                // TYPE-ONLY kinds never reach a VALUE resolution:
                // `bindings_of_name_in_scope` filters them at every hop
                // (they occupy no value space). Classifying them as
                // unmodelable keeps the arm conservative if that filter
                // is ever relaxed.
                SkeletonBindingKind::TypeAlias | SkeletonBindingKind::Interface => {
                    unmodeled = true;
                }
            }
        }
        if unmodeled {
            return NameBinding::Unmodeled;
        }
        if nested_function {
            return NameBinding::NestedFunction;
        }
        if modelable_local {
            // A hoisted `var` REDECLARING a parameter shares that
            // parameter's slot: the declarator's reaching definition wins
            // from the declaration onward and the parameter rides along
            // as the evaluator's not-yet-bound fallback.
            return NameBinding::Local(param);
        }
        match param {
            Some(ordinal) => NameBinding::Param(ordinal),
            // Defensive: an empty set never reaches here (the caller
            // returns early) and every kind above is covered.
            None => NameBinding::Unmodeled,
        }
    }

    /// The capture scope one nested function value lowers under: every
    /// name the ENCLOSING frames bind at the function value's own
    /// position, classified by the enclosing frame's authority. Inner
    /// frames shadow outer ones.
    fn capture_scope_for(&self, function_span: oxc_span::Span) -> CaptureScope {
        let region = self
            .skeleton
            .innermost_region_containing(self.rebase(function_span));
        let mut names = FxHashMap::default();
        for (name, binding) in self.captures.names.iter() {
            names.insert(Arc::clone(name), *binding);
        }
        let mut type_names = self.captures.type_names.clone();
        let mut namespace_names = self.captures.namespace_names.clone();
        // This frame's OWN type parameters join every enclosing frame's
        // as BINDERS of the nested frame — a separate inventory from the
        // captured type-space names, because the nested frame's answer
        // resolves them through the composed binder environment rather
        // than through any scope lookup.
        //
        // NEAREST WINS, in BOTH directions, so this frame's own
        // contribution REMOVES the name from the opposite inventory: a
        // `<T>` here shadows an enclosing frame's `class T`, and a
        // `class T` here shadows an enclosing frame's `<T>`. TS2300
        // forbids the collision only INSIDE one frame, so the two
        // inventories are not disjoint by construction across frames —
        // they are kept disjoint per name here, which is what lets
        // `name_is_frame_bound` consult them as one unordered step.
        let mut binder_names = self.captures.binder_names.clone();
        for binder in self.type_param_names {
            binder_names.insert(Arc::clone(binder));
            type_names.remove(binder.as_ref());
            namespace_names.remove(binder.as_ref());
        }
        let mut seen: FxHashSet<verter_semantic::analysis::flow::FlowNameId> = FxHashSet::default();
        for binding in self.skeleton.bindings.iter() {
            if !seen.insert(binding.name) {
                continue;
            }
            let text = self.skeleton.name(binding.name);
            // The TYPE-space bits are resolved SEPARATELY from the value
            // classification below, and BEFORE the value lookup's
            // empty-set bail: the spaces disagree on the same region
            // chain (a `const` is a value capture that shadows no type; a
            // `class` shadows the bare type but not a qualified head), so
            // no space may gate another's answer.
            let mut declares_type_space = false;
            if self
                .skeleton
                .declares_meaning_in_scope(binding.name, region, NameMeaning::Type)
            {
                type_names.insert(Arc::from(text));
                declares_type_space = true;
            }
            if self
                .skeleton
                .declares_meaning_in_scope(binding.name, region, NameMeaning::Namespace)
            {
                namespace_names.insert(Arc::from(text));
                declares_type_space = true;
            }
            // This frame's own type-space declaration is NEARER than any
            // enclosing frame's binder of the same name. THIS frame's own
            // clause is a separate question, decided per REFERENCE by
            // [`Lowerer::name_is_frame_bound`]'s region walk rather than
            // here: a BODY-level collision is TS2300, but a BLOCK-scoped
            // one is legal and wins only inside its block, so a
            // frame-wide inventory could not express it.
            if declares_type_space {
                binder_names.remove(text);
            }
            let resolved = self
                .skeleton
                .bindings_of_name_in_scope(binding.name, region);
            if resolved.is_empty() {
                continue;
            }
            let captured = match self.classify_bindings(text, &resolved) {
                NameBinding::Param(_) | NameBinding::Local(_) | NameBinding::Captured => {
                    CapturedBinding::Local
                }
                NameBinding::Free => continue,
                NameBinding::NestedFunction | NameBinding::Unmodeled => CapturedBinding::Unmodeled,
            };
            names.insert(Arc::from(text), captured);
        }
        CaptureScope {
            names,
            type_names,
            namespace_names,
            binder_names,
        }
    }

    /// Whether the statement's control region contains a `return` of the
    /// current function — read from the control skeleton (the index's
    /// single inventory walk, or the same walk over a nested function
    /// value's body). A skeleton miss FAILS CLOSED (return-bearing →
    /// typed-Unsupported).
    fn control_has_return(&self, statement: &Statement<'_>) -> bool {
        let span = statement.span();
        self.control
            .iter()
            .find(|region| region.span == span.into())
            .is_none_or(|region| region.has_return)
    }

    /// Lower a sequential statement list into a region. Statements after a
    /// terminal path are unreachable and dropped; an unsupported construct
    /// ends the region with its marker and propagates.
    fn lower_region(&mut self, statements: &[Statement<'_>]) -> LoweredRegion {
        let mut out: Vec<SliceStatement> = Vec::new();
        let mut can_fall_through = true;
        let mut hit_unsupported = false;
        for statement in statements {
            if !can_fall_through {
                break;
            }
            match statement {
                Statement::ReturnStatement(ret) => {
                    let widening_literal = ret.argument.as_ref().is_some_and(expr_is_bare_literal);
                    let argument = ret.argument.as_ref().map(|arg| {
                        if self.value_span_selected(arg.span()) {
                            self.lower_expr(arg, ExprMode::Return)
                        } else {
                            SliceExpr::Elided
                        }
                    });
                    out.push(SliceStatement::Return {
                        argument,
                        widening_literal,
                    });
                    can_fall_through = false;
                }
                Statement::BlockStatement(block) => {
                    let child = self.lower_region(&block.body);
                    can_fall_through = child.region.can_fall_through;
                    hit_unsupported = child.hit_unsupported;
                    out.push(SliceStatement::Block(child.region));
                }
                Statement::IfStatement(if_stmt) => {
                    // The evaluator never consumes the test's value, so no
                    // test content lowers (guard narrowing is a later
                    // edge-class block on the same graph).
                    let consequent = self.lower_arm(&if_stmt.consequent);
                    let alternate = if_stmt
                        .alternate
                        .as_ref()
                        .map(|alternate| self.lower_arm(alternate));
                    can_fall_through = consequent.region.can_fall_through
                        || alternate
                            .as_ref()
                            .map(|region| region.region.can_fall_through)
                            .unwrap_or(true);
                    hit_unsupported = consequent.hit_unsupported
                        || alternate
                            .as_ref()
                            .is_some_and(|region| region.hit_unsupported);
                    out.push(SliceStatement::If {
                        consequent: Box::new(consequent.region),
                        alternate: alternate.map(|region| Box::new(region.region)),
                    });
                }
                Statement::VariableDeclaration(decl) => {
                    let kind = match decl.kind {
                        VariableDeclarationKind::Const
                        | VariableDeclarationKind::Using
                        | VariableDeclarationKind::AwaitUsing => SliceBindingKind::Const,
                        VariableDeclarationKind::Let => SliceBindingKind::Let,
                        VariableDeclarationKind::Var => SliceBindingKind::Var,
                    };
                    for declarator in &decl.declarations {
                        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                            // Destructuring declarators are not simple
                            // local reaching definitions.
                            continue;
                        };
                        // A binding OUTSIDE the slice's value-selected
                        // slot set never lowers: the elided declaration's
                        // initializer stays cold (no lowering, no
                        // resolution, no budget charge). Classification of
                        // later reads/calls is unchanged — the skeleton
                        // indexes the declaration regardless of the
                        // demand. The gate is the binding-identifier SPAN
                        // (declaration-precise), never the name — a
                        // shadowed same-named sibling the plan kept out
                        // must not lower.
                        if !self.slot_selected(id.span) {
                            continue;
                        }
                        // An ANNOTATED declarator preserves its
                        // initializer's fresh literal: the declared type
                        // governs the binding, and for a union declared
                        // type the initializer only SELECTS which
                        // declared constituents survive — a widened
                        // initializer would select none.
                        let preserve_literal =
                            kind == SliceBindingKind::Const || declarator.type_annotation.is_some();
                        let init = declarator.init.as_ref().map(|expr| {
                            self.lower_expr(expr, ExprMode::BindingInit { preserve_literal })
                        });
                        // The authored annotation is the binding's
                        // DECLARED type — it SUPPLIES a value, it does
                        // not merely suppress the initializer's
                        // widening.
                        // A declarator annotation is a BODY position: it
                        // sits IN this frame's region chain, so it takes
                        // the frame gate with NO nearer-clause binders —
                        // this frame's own clause is consulted BEHIND
                        // this frame's lexical authority, which is what
                        // lets a block-scoped local of the binder's name
                        // win. The initializer's gate cannot stand in for
                        // it — the `(Some(init), None)` arm binds the
                        // DECLARED node and skips the initializer
                        // entirely.
                        let declared = declarator.type_annotation.as_ref().map(|annotation| {
                            self.gate(
                                lower_ts_type(&annotation.type_annotation, self.source),
                                id.span,
                                &[],
                            )
                        });
                        // A WIDENING literal binding: an unannotated
                        // `const` initialized from a bare literal with no
                        // const assertion. `let` / `var` initializers
                        // already widened at `BindingInit` lowering, and
                        // an annotated `const` takes its declared type.
                        let widening_literal = kind == SliceBindingKind::Const
                            && declared.is_none()
                            && declarator.init.as_ref().is_some_and(expr_is_bare_literal);
                        out.push(SliceStatement::Binding {
                            name: Arc::from(id.name.as_str()),
                            kind,
                            init,
                            declared,
                            widening_literal,
                        });
                    }
                }
                // An expression statement's value is never consumed by the
                // evaluator; its evaluation effects ride the slice's typed
                // effect obligations, not this content tree.
                Statement::ExpressionStatement(_) => {}
                // A `throw` terminates the region path without contributing
                // a return arm.
                Statement::ThrowStatement(_) => {
                    can_fall_through = false;
                }
                Statement::DoWhileStatement(_)
                | Statement::ForInStatement(_)
                | Statement::ForOfStatement(_)
                | Statement::ForStatement(_)
                | Statement::WhileStatement(_) => {
                    // A return-free loop is fall-through TRANSPARENT only
                    // while it binds nothing that outlives it. A `var` it
                    // declares is FUNCTION-scoped, so its reaching
                    // definition escapes the loop and depends on the
                    // iteration count — which the region evaluation does
                    // not model. Fail closed through the same typed loop
                    // rail a return-bearing loop takes.
                    if self.control_has_return(statement) || declares_var(statement) {
                        out.push(SliceStatement::Unsupported(SliceUnsupported::Loop));
                        hit_unsupported = true;
                        can_fall_through = false;
                    } else {
                        out.push(SliceStatement::TransparentLoop);
                    }
                }
                Statement::LabeledStatement(labeled) => {
                    if self.control_has_return(statement) {
                        out.push(SliceStatement::Unsupported(SliceUnsupported::Labeled));
                        hit_unsupported = true;
                        can_fall_through = false;
                    } else {
                        // A return-free label is fall-through transparent,
                        // but its BODY still lowers: the label wraps an
                        // ordinary statement whose own rail decides (a
                        // block's hoisted `var`s, a loop's escaping `var`
                        // fail-close, an `if` arm's conditional binding,
                        // `switch` / `try` / `with` unsupported). Emitting
                        // a bare transparent marker instead would bypass
                        // EVERY inner rail at once.
                        let child = self.lower_arm(&labeled.body);
                        can_fall_through = child.region.can_fall_through;
                        hit_unsupported = child.hit_unsupported;
                        out.push(SliceStatement::Block(child.region));
                    }
                }
                Statement::SwitchStatement(_) => {
                    out.push(SliceStatement::Unsupported(SliceUnsupported::Switch));
                    hit_unsupported = true;
                    can_fall_through = false;
                }
                Statement::TryStatement(_) => {
                    out.push(SliceStatement::Unsupported(SliceUnsupported::Try));
                    hit_unsupported = true;
                    can_fall_through = false;
                }
                Statement::WithStatement(_) => {
                    out.push(SliceStatement::Unsupported(SliceUnsupported::With));
                    hit_unsupported = true;
                    can_fall_through = false;
                }
                Statement::BreakStatement(_) | Statement::ContinueStatement(_) => {
                    out.push(SliceStatement::Unsupported(SliceUnsupported::Jump));
                    hit_unsupported = true;
                    can_fall_through = false;
                }
                Statement::ImportDeclaration(_)
                | Statement::ExportAllDeclaration(_)
                | Statement::ExportDefaultDeclaration(_)
                | Statement::ExportNamedDeclaration(_)
                | Statement::TSExportAssignment(_)
                | Statement::TSNamespaceExportDeclaration(_) => {
                    out.push(SliceStatement::Unsupported(
                        SliceUnsupported::ModuleDeclaration,
                    ));
                    hit_unsupported = true;
                    can_fall_through = false;
                }
                // Declaration / no-op statements: transparent (no return
                // contribution, no content statement).
                Statement::DebuggerStatement(_)
                | Statement::EmptyStatement(_)
                | Statement::FunctionDeclaration(_)
                | Statement::ClassDeclaration(_)
                | Statement::TSTypeAliasDeclaration(_)
                | Statement::TSInterfaceDeclaration(_)
                | Statement::TSEnumDeclaration(_)
                | Statement::TSModuleDeclaration(_)
                | Statement::TSGlobalDeclaration(_)
                | Statement::TSImportEqualsDeclaration(_) => {}
            }
            if hit_unsupported {
                can_fall_through = false;
            }
        }
        LoweredRegion {
            region: SliceRegion {
                statements: Arc::from(out.into_boxed_slice()),
                can_fall_through,
            },
            hit_unsupported,
        }
    }

    /// Lower one `if` arm: a block arm lowers its statement list directly;
    /// any other statement is a one-statement region.
    fn lower_arm(&mut self, statement: &Statement<'_>) -> LoweredRegion {
        match statement {
            Statement::BlockStatement(block) => self.lower_region(&block.body),
            _ => self.lower_region(std::slice::from_ref(statement)),
        }
    }

    /// Lower one expression. Parameter and in-scope local identifiers
    /// become dedicated carriers; a plain string-keyed object literal
    /// lowers STRUCTURALLY (each member value is a flow expression, gated
    /// by the demand selection); a bare-identifier call to the function
    /// itself becomes the recursion hold; every other form lowers through
    /// the shared shallow-pass per-expression lowering for the position.
    fn lower_expr(&mut self, expr: &Expression<'_>, mode: ExprMode) -> SliceExpr {
        match expr {
            Expression::Identifier(identifier) => {
                let name = identifier.name.as_str();
                match self.resolve_name(name, identifier.span) {
                    NameBinding::Param(ordinal) => SliceExpr::Param { ordinal },
                    NameBinding::Local(param) => SliceExpr::Local {
                        name: Arc::from(name),
                        param,
                        captured: false,
                    },
                    NameBinding::Captured => SliceExpr::Local {
                        name: Arc::from(name),
                        param: None,
                        captured: true,
                    },
                    // A hoisted nested function declaration's own value
                    // (its callable type) is not recoverable here, and a
                    // binding the content half does not model has no
                    // whole-slot carrier: both are RESOLVED names, so
                    // the file-scope leaf would bind the wrong symbol.
                    NameBinding::NestedFunction | NameBinding::Unmodeled => {
                        SliceExpr::UnmodeledBinding
                    }
                    NameBinding::Free => self.lower_leaf(expr, mode),
                }
            }
            Expression::ParenthesizedExpression(paren) => {
                // A parenthesized wrapper is structurally transparent.
                self.lower_expr(&paren.expression, mode)
            }
            Expression::FunctionExpression(func) => {
                self.lower_nested_function(&FunctionNode::Function(func))
            }
            Expression::ArrowFunctionExpression(arrow) => {
                self.lower_nested_function(&FunctionNode::Arrow(arrow))
            }
            Expression::ObjectExpression(object) => {
                let mut members = Vec::with_capacity(object.properties.len());
                let mut structural = true;
                for prop in &object.properties {
                    let ObjectPropertyKind::ObjectProperty(p) = prop else {
                        structural = false;
                        break;
                    };
                    let key = match &p.key {
                        PropertyKey::StaticIdentifier(id) => Arc::from(id.name.as_str()),
                        PropertyKey::StringLiteral(lit) => Arc::from(lit.value.as_str()),
                        _ => {
                            structural = false;
                            break;
                        }
                    };
                    // A member value OUTSIDE the demand selection never
                    // lowers — the elided sibling rides the typed carrier
                    // (present in the member LIST so missing-member
                    // detection stays static, content-free forever).
                    if !self.value_span_selected(p.value.span()) {
                        members.push(SliceObjectMember {
                            key,
                            value: SliceExpr::Elided,
                            method_kind: match (p.method, p.kind) {
                                (false, PropertyKind::Init) => None,
                                (_, PropertyKind::Get) => {
                                    Some(verter_type_expr::ObjectMethodKind::Get)
                                }
                                (_, PropertyKind::Set) => {
                                    Some(verter_type_expr::ObjectMethodKind::Set)
                                }
                                (true, PropertyKind::Init) => {
                                    Some(verter_type_expr::ObjectMethodKind::Method)
                                }
                            },
                            spans: verter_type_expr::MemberSpans {
                                declaration: Some(p.span.into()),
                                name: Some(p.key.span().into()),
                                type_annotation: None,
                            },
                        });
                        continue;
                    }
                    // A method / accessor member with a body is a nested
                    // function value (its return evaluates inline through
                    // the same flow machinery); a method without a body
                    // keeps the whole-literal leaf lowering.
                    if p.method || !matches!(p.kind, PropertyKind::Init) {
                        let method_kind = match p.kind {
                            PropertyKind::Get => verter_type_expr::ObjectMethodKind::Get,
                            PropertyKind::Set => verter_type_expr::ObjectMethodKind::Set,
                            _ => verter_type_expr::ObjectMethodKind::Method,
                        };
                        let value = match &p.value {
                            Expression::FunctionExpression(func) => {
                                self.lower_nested_function(&FunctionNode::Function(func))
                            }
                            Expression::ArrowFunctionExpression(arrow) => {
                                self.lower_nested_function(&FunctionNode::Arrow(arrow))
                            }
                            _ => {
                                structural = false;
                                break;
                            }
                        };
                        members.push(SliceObjectMember {
                            key,
                            value,
                            method_kind: Some(method_kind),
                            spans: verter_type_expr::MemberSpans {
                                declaration: Some(p.span.into()),
                                name: Some(p.key.span().into()),
                                type_annotation: None,
                            },
                        });
                        continue;
                    }
                    // An object-literal member's fresh literal ALWAYS
                    // widens to its primitive (the member slot is
                    // mutable), in every enclosing position — tsc's
                    // object-literal property widening rule. An `as
                    // const` member keeps its literal.
                    let widen_member =
                        !verter_semantic::analysis::type_eval_build::expr_is_const_asserted(
                            &p.value,
                            self.source,
                        );
                    let value = self.lower_expr(&p.value, mode);
                    let value = match (widen_member, value) {
                        (true, SliceExpr::Type(leaf)) => SliceExpr::Type(leaf.map_ty(
                            verter_semantic::analysis::type_eval_build::widen_shallow_literal,
                        )),
                        (_, value) => value,
                    };
                    members.push(SliceObjectMember {
                        key,
                        value,
                        method_kind: None,
                        spans: verter_type_expr::MemberSpans {
                            declaration: Some(p.span.into()),
                            name: Some(p.key.span().into()),
                            type_annotation: None,
                        },
                    });
                }
                if structural {
                    SliceExpr::Object {
                        members: Arc::from(members.into_boxed_slice()),
                    }
                } else {
                    self.lower_leaf(expr, mode)
                }
            }
            Expression::CallExpression(call)
                if matches!(
                    unwrap_parenthesized(&call.callee),
                    Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)
                ) =>
            {
                // An IIFE: the call's value is the nested function's
                // evaluated return.
                let function = match unwrap_parenthesized(&call.callee) {
                    Expression::FunctionExpression(func) => {
                        self.lower_nested_function(&FunctionNode::Function(func))
                    }
                    Expression::ArrowFunctionExpression(arrow) => {
                        self.lower_nested_function(&FunctionNode::Arrow(arrow))
                    }
                    _ => unreachable!("the guard admits function values only"),
                };
                SliceExpr::Call(SliceCall::Nested(Box::new(function)), call_site(call))
            }
            Expression::CallExpression(call) => {
                if let Expression::Identifier(callee) = &call.callee {
                    let name = callee.name.as_str();
                    // ONE lexical binding authority (the frame's
                    // skeleton), then the file-level callee rails.
                    match self.resolve_name(name, callee.span) {
                        // A hoisted nested function declaration shadows
                        // every outer same-name callee; exact recovery of
                        // its own return is not implemented (fail closed).
                        NameBinding::NestedFunction => {
                            return SliceExpr::Call(SliceCall::LocalFunctionShadow, call_site(call))
                        }
                        NameBinding::Unmodeled => return SliceExpr::UnmodeledBinding,
                        // A parameter or local SHADOWS the file-level
                        // declaration: the call goes through the binding's
                        // signature, never a flow obligation edge.
                        NameBinding::Param(ordinal) => {
                            return SliceExpr::Call(
                                SliceCall::OnBinding {
                                    param: Some(ordinal),
                                    name: Arc::from(name),
                                    captured: false,
                                },
                                call_site(call),
                            )
                        }
                        NameBinding::Local(param) => {
                            return SliceExpr::Call(
                                SliceCall::OnBinding {
                                    param,
                                    name: Arc::from(name),
                                    captured: false,
                                },
                                call_site(call),
                            )
                        }
                        NameBinding::Captured => {
                            return SliceExpr::Call(
                                SliceCall::OnBinding {
                                    param: None,
                                    name: Arc::from(name),
                                    captured: true,
                                },
                                call_site(call),
                            )
                        }
                        NameBinding::Free => {}
                    }
                    // A bare-identifier call to the function itself — a
                    // direct same-slot recursion hold.
                    if Some(name) == self.self_name {
                        return SliceExpr::Call(SliceCall::DirectSelf, call_site(call));
                    }
                    // A bare-identifier callee the function index resolves
                    // EXACTLY (same-file served function position, the
                    // trailing implementation of its overload group) is a
                    // Flow obligation edge — the fixed point's mutual
                    // recursion discharges through it.
                    if let Some(direct) = self
                        .direct_calls
                        .iter()
                        .find(|direct| direct.span == call.span.into())
                    {
                        return SliceExpr::Call(
                            SliceCall::Direct(direct.target.clone()),
                            call_site(call),
                        );
                    }
                }
                // The SAME root-identifier gate the leaf path takes: a
                // non-identifier callee rooted at a frame binding
                // (`localObj.m()`) resolves in owner scope exactly like a
                // bare read would, so it is gated here too.
                match self.leaf_type(expr, mode) {
                    LeafLowering::FrameShadowedRoot => SliceExpr::UnmodeledBinding,
                    LeafLowering::Free(ty) if is_any(&ty) => SliceExpr::Any,
                    LeafLowering::Free(ty) => {
                        SliceExpr::Call(SliceCall::Symbolic(ty), call_site(call))
                    }
                    LeafLowering::FrameShadowed { ty, shadowed } => SliceExpr::FrameShadowed {
                        inner: Box::new(SliceExpr::Call(SliceCall::Symbolic(ty), call_site(call))),
                        shadowed,
                    },
                }
            }
            // A CONDITIONAL's value is the union of its branch values,
            // and each branch is lowered as a flow expression — so a call
            // in a branch rides `SliceExpr::Call` to the evaluator's one
            // call sink, exactly as the `if` / `return` twin's does.
            // Folding the whole ternary through the leaf lowering instead
            // published the callee's UNREDUCED return carrier: its own
            // binders intact, its overload group unconsulted, warm.
            Expression::ConditionalExpression(conditional) => {
                let consequent = self.lower_expr(&conditional.consequent, mode);
                let alternate = self.lower_expr(&conditional.alternate, mode);
                SliceExpr::Union {
                    arms: Arc::from(vec![consequent, alternate].into_boxed_slice()),
                }
            }
            // ── Leaf-answered forms ──────────────────────────────────
            //
            // Everything below takes the shared shallow-pass leaf
            // lowering THROUGH `lower_leaf`, whose gate refuses a leaf
            // answer that embeds an unreduced call-return carrier. So a
            // form here either contains no call in a value position, or
            // fails closed — it never publishes a callee's raw return.
            //
            // The match is EXHAUSTIVE by construction: there is no `_`
            // arm, so a new `Expression` variant does not compile until
            // someone decides which of the three dispositions it takes
            // (a structural arm above, this leaf list, or a fail-closed
            // carrier). The wildcard is what let a conditional expression
            // sit silently in the leaf list for the whole life of the
            // substrate.
            Expression::BooleanLiteral(_)
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
            | Expression::ClassExpression(_)
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
            | Expression::TSAsExpression(_)
            | Expression::TSSatisfiesExpression(_)
            | Expression::TSTypeAssertion(_)
            | Expression::TSNonNullExpression(_)
            | Expression::TSInstantiationExpression(_)
            | Expression::V8IntrinsicExpression(_)
            | Expression::ComputedMemberExpression(_)
            | Expression::StaticMemberExpression(_)
            | Expression::PrivateFieldExpression(_) => self.lower_leaf(expr, mode),
        }
    }

    /// Lower a NESTED function node (a function / arrow expression or an
    /// object-literal method) into an owned nested function value: the
    /// nested body lowers under its OWN [`FunctionBodySkeleton`] — the
    /// same lexical authority the root frame uses, built over the nested
    /// body alone — plus the CAPTURE SCOPE of every enclosing frame,
    /// resolved at this function value's own position. A name the nested
    /// frame does not bind but an enclosing frame does is a CAPTURE (read
    /// by name from the evaluator's seeded snapshot), never a file-scope
    /// leaf. Nested bodies are one selected value: content inside them is
    /// never selection-gated.
    fn lower_nested_function(&mut self, node: &FunctionNode<'_>) -> SliceExpr {
        // A NESTED function value's signature sits INSIDE this frame's
        // body, so this frame's body-local declarations ARE in scope in
        // it: every answer minted for it is gated against THIS frame at
        // the function value's own position. Its own type-parameter
        // clause binds inside its parameter list (the evaluator's nested
        // binder environment interns exactly those names).
        let type_param_names = slice_type_param_names(node);
        let scope = SignatureScope::Nested {
            gate: self,
            at: node_span(node),
            binders: &type_param_names,
        };
        // A named function EXPRESSION binds its own name inside its own
        // body: the nested skeleton carries it, so `function h() { … h … }`
        // resolves `h` to THIS frame rather than looking free and
        // falling through to an enclosing (or module-scope) `h`. It is
        // built BEFORE the signature lowers, because the nested
        // signature's own PARAMETER LIST is a shadowing inventory of that
        // signature and the skeleton is its authority.
        let nested_skeleton = match node {
            FunctionNode::Function(func) => FunctionBodySource::from_function_expression(func)
                .map(|source| build_function_body_skeleton(&source)),
            FunctionNode::Arrow(arrow) => Some(build_function_body_skeleton(
                &FunctionBodySource::from_arrow(arrow),
            )),
        };
        let Some(nested_skeleton) = nested_skeleton else {
            // A bodiless nested position has no lexical frame to lower.
            return SliceExpr::UnmodeledBinding;
        };
        let nested_anchor = node_span(node).start;
        let params = lower_params(
            node.params(),
            self.source,
            &scope,
            &nested_skeleton,
            nested_anchor,
        );
        let type_parameters = lower_slice_type_params(node, self.source, &scope);
        // A nested function value has no index entry of its own — its
        // control regions come from the SAME single inventory walk over
        // its own body, and its lexical authority is its own skeleton
        // over the same positions.
        let inventory = node
            .body()
            .map(|body| inventory_statement_list(&body.statements))
            .unwrap_or_default();
        let control: Arc<[FunctionControlRegion]> = Arc::from(inventory.control);
        let captures = self.capture_scope_for(node_span(node));
        let mut nested = Lowerer {
            source: self.source,
            anchor: nested_anchor,
            selection: None,
            params: &params,
            type_param_names: &type_param_names,
            // A nested function value is NOT the demanded flow slot, so
            // it has no same-slot recursion to hold on: its own name is
            // a binding of its skeleton (above), and the outer frame's
            // self name must never mint a `DirectSelfCall` from in here
            // — that hold would name the WRONG function.
            self_name: None,
            skeleton: &nested_skeleton,
            captures: &captures,
            control,
            direct_calls: self.direct_calls,
            budget_failure: None,
        };
        let region = if node.is_expression_body() {
            // An expression-bodied arrow's body is one synthesized
            // expression statement; it lowers to a single `return` of the
            // expression.
            let body = node.body();
            let argument = body
                .and_then(|body| body.statements.first())
                .map(|statement| match statement {
                    Statement::ExpressionStatement(expression) => (
                        nested.lower_expr(&expression.expression, ExprMode::Return),
                        expr_is_bare_literal(&expression.expression),
                    ),
                    _ => (SliceExpr::Any, false),
                });
            SliceRegion {
                statements: Arc::from(
                    argument
                        .map(|(argument, widening_literal)| SliceStatement::Return {
                            argument: Some(argument),
                            widening_literal,
                        })
                        .into_iter()
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                ),
                can_fall_through: false,
            }
        } else {
            match node.body() {
                Some(body) => nested.lower_region(&body.statements).region,
                None => SliceRegion {
                    statements: Arc::from(Vec::new().into_boxed_slice()),
                    can_fall_through: true,
                },
            }
        };
        if let Some(reason) = nested.budget_failure {
            self.budget_failure.get_or_insert(reason);
        }
        SliceExpr::NestedFunctionValue {
            params: Arc::from(params.into_boxed_slice()),
            type_parameters: Arc::from(type_parameters.into_boxed_slice()),
            can_fall_through: region.can_fall_through,
            body: region,
        }
    }

    /// Lower a leaf expression through the shared shallow-pass entry,
    /// wrapping the result. The `any` fallback surfaces as
    /// [`SliceExpr::Any`]; an unmodellable form read THROUGH a frame
    /// binding is the typed fail-closed [`SliceExpr::UnmodeledBinding`];
    /// a modelled answer naming a frame binding rides the
    /// [`SliceExpr::FrameShadowedType`] carrier.
    fn lower_leaf(&mut self, expr: &Expression<'_>, mode: ExprMode) -> SliceExpr {
        match self.leaf_type(expr, mode) {
            LeafLowering::FrameShadowedRoot => SliceExpr::UnmodeledBinding,
            LeafLowering::Free(ty) if is_any(&ty) => SliceExpr::Any,
            // THE call-carrier gate. The shallow pass answers a CALL it
            // meets with an unreduced `ReturnType<callee>` carrier —
            // honest for a declaration initializer that is re-resolved
            // later, and a foreign binder for a consumer that PUBLISHES
            // the answer. Every call form with a structural arm was
            // routed above; a call reached through any remaining form
            // has no honest leaf value, so it fails closed here rather
            // than at each form that might contain one.
            LeafLowering::Free(ty) | LeafLowering::FrameShadowed { ty, .. }
                if embeds_call_return_carrier(&ty) =>
            {
                SliceExpr::UnreducedCallValue
            }
            LeafLowering::Free(ty) => SliceExpr::Type(GatedLeaf(ty)),
            LeafLowering::FrameShadowed { ty, shadowed } => SliceExpr::FrameShadowed {
                inner: Box::new(SliceExpr::Type(GatedLeaf(ty))),
                shadowed,
            },
        }
    }

    /// THE root-identifier gate, half one: the names in the leaf
    /// lowering's ANSWER that this frame owns.
    ///
    /// The shared shallow-pass leaf lowering has no frame — it resolves
    /// every name it meets in FILE-OWNER SCOPE. So whenever its answer
    /// carries a `typeof x…` value root or a named type reference the
    /// frame BINDS, the published answer names whatever the OWNER scope
    /// has under that name (`typeof CBait.s` / `ReturnType<typeof obj.m>`
    /// bind the module-scope `CBait` / `obj`, not the local class / local
    /// object). The name set is read off the produced typed IR through
    /// the shared exhaustive walk, so it is exactly what the leaf
    /// referenced — never a re-derivation of the leaf's own traversal.
    ///
    /// `span` is the leaf expression's own position: the region the
    /// frame's authority resolves those names in.
    fn answer_names_frame_bound(
        &self,
        ty: &TypeExpr,
        span: oxc_span::Span,
        binders: &[Arc<str>],
    ) -> Vec<FrameShadowedName> {
        let names = verter_type_expr::referenced_names(ty);
        let mut shadowed: Vec<FrameShadowedName> = Vec::new();
        for name in &names.value_roots {
            if !matches!(self.resolve_name(name, span), NameBinding::Free) {
                let entry = FrameShadowedName::Value(Arc::from(name.as_str()));
                if !shadowed.contains(&entry) {
                    shadowed.push(entry);
                }
            }
        }
        for occurrence in &names.type_names {
            // TYPE space, not value space: the answer's type names name
            // TYPES, and only a type-DECLARING local shadows the owner
            // scope's. `resolve_name` here would fail closed on a plain
            // `const` / `let` / `var` / parameter / nested function that
            // never shadowed the type at all.
            //
            // The MEANING is chosen PER OCCURRENCE, not per name: the
            // same head can appear bare (`N`) and qualified (`N.B`) in
            // one answer, and the local declarations that shadow the two
            // are different sets. A single verdict for both makes one of
            // them wrong.
            let (meaning, entry) = if occurrence.qualified {
                (
                    NameMeaning::Namespace,
                    FrameShadowedName::Namespace(Arc::from(occurrence.head.as_str())),
                )
            } else {
                (
                    NameMeaning::Type,
                    FrameShadowedName::Type(Arc::from(occurrence.head.as_str())),
                )
            };
            if self.name_is_frame_bound(&occurrence.head, span, meaning, binders)
                && !shadowed.contains(&entry)
            {
                shadowed.push(entry);
            }
        }
        shadowed
    }

    /// THE root-identifier gate, half two: whether an expression the leaf
    /// could not model AT ALL (its answer is a bare `any`) nevertheless
    /// reads THROUGH a binding this frame owns.
    ///
    /// This is the other face of the same defect. `obj[k]`, `obj.#p`,
    /// `obj?.y`, `new C()`, and `` tag`…` `` name nothing in the answer
    /// because the leaf produces no answer — it returns `any`, which then
    /// publishes CLEAN and WARM for an expression whose value is a frame
    /// binding's. There is no answer to gate here and nothing an owner
    /// scope could ever supply, so this half fails closed outright.
    ///
    /// The subject is the REFERENCE CHAIN's root, not every identifier in
    /// the subtree: the chain root is the binding the unmodelled form
    /// actually reads through, while a name in a position the leaf never
    /// consumes (an assignment's target in `{ a: (x = "s") }`, a
    /// conditional test in `c ? 1 : 2`, a call argument) says nothing
    /// about the answer.
    fn unmodelled_leaf_root_is_frame_bound(&self, expr: &Expression<'_>) -> bool {
        chain_root_identifier(expr).is_some_and(|root| {
            !matches!(
                self.resolve_name(root.name.as_str(), root.span),
                NameBinding::Free
            )
        })
    }

    /// The shared shallow-pass per-expression lowering for the position
    /// (`infer_declaration_expression_type`): return arguments, `const`
    /// initializers, and annotated declarators preserve the fresh
    /// TOP-LEVEL literal; unannotated `let` / `var` initializers widen it.
    /// Structural widening (array elements, object members) is a producer
    /// rule the callee applies in every position — it is not on this axis.
    /// Budget exhaustion degrades the one expression to `any` and records
    /// the typed budget edge.
    ///
    /// Every leaf answer in the module is minted HERE, and every one of
    /// them carries the root-identifier gate's verdict, so "take the leaf
    /// value without the gate" is not expressible at any call site.
    fn leaf_type(&mut self, expr: &Expression<'_>, mode: ExprMode) -> LeafLowering {
        // A return argument PRESERVES its top-level literal: the aggregate
        // widening decision belongs to the return join, which is the only
        // place the deduplicated contributor cardinality is known.
        let policy = match mode {
            ExprMode::Return => TopLevelLiteralPolicy::Preserve,
            ExprMode::BindingInit {
                preserve_literal: true,
            } => TopLevelLiteralPolicy::Preserve,
            ExprMode::BindingInit {
                preserve_literal: false,
            } => TopLevelLiteralPolicy::Widen,
        };
        let ty =
            infer_declaration_expression_type(expr, self.source, policy).unwrap_or_else(|reason| {
                if self.budget_failure.is_none() {
                    self.budget_failure = Some(reason);
                }
                TypeExpr::Primitive(PrimitiveName::Any)
            });
        if is_any(&ty) {
            if self.unmodelled_leaf_root_is_frame_bound(expr) {
                return LeafLowering::FrameShadowedRoot;
            }
            return LeafLowering::Free(ty);
        }
        // A leaf expression is a BODY position: it sits IN this frame's
        // region chain, so no clause is NEARER than the frame's own
        // lexical authority. This frame's clause answers at its own step,
        // behind the skeleton.
        let shadowed = self.answer_names_frame_bound(&ty, expr.span(), &[]);
        if shadowed.is_empty() {
            LeafLowering::Free(ty)
        } else {
            LeafLowering::FrameShadowed {
                ty,
                shadowed: Arc::from(shadowed.into_boxed_slice()),
            }
        }
    }
}

/// The root-identifier gate's verdict on one leaf lowering.
enum LeafLowering {
    /// Every name the answer depends on is genuinely FREE in this frame:
    /// the owner-scope answer is the right one.
    Free(TypeExpr),
    /// The leaf modelled nothing (`any`) for a form read THROUGH a frame
    /// binding — no answer to carry, and no owner-scope resolution could
    /// ever be the right one. Fails closed.
    FrameShadowedRoot,
    /// The leaf modelled an answer that NAMES frame-owned bindings: the
    /// evaluator decides, against the live owner scope, whether those
    /// names would bind (fail closed) or genuinely answer nothing.
    FrameShadowed {
        ty: TypeExpr,
        shadowed: Arc<[FrameShadowedName]>,
    },
}

fn is_any(ty: &TypeExpr) -> bool {
    matches!(ty, TypeExpr::Primitive(PrimitiveName::Any))
}
