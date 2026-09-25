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
//! carrier, and an `if`-test / expression-statement position carries no
//! VALUE content at all (the evaluator never consumes their values). A
//! test DOES lower its narrowing facts ([`SliceGuard`]), and an
//! expression statement lowers the two value-neutral effects the
//! evaluator can apply in source order — a whole-binding `=` write and a
//! same-file assertion call.
//!
//! Elided and unmodeled positions still EXECUTE, so each is scanned for
//! the runtime effects that could alter what the frame later evaluates —
//! an `asserts` call with a frame-owned subject, a whole-binding write
//! the slice's effect ledger cannot see — under one fail-closed
//! discipline: a provably inert effect is certified decided-above, any
//! other flags the enclosing statement's typed `GuardNarrowing` gap, and
//! an effect-free position (a pure literal initializer) stays silent.
//!
//! Control semantics: sequential region evaluation (a terminal return or
//! throw ends the region; statements after it are unreachable and
//! dropped), an `if` whose arms both terminate cannot fall through, blocks
//! nest, transfer-inert return-free loops and return-free labeled constructs
//! are fall-through transparent, and `switch` / `try` / return-bearing labeled constructs lower their
//! clauses as regions whose return contributions the evaluator joins —
//! a `break` targeting an enclosing `switch` or labeled statement is a
//! path terminator the lowering absorbs into that construct's
//! reachability, never a function-level jump. Return-bearing loops,
//! `with`, cross-function jumps, and module-level statements stay
//! UNSUPPORTED — typed, fail-closed: the region is produced up to the
//! first [`SliceStatement::Unsupported`] marker and the marker propagates
//! to the root so the evaluator degrades the whole result.
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
//! callee) as [`SliceCall::Symbolic`]. A `new` expression is
//! [`SliceCall::Construct`] over its lowered constructor value, and a
//! tagged template is [`SliceCall::TaggedTemplate`] over its lowered tag.

use std::sync::Arc;

use oxc_ast::ast::{
    BindingPattern, Expression, FormalParameters, LogicalOperator, Program, Statement, TSType,
    UnaryOperator, VariableDeclarationKind,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::flow_completion_inventory::{
    transports_completion, CompletionConstruction, CompletionDischarge, NormalCompletion,
};
use verter_semantic::analysis::flow::flow_ir::{FlowExprRole, FlowSliceIR};
use verter_semantic::analysis::flow::{
    object_entry_descent, sequence_value_takes_await_arm, sequence_value_takes_call_rail,
    value_descent, FlowBindingRef, FrameSpan, FunctionBodySkeleton, NameMeaning,
    ObjectEntryDescent, ObjectEntryKey, ObjectEntryKind, SkeletonBindingId, SkeletonBindingKind,
    SkeletonPathSegment, ValueDescent,
};
use verter_semantic::analysis::function_program::{
    for_each_call_expression, inventory_statement_list, FunctionControlRegion, FunctionDescentStep,
    FunctionNode, FunctionProgramEntry, ResolvedFunctionNode,
};
use verter_semantic::analysis::type_eval_build::{
    embeds_call_return_carrier, expr_is_widening_nullish, infer_declaration_expression_type,
    infer_declaration_expression_type_with_nested_nullish, ExpressionInferenceCompleteness,
    NestedNullishLiterals, TopLevelLiteralPolicy,
};
use verter_type_expr::{PrimitiveName, TypeExpr};
use verter_type_expr_oxc::{lower_return_annotation, lower_ts_type};

#[path = "flow_slice_content_class.rs"]
mod class_expression;

/// The demand selection one content lowering serves: the value-selected
/// expression spans and the value-selected slot declaration spans of ONE
/// lowered flow slice. Derived from the content-free `FlowSliceIR` — the
/// plan is the sole authority for what lowers; this carrier only
/// transports the selection into the lease-only run.
#[derive(Debug, Clone)]
pub(crate) struct FlowSliceSelection {
    /// VALUE-selected expression addresses, in the frame's own coordinates.
    /// A conflicting duplicate span stays selected but has no unique site;
    /// content must never attach one site's definition to another site.
    value_sites: rustc_hash::FxHashMap<
        FrameSpan,
        Option<verter_semantic::analysis::flow::SkeletonExprSiteId>,
    >,
    /// Binding-identifier spans of the slice's VALUE-selected slots —
    /// DECLARATION-precise identity, so a shadowed same-named sibling
    /// declarator the plan kept out never lowers (name identity would
    /// re-conflate what the plan's lexical resolution separated).
    value_slot_spans: FxHashSet<FrameSpan>,
    #[cfg(test)]
    pub(crate) assignment_lookup_work: Option<Arc<std::sync::atomic::AtomicUsize>>,
}

impl FlowSliceSelection {
    /// The selection of one lowered slice.
    pub(crate) fn from_slice_ir(ir: &FlowSliceIR) -> Self {
        let mut value_sites = rustc_hash::FxHashMap::default();
        for expression in ir
            .exprs
            .iter()
            .filter(|expression| expression.role == FlowExprRole::Value)
        {
            value_sites
                .entry(expression.span)
                .and_modify(|site| {
                    if *site != Some(expression.site) {
                        *site = None;
                    }
                })
                .or_insert(Some(expression.site));
        }
        Self {
            value_sites,
            #[cfg(test)]
            assignment_lookup_work: None,
            value_slot_spans: ir
                .slots
                .iter()
                .filter(|slot| slot.value_selected)
                .map(|slot| slot.span)
                .collect(),
        }
    }

    fn value_span(&self, span: FrameSpan) -> bool {
        self.value_sites.contains_key(&span)
    }

    fn value_site(
        &self,
        span: FrameSpan,
    ) -> Option<verter_semantic::analysis::flow::SkeletonExprSiteId> {
        // This also performs the pre-existing selection filter. Only an
        // admitted value position requests definition identity; no unrelated
        // write inventory is inspected after that filter.
        let site = self.value_sites.get(&span).copied().flatten()?;
        #[cfg(test)]
        if let Some(work) = &self.assignment_lookup_work {
            work.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Some(site)
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
    pub bindings: Arc<verter_semantic::analysis::flow::FlowBindingMap>,
    pub declared_return: Option<GatedType>,
    /// The authored return's type predicate (`x is T`, `asserts x`, …),
    /// beside the `boolean` / `void` [`Self::declared_return`].
    pub declared_predicate: Option<SlicePredicate>,
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
    pub can_fall_through: NormalCompletion,
    /// What a body that contributes NO return arm and never completes
    /// normally models as. Producer-owned: it is a property of the
    /// function's authored FORM, which only this lowering can see.
    pub empty_completion: EmptyCompletion,
    /// A budget edge one SELECTED leaf's expression lowering hit (the
    /// expression itself degrades to `any`, the whole evaluation fails
    /// with the typed budget reason). Unselected content never lowers,
    /// so it can never charge this edge.
    pub budget_failure: Option<verter_type_expr::facts::InferenceUnavailableReason>,
    /// Skeleton write spans proven inert by the content-side syntactic
    /// reachability filter. The evaluator subtracts them from unapplied write
    /// effects exactly as it subtracts writes it applies in source order.
    pub inert_write_spans: FxHashSet<FrameSpan>,
    /// The authored call / construct spans this lowering DECIDED ABOVE
    /// the call: a call folded into a surviving decided leaf
    /// ([`SliceExpr::Type`] — the fabricated-value gate proves the leaf's
    /// type does not derive from any call inside it, e.g. a
    /// type-replacing `as T` / `<T>x` carrier or a form the shallow pass
    /// models without the call's return), and a call in a CONTROL
    /// position (an `if` / ternary test) ONLY when its result provably
    /// cannot control the arms' narrowing — a `new` construct, or a
    /// module-local, unexported, single-declaration same-file callee
    /// with an authored non-predicate return annotation (the provably
    /// closed callee: a script global's or an exported binding's
    /// checker-visible signature set may hold a predicate overload this
    /// file never shows). A predicate call in a test CONTROLS narrowing
    /// and is never recorded here: it takes real evaluator evidence at
    /// guard application, or its obligations stay unclaimed. Each span is
    /// a call occurrence whose type position this run decided without the
    /// call — the containment evidence the discharge-report producer
    /// accepts for a call obligation the evaluator's call sink never
    /// reached. Absolute spans: the report rebases them onto the frame
    /// anchor when pairing against the skeleton footprint.
    pub decided_above_call_spans: Vec<verter_span::Span>,
    /// The argument values of each authored call, lowered in THIS frame
    /// as whole values (every member and element of a literal argument,
    /// every read from the frame), keyed by the call's span — the values
    /// the call executor infers from and selects an overload by. A call
    /// is absent when an argument spreads or its lowering reached a side
    /// channel (a budget edge, a decided-above call, a control-test gap);
    /// the executor then reads that call's arguments from the indexed
    /// program.
    pub call_arguments: Arc<FxHashMap<verter_span::Span, Arc<[SliceExpr]>>>,
}

/// What a function body models as when it contributes no return arm and
/// its end point is unreachable — a throw-only body, or one whose last
/// reachable construct is a divergent loop.
///
/// The checker splits this purely on the function's authored FORM: a
/// function DECLARATION and a CLASS method model as `void`, while a
/// function EXPRESSION, an ARROW, and an OBJECT-LITERAL method model as
/// `never`. This is the checker's `mayReturnNever` rule, and it is the
/// only thing that distinguishes the two seeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyCompletion {
    /// A function declaration or a class method.
    Void,
    /// A function expression, an arrow, or an object-literal method.
    Never,
}

/// One formal parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceParam {
    pub binding: Option<SkeletonBindingId>,
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
    /// The modelled elements of a destructured OBJECT-pattern parameter
    /// (`{ label = "x", n }`, aliases included): identifier bindings
    /// whose value is the annotation member `key` with the default rule
    /// applied. Empty for a plain identifier parameter. Nested, computed,
    /// and rest elements are NOT modelled — a read of one keeps the
    /// fail-closed classification it has today.
    pub destructured: Arc<[SliceDestructuredElement]>,
}

/// One modelled element of a destructured object-pattern parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceDestructuredElement {
    pub binding: SkeletonBindingId,
    /// The binding name the body reads.
    pub name: Arc<str>,
    /// The annotation member whose value the element binds — equal to
    /// `name` for a shorthand element, the member's own name for an
    /// aliased one (`{ b: renamed }` binds `renamed` from member `b`).
    pub key: Arc<str>,
    /// Whether the element authored a default initializer (`= "x"`): the
    /// binding's type drops the member's `undefined` arm.
    pub has_default: bool,
}

/// One sequential statement list with its reachability result.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceRegion {
    /// The reachable statements, in source order. Statements after a
    /// terminal path (return / throw / unsupported construct) are
    /// unreachable and dropped.
    pub statements: Arc<[SliceStatement]>,
    /// Whether execution can reach past this region without a `return`.
    pub can_fall_through: NormalCompletion,
}

transports_completion!(SliceRegion => SliceRegion);
transports_completion!(SliceContent => SliceContent);
transports_completion!(SliceSwitchCase => SliceSwitchCase);

/// One statement of the slice content.
#[derive(Debug, Clone, PartialEq)]
pub enum SliceStatement {
    Gap(crate::semantic_query::FlowGap),
    /// A `return` (bare `return;` carries no argument).
    Return {
        /// The lowered return argument, when present.
        argument: Option<SliceExpr>,
        /// The argument's freshness mirror ([`SliceFreshness::Fresh`] =
        /// a bare literal expression with no const assertion; a ternary
        /// recurses per arm). tsc widens a fresh literal return only
        /// when it is the function's SOLE return contributor — a
        /// multi-contributor join keeps every literal — so the widening
        /// decision belongs to the join, not to this position; the
        /// per-arm tree additionally tells the evaluator WHICH kept
        /// constituents stay fresh on the sealed return.
        freshness: SliceFreshness,
        /// What the returned expression establishes over the function's
        /// parameters, carried only on the single return of a function
        /// the checker may infer a type predicate for
        /// ([`ReturnPredicateTest`]).
        predicate_test: Option<ReturnPredicateTest>,
    },
    /// A statement-position `yield x` in a generator body. The yielded
    /// expression lowers like a return argument (a call rides the call
    /// carrier, a literal the leaf lowering); the evaluator collects the
    /// contributions and the generator's return wrap joins them into the
    /// `Generator<Y, R, N>` yield parameter. `yield*` delegation and a
    /// yield nested inside another expression keep their existing
    /// fail-closed classification — only the statement spelling is
    /// modelled.
    Yield {
        /// The lowered yielded expression (`None` for a bare `yield;`).
        argument: Option<SliceExpr>,
        /// The argument's freshness mirror, same rule as
        /// [`SliceStatement::Return::freshness`].
        freshness: SliceFreshness,
    },
    /// An `if` statement. Each arm is its own region; the test lowers to
    /// a [`SliceGuard`] — never to value content, since the evaluator
    /// never consumes the test's value, only its narrowing facts.
    If {
        /// The narrowing facts the test establishes (its positive reading
        /// applies to the consequent, its negated reading to the
        /// alternate and to fall-through after a consequent that
        /// terminates).
        guard: SliceGuard,
        /// The consequent region.
        consequent: Box<SliceRegion>,
        /// The alternate region, when an `else` exists.
        alternate: Option<Box<SliceRegion>>,
    },
    /// A whole-binding write (`x = v`, never `x.a = v` and never a
    /// compound operator) at statement position, targeting a formal
    /// parameter or modelable same-frame local, whose right-hand side
    /// the demand slice value-selected. This is the ONE statement form
    /// through which a write re-enters evaluation: the evaluator applies
    /// it (retyping the binding in source order), so the write-effect
    /// ledger no longer has to degrade on sight of it.
    ///
    /// Every other write shape stays out of the content tree exactly as
    /// before and keeps the typed unapplied-write degradation: a
    /// projection-path write (`x.a = v`) never retypes the binding. A
    /// write in expression position lowers through
    /// [`SliceExpr::Assignment`] and is applied in evaluation order.
    Assignment {
        /// The write target (a binding root; the path is always empty for
        /// this variant — a member-path write never lowers).
        target: SliceNarrowSubject,
        definition: verter_semantic::analysis::flow::SkeletonExprSiteId,
        /// The write expression's span, in this frame's coordinates — the
        /// identity the evaluator's write-effect ledger matches against,
        /// so a lowered write and a degraded write are the same fact seen
        /// by the two halves, never two independent verdicts.
        span: FrameSpan,
        /// The lowered right-hand side.
        value: Box<SliceExpr>,
        /// The right-hand side's top-level freshness shape, aligned with
        /// `value` — the evaluator's evolving-target widening input.
        freshness: SliceFreshness,
    },
    /// A same-file assertion call at statement position
    /// (`assertStr(u);`): the callee's declared return is
    /// `asserts x is T`, so the call narrows its argument for the rest of
    /// the region. There is no syntactic guard at the use site — the
    /// narrowing fact lives entirely in the callee's signature, which the
    /// content half reads from the same parse snapshot.
    Assertion {
        /// The argument the predicate talks about.
        subject: SliceNarrowSubject,
        /// The predicate's target type, lowered through the frame gate
        /// exactly like a declarator annotation. `None` for a TARGETLESS
        /// `asserts x`: the assertion then excludes the subject's
        /// definitely-falsy arms (the checker's truthiness narrowing for
        /// an assertion signature with no type predicate).
        target: Option<GatedType>,
    },
    /// A statement call whose callee is a dotted name this half cannot
    /// settle alone (`o.m();`, `obj.run();`): the checker's
    /// `getEffectsSignature` reads the callee's explicit type, and an
    /// `asserts` signature narrows what follows while a `never` return
    /// ends the path. The evaluator reads the callee's call signatures
    /// (`SignaturesOfType`): none asserting and none returning `never`
    /// leaves the path as it is; anything else takes the typed
    /// guard-narrowing gap.
    CallEffect {
        /// The callee's type source.
        callee: SliceEffectCallee,
        /// The call expression's absolute span — the call obligation a
        /// settled callee discharges.
        call: verter_span::Span,
    },
    /// A statement call whose bare callee names a value this file does not
    /// declare as ONE closed function (an import, a declared constant, an
    /// overload group): the checker reads the call's effect from the
    /// callee's declared signatures alone. None asserting ⇒ no effect; one
    /// non-generic `asserts x is T` / `asserts x` ⇒ its argument narrows
    /// for the rest of the region; any other signature set — a declared
    /// `never` return included — takes the typed guard-narrowing gap.
    CalleeEffect {
        /// `typeof callee`, resolved in owner scope.
        callee: GatedType,
        /// Each argument's narrowable reference, positionally.
        arguments: Arc<[Option<SliceNarrowSubject>]>,
    },
    /// A nested block, as its own region.
    Block(SliceRegion),
    /// A `break` whose target an enclosing modelled construct absorbs
    /// (an anonymous break targets the innermost switch, a named one its
    /// labeled statement). The statement carries the target so the
    /// EVALUATOR captures the full layer state at the break point: the
    /// edge past the absorbing construct is that state, never the end
    /// state of the region the break happens to sit in. Statements after
    /// it in the same region are unreachable and never evaluate.
    Break {
        /// `None` for an anonymous (switch or loop) break, the label's
        /// name for a named one.
        target: Option<Arc<str>>,
    },
    /// A `continue` of an enclosing modelled loop: the evaluator captures
    /// the full layer state at this point as one of the loop's back edges.
    /// Statements after it in the same region are unreachable.
    Continue {
        /// `None` for the innermost loop, a label naming the loop
        /// otherwise.
        target: Option<Arc<str>>,
    },
    /// A loop the evaluator iterates to the checker's fixed point
    /// ([`SliceLoop`]).
    Loop(Box<SliceLoop>),
    /// Statements no path reaches that hold a `return` or a `yield` —
    /// always the last statement of its region. The checker still
    /// aggregates those contributions, its references reading their
    /// declared types there; the evaluator reads the region for them alone
    /// and nothing it does reaches the live state.
    Unreachable(Box<SliceRegion>),
    /// A compound write statement to a parameter or modelable local
    /// (`x += v;`, `i++;`): the checker's flow type after it is the
    /// base type of the literal type the target held before
    /// (`getBaseTypeOfLiteralType` of the antecedent flow type), whatever
    /// the operand is. The operand's evaluation effects take the statement
    /// scan.
    CompoundAssignment {
        target: SliceNarrowSubject,
        /// The write's span, in this frame's coordinates — the identity
        /// the evaluator's write-effect ledger matches against.
        span: FrameSpan,
        /// The write's value site, when the skeleton records one (the
        /// right-hand side of `x += v`; an update expression has none).
        definition: Option<verter_semantic::analysis::flow::SkeletonExprSiteId>,
    },
    /// A write to a MEMBER PATH of a parameter or modelable local
    /// (`o.y = v;`, `o.p.q = v;`, `a["k"] = v;`, `o.n += v;`, `o.n++;`),
    /// the path static or named by a literal key. The checker narrows
    /// the reference the write targets: after it, a read of exactly that
    /// path reads the written value reduced against the path's declared
    /// type (`getAssignmentReducedType`), a read of a longer path under
    /// it reads its declared type again, and a call never invalidates it.
    MemberWrite {
        /// The written reference (a non-empty path) — the OBJECT's
        /// reference when `key` is present.
        target: SliceNarrowSubject,
        /// The key of a computed write `o[k] = v` reading a frame binding:
        /// the written reference is `target` extended by the member or
        /// identity segment the key spells, and a key spelling none writes
        /// no reference.
        key: Option<SliceWriteKey>,
        /// The target member expression's span, in this frame's
        /// coordinates — the identity the write-effect ledger matches
        /// against (the skeleton records a member write there).
        span: FrameSpan,
        /// What the write assigns.
        write: SliceMemberWrite,
    },
    /// A destructuring assignment statement (`[a, b] = t;`, `({ x } = o);`):
    /// the right-hand side's value is written through the pattern's
    /// targets in source order.
    DestructureAssign {
        pattern: SlicePattern,
        value: SliceExpr,
        /// The right-hand side's value site — the writes' definition.
        definition: verter_semantic::analysis::flow::SkeletonExprSiteId,
    },
    /// A destructuring declarator (`const [a, b] = t`, `let { x } = o`):
    /// the parent value — the declarator's annotation, else its
    /// initializer's value — binds each element of the pattern.
    Destructure {
        pattern: SlicePattern,
        kind: SliceBindingKind,
        /// The initializer, lowered with its literals kept.
        init: Option<SliceExpr>,
        /// The declarator's annotation.
        declared: Option<GatedType>,
        /// Whether the parent value `init` is an AUTHORED type (a
        /// destructured parameter's annotation): an element's default then
        /// only removes the member's `undefined`.
        annotated: bool,
        /// Whether the pattern's elements are CORRELATED — a `const`
        /// declaration, or a parameter none of whose pattern bindings is
        /// assigned (the checker's `getNarrowedTypeOfSymbol`): a test of one
        /// element narrows the pattern's parent union, and every sibling
        /// reads its member of the narrowed parent.
        correlated: bool,
        /// The narrowable reference a `const` declarator without an
        /// annotation destructures (`const { kind } = o`): a test of one
        /// of its top-level elements narrows that reference as a test of
        /// its member does (the checker's destructured discriminant
        /// alias).
        source: Option<SliceNarrowSubject>,
    },
    /// A `throw`: terminates the region path without contributing a
    /// return arm. The marker lets the evaluator capture the state at the
    /// throw point (a `catch` clause is entered from every throw point of
    /// its try block, this one included) and stops the region path here.
    Throw,
    /// A bare call at statement position (`mayThrow();`): value-neutral —
    /// its value is never consumed and its effects ride the slice's typed
    /// effect obligations — but a call is a THROW POINT, so the marker
    /// lets the evaluator snapshot the state a `catch` / `finally` clause
    /// can be entered from.
    ThrowPoint,
    /// A `switch` statement. The discriminant lowers no VALUE content (the
    /// evaluator never consumes it) — but when it is a narrowable
    /// reference it IS carried, so each case clause's dispatch edge narrows
    /// it by the clause's test (the default clause by the negation of every
    /// test), and a discriminant whose finite union the tests cover makes
    /// the no-matching-case path dead. Each case clause's statements lower
    /// as their own region, in source order. A `break` targeting the
    /// switch ends that case's path and reaches past the switch — the
    /// lowering absorbs it into [`SliceSwitchCase::breaks`] for
    /// reachability, and the [`SliceStatement::Break`] marker carries its
    /// state to the evaluator's after-switch join. The case regions share
    /// one block scope, exactly as the authored switch body does.
    Switch {
        /// The discriminant as a narrowable subject, when it is one (a
        /// static member chain rooted at a parameter or modelable local).
        discriminant: Option<SliceNarrowSubject>,
        /// One lowered clause per case, in source order.
        cases: Arc<[SliceSwitchCase]>,
        /// Whether a `default` clause exists. Without one, the
        /// no-matching-case path reaches past the switch untouched — unless
        /// the evaluator proves the case tests exhaust the discriminant.
        has_default: bool,
    },
    /// A `try` statement. Each clause is its own region. The evaluator
    /// aggregates every authored return for inference, while an abrupt
    /// `finally` still replaces the pending control edges that would enter
    /// an enclosing `finally`.
    Try {
        /// The try block's region.
        block: Box<SliceRegion>,
        /// The catch clause, when authored.
        catch: Option<Box<SliceCatchClause>>,
        /// The finally clause's region, when authored.
        finally: Option<Box<SliceRegion>>,
        /// Whether an abrupt finally can replace a pending break from the
        /// try/catch clauses. The evaluator retains that authored exit as an
        /// implicit-undefined return-inference contributor while keeping the
        /// runtime control edge overridden.
        pending_break_contributes_undefined: bool,
        /// Named pending breaks whose crossed label is followed by a
        /// guaranteed return. Inference retains that suffix-return edge even
        /// though the abrupt finally replaces the runtime completion.
        pending_break_following_return_targets: Arc<[Arc<str>]>,
    },
    /// A labeled statement. The label is a break target for its OWN body:
    /// a `break` naming it exits to after the statement, which the lowering
    /// folds into the statement's reachability and the evaluator joins as
    /// the break's captured edge state. The name rides the statement so
    /// the evaluator drains exactly the exits that target it.
    Labeled {
        /// The label's name.
        label: Arc<str>,
        /// The body region.
        body: Box<SliceRegion>,
    },
    /// A `const` / `let` / `var` declarator with an identifier binding.
    Binding {
        binding: verter_semantic::analysis::flow::SkeletonBindingId,
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
        /// The initializer's FRESHNESS shape for an unannotated `const` —
        /// the evaluator's widening-membership input, mirroring the
        /// lowered value tree exactly as an assignment's does. An
        /// all-fresh tree (`const b = 1`, `f ? 1 : "s"`) is the classic
        /// widening-literal binding: reads widen every literal arm at
        /// return-object member positions and at the return join. A MIXED
        /// tree carries per-arm verdicts, so the evaluator widens exactly
        /// the fresh arms and keeps authored pins (`1 as const`, a call,
        /// a reference). An ANNOTATED declarator carries its initializer's
        /// shape too: the declared union's reduction keeps a fresh boolean
        /// literal fresh. An unannotated `let` / `var` (whose bare
        /// literal initializer already widened at lowering) stays
        /// `Pinned` — except one initialised to a bare `null` /
        /// `undefined` / `void` value, which carries its
        /// [`SliceFreshness::WideningNullish`] shape.
        freshness: SliceFreshness,
        /// Whether the declaration has the checker's AUTO-TYPED form: an
        /// unannotated `let` / `var` with no initializer or with a bare
        /// `null` / free `undefined` one (`isNullOrUndefined`; `void 0`
        /// and a conditional do not qualify). Under `noImplicitAny` such a
        /// variable's type follows its assignments; without it the
        /// variable is declared as its initializer's widened type.
        auto_typed_form: bool,
        /// Whether the declaration has the checker's EVOLVING-array form
        /// (`autoArrayType`, [`verter_semantic::analysis::flow::SkeletonBinding::evolving_array`]):
        /// an unannotated declarator initialised to an empty array literal.
        /// Under `noImplicitAny` its type follows the
        /// [`SliceStatement::EvolvingArray`] operations that reach each
        /// read; without it the binding is declared as the literal's type.
        evolving_array: bool,
    },
    /// A statement-position operation on an EVOLVING array
    /// ([`SliceEvolvingOperation`]).
    EvolvingArray(SliceEvolvingOperation),
    /// A return-free loop with no selected downstream transfer: fall-through
    /// transparent because no captured guard, call, write, or escaping `var`
    /// can change a later selected read. (A return-free LABELED statement is
    /// transparent too, but its body still lowers — as a labeled region, so
    /// its inner rails and its break exits keep deciding.)
    TransparentLoop,
    /// A return-free loop that is entered and never completes normally —
    /// its own exit edge is statically unreachable and no `break` targets
    /// it (`while (true) {}`, `for (;;) {}`). It contributes no return arm
    /// and, like a `throw`, ends the enclosing region's normal path, so
    /// the body contributes no implicit `undefined`.
    DivergentLoop,
    /// An unsupported construct (return-bearing loop, `with`, a
    /// `break`/`continue` jump no enclosing modelled construct absorbs, a
    /// module-level statement). The whole function is unsupported: the
    /// region is produced up to this marker and the evaluator degrades
    /// the whole result.
    Unsupported(SliceUnsupported),
}

/// The operator of one [`SliceExpr::Logical`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceLogical {
    /// `&&`: the right operand runs on the left's truthy edge.
    And,
    /// `||`: the right operand runs on the left's falsy edge.
    Or,
    /// `??`: the right operand runs on the left's nullish edge.
    Coalesce,
}

/// The operator of one [`SliceExpr::Arithmetic`], grouped by the checker's
/// result rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceArithmetic {
    /// Binary `+`: `number`, `bigint`, `string` or `any` by its operands.
    Add,
    /// Binary `-`, `*`, `/`, `%`, `**`, `<<`, `>>`, `>>>`, `&`, `|`, `^`:
    /// `number` unless an operand may be a `bigint`.
    Numeric,
    /// Unary `-` and `~`: `number`, `bigint` or `number | bigint`.
    Negate,
    /// Unary `+`: always `number`.
    Plus,
}

/// What one [`SliceStatement::MemberWrite`] assigns to its path.
#[derive(Debug, Clone, PartialEq)]
pub enum SliceMemberWrite {
    /// A plain `=` write: the right-hand side, lowered with its fresh
    /// literals preserved for the reduction against the declared type.
    Assign {
        value: Box<SliceExpr>,
        freshness: SliceFreshness,
    },
    /// A compound write (`+=`, `++`, …): the checker reduces the BASE type
    /// of the path's declared type (`getBaseTypeOfLiteralType`) by the
    /// operation's result, so a declared type whose base is not a union
    /// is exactly that base.
    Compound,
}

/// One loop, lowered for the evaluator's fixed point: the state at the
/// loop head is the join of the state entering the loop and every back
/// edge (the body's end and each `continue`, after the `for` update),
/// iterated until it stops changing, exactly as the checker's loop label
/// joins its antecedents. Every return, yield and break of the body is
/// evaluated from the converged head.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceLoop {
    /// What a `for` initializer runs once, before the first test.
    pub init: SliceRegion,
    /// Where and whether the loop tests a condition.
    pub test: SliceLoopTest,
    /// The writes every evaluation of the test applies before its
    /// condition splits (`while (n-- > 0)`), on every path through it.
    pub test_effects: SliceRegion,
    /// The element a `for…of` / `for…in` binds at every iteration.
    pub element: Option<SliceLoopElement>,
    /// The loop body.
    pub body: SliceRegion,
    /// A `for` update's effects: run after the body's end and after
    /// every `continue`, before the next test.
    pub update: SliceRegion,
    /// The labels naming the loop — what a labeled `continue` targets.
    pub labels: Arc<[Arc<str>]>,
    /// The test calls a function: every evaluation of it is a throw point.
    pub test_throws: bool,
    /// Every write inside the loop that retypes a binding — a
    /// whole-binding write, or an operation on an EVOLVING array — with
    /// the bindings its value reads: the dependencies a reference's
    /// loop-head type follows, and those an inferred binding's cycle
    /// closes through.
    pub writes: Arc<[SliceLoopWrite]>,
    /// Every binding the loop declares with an INFERRED type (no
    /// annotation, an initializer), with the bindings its initializer
    /// reads: the checker types such a binding from its initializer, and
    /// when the loop's back edges feed that initializer a value computed
    /// from the binding itself, the checker has no type for it and uses
    /// `any` (TS7022 under `noImplicitAny`).
    pub inferred: Arc<[SliceLoopDependency]>,
}

/// One binding and the bindings its value reads ([`SliceLoop::inferred`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SliceLoopDependency {
    pub binding: SkeletonBindingId,
    pub reads: Arc<[SkeletonBindingId]>,
}

/// One write a loop performs and the bindings its value reads
/// ([`SliceLoop::writes`]); a local is named by its canonical binding.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceLoopWrite {
    pub binding: verter_semantic::analysis::flow::FlowBindingRef,
    pub reads: Arc<[verter_semantic::analysis::flow::FlowBindingRef]>,
}

/// Where a loop tests its condition.
#[derive(Debug, Clone, PartialEq)]
pub enum SliceLoopTest {
    /// Before every iteration (`while`, a `for` with a test): the body
    /// is entered under the test's positive reading and the loop exits
    /// under its negated one. A literal `true` test has no exit edge, and
    /// a literal `false` one never enters the body — no path reaches it,
    /// and its returns and yields count as unreachable code's do.
    Before {
        guard: SliceGuard,
        constant: Option<bool>,
    },
    /// After every iteration (`do…while`): the first iteration is entered
    /// unconditionally, a back edge under the positive reading, and the
    /// loop exits under the negated one. A literal `true` test has no
    /// exit edge and a literal `false` one no back edge.
    After {
        guard: SliceGuard,
        constant: Option<bool>,
    },
    /// A `for` with no test: only a `break` leaves.
    Never,
    /// A `for…of` / `for…in`: the iteration may end at every head.
    Exhausted,
}

/// The element a `for…of` / `for…in` iterates.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceLoopElement {
    /// The one declared identifier the element binds at every iteration;
    /// `None` for a destructuring pattern or an existing assignment
    /// target.
    pub binding: Option<SliceLoopBinding>,
    /// A declared destructuring pattern the element binds at every
    /// iteration, with its declaration kind.
    pub pattern: Option<(SlicePattern, SliceBindingKind)>,
    /// The iterated expression, evaluated once when the loop is entered.
    pub iterable: SliceExpr,
    /// `for…in` binds the iterated object's keys, `for…of` its
    /// iterated values.
    pub keys: bool,
}

/// A destructuring pattern the evaluator binds by the checker's
/// binding-element rules (`getTypeForBindingElement`): each element reads
/// its parent's member (an object pattern's property, an array pattern's
/// position), a default replaces the member's `undefined`, a rest element
/// binds what the others leave, and a nested pattern recurses.
#[derive(Debug, Clone, PartialEq)]
pub enum SlicePattern {
    /// A binding identifier: the element's value binds here.
    Binding {
        binding: SkeletonBindingId,
        /// The binding identifier's span — the identity of the write a
        /// `for…of` element records there.
        span: FrameSpan,
    },
    /// A destructuring ASSIGNMENT's target (`[a] = t`, `({ a } = o)`): an
    /// existing parameter or modelable local the element's value is
    /// written to, reduced against its declared type as a plain `=` write.
    Target {
        target: SliceNarrowSubject,
        /// The write's span — the identity of the skeleton's write effect.
        span: FrameSpan,
    },
    /// An object pattern.
    Object {
        /// The properties in source order, keyed by the property each
        /// names.
        properties: Arc<[(SlicePatternKey, SlicePatternElement)]>,
        /// The rest element (`...rest`) and its identifier span.
        rest: Option<(SkeletonBindingId, FrameSpan)>,
    },
    /// An array pattern: `None` is a hole.
    Array {
        elements: Arc<[Option<SlicePatternElement>]>,
        /// The rest element (`...rest`) and its identifier span.
        rest: Option<(SkeletonBindingId, FrameSpan)>,
    },
}

/// The property one object-pattern property names.
#[derive(Debug, Clone, PartialEq)]
pub enum SlicePatternKey {
    /// A static name, a string or numeric literal, or a literal computed
    /// key: the property's name.
    Named(Arc<str>),
    /// Any other computed key (`{ [k]: v }`): the name is the key value's
    /// literal type.
    Computed(Box<SliceExpr>),
}

/// One element of a [`SlicePattern`]: the nested pattern and its default.
#[derive(Debug, Clone, PartialEq)]
pub struct SlicePatternElement {
    pub pattern: SlicePattern,
    /// The default initializer (`= v`), lowered with its literals kept.
    pub default: Option<Box<SliceExpr>>,
    /// Whether the default is a bare literal — a FRESH literal type.
    pub default_fresh: bool,
}

impl SlicePattern {
    /// The write spans of every assignment target the pattern writes.
    pub fn target_spans(&self) -> Vec<FrameSpan> {
        match self {
            Self::Binding { .. } => Vec::new(),
            Self::Target { span, .. } => vec![*span],
            Self::Object { properties, .. } => properties
                .iter()
                .flat_map(|(_, element)| element.pattern.target_spans())
                .collect(),
            Self::Array { elements, .. } => elements
                .iter()
                .flatten()
                .flat_map(|element| element.pattern.target_spans())
                .collect(),
        }
    }

    /// Every binding the pattern binds, with its identifier span.
    pub fn bindings(&self) -> Vec<(SkeletonBindingId, Option<FrameSpan>)> {
        let mut out = Vec::new();
        self.collect_bindings(&mut out);
        out
    }

    fn collect_bindings(&self, out: &mut Vec<(SkeletonBindingId, Option<FrameSpan>)>) {
        match self {
            Self::Binding { binding, span } => out.push((*binding, Some(*span))),
            Self::Target { .. } => {}
            Self::Object { properties, rest } => {
                for (_, element) in properties.iter() {
                    element.pattern.collect_bindings(out);
                }
                out.extend(rest.map(|(rest, span)| (rest, Some(span))));
            }
            Self::Array { elements, rest } => {
                for element in elements.iter().flatten() {
                    element.pattern.collect_bindings(out);
                }
                out.extend(rest.map(|(rest, span)| (rest, Some(span))));
            }
        }
    }
}

/// The identifier a `for…of` / `for…in` declares for its element.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceLoopBinding {
    pub binding: SkeletonBindingId,
    pub kind: SliceBindingKind,
    /// The binding identifier's span — the identity of the per-iteration
    /// write the skeleton records there.
    pub span: FrameSpan,
}

/// One `switch` case clause: its statements as a region, and whether a
/// path through the clause exits the switch via `break`.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceSwitchCase {
    /// The clause's statements. The region's `can_fall_through` means
    /// "falls into the NEXT case" here — a `break` terminates the path
    /// without setting it, exactly like a `return`.
    pub region: SliceRegion,
    /// A path through the clause exits the switch via `break` (reaching
    /// the statement after the switch) — a reachability fact about the
    /// statement AFTER the switch, which is why it shares the
    /// normal-completion carrier.
    pub breaks: NormalCompletion,
    /// What the clause's dispatch relation establishes.
    pub test: SliceSwitchTest,
}

/// What ONE `switch` clause's dispatch relation establishes.
///
/// The default clause and an unrecognized relation are DIFFERENT facts,
/// and one carrier for both is a wrong VALUE rather than a superset: the
/// default clause's dispatch edge is "the discriminant minus every
/// carried test", so routing an unrecognized relation onto it narrows
/// the clause to a set it was never proven to be reached with, and the
/// remainder that edge is computed from silently loses the unrecognized
/// clause's values.
#[derive(Debug, Clone, PartialEq)]
pub enum SliceSwitchTest {
    /// A `default` clause: no test at all. Its dispatch edge is the
    /// discriminant minus every carried test.
    Default,
    /// `case <literal>:` — the one relation this lowering carries. Its
    /// dispatch edge narrows the discriminant to the literal.
    Literal(SliceGuardLiteral),
    /// A case test whose relation this lowering cannot carry. Its
    /// dispatch edge applies NO narrow — the clause is reachable for
    /// discriminant values this half cannot enumerate — and the switch
    /// carries the typed `GuardNarrowing` gap.
    Unmodeled,
}

/// One `catch` clause: its parameter binding (a plain identifier only — a
/// destructured catch parameter binds nothing here) and its body region.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceCatchClause {
    pub binding: Option<SkeletonBindingId>,
    /// The catch parameter's annotation (`catch (e: unknown)`,
    /// `catch (e: any)`), lowered through the frame gate.
    pub declared: Option<GatedType>,
    /// The catch parameter's binding name, when authored as a plain
    /// identifier.
    pub param: Option<Arc<str>>,
    /// The clause body's region.
    pub region: SliceRegion,
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

// ---------------------------------------------------------------------------
// Guards — the narrowing facts a conditional test establishes
// ---------------------------------------------------------------------------

/// The root of a narrowable reference: a binding THIS frame owns.
///
/// A guard only ever narrows a frame-owned binding — a free (module- /
/// outer-scope) name is never a narrowable root, because the evaluator
/// cannot substitute it positionally.
#[derive(Debug, Clone)]
pub enum SliceNarrowRoot {
    /// A simple formal parameter, by ordinal in source order.
    Param {
        ordinal: u32,
        binding: verter_semantic::analysis::flow::SkeletonBindingId,
    },
    /// A modelable same-frame local (`const` / `let` / `var`), by name.
    Local {
        name: Arc<str>,
        binding: verter_semantic::analysis::flow::FlowBindingRef,
    },
}

impl PartialEq for SliceNarrowRoot {
    fn eq(&self, other: &Self) -> bool {
        use verter_semantic::analysis::flow::FlowBindingRef;
        match (self, other) {
            (Self::Param { binding: a, .. }, Self::Param { binding: b, .. }) => a == b,
            (Self::Local { binding: a, .. }, Self::Local { binding: b, .. }) => a == b,
            (
                Self::Param { binding: a, .. },
                Self::Local {
                    binding: FlowBindingRef::Local(b),
                    ..
                },
            )
            | (
                Self::Local {
                    binding: FlowBindingRef::Local(a),
                    ..
                },
                Self::Param { binding: b, .. },
            ) => a == b,
            _ => false,
        }
    }
}

impl Eq for SliceNarrowRoot {}

impl std::hash::Hash for SliceNarrowRoot {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Self::Param { binding, .. } => std::hash::Hash::hash(
                &verter_semantic::analysis::flow::FlowBindingRef::Local(*binding),
                state,
            ),
            Self::Local { binding, .. } => std::hash::Hash::hash(binding, state),
        }
    }
}

/// A narrowable reference: a binding root plus a static member path under
/// it (`u.v` carries `[v]`; the empty path is the binding itself).
///
/// Which position a fact narrows is the guard variant's call, not this
/// type's: a `typeof u.v === "string"` narrows the type AT the path,
/// while `u.kind === "a"` narrows the ROOT (the discriminant selects
/// which of the root's union arms survives).
/// One applied `asserts` call ([`SliceStatement::Assertion`]'s payload in
/// expression position).
#[derive(Debug, Clone, PartialEq)]
pub struct SliceAssertion {
    /// The argument the predicate talks about.
    pub subject: SliceNarrowSubject,
    /// The predicate's target type; `None` for a targetless `asserts x`.
    pub target: Option<GatedType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SliceNarrowSubject {
    /// The binding the reference is rooted at.
    pub root: SliceNarrowRoot,
    /// The static member segments under the root, outermost first.
    pub path: Arc<[Arc<str>]>,
}

/// The string literal of a `typeof` comparison, closed over the values
/// the operator can return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceTypeofKind {
    /// `"string"`.
    String,
    /// `"number"`.
    Number,
    /// `"bigint"`.
    BigInt,
    /// `"boolean"`.
    Boolean,
    /// `"symbol"`.
    Symbol,
    /// `"undefined"`.
    Undefined,
    /// `"object"` (including `null` — the operator's own quirk).
    Object,
    /// `"function"`.
    Function,
}

/// The literal operand of an equality guard (`u === 1`,
/// `u.kind === "a"`).
#[derive(Debug, Clone, PartialEq)]
pub enum SliceGuardLiteral {
    /// A string literal.
    String(Arc<str>),
    /// A numeric literal, as authored text (parsed evaluator-side).
    Number(Arc<str>),
    /// A boolean literal.
    Boolean(bool),
    /// `null`.
    Null,
    /// `undefined`.
    Undefined,
    /// A value named by a static member path whose root the frame leaves
    /// free (`E.A`, `NS.E.A`) — an enum member. The evaluator reads its
    /// type as a `typeof` of the path, which narrows as a literal does
    /// when it is a unit type (a literal or an enum member's literal).
    Value(Arc<[Arc<str>]>),
}

/// The other operand of a [`SliceGuard::EqReference`].
#[derive(Debug, Clone, PartialEq)]
pub enum SliceEqOther {
    /// A value read for its type: a free name, a static member path rooted
    /// at one, or a call.
    Value(Box<SliceExpr>),
    /// A reference a narrow lands on: read for its type at the test, and
    /// narrowed in turn by the subject's.
    Reference(SliceNarrowSubject),
}

/// The narrowing facts ONE conditional test establishes, lowered once and
/// shared by the ternary's branch join and the `if` statement's arms —
/// the single authority over test-expression forms, so the two control
/// spellings of one guard can never disagree about what it narrows.
///
/// This is a structural description only: it carries no evaluated types
/// (the content half has no resolver), so a consumer can never inherit a
/// narrow the evaluator did not itself compute.
///
/// [`SliceGuard::None`] means PROVED NON-NARROWING and nothing else. A
/// test form this vocabulary cannot express never reaches the evaluator
/// as a bare `None`: the lowering answers with a third disposition and
/// the construct carries the typed `GuardNarrowing` gap, so an
/// unnarrowed arm can never be published as a complete answer.
///
/// Composition is De Morgan-complete at LOWERING time (`!` flips leaf
/// `negated` flags and swaps `And`/`Or`), so the evaluator's branch
/// application only ever asks "the positive reading" or "the negated
/// reading" of one tree — there is no third combination to drift.
#[derive(Debug, Clone, PartialEq)]
pub enum SliceGuard {
    /// No narrowing derivable from the test.
    None,
    /// `typeof subject === "<kind>"` (`!==` negates).
    Typeof {
        /// The reference whose type the guard tests (the narrow lands AT
        /// this path).
        subject: SliceNarrowSubject,
        /// The compared `typeof` string.
        kind: SliceTypeofKind,
        /// Whether the comparison is negated (`!==`).
        negated: bool,
    },
    /// The subject used as a bare truthiness test (`!subject` negates).
    Truthy {
        /// The tested reference.
        subject: SliceNarrowSubject,
        /// Whether the test is negated.
        negated: bool,
    },
    /// `subject === <literal>` (`!==` negates). An EMPTY subject path is
    /// a literal-equality narrow of the binding itself; a non-empty path
    /// narrows the tested member and is a DISCRIMINANT of its parent —
    /// the narrow selects the parent's union arms by the member's type.
    EqLiteral {
        /// The compared reference.
        subject: SliceNarrowSubject,
        /// The literal operand.
        literal: SliceGuardLiteral,
        /// Whether the comparison is negated.
        negated: bool,
        /// The loose spelling (`==` / `!=`) against a literal that is not
        /// `null` or `undefined`. It narrows exactly as the strict one
        /// except that `unknown` and an empty object arm are never
        /// replaced by the literal on the positive edge — the checker's
        /// double-equals rule (a literal operand is never coerced).
        loose: bool,
    },
    /// `subject === value` (`!==` negates) against a VALUE that is not a
    /// literal. The evaluator reads the value's type and narrows the
    /// subject by it (the checker's `narrowTypeByEquality`), a member
    /// subject's parent as a discriminant, and a value that is itself a
    /// reference by the subject's type.
    EqReference {
        /// The compared reference.
        subject: SliceNarrowSubject,
        /// The other operand.
        value: SliceEqOther,
        /// Whether the comparison is negated.
        negated: bool,
        /// The loose spelling (`==` / `!=`).
        loose: bool,
    },
    /// `subject instanceof Ctor`, the constructor named by a bare
    /// identifier the frame leaves FREE that provably denotes the module's
    /// single same-file `class` declaration (resolved evaluator-side as
    /// an owner-scope type reference — that class's instance type). A
    /// right-hand side the lowering cannot prove (a frame-bound name, a
    /// namespace-owned site, a non-class value, an import, a member or
    /// call expression) lowers to [`SliceGuard::None`] behind the typed
    /// guard-narrowing gap.
    Instanceof {
        /// The tested reference (its root narrows).
        subject: SliceNarrowSubject,
        /// The constructor name.
        ctor: Arc<str>,
        /// Whether the test is negated.
        negated: bool,
    },
    /// `"key" in subject`: the root's union arms are selected by whether
    /// they carry the member.
    In {
        /// The member key.
        key: Arc<str>,
        /// The tested reference (its root narrows).
        subject: SliceNarrowSubject,
        /// Whether the test is negated.
        negated: bool,
    },
    /// `left === right` / `left == right` (`!==` / `!=` negate) between
    /// two values at least one of which is a narrowable reference and
    /// neither a form the literal and `typeof` guards carry: EACH
    /// reference narrows by the OTHER operand's type at the test (the
    /// checker's `narrowTypeByEquality`), both operand types read before
    /// either narrow applies.
    EqValue {
        /// The left operand.
        left: Box<SliceEqOperand>,
        /// The right operand.
        right: Box<SliceEqOperand>,
        /// Whether the comparison is the coercing `==` / `!=`.
        loose: bool,
        /// Whether the comparison is negated.
        negated: bool,
    },
    /// `predicate(subject)` — a module-local, unexported,
    /// single-declaration same-file function whose declared return is
    /// `x is T` (the provably closed callee: a module-scoped file, one
    /// declaration, no export spelling). The target type lowers through
    /// the frame gate exactly like a declarator annotation; a cross-file
    /// callee, a script global, an exported binding, an overloaded group,
    /// or a callee without the predicate spelling lowers to
    /// [`SliceGuard::None`].
    TypePredicate {
        /// The argument the predicate talks about.
        subject: SliceNarrowSubject,
        /// The predicate's target type.
        target: GatedType,
        /// Whether this is the predicate's negative reading.
        negated: bool,
        /// The authored predicate call's span (absolute). A predicate
        /// call CONTROLS the arms' narrowing, so it is never decided
        /// above the call: the evaluator records real call evidence at
        /// guard application against exactly this span.
        call: verter_span::Span,
    },
    /// `callee(a, b)` / `receiver.callee(a)` whose callee is not a closed
    /// same-file predicate: the evaluator resolves the call's signature
    /// (the checker's `getEffectsSignature`) and narrows the argument —
    /// or, for a `this is T` predicate, the receiver — its type
    /// predicate names, when that argument is a narrowable reference. A
    /// resolved signature with no type predicate narrows nothing.
    CallPredicate {
        /// The callee VALUE, lowered like any other expression.
        callee: Box<SliceExpr>,
        /// The authored call occurrence the executor resolves.
        site: SliceCallSite,
        /// The narrowable reference each positional argument is, if any.
        arguments: Arc<[Option<SliceNarrowSubject>]>,
        /// The narrowable reference the member call's receiver is, if any.
        receiver: Option<SliceNarrowSubject>,
        /// Whether this is the negative reading.
        negated: bool,
    },
    /// A call whose bare callee names a value this file does not declare
    /// as ONE closed function — an import, a declared constant or `let`,
    /// an overload group, a script global. The checker reads the call's
    /// narrowing from the callee's declared signatures alone: none carries
    /// a type predicate ⇒ no narrowing; one non-generic signature with
    /// `x is T` ⇒ its argument narrows to `T`. The evaluator reads those
    /// signatures through `SignaturesOfType`; any other signature set
    /// takes the typed guard-narrowing gap.
    CalleePredicate {
        /// `typeof callee`, resolved in owner scope.
        callee: GatedType,
        /// Each argument's narrowable reference, positionally.
        arguments: Arc<[Option<SliceNarrowSubject>]>,
        /// Whether this is the predicate's negative reading.
        negated: bool,
        /// The authored call's span (absolute).
        call: verter_span::Span,
    },
    /// A conjunction: every fact applies at once.
    And(Arc<[SliceGuard]>),
    /// A disjunction: the positive reading unions each disjunct's
    /// positive narrow; the negated reading applies every negation.
    Or(Arc<[SliceGuard]>),
    /// Facts over DISTINCT references read under the SAME polarity — a
    /// test of an alias narrows the alias itself and, independently, the
    /// references its initializer names (the checker narrows each
    /// reference on its own). Both readings apply every part; negation
    /// negates each part.
    Both(Arc<[SliceGuard]>),
}

/// One operand of an equality between two values ([`SliceGuard::EqValue`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SliceEqOperand {
    /// The operand's value, lowered like any other expression: the OTHER
    /// operand narrows by its type at the test.
    pub value: SliceExpr,
    /// The reference the operand is, when it is one this half narrows
    /// (always a whole binding: a member reference narrows its parent as a
    /// discriminant, which this guard does not carry).
    pub subject: Option<SliceNarrowSubject>,
}

/// How many levels of aliased conditions the checker inlines when it
/// narrows a reference through a `const` alias (`narrowType`'s
/// `inlineLevel < 5`): a test of an alias reaches through at most this
/// many alias initializers.
const ALIAS_INLINE_LIMIT: usize = 5;

/// The single returned expression of a function the checker may infer a
/// type predicate for, read as a test over the function's parameters.
///
/// The checker's rule (`getTypePredicateFromBody`): a plain (not `async`,
/// not generator) function with NO return annotation and exactly one
/// `return` statement — or an expression body — whose returned expression
/// is `boolean` infers `p is T` for the FIRST parameter `p` the expression
/// narrows to `T` on its true edge while its false edge narrows `T` itself
/// to `never`. A parameter takes part only when it is a plain identifier,
/// not a rest parameter, never assigned anywhere in the function (a nested
/// closure's write included), and not itself `boolean`; the evaluator
/// decides the last clause and the narrowing, the lowering the rest. An
/// accessor never infers one: a getter has no parameter and a setter
/// cannot return a value.
#[derive(Debug, Clone, PartialEq)]
pub enum ReturnPredicateTest {
    /// The narrowing facts the returned expression establishes
    /// ([`SliceGuard::None`] when it establishes none), and the ordinals
    /// of the parameters a predicate may name, in source order.
    Guard {
        guard: Box<SliceGuard>,
        parameters: Arc<[u32]>,
    },
    /// The returned expression narrows a parameter in a form the guard
    /// vocabulary cannot express, so the predicate the checker may infer
    /// is unknown: a `boolean` return degrades rather than publishing a
    /// predicate-less signature.
    Unexpressible,
}

fn collect_guard_subjects(guard: &SliceGuard, visitor: &mut impl FnMut(&SliceNarrowSubject)) {
    match guard {
        SliceGuard::None => {}
        SliceGuard::Typeof { subject, .. }
        | SliceGuard::Truthy { subject, .. }
        | SliceGuard::EqLiteral { subject, .. }
        | SliceGuard::EqReference {
            subject,
            value: SliceEqOther::Value(_),
            ..
        }
        | SliceGuard::Instanceof { subject, .. }
        | SliceGuard::TypePredicate { subject, .. }
        | SliceGuard::In { subject, .. } => visitor(subject),
        SliceGuard::CallPredicate {
            arguments,
            receiver,
            ..
        } => {
            for subject in arguments.iter().flatten().chain(receiver.iter()) {
                visitor(subject);
            }
        }
        SliceGuard::CalleePredicate { arguments, .. } => {
            for subject in arguments.iter().flatten() {
                visitor(subject);
            }
        }
        SliceGuard::EqValue { left, right, .. } => {
            for subject in left.subject.iter().chain(right.subject.iter()) {
                visitor(subject);
            }
        }
        SliceGuard::EqReference {
            subject,
            value: SliceEqOther::Reference(reference),
            ..
        } => {
            visitor(subject);
            visitor(reference);
        }
        SliceGuard::And(parts) | SliceGuard::Or(parts) | SliceGuard::Both(parts) => {
            for part in parts.iter() {
                collect_guard_subjects(part, visitor);
            }
        }
    }
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
pub struct GatedLeaf(TypeExpr, Option<FlowBindingRef>);

impl GatedLeaf {
    pub fn frame_root(&self) -> Option<&FlowBindingRef> {
        self.1.as_ref()
    }
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
        Self(f(self.0), self.1)
    }
}

/// One expression of the slice content.
#[derive(Debug, Clone, PartialEq)]
pub enum SliceExpr {
    /// A fully lowered leaf: literals, arrays, object literals this half
    /// cannot lower structurally (spread members in one of those ride as
    /// `ObjectMember::Spread`, for the shared object-spread projection),
    /// templates, `typeof` paths, `as` / `satisfies` / parenthesized
    /// results — the shared shallow-pass per-expression lowering, through
    /// the frame gate.
    Type(GatedLeaf),
    /// A leaf answer that names one or more bindings THIS FRAME owns —
    /// the root-identifier gate's carrier.
    ///
    /// The shared shallow-pass leaf lowering has no frame: it resolves
    /// every name in FILE OWNER SCOPE. So the leaf's `typeof CBait.s` /
    /// `ReturnType<typeof obj.m>` / `{ ...base, [k]: 1 }` answers are only
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
        binding: SkeletonBindingId,
        /// The parameter's ordinal in source order (rest last).
        ordinal: u32,
    },
    /// The value a formal parameter at `ordinal` holds on entry — the
    /// parent value of a destructured parameter's pattern.
    ParamValue {
        ordinal: u32,
    },
    /// An optional-chain value whose root is evaluated through this frame.
    /// The evaluator admits the chain as semantic `any` only when this
    /// reaching root value is still `any` at the read. The syntax gate that
    /// constructs this carrier permits member steps plus one terminal call;
    /// type-changing wrappers and interposed calls never reach it.
    OptionalAnyChain {
        root: Box<SliceExpr>,
    },
    /// A MEMBER-valued optional chain (`maybeObj?.b`, `a.b?.c`) — a typed
    /// optional member read over a NON-call base. The root rides the same
    /// carriers a bare read of it takes (substitution and narrowing
    /// included); the evaluator strips each optional link's nullish arms,
    /// projects the link through the ONE shared path walk, and unions
    /// `undefined` exactly when a strip removed arms — the checker's
    /// `T | undefined` for a nullable base, plain `T` for a non-nullable
    /// one. Every link is STATIC (a computed key or a terminal call keeps
    /// the optional-`any`-chain / fail-closed rails instead). A plain
    /// static member chain over a CALL (`f(c).a`) rides the same carrier
    /// with no optional link: its root is the call's value.
    OptionalMember {
        root: Box<SliceExpr>,
        /// The member links in evaluation order, each with its own
        /// `?.`-authored optionality.
        links: Arc<[(Arc<str>, bool)]>,
    },
    /// A local binding reference; its reaching definition is resolved by
    /// the evaluator. Covers BOTH a same-frame local and a binding an
    /// ENCLOSING frame declares (read from inside a nested function
    /// value) — the two differ only in `captured`, so every consumer that
    /// reasons about "a read of a local binding" (the widening-literal
    /// widen, the freshness classification) covers both by construction
    /// rather than by remembering to name a second carrier.
    Local {
        binding: verter_semantic::analysis::flow::FlowBindingRef,
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
    /// An object-literal return evaluated STRUCTURALLY: every entry's
    /// contributing expression is a flow expression (parameter / local
    /// references substitute). Plain string-keyed properties, method /
    /// accessor members, and SPREADS lower this way; a computed key still
    /// keeps the whole-literal leaf lowering.
    Object {
        /// The entries in source order — construction order is meaning
        /// (a later entry overrides what an earlier one provisioned).
        entries: Arc<[SliceObjectEntry]>,
        /// The literal's start offset in the defining file — what
        /// identifies it among the file's object literals when its type is
        /// recursive through its own `this`.
        offset: u32,
    },
    /// An array literal evaluated STRUCTURALLY: every element is a flow
    /// expression (parameter / local references substitute). Without a
    /// const assertion the value is `E[]`, `E` the subtype-reduced union
    /// of the element values (a fresh literal widened, a spread
    /// contributing its source's element type); under `as const` it is
    /// the readonly tuple of the element values, a spread splicing its
    /// source in.
    Array {
        /// The elements in source order.
        elements: Arc<[SliceArrayElement]>,
        /// Whether an enclosing `as const` pins the literal.
        const_asserted: bool,
    },
    /// A nested function VALUE (a function / arrow expression or an
    /// object-literal method in any expression position): its parameters
    /// and OWNED body region, lowered inline — the evaluator answers its
    /// body-derived return through the same flow evaluation, never a body
    /// scan and never a leaf fallback.
    NestedFunctionValue {
        function: verter_semantic::analysis::function_program::FunctionProgramKey,
        context: Arc<NestedFlowContext>,
        has_declared_return: bool,
        gap: Option<crate::semantic_query::FlowGap>,
        /// The captured bindings of THIS frame whose narrowing at the
        /// function's creation reaches its body: a `const`, or a parameter
        /// or `let` / `var` past its last assignment (no write after the
        /// creation, none in any nested callable) — the checker's closure
        /// extension of the control-flow container. Empty for a callable
        /// in a class property initializer, whose container stops there.
        extended_captures: Arc<[SkeletonBindingId]>,
        /// The captured EVOLVING arrays the function reads through their
        /// DECLARED type (`autoArrayType`, read as `any[]` under
        /// `noImplicitAny`): every one the checker does not extend the
        /// control-flow container for — a `const` or `var` one always
        /// (`isConstantVariable` excludes `autoArrayType`), a `let` one
        /// written after the function is created or inside a nested
        /// function. An extended capture continues from the evolving array
        /// it holds where the function is created.
        declared_evolving_captures:
            Arc<[verter_semantic::analysis::function_program::FlowBindingIdentity]>,
    },
    /// A value-position operation on an EVOLVING array
    /// ([`SliceEvolvingOperation`]): `a.push(v)` is the new length,
    /// `number`; `a[i] = v` is the assigned value.
    EvolvingArray(Box<SliceEvolvingOperation>),
    /// A `this` read inside a class declaration's member (or an arrow a
    /// member body creates): the receiver the member runs against.
    This(SliceThis),
    /// A class EXPRESSION's value — its constructor. The evaluator composes
    /// the constructor type and the instance surface from the lowered
    /// class body ([`SliceClass`]); a class form this half does not model
    /// keeps the typed [`Self::Gap`] instead.
    Class(Arc<SliceClass>),
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
    /// the way the callee was reached. So do the call's
    /// [`SliceCallArguments`]: its arguments that are themselves calls,
    /// lowered in this frame so they evaluate against its bindings.
    Call(SliceCall, SliceCallSite, SliceCallArguments),
    /// An expression the leaf lowering cannot represent (its `any`
    /// fallback), including a call with an unrepresentable callee.
    SemanticAny,
    /// An `await x` — the operand lowered through its own arm (a call
    /// operand rides the one call carrier, a binding read the binding
    /// carriers, a leaf the shared leaf lowering). The evaluator unwraps
    /// the resolved operand through the lib `Awaited` surface; an operand
    /// the substrate cannot type keeps its typed gap and degrades.
    /// A comma sequence whose operands include calls the checker enters
    /// into control flow as `asserts` calls: `before` narrows ahead of the
    /// value (the discarded operands' entered effects, in order — each an
    /// [`SliceStatement::Assertion`] or the [`SliceStatement::If`] join of
    /// a conditional's arms), `after` once the value operand — itself the
    /// assertion call — has evaluated.
    Sequence {
        before: Arc<[SliceStatement]>,
        value: Box<SliceExpr>,
        after: Option<SliceAssertion>,
    },
    Awaited {
        operand: Box<SliceExpr>,
    },
    /// `operand satisfies target` over an object or array literal: the
    /// operand lowered as the bare literal it is (every member and
    /// element carries its pre-widening view beside its widened value),
    /// and the target the evaluation CONTEXTUALLY types it by — a fresh
    /// literal keeps its literal type exactly where the target's matching
    /// position is a literal context, and an array literal in a tuple
    /// context is a tuple (the checker's `checkSatisfiesExpression`).
    Satisfies {
        operand: Box<SliceExpr>,
        target: GatedType,
    },
    /// `void operand` in VALUE position over a modeled whole-binding `=`
    /// write (`return [void (x = "s"), x]`): the operand is the
    /// [`SliceExpr::Assignment`] the evaluator applies in evaluation
    /// order, and `value` is the expression's own answer whatever the
    /// operand produced — the `undefined` a `void` expression is, or
    /// the `any` a `strictNullChecks`-off object member widens it to.
    Void {
        operand: Box<SliceExpr>,
        value: Box<SliceExpr>,
    },
    /// An arithmetic, bitwise or string-concatenating operation whose
    /// operands read this frame (`n + 1`, `-a`, `i * 2`): the operands are
    /// flow expressions, and the evaluator types the result by the
    /// checker's operator rules over their types (`number`, `bigint`,
    /// `string` or `any`).
    Arithmetic {
        operator: SliceArithmetic,
        /// The operands in source order: two for a binary operator, one
        /// for a unary one.
        operands: Arc<[SliceExpr]>,
    },
    /// An element access whose key this frame evaluates (`xs[i]`): the
    /// value is the checker's indexed access of the object's type by the
    /// key's type for an array, tuple or string object, `any` for an `any`
    /// object, and a flow gap for any other object. When the object is a
    /// narrowable reference and the key a read of a frame binding, the
    /// access is the reference [`SliceElementKey`] identifies, and reads
    /// the narrowing standing on it.
    ElementAccess {
        object: Box<SliceExpr>,
        index: Box<SliceExpr>,
        /// The object's narrowable reference, when the key reads a frame
        /// binding.
        reference: Option<SliceNarrowSubject>,
        /// The key binding's reference identity, beside `reference`.
        key: Option<SliceElementKey>,
    },
    /// A logical expression (`a && b`, `a || b`, `a ?? b`) whose operands
    /// read this frame: the right operand is evaluated only on the edge the
    /// operator selects, under the left's narrowing on that edge, the two
    /// edges join past the expression, and the value is the checker's
    /// logical result type over the operands' types.
    Logical {
        operator: SliceLogical,
        left: Box<SliceExpr>,
        right: Box<SliceExpr>,
        /// The narrowing the left operand's TRUE edge establishes (for
        /// `??`, its NULLISH edge); the other edge takes the negated
        /// reading.
        guard: SliceGuard,
        /// `Some` when the left operand is a bare `true` / `false`
        /// keyword: whether the right operand's edge is reachable at all.
        right_reachable: Option<bool>,
        /// Which operands are bare literals — FRESH literal types, which
        /// stay fresh in the result.
        fresh_operands: (bool, bool),
        /// Whether the position widens the result's fresh literals (a
        /// mutable member, element or binding slot).
        widen: bool,
    },
    /// `!operand`: `true` when the operand is definitely falsy, `false`
    /// when definitely truthy, `boolean` otherwise — a FRESH literal.
    Not {
        operand: Box<SliceExpr>,
        /// Whether the position widens the fresh result.
        widen: bool,
    },
    /// A named member read off a constructed value (`new C().p`): the
    /// object is a flow value of this frame, the member read through the
    /// shared member-read projection.
    MemberOf {
        object: Box<SliceExpr>,
        member: Arc<str>,
    },
    /// A non-null assertion over a flow expression (`a!`): the checker's
    /// non-nullable type of the operand's value.
    NonNull {
        operand: Box<SliceExpr>,
    },
    /// An update expression in VALUE position over a parameter or
    /// modelable local (`return i++`, `xs[i++]`): the value is the
    /// checker's unary numeric result over the target's current type, and
    /// the write retypes the target to the base type of the literal type
    /// it held — the expression twin of
    /// [`SliceStatement::CompoundAssignment`].
    Update {
        target: SliceNarrowSubject,
        /// The update expression's span, in this frame's coordinates —
        /// the identity the write-effect ledger matches against.
        span: FrameSpan,
    },
    Gap(crate::semantic_query::FlowGap),
    /// A read (or call) of a name the frame's lexical authority resolves
    /// to a FUNCTION-LOCAL binding this content half does not model: a
    /// destructuring-pattern element, a local `class` / `enum` /
    /// `namespace` / `import =`, or a `catch` parameter.
    ///
    /// The name is RESOLVED, not free — falling back to the shared leaf
    /// lowering would resolve it in FILE OWNER SCOPE and silently bind an
    /// unrelated module-scope (or cross-file imported) value of the same
    /// name, cleanly and warm. The evaluator fails closed POSITIONALLY
    /// instead: this slot carries the typed unresolved marker and the
    /// enclosing structure keeps every sibling it did model.
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
    ///
    /// `arms[0]` is the consequent and evaluates under the guard's
    /// POSITIVE reading, `arms[1]` the alternate under its NEGATED one.
    Union {
        /// The branch values in source order.
        arms: Arc<[SliceExpr]>,
        /// The narrowing facts the test establishes ([`SliceGuard::None`]
        /// when the test has no expressible narrowing).
        guard: SliceGuard,
    },
    /// An expression whose leaf answer EMBEDS an unreduced call-return
    /// carrier: a call reached through a form with no structural arm.
    ///
    /// The shared shallow pass has no frame and no resolver, so a CALL it
    /// meets answers as `ReturnType<callee>` with nothing instantiated —
    /// or, for a form it has no model for at all, as a fabricated `any`
    /// at the root or nested inside the structure it composed. Publishing
    /// either as this frame's value hands out the callee's own
    /// type-parameter binders (skipping its overload group) or a value
    /// indistinguishable from an authored `any`, warm. There is no honest
    /// value here, so the evaluator fails closed POSITIONALLY: this slot
    /// carries the typed unresolved marker and the enclosing structure
    /// survives.
    UnreducedCallValue,
    /// Content the demand slice did NOT select: never lowered, never
    /// evaluable. Observing an elided value is a planner/content mismatch
    /// and fails closed at the evaluator — it is never a fabricated
    /// `any` and never a silently widened sibling.
    Elided,
    /// A whole-binding `=` write at VALUE position (`{ a: (x = "s") }`,
    /// `const v = (x = 1)`), targeting a formal parameter or modelable
    /// same-frame local, whose right-hand side the demand slice
    /// value-selected — the expression twin of
    /// [`SliceStatement::Assignment`].
    ///
    /// The evaluator applies it IN EVALUATION ORDER: a read evaluated
    /// before it keeps the pre-write reaching definition, a read after it
    /// observes the write, and a DEFERRED closure read observes it too
    /// (the evaluator's capture look-ahead), because the checker's own
    /// deferred read observes every write of the enclosing frame. That
    /// evaluation-order reasoning is exactly why a bare span comparison
    /// ("drop the degradation when every read span precedes the write
    /// span") is unsound and stays rejected.
    Assignment {
        /// The write target (a binding root; the path is always empty for
        /// this variant — a member-path write never lowers).
        target: SliceNarrowSubject,
        definition: verter_semantic::analysis::flow::SkeletonExprSiteId,
        /// The write expression's span, in this frame's coordinates — the
        /// identity the evaluator's write-effect ledger matches against
        /// (recorded at the TARGET IDENTIFIER, exactly like the statement
        /// twin).
        span: FrameSpan,
        /// The lowered right-hand side.
        value: Box<SliceExpr>,
        /// The right-hand side's top-level freshness shape, aligned with
        /// the statement twin.
        freshness: SliceFreshness,
        /// Whether the expression's value sits in a mutable slot (an array
        /// element, an object member): its fresh literals widen there.
        widen: bool,
    },
}

/// The reference identity of an element access `o[k]` whose key reads a
/// frame binding `k` — the checker's `isMatchingReference` over element
/// accesses: a `const` key of one string or numeric literal type names the
/// member it spells (`const k = "k"; o[k]` is `o.k`), and a key binding no
/// write reaches (`const`, or a parameter or local never assigned) makes
/// `o[k]` one reference wherever it reads that same binding, distinct from
/// every named member (`o[k]` over `k: "k"` is not `o.k`).
#[derive(Debug, Clone, PartialEq)]
pub struct SliceElementKey {
    /// Whether the key binding is `const`.
    pub constant: bool,
    /// The key binding's identity segment (U+0000, then the frame anchor
    /// and the binding's index — no property name spells it), `None`
    /// for a key binding some write reaches: that access matches no
    /// reference at all.
    pub identity: Option<Arc<str>>,
}

/// The computed key of a member write `o[k] = v` whose key reads a frame
/// binding ([`SliceStatement::MemberWrite`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SliceWriteKey {
    /// The key's lowered read.
    pub value: Box<SliceExpr>,
    /// The reference identity it spells.
    pub key: SliceElementKey,
}

/// Where a statement call's callee type comes from
/// ([`SliceStatement::CallEffect`]).
#[derive(Debug, Clone, PartialEq)]
pub enum SliceEffectCallee {
    /// A static member path of an annotated parameter or local: the
    /// declared type the checker's `getTypeOfDottedName` reads.
    Declared(SliceNarrowSubject),
    /// Any other dotted name (a module or global binding, a nested
    /// function declaration): its value.
    Value(Box<SliceExpr>),
}

/// One class expression's body, lowered for the evaluator's class
/// composition: the constructor type is the class's construct signatures
/// (its own constructor's, else the base constructor's) over its static
/// members, and the instance type is its own members over the base
/// instance, under the class's own identity.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceClass {
    /// The class expression's start offset in the defining file — what
    /// identifies the class among every class expression of that file.
    pub offset: u32,
    /// The class's printed name: its binding identifier, else the name it
    /// is assigned to, else the checker's `(Anonymous class)`.
    pub name: Arc<str>,
    /// The type-parameter clauses enclosing the class — its OUTER type
    /// parameters, outermost first.
    pub outer_clauses: Arc<[crate::semantic_query::ClassExpressionClause]>,
    /// The class's own type-parameter clause (`class<T> { … }`).
    pub type_parameters: Arc<[SliceTypeParam]>,
    /// The `extends` clause.
    pub heritage: Option<SliceClassHeritage>,
    /// The declared constructor's visible signatures — its overload
    /// signatures, else its implementation's; `None` when the class
    /// declares no constructor (the base constructor's signatures apply).
    pub constructors: Option<Arc<[Arc<[SliceClassParam]>]>>,
    /// The accessibility the class's first constructor declares; `None`
    /// when it declares no constructor.
    pub constructor_visibility: Option<verter_type_expr::MemberVisibility>,
    /// The instance and static members in declaration order, including
    /// the constructor's parameter properties. An overloaded method is one
    /// member per visible overload signature, in order.
    pub members: Arc<[SliceClassMember]>,
    /// The declared index signatures, instance and static.
    pub index_signatures: Arc<[SliceClassIndexSignature]>,
}

/// One class expression's `extends` clause.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceClassHeritage {
    /// The base constructor value, lowered as a flow value — a parameter
    /// or a local rides its own binding carrier, a call its call carrier.
    pub base: Box<SliceExpr>,
    /// The authored `extends Base<Args>` type arguments.
    pub type_arguments: Arc<[GatedType]>,
}

/// One parameter of a class expression's declared constructor.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceClassParam {
    /// The binding name (`None` for a destructured parameter).
    pub name: Option<Arc<str>>,
    /// The annotation, else the default initializer's widened type, else
    /// `any` (an array of `any` for a rest parameter).
    pub ty: GatedType,
    /// Whether the parameter is optional (`?` or a default initializer).
    pub optional: bool,
    /// Whether this is the rest parameter.
    pub rest: bool,
}

/// One declared index signature of a class expression.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceClassIndexSignature {
    /// Whether the signature is on the constructor (`static`).
    pub is_static: bool,
    /// The key type.
    pub key: GatedType,
    /// The value type.
    pub value: GatedType,
    pub readonly: bool,
}

/// One member of a class expression.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceClassMember {
    /// The member's name: a static name, else the computed key's value
    /// (a literal or unique-symbol key names the member; any other key
    /// contributes to the class's implicit index signature).
    pub key: SliceObjectKey,
    /// Whether the member is on the constructor (`static`) rather than the
    /// instance.
    pub is_static: bool,
    pub optional: bool,
    pub readonly: bool,
    pub visibility: verter_type_expr::MemberVisibility,
    /// `Some` for a method, `None` for a property (accessors included).
    pub method_kind: Option<verter_type_expr::ObjectMethodKind>,
    pub spans: verter_type_expr::MemberSpans,
    /// The member's type.
    pub value: SliceClassMemberValue,
}

/// Where one class member's type comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum SliceClassMemberValue {
    /// An authored type: an annotation, or an overload signature composed
    /// from annotations.
    Declared(GatedType),
    /// A property initializer, evaluated in this frame over the DECLARED
    /// type of every binding it reads (a property initializer is its own
    /// flow container: no narrowing of the enclosing frame reaches it),
    /// widened unless the property is `readonly`.
    Initializer { value: Box<SliceExpr>, widen: bool },
    /// A method's nested function value: the member's type is its
    /// signature, with a body-derived return inferred by the flow lane.
    Method(Box<SliceExpr>),
    /// A getter's nested function value: the property's type is its
    /// signature's return.
    Getter(Box<SliceExpr>),
    /// A member whose type this half cannot model: it stays on the surface
    /// over the typed unresolved marker.
    Unmodeled,
}

/// The source of one mutable closure capture's authored declaration authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SliceCaptureAuthoritySource {
    /// A lexical or function-scoped local declaration.
    Local(SliceBindingKind),
    /// A formal parameter, optionally projected from an object pattern.
    Parameter {
        /// The annotated object's member key for a destructured element.
        key: Option<Arc<str>>,
        /// Whether the destructured element authored a default.
        has_default: bool,
    },
}

/// One mutable closure capture's authored declaration authority.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceCaptureAuthority {
    pub binding: verter_semantic::analysis::function_program::FlowBindingIdentity,
    pub name: Arc<str>,
    pub declared: GatedType,
    pub source: SliceCaptureAuthoritySource,
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
/// declared defaults. This substrate cannot yet do the first two, and
/// `unknown` is its recorded interim for both.
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
    /// Resolving them belongs to call-site type-argument resolution, which
    /// this substrate does not perform; until then their presence means the
    /// DECLARED DEFAULT is definitely not the answer.
    has_explicit_type_arguments: bool,
    /// The authored call expression's span — the address a call-shaped
    /// consumer re-reads the expression from the retained snapshot with
    /// (argument points and explicit type arguments are parse facts,
    /// never re-derived).
    span: verter_span::Span,
}

impl SliceCallSite {
    /// The call-site facts of one authored call expression.
    #[must_use]
    pub fn new(
        fixed_argument_count: u32,
        spreads_arguments: bool,
        has_explicit_type_arguments: bool,
        span: verter_span::Span,
    ) -> Self {
        Self {
            fixed_argument_count,
            spreads_arguments,
            has_explicit_type_arguments,
            span,
        }
    }

    /// The authored call expression's span.
    #[must_use]
    pub fn span(self) -> verter_span::Span {
        self.span
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
    authored_call_site(
        &call.arguments,
        call.type_arguments.is_some(),
        call.span.into(),
    )
}

/// The [`SliceCallSite`] of one authored `new` expression.
fn construct_site(new: &oxc_ast::ast::NewExpression<'_>) -> SliceCallSite {
    authored_call_site(
        &new.arguments,
        new.type_arguments.is_some(),
        new.span.into(),
    )
}

/// The [`SliceCallSite`] of one authored tagged template: the template
/// strings are its first argument and every substitution one more.
fn tagged_template_site(tagged: &oxc_ast::ast::TaggedTemplateExpression<'_>) -> SliceCallSite {
    SliceCallSite::new(
        u32::try_from(tagged.quasi.expressions.len() + 1).unwrap_or(u32::MAX),
        false,
        tagged.type_arguments.is_some(),
        tagged.span.into(),
    )
}

fn authored_call_site(
    arguments: &[oxc_ast::ast::Argument<'_>],
    has_explicit_type_arguments: bool,
    span: verter_span::Span,
) -> SliceCallSite {
    let fixed = arguments
        .iter()
        .take_while(|argument| !matches!(argument, oxc_ast::ast::Argument::SpreadElement(_)))
        .count();
    SliceCallSite::new(
        fixed as u32,
        fixed != arguments.len(),
        has_explicit_type_arguments,
        span,
    )
}

/// The arguments of one call that are themselves calls or member reads,
/// calling frame, by argument ordinal.
///
/// A call's arguments are otherwise parse facts its call sink re-reads
/// from the retained snapshot and types as values. An argument that is
/// itself a call is different: its callee, its own arguments and the
/// bindings they read belong to THIS frame — `id(c(x))` reads the frame's
/// `x` — so the sink evaluates it through the frame's own call carrier,
/// as the checker checks an argument expression in the scope it appears
/// in. Empty when no argument is a call. An immediately invoked function
/// and a spread element are never lowered here; the sink types them as
/// it types any other argument.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SliceCallArguments(Arc<[Option<SliceExpr>]>);

impl SliceCallArguments {
    /// A call none of whose arguments is lowered in the frame.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// The frame-lowered argument at `ordinal`, when it is lowered here.
    #[must_use]
    pub fn get(&self, ordinal: usize) -> Option<&SliceExpr> {
        self.0.get(ordinal).and_then(Option::as_ref)
    }

    /// Every frame-lowered argument, in argument order.
    pub fn iter(&self) -> impl Iterator<Item = &SliceExpr> {
        self.0.iter().flatten()
    }
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
        binding: verter_semantic::analysis::flow::FlowBindingRef,
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
    /// A `super.m()` call — the callee is a static member chain rooted at
    /// `super`. The base member resolves through the HERITAGE surface: the
    /// enclosing class's `extends` expression, lowered here as a gated
    /// value type, then projected `prototype` + the authored member path
    /// (the member path directly, for a STATIC member's base access) and
    /// called through the ONE call sink. A heritage this half cannot lower
    /// (a call, a mixin) never reaches this carrier — the call keeps the
    /// fail-closed rail.
    OnHeritage {
        /// The heritage expression's gated value type (`typeof Base`).
        heritage: GatedType,
        /// The authored member path off the base (`m` for `super.m()`).
        member: Arc<[Arc<str>]>,
        /// Whether the member declaring this frame is STATIC — its `super`
        /// reads the base constructor's own side, not its prototype.
        static_side: bool,
    },
    /// A call lowered to the symbolic `ReturnType<typeof …>` carrier.
    Symbolic(TypeExpr, Option<FlowBindingRef>),
    /// A call of a member read off a lowered receiver (`this.m()`): the
    /// evaluator projects `member` off the receiver's value and resolves
    /// the call over the member's signatures.
    Member {
        /// The receiver whose member is called.
        receiver: Box<SliceExpr>,
        /// The authored static member path off the receiver.
        member: Arc<[Arc<str>]>,
    },
    /// A `new` expression. The constructor is lowered as a flow value (a
    /// parameter or local rides its binding carrier, a free name the
    /// shared leaf lowering), and the evaluator resolves the construction
    /// through the call executor over the constructor's construct
    /// signatures — the checker's `resolveNewExpression`.
    Construct(Box<SliceExpr>),
    /// A call of a named member of a constructed value (`new C().m()`):
    /// the object is a flow value of this frame, the member its projected
    /// property, resolved at the one call sink as a method call.
    OnValue {
        object: Box<SliceExpr>,
        member: Arc<str>,
    },
    /// A tagged template: a call of its tag, lowered as a flow value like
    /// a constructor, whose arguments are the template strings and then
    /// each substitution — the checker's `resolveTaggedTemplateExpression`.
    TaggedTemplate(Box<SliceExpr>),
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
/// A function's authored type predicate: its subject and assertion flag,
/// with the target gated exactly like the return it stands beside.
#[derive(Debug, Clone, PartialEq)]
pub struct SlicePredicate {
    predicate: Arc<verter_type_expr::TypePredicate>,
    target: Option<GatedType>,
}

impl SlicePredicate {
    /// The authored predicate (subject, `asserts`, ungated target).
    #[must_use]
    pub fn predicate(&self) -> &verter_type_expr::TypePredicate {
        &self.predicate
    }

    /// The gated target; `None` for `asserts x` / `asserts this`.
    #[must_use]
    pub fn target(&self) -> Option<&GatedType> {
        self.target.as_ref()
    }
}

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
enum SignatureScope<'a> {
    /// The indexed function's OWN signature: body-local declarations are
    /// NOT in scope here, so the owner-scope answer is the right one.
    Root,
    /// A NESTED function value's signature, which sits INSIDE the
    /// enclosing frame's body and therefore sees that frame's
    /// body-locals.
    Nested {
        /// The ENCLOSING frame's lexical authority.
        gate: &'a CaptureScope,
        /// The nested function's OWN type parameters: they bind inside
        /// its signature and the evaluator's binder environment carries
        /// them, so they are the answer's binders, not references into
        /// the enclosing frame.
        binders: &'a [Arc<str>],
    },
}

/// One ENTRY of a structurally lowered object-literal return, in authored
/// order.
///
/// The two variants are the two dispositions
/// `verter_semantic::analysis::flow::object_entry_descent` assigns, which
/// is the same classification the skeleton's `open_object_site` opens
/// child sites from — so an entry this half lowers is an entry the demand
/// planner reached.
#[derive(Debug, Clone, PartialEq)]
pub enum SliceObjectEntry {
    /// An entry provisioning exactly one key.
    Member(Box<SliceObjectMember>),
    /// A SPREAD (`...source`): every key the source's value carries
    /// enters the surface at this position, and a later entry overrides
    /// what it provisioned.
    Spread {
        /// The spread source's lowered value.
        source: Box<SliceExpr>,
    },
}

/// One operation on an EVOLVING array binding
/// ([`SliceStatement::Binding::evolving_array`]) — the checker's
/// array-mutation flow nodes (a `push` / `unshift` call, an element write
/// `a[i] = v`) and the empty-array assignment that starts a new evolving
/// array.
///
/// Every other reference to the binding FINALIZES its type there (`any[]`
/// while nothing has evolved it, else the array of the evolved element
/// union): the evaluator's reaching product carries the evolving elements
/// beside the finalized array every ordinary read takes.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceEvolvingOperation {
    /// The evolving binding: a local of this frame, or an enclosing
    /// frame's binding this frame captures.
    pub binding: FlowBindingRef,
    /// The operation.
    pub kind: SliceEvolvingOperationKind,
    /// The span the skeleton records the operation's write at (the call
    /// of a `push` / `unshift`, the element of `a[i] = v`, the
    /// assignment of a reset) — the identity the unapplied-write ledger
    /// subtracts once the evaluator applies the operation.
    pub span: FrameSpan,
}

impl SliceEvolvingOperation {
    /// The operation's operand expressions, in evaluation order.
    #[must_use]
    pub fn operands(&self) -> Vec<&SliceExpr> {
        match &self.kind {
            SliceEvolvingOperationKind::Append(arguments) => {
                arguments.iter().map(|argument| &argument.value).collect()
            }
            SliceEvolvingOperationKind::ElementWrite { index, value, .. } => {
                vec![&**index, &**value]
            }
            SliceEvolvingOperationKind::Reset { value, .. } => vec![&**value],
        }
    }
}

/// The kinds of [`SliceEvolvingOperation`].
#[derive(Debug, Clone, PartialEq)]
pub enum SliceEvolvingOperationKind {
    /// `a.push(..)` / `a.unshift(..)`: each argument adds its type (a
    /// spread argument its source's element types). The call's value is
    /// the new length, `number`.
    Append(Arc<[SliceMutationArgument]>),
    /// `a[index] = value`: adds the value's type when the index is
    /// number-like. The expression's value is the assigned value.
    ElementWrite {
        /// The index expression.
        index: Box<SliceExpr>,
        /// The assigned value.
        value: Box<SliceExpr>,
        /// The assigned value's freshness mirror (a bare `null` is the
        /// widening nullable type).
        freshness: SliceFreshness,
    },
    /// `a = []`: the binding holds a new evolving array with no elements
    /// (not through parentheses — `a = ([])` assigns `never[]` on 7.0.2,
    /// an ordinary [`SliceStatement::Assignment`]). The expression's value
    /// is the empty literal's.
    Reset {
        /// The written value's site (the reaching definition).
        definition: verter_semantic::analysis::flow::SkeletonExprSiteId,
        /// The write's span — the identity the unapplied-write ledger
        /// subtracts, exactly like an applied assignment's.
        span: FrameSpan,
        /// The lowered empty literal.
        value: Box<SliceExpr>,
    },
}

/// One argument of an [`SliceEvolvingOperationKind::Append`].
#[derive(Debug, Clone, PartialEq)]
pub struct SliceMutationArgument {
    /// The lowered argument (a spread's source).
    pub value: SliceExpr,
    /// Whether the argument is a spread (`...xs`).
    pub spread: bool,
    /// The argument's freshness mirror (a bare `null` is the widening
    /// nullable type).
    pub freshness: SliceFreshness,
}

/// One element of a structurally lowered array literal.
#[derive(Debug, Clone, PartialEq)]
pub enum SliceArrayElement {
    /// An element value with its freshness mirror.
    Value {
        /// The lowered element value.
        value: SliceExpr,
        /// The element expression's top-level freshness shape.
        freshness: SliceFreshness,
        /// The element's value before the mutable-slot widening of its
        /// fresh literals, when that widening changed it: the view a
        /// contextual type selects when it is a literal context.
        pre_widening: Option<Box<SliceExpr>>,
    },
    /// A spread (`...source`): the source's elements enter here.
    Spread {
        /// The spread source's lowered value.
        source: SliceExpr,
    },
    /// A hole (`[a, , b]`).
    Elision,
}

/// How one structurally lowered object-literal member NAMES its key.
///
/// A key spelling whose property name is not the authored text —
/// `{ [k]: 1 }`, `{ 1: 2 }` — is not a reason to abandon the structural
/// lowering of the WHOLE literal. Doing that folds every sibling,
/// spreads included, into one shallow-pass leaf answer, and a leaf answer
/// over a CALL-sourced spread embeds the callee's unreduced
/// `ReturnType<…>` carrier — which the leaf's fabricated-value gate
/// refuses, failing the whole return closed for a value the checker types
/// without difficulty (`{ ...base(), [k]: 1 }` is `{ label: string;
/// z: number }`).
///
/// So a non-static key becomes its own lowered VALUE position instead.
/// The evaluator resolves it exactly as far as it resolves any other
/// value: to a literal, which names the key, or to something else, which
/// fails the literal closed — the same verdict the whole-literal fallback
/// reached, now without taking the siblings down with it.
#[derive(Debug, Clone, PartialEq)]
pub enum SliceObjectKey {
    /// A statically-known key: an identifier or string-literal spelling,
    /// whose authored text IS the property name.
    ///
    /// Note for readers reaching for this in a comparison: a
    /// [`Self::Computed`] key MAY name the same property, and only its
    /// VALUE says so. A `matches!(key, Static(n) if n == wanted)` test is
    /// therefore "this member definitely names `wanted`", never "no
    /// member does".
    Static(Arc<str>),
    /// A key whose property name is the VALUE of an expression — a
    /// computed key (`[k]`) or a numeric-literal key (`1`), whose
    /// authored text is not its name.
    Computed {
        /// The key expression, lowered as an ordinary value position.
        /// Its evaluated LITERAL (or `unique symbol` carrier) names the
        /// property; any other property-key type is late-bound.
        value: Box<SliceExpr>,
    },
}

impl SliceObjectKey {
    /// The statically-known property name, when there is one.
    #[must_use]
    pub fn static_name(&self) -> Option<&str> {
        match self {
            Self::Static(name) => Some(name.as_ref()),
            Self::Computed { .. } => None,
        }
    }
}

/// The member-literal policy an enclosing TYPE CARRIER imposes on an
/// object literal it wraps.
///
/// A carrier over an object literal does not change the literal's SHAPE —
/// it changes how each member's fresh literal is published. Carrying that
/// as a policy is what lets the carrier keep the structural lowering
/// instead of folding the whole literal (spreads included) into one leaf
/// answer.
///
/// The three states mirror the shared shallow pass's own object-literal
/// widening contexts exactly, because they answer the same question about
/// the same literal — a carrier that lowers structurally here and one
/// that reaches the leaf lowering must not disagree about whether
/// `{ mode: "dark" }` keeps its literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectMemberPolicy {
    /// The bare-literal rule: every fresh member literal widens to its
    /// primitive, because the member slot is mutable. A per-member `as
    /// const` still pins that one member.
    Widen,
    /// Under an enclosing `as const`: every member keeps its literal AND
    /// is `readonly`.
    ConstAssert,
}

impl ObjectMemberPolicy {
    /// Whether a fresh member literal widens to its primitive.
    const fn widens_member_literals(self) -> bool {
        matches!(self, Self::Widen)
    }

    /// Whether members are `readonly`.
    const fn readonly(self) -> bool {
        matches!(self, Self::ConstAssert)
    }
}

/// The member policy a TYPE CARRIER over an object literal imposes, or
/// `None` when the carrier's type is genuinely its own rather than its
/// operand's.
///
/// Only `x as const` qualifies here: it pins every member. `x satisfies T`
/// over a literal keeps the operand's own lowering and its target
/// ([`SliceExpr::Satisfies`]). A non-const `as T` / `<T>x` REPLACES the
/// type with `T`, a `!` non-null assertion and a `<T>`-instantiation say
/// nothing about members — every one of those keeps the whole-carrier leaf
/// lowering, where the carrier's own answer is the honest one.
fn member_literal_policy(expression: &Expression<'_>, source: &str) -> Option<ObjectMemberPolicy> {
    match expression {
        Expression::ParenthesizedExpression(paren) => {
            member_literal_policy(&paren.expression, source)
        }
        // The SHARED const-assertion authority decides, so `as const`
        // and a non-const `as T` are never told apart twice.
        Expression::TSAsExpression(_) => {
            verter_semantic::analysis::type_eval_build::expr_is_const_asserted(expression, source)
                .then_some(ObjectMemberPolicy::ConstAssert)
        }
        _ => None,
    }
}

/// The `satisfies` carrier an expression is, through
/// parentheses.
fn satisfies_target<'a, 'ast>(
    expression: &'a Expression<'ast>,
) -> Option<&'a oxc_ast::ast::TSSatisfiesExpression<'ast>> {
    match expression {
        Expression::ParenthesizedExpression(paren) => satisfies_target(&paren.expression),
        Expression::TSSatisfiesExpression(satisfies) => Some(satisfies),
        _ => None,
    }
}

/// One member of a structurally lowered object-literal return.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceObjectMember {
    /// How the member names its key.
    pub key: SliceObjectKey,
    /// The member value.
    pub value: SliceExpr,
    /// The member's pre-widening value, when ordinary mutable-property
    /// widening changed it. Assignment reduction uses this contextual fresh
    /// view to select declared union constituents; ordinary object evaluation
    /// continues to use `value`.
    pub assignment_value: Option<SliceExpr>,
    /// The value a bare `null` / `undefined` / `void` member holds before
    /// the literal's widening turns it into `any` (`strictNullChecks`
    /// off): a join of several values compares them unwidened, as the
    /// checker's subtype reduction runs before its widening.
    pub unwidened: Option<SliceExpr>,
    /// The authored method / accessor kind (`None` for a plain property).
    pub method_kind: Option<verter_type_expr::ObjectMethodKind>,
    /// Whether the member is `readonly` — true exactly under an enclosing
    /// `as const`, which is the only object-literal form that mints one.
    pub readonly: bool,
    /// The authored member spans (declaration / name) — they keep two
    /// same-shaped return objects at distinct source sites distinct at
    /// interning.
    pub spans: verter_type_expr::MemberSpans,
    /// Whether an accessor declares its type: a getter's return
    /// annotation, a setter's parameter annotation. `false` for every
    /// other member.
    pub accessor_annotated: bool,
}

/// The unsupported-construct classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceUnsupported {
    /// A return-bearing loop.
    Loop,
    /// A `break` / `continue` jump of the current function's statement
    /// list that no enclosing modelled construct absorbs.
    Jump,
    /// A directly invoked closure statement whose captured flow effects are
    /// selected but not modelled by the sequential evaluator.
    InvokedClosureEffect,
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
    scope: &SignatureScope<'_>,
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
    scope: &SignatureScope<'_>,
) -> Vec<SliceTypeParam> {
    lower_type_param_clause(node.type_parameters(), source, scope)
}

impl SignatureScope<'_> {
    /// Gate one signature-position answer for this scope.
    fn gate(&self, ty: TypeExpr, binders: &[Arc<str>]) -> GatedType {
        match self {
            SignatureScope::Root => GatedType::root_signature(ty),
            SignatureScope::Nested { gate, .. } => gate.gate(ty, binders),
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
        parameters: &rustc_hash::FxHashMap<Arc<str>, u32>,
    ) -> GatedType {
        let mut gated = match self {
            SignatureScope::Root => GatedType::root_signature(ty),
            SignatureScope::Nested { gate, .. } => {
                let mut gated = gate.gate(ty, binders);
                if let Some(root) = chain_root_identifier(initializer) {
                    if !matches!(gate.lookup(root.name.as_str()), NameBinding::Free) {
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
                .get(root.name.as_str())
                .is_some_and(|end| *end <= limit)
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
    resolved: ResolvedFunctionNode<'_>,
    source: &str,
) -> Vec<SliceTypeParam> {
    lower_slice_type_params(&resolved.node, source, &SignatureScope::Root)
}

/// Classify the function's authored form for the empty-completion seed.
///
/// An arrow is always `never`-seeded. A `function` node is `void`-seeded
/// when it is a declaration, and otherwise only when it is a CLASS member —
/// the locator's final descent step places a served class member, and the
/// index marks a class expression's member nested in a body. An
/// object-literal method and a plain function expression share the
/// declaration's OXC node type and are separated only by that position.
fn empty_completion_of(node: &FunctionNode<'_>, entry: &FunctionProgramEntry) -> EmptyCompletion {
    use verter_semantic::analysis::function_program::FunctionDescentStep;
    let FunctionNode::Function(function) = node else {
        return EmptyCompletion::Never;
    };
    if function.r#type == oxc_ast::ast::FunctionType::FunctionDeclaration || entry.class_member {
        return EmptyCompletion::Void;
    }
    match entry.locator.descent.last() {
        Some(FunctionDescentStep::ClassMember { .. }) => EmptyCompletion::Void,
        _ => EmptyCompletion::Never,
    }
}

/// One function borrowed from its exact retained parse address table.
pub(crate) struct FlowSliceSource<'a> {
    pub program: &'a Program<'a>,
    pub resolved: ResolvedFunctionNode<'a>,
}

/// One namespace block enclosing a namespace-owned function, and the value
/// names a free read in the function's body finds there before the file's
/// top level.
#[derive(Debug, Default)]
struct NamespaceBlockScope {
    /// The block's qualified name (`N.Inner`).
    qualified: String,
    /// Every value name the block itself declares, and the names its
    /// same-name sibling blocks export, each with whether it is exported
    /// (and so a qualified declaration of its own).
    names: rustc_hash::FxHashMap<String, bool>,
}

/// The namespace declaration a statement is — `namespace N { … }` or
/// `export namespace N { … }` — with its block body.
fn namespace_block_of<'s, 'a>(
    statement: &'s Statement<'a>,
) -> Option<(&'s str, &'s oxc_ast::ast::TSModuleBlock<'a>)> {
    let module = match statement {
        Statement::TSModuleDeclaration(module) => module,
        Statement::ExportNamedDeclaration(export) => match export.declaration.as_ref()? {
            oxc_ast::ast::Declaration::TSModuleDeclaration(module) => module,
            _ => return None,
        },
        _ => return None,
    };
    let oxc_ast::ast::TSModuleDeclarationName::Identifier(id) = &module.id else {
        return None;
    };
    match module.body.as_ref()? {
        oxc_ast::ast::TSModuleDeclarationBody::TSModuleBlock(block) => {
            Some((id.name.as_str(), block))
        }
        oxc_ast::ast::TSModuleDeclarationBody::TSModuleDeclaration(_) => None,
    }
}

/// The value names one namespace-block statement declares, and whether it
/// exports them.
fn namespace_statement_value_names<'s>(statement: &'s Statement<'_>) -> (Vec<&'s str>, bool) {
    use oxc_ast::ast::Declaration;
    fn declaration_names<'s>(declaration: &'s Declaration<'_>) -> Vec<&'s str> {
        match declaration {
            Declaration::VariableDeclaration(variables) => variables
                .declarations
                .iter()
                .filter_map(|declarator| match &declarator.id {
                    BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
                    _ => None,
                })
                .collect(),
            Declaration::FunctionDeclaration(function) => {
                function.id.iter().map(|id| id.name.as_str()).collect()
            }
            Declaration::ClassDeclaration(class) => {
                class.id.iter().map(|id| id.name.as_str()).collect()
            }
            Declaration::TSEnumDeclaration(declaration) => vec![declaration.id.name.as_str()],
            Declaration::TSModuleDeclaration(module) => match &module.id {
                oxc_ast::ast::TSModuleDeclarationName::Identifier(id) => vec![id.name.as_str()],
                oxc_ast::ast::TSModuleDeclarationName::StringLiteral(_) => Vec::new(),
            },
            Declaration::TSImportEqualsDeclaration(declaration) => {
                vec![declaration.id.name.as_str()]
            }
            _ => Vec::new(),
        }
    }
    match statement {
        Statement::ExportNamedDeclaration(export) => (
            export
                .declaration
                .as_ref()
                .map(declaration_names)
                .unwrap_or_default(),
            true,
        ),
        other => (
            other
                .as_declaration()
                .map(declaration_names)
                .unwrap_or_default(),
            false,
        ),
    }
}

/// The namespace blocks enclosing the function `descent` reaches from
/// `statement`, outermost first: at each level the block the descent
/// enters, with the names it declares and the names every same-name sibling
/// block at that level exports (a namespace declared in several blocks is
/// one namespace whose EXPORTED members every block sees).
fn enclosing_namespace_scopes(
    program: &Program<'_>,
    contributor: usize,
    descent: &[FunctionDescentStep],
) -> Vec<NamespaceBlockScope> {
    let mut scopes = Vec::new();
    let Some(mut statement) = program.body.get(contributor) else {
        return scopes;
    };
    let mut siblings: &[Statement<'_>] = &program.body;
    let mut parent_siblings: Vec<&oxc_ast::ast::TSModuleBlock<'_>> = Vec::new();
    let mut qualified = String::new();
    for step in descent {
        let FunctionDescentStep::NamespaceMember { statement_ordinal } = step else {
            break;
        };
        let Some((name, block)) = namespace_block_of(statement) else {
            break;
        };
        if !qualified.is_empty() {
            qualified.push('.');
        }
        qualified.push_str(name);
        let mut scope = NamespaceBlockScope {
            qualified: qualified.clone(),
            names: rustc_hash::FxHashMap::default(),
        };
        for inner in &block.body {
            let (names, exported) = namespace_statement_value_names(inner);
            for declared in names {
                let entry = scope.names.entry(declared.to_string()).or_insert(false);
                *entry |= exported;
            }
        }
        // Same-name sibling blocks at this level: the top level's own
        // statements, or every same-name sibling of the enclosing block.
        let sibling_statements: Vec<&Statement<'_>> = if parent_siblings.is_empty() {
            siblings.iter().collect()
        } else {
            parent_siblings
                .iter()
                .flat_map(|sibling| sibling.body.iter())
                .collect()
        };
        let mut same_name_blocks = Vec::new();
        for sibling in sibling_statements {
            let Some((sibling_name, sibling_block)) = namespace_block_of(sibling) else {
                continue;
            };
            if sibling_name != name {
                continue;
            }
            same_name_blocks.push(sibling_block);
            if std::ptr::eq(sibling_block, block) {
                continue;
            }
            for inner in &sibling_block.body {
                let (names, exported) = namespace_statement_value_names(inner);
                if exported {
                    for declared in names {
                        scope.names.insert(declared.to_string(), true);
                    }
                }
            }
        }
        scopes.push(scope);
        parent_siblings = same_name_blocks;
        siblings = &[];
        let Some(next) = block.body.get(*statement_ordinal as usize) else {
            break;
        };
        statement = next;
    }
    scopes
}

/// Build the slice content for one indexed function entry against the
/// retained parse snapshot, lowering ONLY `selection`-selected expression
/// content. Runs inside the memo's lease-only job: pure, owned output, no
/// host re-entry. Returns `None` on any locator miss (a typed miss, never
/// a panic).
pub(crate) fn build_flow_slice_content(
    retained: FlowSliceSource<'_>,
    source: &str,
    index: &verter_semantic::analysis::function_program::FunctionProgramIndex,
    entry: &FunctionProgramEntry,
    selection: Option<&FlowSliceSelection>,
    skeleton: &Arc<FunctionBodySkeleton>,
    bindings: Arc<verter_semantic::analysis::flow::FlowBindingMap>,
    carrier_module: bool,
    snapshot: &crate::decl_lowering::SnapshotKey,
    context: Option<&NestedFlowContext>,
    nullability: crate::semantic_query::NullabilityPolicy,
) -> Option<SliceContent> {
    let FlowSliceSource { program, resolved } = retained;
    let module_scope = carrier_module || program_has_module_syntax(program);
    // Whether the served function is NAMESPACE-OWNED: its locator descends
    // through a `namespace` / `module` block. Every call site in its body
    // (nested function values included) then resolves a bare callee
    // through that block's scope BEFORE the top level — the same lexical
    // rule under which the function index binds a namespace-qualified
    // direct-call target over the file-global one.
    let namespace_owned = entry
        .locator
        .descent
        .iter()
        .any(|step| matches!(step, FunctionDescentStep::NamespaceMember { .. }));
    let namespace_scopes = if namespace_owned {
        enclosing_namespace_scopes(
            program,
            entry.locator.contributor.contributor_index as usize,
            &entry.locator.descent,
        )
    } else {
        Vec::new()
    };
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
    let root_captures = CaptureScope::default();
    let captures = context
        .map(|context| &context.captures)
        .unwrap_or(&root_captures);
    let signature_scope = match context {
        Some(_) => SignatureScope::Nested {
            gate: captures,
            binders: &type_param_names,
        },
        None => SignatureScope::Root,
    };
    let (declared_return, declared_predicate) = match node.return_type() {
        Some(annotation) => {
            let (returned, predicate) =
                lower_return_annotation(&annotation.type_annotation, source);
            let gate = |ty: TypeExpr| signature_scope.gate(ty, signature_scope.param_binders());
            let declared_predicate = predicate.map(|predicate| SlicePredicate {
                target: predicate.ty.as_deref().cloned().map(gate),
                predicate,
            });
            (Some(gate(returned)), declared_predicate)
        }
        None => (None, None),
    };
    let anchor = node_span(&node).start;
    let params = match lower_params(
        node.params(),
        source,
        &signature_scope,
        skeleton,
        &bindings,
        anchor,
        nullability,
    ) {
        Ok(params) => params,
        Err(reason) => {
            return Some(SliceContent {
                bindings,
                declared_return,
                declared_predicate,
                can_fall_through: NormalCompletion::minted(
                    false,
                    CompletionConstruction::SynthesizedRegion,
                ),
                empty_completion: empty_completion_of(&node, entry),
                params: Arc::from(Vec::new().into_boxed_slice()),
                type_parameters: Arc::from(Vec::new().into_boxed_slice()),
                enclosing_type_parameters: Arc::from(Vec::new().into_boxed_slice()),
                body: SliceRegion {
                    statements: Arc::from(Vec::new().into_boxed_slice()),
                    can_fall_through: NormalCompletion::minted(
                        false,
                        CompletionConstruction::SynthesizedRegion,
                    ),
                },
                budget_failure: Some(reason),
                inert_write_spans: FxHashSet::default(),
                decided_above_call_spans: Vec::new(),
                call_arguments: Arc::default(),
            });
        }
    };
    let type_parameters = lower_slice_type_params(&node, source, &signature_scope);
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
    let frame_gate =
        Arc::new(DefiningFrameGate {
            skeleton: Arc::clone(skeleton),
            bindings: Arc::clone(&bindings),
            type_parameters: Arc::from(type_param_names.clone()),
            enclosing_type_parameters: enclosing_type_parameters
                .iter()
                .map(|param| Arc::clone(&param.name))
                .collect(),
            parameters: params
                .iter()
                .enumerate()
                .flat_map(|(ordinal, param)| {
                    param
                        .binding
                        .map(|binding| CaptureParameterLocator {
                            binding,
                            ordinal,
                            key: None,
                            has_default: false,
                        })
                        .into_iter()
                        .chain(param.destructured.iter().map(move |element| {
                            CaptureParameterLocator {
                                binding: element.binding,
                                ordinal,
                                key: Some(Arc::clone(&element.key)),
                                has_default: element.has_default,
                            }
                        }))
                })
                .map(|parameter| (parameter.binding, parameter))
                .collect::<rustc_hash::FxHashMap<_, _>>()
                .into(),
            parameter_names: Arc::new(signature_parameter_bindings(skeleton, anchor)),
            modelled_patterns: Arc::new(modelled_pattern_bindings(
                &node.params().items,
                &body.statements,
                &bindings,
                anchor,
            )),
            body_hash: entry.flow_body_exact_hash?,
            snapshot: snapshot.clone(),
            outer: captures.clone(),
            anchor,
        });
    // A direct class-declaration member reads its receiver; a nested
    // function reads the `this` its creating frame handed it.
    let this = match context {
        Some(context) => context.this.clone(),
        None => resolved.enclosing_this.and_then(|this| {
            let class = Arc::clone(&entry.key.declaration.name);
            Some(match this {
                verter_semantic::analysis::function_program::EnclosingThis::Instance => {
                    SliceThis::Instance {
                        class,
                        type_parameters: enclosing_type_parameters
                            .iter()
                            .map(|param| Arc::clone(&param.name))
                            .collect(),
                    }
                }
                verter_semantic::analysis::function_program::EnclosingThis::Static => {
                    SliceThis::Static {
                        class,
                        contributor: matches!(
                            entry.locator.descent.as_ref(),
                            [FunctionDescentStep::ClassMember { .. }]
                        )
                        .then_some(entry.locator.contributor.contributor_index),
                    }
                }
                verter_semantic::analysis::function_program::EnclosingThis::ObjectLiteral => {
                    match entry.locator.descent.as_ref() {
                        [FunctionDescentStep::VariableInitializer { declarator_ordinal }, FunctionDescentStep::ObjectMember { .. }] => {
                            SliceThis::Value {
                                value: class,
                                contributor: entry.locator.contributor.contributor_index,
                                declarator: *declarator_ordinal,
                            }
                        }
                        _ => return None,
                    }
                }
            })
        }),
    };
    let mut lowerer = Lowerer {
        frame_gate,
        bindings: &bindings,
        index,
        source,
        anchor,
        selection,
        params: &params,
        type_param_names: &type_param_names,
        self_name: self_name.as_deref(),
        enclosing_heritage: resolved.enclosing_heritage,
        this,
        member_this: None,
        skeleton,
        captures,
        control: Arc::clone(&entry.control),
        direct_calls: &entry.direct_calls,
        program,
        module_scope,
        namespace_owned,
        namespace_scopes: &namespace_scopes,
        contributor: entry.locator.contributor.contributor_index,
        budget_failure: None,
        inert_write_spans: FxHashSet::default(),
        logical_value_sites: Vec::new(),
        decided_above_call_spans: Vec::new(),
        call_arguments: FxHashMap::default(),
        whole_value_nesting: 0,
        predicate_guard_call_spans: FxHashSet::default(),
        non_narrowing_call_spans: FxHashSet::default(),
        predicate_parameters: predicate_parameters(
            declared_return.is_some(),
            skeleton,
            &bindings,
            entry,
            &params,
        ),
        control_test_gap: false,
        entered_assertion_sink: None,
        narrowing_alias_locals: FxHashSet::default(),
        alias_conditions: rustc_hash::FxHashMap::default(),
        discriminant_aliases: rustc_hash::FxHashMap::default(),
        alias_inline_budget: ALIAS_INLINE_LIMIT,
        unsafe_invoked_closure_effects: FxHashSet::default(),
        nested_free_writes: FxHashSet::default(),
        assignment_extent_statements: Vec::new(),
        class_property_initializers: 0,
        active_guard_bindings: Vec::new(),
        condition_guard_bindings: None,
        open_value_rooted_reads: 0,
        aliased_bindings: rustc_hash::FxHashMap::default(),
        annotated_params: node
            .params()
            .items
            .iter()
            .enumerate()
            .filter(|(_, param)| param.type_annotation.is_some())
            .map(|(ordinal, _)| ordinal as u32)
            .collect(),
        break_targets: Vec::new(),
        loop_direct_labels: Vec::new(),
        pending_loop_labels: Vec::new(),
        continue_targets: Vec::new(),
        break_target_followed_by_return: Vec::new(),
        current_statement_followed_by_return: SuffixReturn::NotGuaranteed,
        nullability,
        frame_is_async: match node {
            FunctionNode::Function(function) => function.r#async,
            FunctionNode::Arrow(arrow) => arrow.r#async,
        },
    };
    if selection.is_some() {
        lowerer.unsafe_invoked_closure_effects =
            lowerer.index_unsafe_invoked_closure_effects(&body.statements);
        lowerer.nested_free_writes = lowerer.build_nested_free_writes();
        lowerer.record_parameter_pattern_aliases(&node.params().items);
        collect_assignment_extent_statements(
            &body.statements,
            &mut lowerer.assignment_extent_statements,
        );
    }
    let region = if selection.is_none() {
        SliceRegion {
            statements: Arc::from([]),
            can_fall_through: NormalCompletion::minted(
                false,
                CompletionConstruction::SynthesizedRegion,
            ),
        }
    } else if node.is_expression_body() {
        // An expression-bodied arrow's body is one synthesized expression
        // statement; it lowers to a single `return` of the expression (the
        // expression cannot fall through).
        let statement = body.statements.first()?;
        let Statement::ExpressionStatement(expression) = statement else {
            return None;
        };
        if lowerer.span_contains_unsafe_invoked_closure(expression.expression.span()) {
            SliceRegion {
                statements: Arc::from([SliceStatement::Unsupported(
                    SliceUnsupported::InvokedClosureEffect,
                )]),
                can_fall_through: NormalCompletion::minted(
                    false,
                    CompletionConstruction::SynthesizedRegion,
                ),
            }
        } else {
            let freshness = expression_freshness(&expression.expression);
            let (argument, predicate_test) =
                if lowerer.value_span_selected(expression.expression.span()) {
                    (
                        lowerer.lower_expr(&expression.expression, ExprMode::Return),
                        lowerer.return_predicate_test(&expression.expression),
                    )
                } else {
                    (SliceExpr::Elided, None)
                };
            // An expression body has no statement loop to drain the
            // ternary-test gap into: it lands ahead of the synthesized
            // `return` here.
            let mut statements = Vec::with_capacity(2);
            if std::mem::take(&mut lowerer.control_test_gap) {
                statements.push(SliceStatement::Gap(
                    crate::semantic_query::FlowGap::GuardNarrowing,
                ));
            }
            statements.push(SliceStatement::Return {
                argument: Some(argument),
                freshness,
                predicate_test,
            });
            SliceRegion {
                statements: Arc::from(statements.into_boxed_slice()),
                can_fall_through: NormalCompletion::minted(
                    false,
                    CompletionConstruction::SynthesizedRegion,
                ),
            }
        }
    } else {
        let region = lowerer.lower_region(&body.statements).region;
        // Only a statement-position `yield x` / `yield;` contributes to
        // the yield join. A yield anywhere else (`const r = yield x`,
        // `f(yield x)`, a delegating `yield*`) still yields, so a body
        // holding one has no complete yield type: the region carries the
        // typed gap ahead of its statements.
        if body_has_unmodeled_yield(&body.statements) {
            let mut statements = Vec::with_capacity(region.statements.len() + 1);
            statements.push(SliceStatement::Gap(
                crate::semantic_query::FlowGap::UnmodeledExpression,
            ));
            statements.extend(region.statements.iter().cloned());
            SliceRegion {
                statements: Arc::from(statements.into_boxed_slice()),
                can_fall_through: region.can_fall_through,
            }
        } else {
            region
        }
    };
    // A destructured parameter whose pattern the flat object-element
    // model does not cover binds its whole pattern from the parameter's
    // value on entry, ahead of the body.
    let region = if selection.is_some() {
        let mut entry = Vec::new();
        for (ordinal, param) in node.params().items.iter().enumerate() {
            let pattern = match &param.pattern {
                BindingPattern::AssignmentPattern(assignment) => &assignment.left,
                other => other,
            };
            if matches!(pattern, BindingPattern::BindingIdentifier(_))
                || param_pattern_is_flat(pattern)
            {
                continue;
            }
            let Some(lowered) = lowerer.lower_pattern(pattern) else {
                continue;
            };
            let correlated = !lowered.bindings().iter().any(|(binding, _)| {
                lowerer.binding_is_written(*binding)
                    || lowerer
                        .nested_free_writes
                        .contains(&lowerer.bindings.canonical_local(*binding))
            });
            entry.push(SliceStatement::Destructure {
                correlated,
                source: None,
                pattern: lowered,
                kind: SliceBindingKind::Let,
                init: Some(SliceExpr::ParamValue {
                    ordinal: ordinal as u32,
                }),
                declared: None,
                annotated: param.type_annotation.is_some(),
            });
        }
        if entry.is_empty() {
            region
        } else {
            entry.extend(region.statements.iter().cloned());
            SliceRegion {
                statements: Arc::from(entry.into_boxed_slice()),
                can_fall_through: region.can_fall_through,
            }
        }
    } else {
        region
    };
    let budget_failure = lowerer.budget_failure;
    let inert_write_spans = lowerer.inert_write_spans;
    let decided_above_call_spans = lowerer.decided_above_call_spans;
    let call_arguments = lowerer.call_arguments;
    Some(SliceContent {
        bindings,
        declared_return,
        declared_predicate,
        can_fall_through: NormalCompletion::minted(
            region
                .can_fall_through
                .reaches_end(CompletionDischarge::BodyComposition),
            CompletionConstruction::BodyFromRootRegion,
        ),
        empty_completion: empty_completion_of(&node, entry),
        params: Arc::from(params.into_boxed_slice()),
        type_parameters: Arc::from(type_parameters.into_boxed_slice()),
        enclosing_type_parameters: Arc::from(enclosing_type_parameters.into_boxed_slice()),
        body: region,
        budget_failure,
        inert_write_spans,
        decided_above_call_spans,
        call_arguments: Arc::new(call_arguments),
    })
}

/// The parameters a type predicate inferred from the function's body may
/// name, in source order ([`ReturnPredicateTest`]): every plain identifier
/// parameter that is not a rest parameter and that nothing in the function
/// ever assigns — a nested closure's write included. `None` when the
/// function cannot infer one at all: it has a return annotation, it is
/// `async` or a generator, or its body does not hold exactly one `return`
/// (reachable or not), carrying a value.
fn predicate_parameters(
    declared_return: bool,
    skeleton: &FunctionBodySkeleton,
    bindings: &verter_semantic::analysis::flow::FlowBindingMap,
    entry: &FunctionProgramEntry,
    params: &[SliceParam],
) -> Option<Arc<[u32]>> {
    if declared_return || skeleton.kind != verter_semantic::analysis::flow::FunctionBodyKind::Plain
    {
        return None;
    }
    let [site] = skeleton.return_sites.as_ref() else {
        return None;
    };
    site.argument?;
    let parameters: Vec<u32> = params
        .iter()
        .enumerate()
        .filter(|(_, param)| !param.rest && param.name.is_some() && param.destructured.is_empty())
        .filter_map(|(ordinal, param)| {
            let binding = param.binding?;
            (!binding_is_assigned(skeleton, bindings, Some(entry), binding))
                .then(|| u32::try_from(ordinal).ok())?
        })
        .collect();
    (!parameters.is_empty()).then(|| Arc::from(parameters.into_boxed_slice()))
}

/// Whether anything in the function whose `skeleton` this is ever writes
/// `binding` whole — an assignment or update in its own body, or one a
/// nested closure makes (`entry`'s descendant assignments). A closure's
/// read or member write is not one (the checker's `isSymbolAssigned`).
fn binding_is_assigned(
    skeleton: &FunctionBodySkeleton,
    bindings: &verter_semantic::analysis::flow::FlowBindingMap,
    entry: Option<&FunctionProgramEntry>,
    binding: SkeletonBindingId,
) -> bool {
    let runtime = bindings.canonical_local(binding);
    skeleton.writes.iter().any(|write| {
        write.path.is_empty()
            && matches!(write.binding, Some(FlowBindingRef::Local(local))
                if bindings.canonical_local(local) == runtime)
    }) || entry.is_some_and(|entry| {
        entry
            .descendant_assignments
            .iter()
            .filter_map(|identity| bindings.local(identity))
            .any(|local| bindings.canonical_local(local) == runtime)
    })
}

/// Whether the retained program carries top-level MODULE syntax: an
/// `import` / `export` declaration of any spelling (including the empty
/// `export {}`), an `export =` assignment, or an `import x = require(…)`
/// external-module reference. The proof is ONE-DIRECTIONAL: module syntax
/// proves module scope; its absence proves nothing (`import.meta`, a
/// project's module-detection setting, or a carrier projection can still
/// make the file a module), so a caller treats absence as "not provable"
/// — never as "script".
fn program_has_module_syntax(program: &Program<'_>) -> bool {
    program.body.iter().any(|statement| match statement {
        Statement::ImportDeclaration(_)
        | Statement::ExportAllDeclaration(_)
        | Statement::ExportDefaultDeclaration(_)
        | Statement::ExportNamedDeclaration(_)
        | Statement::TSExportAssignment(_) => true,
        Statement::TSImportEqualsDeclaration(import) => matches!(
            import.module_reference,
            oxc_ast::ast::TSModuleReference::ExternalModuleReference(_)
        ),
        _ => false,
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

/// Whether a same-file predicate's TARGET references a name the CALLEE's
/// own declaration binds: a type parameter of its clause (`x is T` on
/// `isSame<T>(x: T)`) or a formal parameter (`x is typeof y`). Such a
/// target is instantiated by the CALL — `T` from the argument's type — an
/// inference this half does not perform, so it is not closed over
/// anything the caller frame's environment can resolve; lowering it there
/// binds whatever the owner scope holds under the same name.
fn predicate_target_names_callee_binding<'a>(
    function: &oxc_ast::ast::Function<'a>,
    target: &TypeExpr,
) -> bool {
    let names = verter_type_expr::referenced_names(target);
    let names_type_parameter = function.type_parameters.as_ref().is_some_and(|clause| {
        clause.params.iter().any(|binder| {
            names
                .type_names
                .iter()
                .any(|occurrence| occurrence.head == binder.name.name.as_str())
        })
    });
    if names_type_parameter {
        return true;
    }
    if names.value_roots.is_empty() {
        return false;
    }
    let mut parameter_names: FxHashSet<&'a str> = FxHashSet::default();
    for param in &function.params.items {
        collect_binding_pattern_names(&param.pattern, &mut parameter_names);
    }
    if let Some(rest) = &function.params.rest {
        collect_binding_pattern_names(&rest.rest.argument, &mut parameter_names);
    }
    names
        .value_roots
        .iter()
        .any(|root| parameter_names.contains(root.as_str()))
}

/// Every identifier a binding pattern binds, nested patterns included.
fn collect_binding_pattern_names<'a>(pattern: &BindingPattern<'a>, out: &mut FxHashSet<&'a str>) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => {
            out.insert(id.name.as_str());
        }
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                collect_binding_pattern_names(&property.value, out);
            }
            if let Some(rest) = &object.rest {
                collect_binding_pattern_names(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for element in array.elements.iter().flatten() {
                collect_binding_pattern_names(element, out);
            }
            if let Some(rest) = &array.rest {
                collect_binding_pattern_names(&rest.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(assignment) => {
            collect_binding_pattern_names(&assignment.left, out);
        }
    }
}

/// Where a discarded value sits — which effect scan its unmodeled
/// positions take ([`Lowerer::lower_discarded_effects`]).
#[derive(Clone, Copy)]
enum DiscardedContext {
    /// An expression statement.
    Statement,
    /// A declarator initializer the demand did not select.
    Initializer,
}

/// The whole-binding `=` write a `void (x = v)` discards (through
/// parentheses on both sides of the `void`).
fn void_write_assignment<'a>(
    expression: &'a Expression<'a>,
) -> Option<&'a oxc_ast::ast::AssignmentExpression<'a>> {
    let Expression::UnaryExpression(unary) = unwrap_parenthesized(expression) else {
        return None;
    };
    if unary.operator != UnaryOperator::Void {
        return None;
    }
    match unwrap_parenthesized(&unary.argument) {
        Expression::AssignmentExpression(assignment)
            if matches!(
                assignment.operator,
                oxc_ast::ast::AssignmentOperator::Assign
            ) && assigned_identifier(&assignment.left).is_some() =>
        {
            Some(assignment)
        }
        _ => None,
    }
}

/// Whether a discarded value holds a whole-binding `=` write where the
/// evaluator can apply it ([`Lowerer::lower_discarded_effects`]): the
/// value itself, `void` of one, or one in a conditional arm, a sequence
/// operand, an object literal's member value or an array literal's
/// element.
fn discarded_value_holds_write(expression: &Expression<'_>) -> bool {
    match unwrap_parenthesized(expression) {
        // A logical assignment to a binding writes it on the path its
        // operator selects (`x ||= v` is `x || (x = v)`).
        Expression::AssignmentExpression(assignment) if assignment.operator.is_logical() => {
            matches!(
                assignment.left,
                oxc_ast::ast::AssignmentTarget::AssignmentTargetIdentifier(_)
            )
        }
        Expression::AssignmentExpression(assignment) => {
            matches!(
                assignment.operator,
                oxc_ast::ast::AssignmentOperator::Assign
            ) && (assigned_identifier(&assignment.left).is_some()
                || matches!(
                    assignment.left,
                    oxc_ast::ast::AssignmentTarget::ObjectAssignmentTarget(_)
                        | oxc_ast::ast::AssignmentTarget::ArrayAssignmentTarget(_)
                )
                || assignment
                    .left
                    .as_member_expression()
                    .is_some_and(member_write_shape))
        }
        void @ Expression::UnaryExpression(unary)
            if unary.operator == oxc_ast::ast::UnaryOperator::Void =>
        {
            void_write_assignment(void).is_some()
        }
        // An operator's operands run before it: `(x = v) === w`,
        // `typeof (x = v)`, `!(x = v)`.
        Expression::UnaryExpression(unary) => discarded_value_holds_write(&unary.argument),
        Expression::BinaryExpression(binary) => {
            discarded_value_holds_write(&binary.left) || discarded_value_holds_write(&binary.right)
        }
        Expression::ConditionalExpression(conditional) => {
            discarded_value_holds_write(&conditional.consequent)
                || discarded_value_holds_write(&conditional.alternate)
        }
        Expression::SequenceExpression(sequence) => {
            sequence.expressions.iter().any(discarded_value_holds_write)
        }
        Expression::LogicalExpression(logical) => {
            discarded_value_holds_write(&logical.left)
                || discarded_value_holds_write(&logical.right)
        }
        Expression::ObjectExpression(object) => object.properties.iter().any(|property| {
            matches!(
                property,
                oxc_ast::ast::ObjectPropertyKind::ObjectProperty(property)
                    if property.kind == oxc_ast::ast::PropertyKind::Init
                        && !property.method
                        && discarded_value_holds_write(&property.value)
            )
        }),
        Expression::ArrayExpression(array) => array.elements.iter().any(|element| {
            !matches!(
                element,
                oxc_ast::ast::ArrayExpressionElement::SpreadElement(_)
            ) && element
                .as_expression()
                .is_some_and(discarded_value_holds_write)
        }),
        _ => false,
    }
}

/// The property a destructuring pattern property names: a static name, a
/// string literal, a numeric literal (its canonical spelling), or a
/// computed literal key. `None` for a computed key that is not a literal.
fn pattern_property_key(key: &oxc_ast::ast::PropertyKey<'_>, computed: bool) -> Option<Arc<str>> {
    match key {
        oxc_ast::ast::PropertyKey::StaticIdentifier(id) if !computed => {
            Some(Arc::from(id.name.as_str()))
        }
        oxc_ast::ast::PropertyKey::StringLiteral(literal) => {
            Some(Arc::from(literal.value.as_str()))
        }
        oxc_ast::ast::PropertyKey::NumericLiteral(literal) => Some(Arc::from(
            crate::semantic_query::index_key::js_number_to_string(literal.value).as_str(),
        )),
        other => other.as_expression().and_then(literal_member_key),
    }
}

/// Whether one binding pattern is a form [`Lowerer::lower_pattern`]
/// models — decided on syntax alone, before any lowering, so every read of
/// a binding it declares classifies the same way.
fn pattern_is_modelled(pattern: &BindingPattern<'_>) -> bool {
    let element = |element: &BindingPattern<'_>| match element {
        BindingPattern::AssignmentPattern(assignment) => pattern_is_modelled(&assignment.left),
        other => pattern_is_modelled(other),
    };
    let rest = |rest: Option<&oxc_allocator::Box<'_, oxc_ast::ast::BindingRestElement<'_>>>| {
        rest.is_none_or(|rest| matches!(rest.argument, BindingPattern::BindingIdentifier(_)))
    };
    match pattern {
        BindingPattern::BindingIdentifier(_) => true,
        BindingPattern::ObjectPattern(object) => {
            object.properties.iter().all(|property| {
                (pattern_property_key(&property.key, property.computed).is_some()
                    || property.key.as_expression().is_some())
                    && element(&property.value)
            }) && rest(object.rest.as_ref())
        }
        BindingPattern::ArrayPattern(array) => {
            array.elements.iter().flatten().all(element) && rest(array.rest.as_ref())
        }
        BindingPattern::AssignmentPattern(_) => false,
    }
}

/// Whether a parameter's object pattern is one the flat element model
/// ([`SliceParam::destructured`]) covers whole: every property a static
/// key over an identifier, defaulted or not, and no rest element.
fn param_pattern_is_flat(pattern: &BindingPattern<'_>) -> bool {
    let BindingPattern::ObjectPattern(object) = pattern else {
        return false;
    };
    object.rest.is_none()
        && object.properties.iter().all(|property| {
            !property.computed
                && matches!(
                    property.key,
                    oxc_ast::ast::PropertyKey::StaticIdentifier(_)
                        | oxc_ast::ast::PropertyKey::StringLiteral(_)
                )
                && match &property.value {
                    BindingPattern::BindingIdentifier(_) => true,
                    BindingPattern::AssignmentPattern(assignment) => {
                        matches!(assignment.left, BindingPattern::BindingIdentifier(_))
                    }
                    _ => false,
                }
        })
}

/// The bindings every modelled destructuring pattern of one function
/// declares — its parameters' (beyond the flat object-element model), its
/// statement declarators' and its `for…of` / `for…in` elements', never a
/// nested function's or class's.
fn modelled_pattern_bindings(
    params: &oxc_allocator::Vec<'_, oxc_ast::ast::FormalParameter<'_>>,
    statements: &[Statement<'_>],
    bindings: &verter_semantic::analysis::flow::FlowBindingMap,
    anchor: u32,
) -> FxHashSet<SkeletonBindingId> {
    struct Collector<'m> {
        bindings: &'m verter_semantic::analysis::flow::FlowBindingMap,
        anchor: u32,
        out: FxHashSet<SkeletonBindingId>,
    }
    impl<'a> Visit<'a> for Collector<'_> {
        fn visit_variable_declarator(&mut self, it: &oxc_ast::ast::VariableDeclarator<'a>) {
            if !matches!(it.id, BindingPattern::BindingIdentifier(_)) && pattern_is_modelled(&it.id)
            {
                let mut identifiers = Vec::new();
                collect_pattern_identifier_spans(&it.id, &mut identifiers);
                for span in identifiers {
                    if let Some(binding) = self
                        .bindings
                        .declaration_at_span(FrameSpan::rebase(self.anchor, span.into()))
                    {
                        self.out.insert(binding);
                    }
                }
            }
            walk::walk_variable_declarator(self, it);
        }
        fn visit_function(
            &mut self,
            _it: &oxc_ast::ast::Function<'a>,
            _flags: oxc_syntax::scope::ScopeFlags,
        ) {
        }
        fn visit_arrow_function_expression(
            &mut self,
            _it: &oxc_ast::ast::ArrowFunctionExpression<'a>,
        ) {
        }
        fn visit_class(&mut self, _it: &oxc_ast::ast::Class<'a>) {}
    }
    let mut collector = Collector {
        bindings,
        anchor,
        out: FxHashSet::default(),
    };
    for param in params {
        let pattern = match &param.pattern {
            BindingPattern::AssignmentPattern(assignment) => &assignment.left,
            other => other,
        };
        if !matches!(pattern, BindingPattern::BindingIdentifier(_))
            && !param_pattern_is_flat(pattern)
            && pattern_is_modelled(pattern)
        {
            let mut identifiers = Vec::new();
            collect_pattern_identifier_spans(pattern, &mut identifiers);
            for span in identifiers {
                if let Some(binding) =
                    bindings.declaration_at_span(FrameSpan::rebase(anchor, span.into()))
                {
                    collector.out.insert(binding);
                }
            }
        }
    }
    for statement in statements {
        collector.visit_statement(statement);
    }
    collector.out
}

/// The binding-identifier spans one pattern declares.
fn collect_pattern_identifier_spans(pattern: &BindingPattern<'_>, out: &mut Vec<oxc_span::Span>) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => out.push(id.span),
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                collect_pattern_identifier_spans(&property.value, out);
            }
            if let Some(rest) = object.rest.as_ref() {
                collect_pattern_identifier_spans(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for element in array.elements.iter().flatten() {
                collect_pattern_identifier_spans(element, out);
            }
            if let Some(rest) = array.rest.as_ref() {
                collect_pattern_identifier_spans(&rest.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(assignment) => {
            collect_pattern_identifier_spans(&assignment.left, out);
        }
    }
}

/// The guard `subject == null` spells: its positive reading is the
/// subject's `null` / `undefined` members, its negated reading the rest.
fn nullish_guard_of(subject: SliceNarrowSubject) -> SliceGuard {
    SliceGuard::Or(Arc::from(
        [SliceGuardLiteral::Null, SliceGuardLiteral::Undefined]
            .into_iter()
            .map(|literal| SliceGuard::EqLiteral {
                subject: subject.clone(),
                literal,
                negated: false,
                loose: false,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    ))
}

/// The property an element-access KEY names without evaluating anything:
/// a string literal names its value, a numeric literal its canonical JS
/// spelling (`a[1]` and `a["1"]` are one property).
///
/// A string key opening with U+0000 is not modelled: that prefix spells a
/// key binding's identity segment ([`SliceElementKey::identity`]), which no
/// property name may collide with.
fn literal_member_key(key: &Expression<'_>) -> Option<Arc<str>> {
    match unwrap_parenthesized(key) {
        Expression::StringLiteral(literal) if literal.value.starts_with('\u{0}') => None,
        Expression::StringLiteral(literal) => Some(Arc::from(literal.value.as_str())),
        Expression::NumericLiteral(literal) => Some(Arc::from(
            crate::semantic_query::index_key::js_number_to_string(literal.value).as_str(),
        )),
        _ => None,
    }
}

/// A member chain whose every step is a static name or a literal key,
/// rooted at an identifier: the root and the path of names, outermost
/// first. `None` for any other shape.
fn literal_member_chain<'a>(
    expression: &'a Expression<'a>,
) -> Option<(&'a oxc_ast::ast::IdentifierReference<'a>, Vec<Arc<str>>)> {
    let mut segments: Vec<Arc<str>> = Vec::new();
    let mut current = unwrap_parenthesized(expression);
    let root = loop {
        match current {
            Expression::StaticMemberExpression(member) => {
                segments.push(Arc::from(member.property.name.as_str()));
                current = unwrap_parenthesized(&member.object);
            }
            Expression::ComputedMemberExpression(member) => {
                segments.push(literal_member_key(&member.expression)?);
                current = unwrap_parenthesized(&member.object);
            }
            Expression::Identifier(identifier) => break identifier,
            _ => return None,
        }
    };
    segments.reverse();
    Some((root, segments))
}

/// The root and literal path of a member WRITE target (`o.y`, `o.p["q"]`,
/// `a[0]`) — [`literal_member_chain`] for an assignment target.
/// Whether a member expression has the shape of a modelled member write
/// target: a static or literal-keyed chain ([`member_target_chain`]), or
/// such a chain's object read by an identifier key.
fn member_write_shape(member: &oxc_ast::ast::MemberExpression<'_>) -> bool {
    member_target_chain(member).is_some()
        || matches!(
            member,
            oxc_ast::ast::MemberExpression::ComputedMemberExpression(computed)
                if matches!(unwrap_parenthesized(&computed.expression), Expression::Identifier(_))
                    && literal_member_chain(&computed.object).is_some()
        )
}

fn member_target_chain<'a>(
    member: &'a oxc_ast::ast::MemberExpression<'a>,
) -> Option<(&'a oxc_ast::ast::IdentifierReference<'a>, Vec<Arc<str>>)> {
    let (object, name) = match member {
        oxc_ast::ast::MemberExpression::StaticMemberExpression(member) => {
            (&member.object, Arc::from(member.property.name.as_str()))
        }
        oxc_ast::ast::MemberExpression::ComputedMemberExpression(member) => {
            (&member.object, literal_member_key(&member.expression)?)
        }
        oxc_ast::ast::MemberExpression::PrivateFieldExpression(_) => return None,
    };
    let (root, mut path) = literal_member_chain(object)?;
    path.push(name);
    Some((root, path))
}

/// Whether a member access's object is a flow VALUE with no reference
/// behind it: a `new` expression or an object literal, or a static member
/// chain rooted at one.
fn value_rooted_member_object(object: &Expression<'_>) -> bool {
    match unwrap_parenthesized(object) {
        Expression::NewExpression(_) | Expression::ObjectExpression(_) => true,
        Expression::StaticMemberExpression(member) => value_rooted_member_object(&member.object),
        _ => false,
    }
}

/// Unwrap a parenthesized expression (the IIFE callee shape).
/// A chain of static member reads (`.a.b`, through parentheses) whose base
/// is a call expression: the call and the member names, first read first.
/// `None` for any other form — a computed or private member, an optional
/// link, a base that is not a call.
fn call_rooted_member_path<'a>(
    expression: &'a Expression<'a>,
) -> Option<(&'a Expression<'a>, Vec<Arc<str>>)> {
    let mut names = Vec::new();
    let mut current = expression;
    loop {
        match current {
            Expression::StaticMemberExpression(member) if !member.optional => {
                names.push(Arc::<str>::from(member.property.name.as_str()));
                current = unwrap_parenthesized(&member.object);
            }
            Expression::CallExpression(call) if !names.is_empty() && !call.optional => {
                names.reverse();
                return Some((current, names));
            }
            _ => return None,
        }
    }
}

fn unwrap_parenthesized<'a>(expression: &'a Expression<'a>) -> &'a Expression<'a> {
    match expression {
        Expression::ParenthesizedExpression(paren) => unwrap_parenthesized(&paren.expression),
        inner => inner,
    }
}

/// The reference an expression NAMES, through the wrappers the checker
/// treats as transparent when it matches a narrowing reference:
/// parentheses and the postfix non-null assertion.
///
/// `satisfies` and the `as` / angle-bracket TYPE ASSERTION are both
/// deliberately absent, and for the same reason: neither is a matching
/// reference for narrowing, in a leaf position or around a whole test.
/// Measured against the checker, `if ((x satisfies string | undefined))`
/// leaves `undefined` in the result and `typeof (x satisfies string |
/// number) === "string"` narrows nothing, exactly as their `as` twins
/// do, while both postfix-`!` spellings narrow. Peeling either one makes
/// this half narrow where the checker does not — a SUBSET of the
/// checker's type, which drops a real contributor rather than merely
/// widening, and is therefore worse than the superset a missing narrow
/// produces.
/// The reference a narrow lands on (the checker's `getReferenceCandidate`
/// and `isMatchingReference`): through parentheses, non-null assertions
/// and a comma sequence's LAST operand — `(touch(), x)` is `x`, the
/// earlier operands only running first.
fn reference_candidate<'a>(expression: &'a Expression<'a>) -> &'a Expression<'a> {
    match unwrap_reference_transparent(expression) {
        Expression::SequenceExpression(sequence) => match sequence.expressions.last() {
            Some(last) => reference_candidate(last),
            None => expression,
        },
        inner => inner,
    }
}

fn unwrap_reference_transparent<'a>(expression: &'a Expression<'a>) -> &'a Expression<'a> {
    match expression {
        Expression::ParenthesizedExpression(paren) => {
            unwrap_reference_transparent(&paren.expression)
        }
        Expression::TSNonNullExpression(non_null) => {
            unwrap_reference_transparent(&non_null.expression)
        }
        inner => inner,
    }
}

/// Where a narrow established by a control test would LAND, relative to
/// the slots this half models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NarrowDestination {
    /// A reference [`Lowerer::narrow_subject_of`] carries.
    Represented,
    /// A reference rooted at a modeled slot whose ACCESS this half
    /// cannot express — a computed member, an optional step, a private
    /// field. The checker still binds a narrow here.
    Unrepresented,
    /// No modeled slot to land on.
    Absent,
}

/// What ONE control test establishes, as three mutually exclusive
/// answers.
///
/// The third is the point: a two-answer result cannot separate "the test
/// provably narrows nothing" from "the test narrows something this
/// vocabulary cannot spell", and collapsing the two publishes the
/// unnarrowed arm as a complete answer.
#[derive(Debug, Clone, PartialEq)]
enum GuardDisposition {
    /// A narrowing fact this half CARRIES to the evaluator.
    Modeled(Box<SliceGuard>),
    /// PROVED to establish no narrowing at any slot this half models.
    NoNarrowing,
    /// The checker narrows a modeled slot through a form this
    /// vocabulary cannot express: the demand degrades through the typed
    /// `GuardNarrowing` gap.
    Unexpressible,
}

impl GuardDisposition {
    fn modeled(guard: SliceGuard) -> Self {
        Self::Modeled(Box::new(guard))
    }

    /// The negated reading. Negation is total on all three answers: the
    /// negation of a proved-inert test is inert, and the negation of an
    /// unexpressible one is still unexpressible.
    fn negated(self) -> Self {
        match self {
            Self::Modeled(guard) => Self::modeled(negate_guard(*guard)),
            Self::NoNarrowing => Self::NoNarrowing,
            Self::Unexpressible => Self::Unexpressible,
        }
    }

    fn is_no_narrowing(&self) -> bool {
        matches!(self, Self::NoNarrowing)
    }

    fn is_unexpressible(&self) -> bool {
        matches!(self, Self::Unexpressible)
    }

    /// The carried guard, with a proved-inert answer becoming the
    /// explicit [`SliceGuard::None`] alternative the composition
    /// reducers preserve. Only ever called once the caller has ruled out
    /// [`Self::Unexpressible`].
    fn into_guard(self) -> SliceGuard {
        match self {
            Self::Modeled(guard) => *guard,
            Self::NoNarrowing | Self::Unexpressible => SliceGuard::None,
        }
    }
}

/// The calls on a test's narrowing spine — the positions the checker's
/// `narrowType` reads a call's type predicate from: the test itself through
/// parentheses and non-null assertions, a `!` operand, `&&` / `||` / `??`
/// operands, a comma sequence's last operand, an assignment's right-hand
/// side and the side of an equality against a boolean literal — plus a
/// comma sequence's earlier operands, where a call may assert. A call
/// anywhere else (an argument, a comparison or arithmetic operand) narrows
/// nothing through its predicate.
fn narrowing_spine_calls<'e, 'a>(
    expression: &'e Expression<'a>,
    out: &mut Vec<&'e oxc_ast::ast::CallExpression<'a>>,
) {
    match expression {
        Expression::ParenthesizedExpression(inner) => narrowing_spine_calls(&inner.expression, out),
        Expression::TSNonNullExpression(inner) => narrowing_spine_calls(&inner.expression, out),
        Expression::CallExpression(call) => out.push(call),
        Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::LogicalNot => {
            narrowing_spine_calls(&unary.argument, out)
        }
        Expression::LogicalExpression(logical) => {
            narrowing_spine_calls(&logical.left, out);
            narrowing_spine_calls(&logical.right, out);
        }
        Expression::SequenceExpression(sequence) => {
            for operand in &sequence.expressions {
                narrowing_spine_calls(operand, out);
            }
        }
        Expression::AssignmentExpression(assignment) => {
            narrowing_spine_calls(&assignment.right, out)
        }
        Expression::BinaryExpression(binary)
            if matches!(
                binary.operator,
                oxc_ast::ast::BinaryOperator::Equality
                    | oxc_ast::ast::BinaryOperator::Inequality
                    | oxc_ast::ast::BinaryOperator::StrictEquality
                    | oxc_ast::ast::BinaryOperator::StrictInequality
            ) =>
        {
            if literal_boolean_value(&binary.right).is_some() {
                narrowing_spine_calls(&binary.left, out);
            } else if literal_boolean_value(&binary.left).is_some() {
                narrowing_spine_calls(&binary.right, out);
            }
        }
        _ => {}
    }
}

/// The statement one applied `asserts` call lowers to.
fn assertion_statement(SliceAssertion { subject, target }: SliceAssertion) -> SliceStatement {
    SliceStatement::Assertion { subject, target }
}

/// A region of entered effects: assertions and joins, which always
/// complete.
fn entered_effect_region(statements: Vec<SliceStatement>) -> Box<SliceRegion> {
    Box::new(SliceRegion {
        statements: Arc::from(statements.into_boxed_slice()),
        can_fall_through: NormalCompletion::minted(true, CompletionConstruction::SynthesizedRegion),
    })
}

/// Whether a statement ends the path: a `throw`, a never-returning call,
/// or a block that cannot complete normally.
fn statement_ends_path(statement: &SliceStatement) -> bool {
    match statement {
        SliceStatement::Throw => true,
        SliceStatement::Block(region) => !region
            .can_fall_through
            .reaches_end(CompletionDischarge::RegionComposition),
        _ => false,
    }
}

/// Statements one expression statement lowers to, as one block that
/// completes unless one of them ends the path.
fn sequential_block(statements: Vec<SliceStatement>) -> SliceStatement {
    let can_fall_through = !statements.iter().any(statement_ends_path);
    SliceStatement::Block(SliceRegion {
        statements: Arc::from(statements.into_boxed_slice()),
        can_fall_through: NormalCompletion::minted(
            can_fall_through,
            CompletionConstruction::RegionAccumulator,
        ),
    })
}
fn literal_boolean_value(expression: &Expression<'_>) -> Option<bool> {
    match unwrap_parenthesized(expression) {
        Expression::BooleanLiteral(literal) => Some(literal.value),
        _ => None,
    }
}

/// Whether a write path can affect a read path. A whole-root access and a
/// computed segment are conservative wildcards; otherwise different static
/// siblings are disjoint and prefix paths overlap.
fn paths_may_overlap(
    write_path: &[SkeletonPathSegment],
    read_path: &[SkeletonPathSegment],
) -> bool {
    if write_path.is_empty() || read_path.is_empty() {
        return true;
    }
    write_path
        .iter()
        .zip(read_path.iter())
        .all(|(write, read)| match (write, read) {
            (SkeletonPathSegment::Computed, _) | (_, SkeletonPathSegment::Computed) => true,
            (SkeletonPathSegment::Static(write), SkeletonPathSegment::Static(read)) => {
                write == read
            }
        })
}

/// Unwrap the wrappers that are TRANSPARENT to literal freshness: a
/// parenthesis, and `satisfies`. `x satisfies T` checks `x` against `T`
/// and evaluates to `x`'s own type unchanged — including its freshness —
/// so `return 1 satisfies number` is `number`, exactly like `return 1`.
///
/// A type ASSERTION is not on this list and must never be added: `1 as 1`
/// PINS to `1` even though the asserted type is the literal's own
/// (TypeScript 7.0.2: `(): 1`).
fn unwrap_freshness_transparent<'a>(expression: &'a Expression<'a>) -> &'a Expression<'a> {
    match expression {
        // `x!` is `getNonNullableType` of the operand's type, which is the
        // literal type itself — freshness included.
        Expression::TSNonNullExpression(non_null) => {
            unwrap_freshness_transparent(&non_null.expression)
        }
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

/// The root of an optional chain whose route contains only transparent
/// parentheses and MEMBER steps. A call is permitted only as the chain's
/// terminal element; encountering one while walking an object/callee rejects
/// the route. Operands the `OptionalAnyChain` carrier cannot retain (computed
/// keys and call arguments) must contain no syntactic write/async effect;
/// otherwise the route is rejected rather than silently dropping it. TS wrappers are
/// deliberately absent because they can change the value being projected even
/// when the underlying identifier was `any`.
fn pure_optional_chain_root_identifier<'a>(
    element: &'a oxc_ast::ast::ChainElement<'a>,
) -> Option<&'a oxc_ast::ast::IdentifierReference<'a>> {
    match element {
        oxc_ast::ast::ChainElement::CallExpression(call) => {
            if !call.arguments.iter().all(|argument| {
                argument
                    .as_expression()
                    .is_some_and(optional_chain_discarded_expr_has_no_syntactic_effect)
            }) {
                return None;
            }
            pure_member_root_identifier(&call.callee)
        }
        oxc_ast::ast::ChainElement::StaticMemberExpression(member) => {
            pure_member_root_identifier(&member.object)
        }
        oxc_ast::ast::ChainElement::ComputedMemberExpression(member) => {
            if !optional_chain_discarded_expr_has_no_syntactic_effect(&member.expression) {
                return None;
            }
            pure_member_root_identifier(&member.object)
        }
        oxc_ast::ast::ChainElement::PrivateFieldExpression(member) => {
            pure_member_root_identifier(&member.object)
        }
        oxc_ast::ast::ChainElement::TSNonNullExpression(_) => None,
    }
}

fn pure_member_root_identifier<'a>(
    expr: &'a Expression<'a>,
) -> Option<&'a oxc_ast::ast::IdentifierReference<'a>> {
    match expr {
        Expression::Identifier(identifier) => Some(identifier),
        Expression::ParenthesizedExpression(paren) => {
            pure_member_root_identifier(&paren.expression)
        }
        Expression::StaticMemberExpression(member) => pure_member_root_identifier(&member.object),
        Expression::ComputedMemberExpression(member)
            if optional_chain_discarded_expr_has_no_syntactic_effect(&member.expression) =>
        {
            pure_member_root_identifier(&member.object)
        }
        Expression::PrivateFieldExpression(member) => pure_member_root_identifier(&member.object),
        Expression::ChainExpression(chain) => {
            pure_optional_member_root_identifier(&chain.expression)
        }
        _ => None,
    }
}

/// Split a MEMBER-valued optional chain into its root identifier and its
/// STATIC member links, each with its own `?.`-authored optionality, in
/// EVALUATION order (`a?.b.c` → `a`, `[(b, true), (c, false)]`).
///
/// `None` for every shape the typed optional-member carrier cannot retain
/// honestly: a computed key (`a?.[k]`), a private-field link, a non-static
/// root, or a root that is not a bare identifier (`this?.b`, a chain over
/// a parenthesised expression). Those keep the rails they had — the leaf
/// lowering and its fail-closed gap — never a half-modeled path.
fn optional_member_chain_parts<'a>(
    element: &'a oxc_ast::ast::ChainElement<'_>,
) -> Option<SplitOptionalMemberChain<'a>> {
    let mut links: Vec<OptionalMemberLink> = Vec::new();
    // The outermost member is the chain element; every inner link is an
    // ordinary (possibly optional) static member expression, and oxc may
    // nest a further `ChainExpression` in the object position.
    let mut next = match element {
        oxc_ast::ast::ChainElement::StaticMemberExpression(member) => {
            links.push(optional_member_link(member));
            &member.object
        }
        _ => return None,
    };
    loop {
        match next {
            Expression::StaticMemberExpression(member) => {
                links.push(optional_member_link(member));
                next = &member.object;
            }
            Expression::ChainExpression(chain) => match &chain.expression {
                oxc_ast::ast::ChainElement::StaticMemberExpression(member) => {
                    links.push(optional_member_link(member));
                    next = &member.object;
                }
                _ => return None,
            },
            Expression::Identifier(identifier) => {
                links.reverse();
                return Some(SplitOptionalMemberChain {
                    root: identifier,
                    links,
                });
            }
            _ => return None,
        }
    }
}

/// The root identifier and member path of a non-optional element read at
/// a non-negative integer literal (`a[0]`, `o.xs[1]`) over a static member
/// chain rooted at an identifier; the index is the path's last key.
fn element_read_path<'a>(
    member: &'a oxc_ast::ast::ComputedMemberExpression<'a>,
) -> Option<(&'a oxc_ast::ast::IdentifierReference<'a>, Vec<Arc<str>>)> {
    let Expression::NumericLiteral(index) = &member.expression else {
        return None;
    };
    if member.optional || index.value < 0.0 || index.value.fract() != 0.0 || index.value > 1e15 {
        return None;
    }
    let mut links: Vec<Arc<str>> = vec![Arc::from(format!("{}", index.value as u64).as_str())];
    let mut current = &member.object;
    loop {
        match current {
            Expression::StaticMemberExpression(object) if !object.optional => {
                links.push(Arc::from(object.property.name.as_str()));
                current = &object.object;
            }
            Expression::Identifier(root) => {
                links.reverse();
                return Some((root, links));
            }
            _ => return None,
        }
    }
}

/// One STATIC link of a member-valued optional chain: the member name and
/// its own `?.`-authored optionality.
type OptionalMemberLink = (Arc<str>, bool);

fn optional_member_link(member: &oxc_ast::ast::StaticMemberExpression<'_>) -> OptionalMemberLink {
    (Arc::from(member.property.name.as_str()), member.optional)
}

/// The split of one member-valued optional chain — its root identifier and
/// its static links in evaluation order.
struct SplitOptionalMemberChain<'a> {
    root: &'a oxc_ast::ast::IdentifierReference<'a>,
    links: Vec<OptionalMemberLink>,
}

/// The static member path of a `super.a.b(…)` callee — the path off the
/// base that the call's value resolves through. `None` unless the callee
/// is a paren-transparent static member chain rooted at a bare `super`
/// (`super.m()`, `super.a.b()`); a computed link keeps the fail-closed
/// rail.
fn super_callee_static_path(callee: &Expression<'_>) -> Option<Vec<Arc<str>>> {
    let mut path = Vec::new();
    let mut current = unwrap_parenthesized(callee);
    loop {
        match current {
            Expression::StaticMemberExpression(member) => {
                path.push(Arc::from(member.property.name.as_str()));
                current = unwrap_parenthesized(&member.object);
            }
            Expression::Super(_) => {
                path.reverse();
                return Some(path);
            }
            _ => return None,
        }
    }
}

fn optional_chain_discarded_expr_has_no_syntactic_effect(expr: &Expression<'_>) -> bool {
    struct EffectScanner {
        safe: bool,
    }

    impl<'a> Visit<'a> for EffectScanner {
        fn visit_expression(&mut self, expression: &Expression<'a>) {
            if !self.safe {
                return;
            }
            // OXC's current expression vocabulary is listed explicitly. The
            // fallback is deliberately fail-closed so a newly introduced
            // expression form cannot bypass this discarded-effect gate.
            #[allow(unreachable_patterns)]
            match expression {
                Expression::AssignmentExpression(_)
                | Expression::AwaitExpression(_)
                | Expression::UpdateExpression(_)
                | Expression::YieldExpression(_) => self.safe = false,
                Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::Delete => {
                    self.safe = false;
                }
                Expression::BooleanLiteral(_)
                | Expression::NullLiteral(_)
                | Expression::NumericLiteral(_)
                | Expression::BigIntLiteral(_)
                | Expression::RegExpLiteral(_)
                | Expression::StringLiteral(_)
                | Expression::TemplateLiteral(_)
                | Expression::Identifier(_)
                | Expression::MetaProperty(_)
                | Expression::Super(_)
                | Expression::ArrayExpression(_)
                | Expression::ArrowFunctionExpression(_)
                | Expression::BinaryExpression(_)
                | Expression::CallExpression(_)
                | Expression::ChainExpression(_)
                | Expression::ClassExpression(_)
                | Expression::ConditionalExpression(_)
                | Expression::FunctionExpression(_)
                | Expression::ImportExpression(_)
                | Expression::LogicalExpression(_)
                | Expression::NewExpression(_)
                | Expression::ObjectExpression(_)
                | Expression::ParenthesizedExpression(_)
                | Expression::SequenceExpression(_)
                | Expression::TaggedTemplateExpression(_)
                | Expression::ThisExpression(_)
                | Expression::UnaryExpression(_)
                | Expression::PrivateInExpression(_)
                | Expression::JSXElement(_)
                | Expression::JSXFragment(_)
                | Expression::TSAsExpression(_)
                | Expression::TSSatisfiesExpression(_)
                | Expression::TSTypeAssertion(_)
                | Expression::TSNonNullExpression(_)
                | Expression::TSInstantiationExpression(_)
                | Expression::V8IntrinsicExpression(_)
                | Expression::StaticMemberExpression(_)
                | Expression::ComputedMemberExpression(_)
                | Expression::PrivateFieldExpression(_) => {
                    walk::walk_expression(self, expression);
                }
                _ => self.safe = false,
            }
        }
    }

    let mut scanner = EffectScanner { safe: true };
    scanner.visit_expression(expr);
    scanner.safe
}

fn pure_optional_member_root_identifier<'a>(
    element: &'a oxc_ast::ast::ChainElement<'a>,
) -> Option<&'a oxc_ast::ast::IdentifierReference<'a>> {
    match element {
        oxc_ast::ast::ChainElement::StaticMemberExpression(member) => {
            pure_member_root_identifier(&member.object)
        }
        oxc_ast::ast::ChainElement::ComputedMemberExpression(member) => {
            if optional_chain_discarded_expr_has_no_syntactic_effect(&member.expression) {
                pure_member_root_identifier(&member.object)
            } else {
                None
            }
        }
        oxc_ast::ast::ChainElement::PrivateFieldExpression(member) => {
            pure_member_root_identifier(&member.object)
        }
        oxc_ast::ast::ChainElement::CallExpression(_)
        | oxc_ast::ast::ChainElement::TSNonNullExpression(_) => None,
    }
}

/// Widen the fresh literals of a value stored into a MUTABLE slot (an
/// object member, an array element): tsc's literal widening at a mutable
/// location. The widening reaches INTO a branch join — a ternary's fresh
/// literal arm is the slot's fresh literal — but a narrowed reference arm
/// is NOT fresh (the checker's own early-return-guard shapes keep their
/// literal unions), so only leaf arms widen. A fresh `!x` literal is a
/// leaf too.
fn widen_mutable_slot_literals(value: SliceExpr) -> SliceExpr {
    match value {
        SliceExpr::Type(leaf) => SliceExpr::Type(
            leaf.map_ty(verter_semantic::analysis::type_eval_build::widen_shallow_literal),
        ),
        SliceExpr::Not { operand, widen: _ } => SliceExpr::Not {
            operand,
            widen: true,
        },
        SliceExpr::Sequence {
            before,
            value,
            after,
        } => SliceExpr::Sequence {
            before,
            value: Box::new(widen_mutable_slot_literals(*value)),
            after,
        },
        SliceExpr::Logical {
            operator,
            left,
            right,
            guard,
            right_reachable,
            fresh_operands,
            widen: _,
        } => SliceExpr::Logical {
            operator,
            left,
            right,
            guard,
            right_reachable,
            fresh_operands,
            widen: true,
        },
        assignment @ SliceExpr::Assignment { .. } => widen_assignment_value(assignment),
        SliceExpr::Union { arms, guard } => SliceExpr::Union {
            arms: Arc::from(
                arms.iter()
                    .map(|arm| match arm {
                        SliceExpr::Type(leaf) => SliceExpr::Type(leaf.clone().map_ty(
                            verter_semantic::analysis::type_eval_build::widen_shallow_literal,
                        )),
                        SliceExpr::Not { .. } | SliceExpr::Logical { .. } => {
                            widen_mutable_slot_literals(arm.clone())
                        }
                        assignment @ SliceExpr::Assignment { .. } => {
                            widen_assignment_value(assignment.clone())
                        }
                        other => other.clone(),
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            guard,
        },
        value => value,
    }
}

/// A value-position `=` write whose value lands in a mutable slot: the
/// right-hand side's fresh literals widen there (`[(x = "s")]` is
/// `string[]`), while the write itself keeps the literal.
fn widen_assignment_value(assignment: SliceExpr) -> SliceExpr {
    match assignment {
        SliceExpr::Assignment {
            target,
            definition,
            span,
            value,
            freshness,
            widen: _,
        } => SliceExpr::Assignment {
            target,
            definition,
            span,
            value,
            freshness,
            widen: true,
        },
        other => other,
    }
}

/// The type of a bare `null` / `undefined` / `void` value
/// ([`expr_is_widening_nullish`]): `null`, `undefined`, or the union of a
/// conditional's two arms.
fn widening_nullish_type(expression: &Expression<'_>) -> TypeExpr {
    match expression {
        Expression::ParenthesizedExpression(paren) => widening_nullish_type(&paren.expression),
        Expression::TSSatisfiesExpression(satisfies) => {
            widening_nullish_type(&satisfies.expression)
        }
        Expression::ConditionalExpression(conditional) => TypeExpr::Union(Arc::from(vec![
            widening_nullish_type(&conditional.consequent),
            widening_nullish_type(&conditional.alternate),
        ])),
        Expression::NullLiteral(_) => TypeExpr::Primitive(PrimitiveName::Null),
        _ => TypeExpr::Primitive(PrimitiveName::Undefined),
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
        | Expression::BigIntLiteral(_)
        | Expression::BooleanLiteral(_) => true,
        Expression::TemplateLiteral(template) => template.expressions.is_empty(),
        // A signed numeric literal (`-1`, `+1`) and a negated bigint
        // literal (`-1n`) are literals of their own value.
        Expression::UnaryExpression(unary) => matches!(
            (unary.operator, &unary.argument),
            (
                UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus,
                Expression::NumericLiteral(_)
            ) | (UnaryOperator::UnaryNegation, Expression::BigIntLiteral(_))
        ),
        _ => false,
    }
}

/// The top-level FRESHNESS shape of one applied write's right-hand side —
/// the lowering-time input to the evaluator's evolving-target widening
/// rule (an assignment into a binding with NO declared authority widens
/// exactly its FRESH literal positions; every pinned position keeps its
/// literal — the checker's own fresh/regular literal-type split).
///
/// The tree MIRRORS the content lowering's own descent so the per-arm
/// facts align 1:1 with the lowered [`SliceExpr`]: a parenthesis is the
/// one wrapper both walks descend through (`value_descent`'s
/// `Transparent`), and a conditional recurses per branch exactly where
/// the lowering mints `SliceExpr::Union { arms: [consequent, alternate] }`
/// (`value_descent`'s `Branches`). Every other form is one leaf verdict
/// from the shared bare-literal authority — a `satisfies` wrapper stays
/// fresh (the checker preserves freshness through it), a const assertion
/// or type assertion pins. A read of a widening-literal `const` is an
/// EVALUATOR fact (widening-locals membership), never spelled here.
#[derive(Debug, Clone, PartialEq)]
pub enum SliceFreshness {
    /// A bare (fresh) literal position: widens at an evolving target.
    Fresh,
    /// Not a fresh literal position: the literal (if any) stays pinned.
    Pinned,
    /// A bare `null` / `undefined` / `void` value position: no literal to
    /// widen (pinned for every literal rule), but the checker's WIDENING
    /// nullable type — with `strictNullChecks` off, a function whose whole
    /// return is only such values returns `any`.
    WideningNullish,
    /// A conditional expression: per-branch verdicts, aligned with the
    /// lowered [`SliceExpr::Union`] arms (`[consequent, alternate]`).
    PerArm(Arc<[SliceFreshness]>),
}

impl SliceFreshness {
    /// Whether EVERY leaf of the tree is fresh (and the tree is
    /// non-empty) — the classic widening-literal shape.
    #[must_use]
    pub fn all_fresh(&self) -> bool {
        match self {
            Self::Fresh => true,
            Self::Pinned | Self::WideningNullish => false,
            Self::PerArm(arms) => !arms.is_empty() && arms.iter().all(Self::all_fresh),
        }
    }

    /// Whether ANY leaf of the tree is fresh.
    #[must_use]
    pub fn any_fresh(&self) -> bool {
        match self {
            Self::Fresh => true,
            Self::Pinned | Self::WideningNullish => false,
            Self::PerArm(arms) => arms.iter().any(Self::any_fresh),
        }
    }

    /// Whether the tree mixes fresh and pinned leaves — the shape whose
    /// widening decision needs PER-ARM evaluation.
    #[must_use]
    pub fn is_mixed(&self) -> bool {
        self.any_fresh() && !self.all_fresh()
    }

    /// Whether EVERY leaf of the tree is a bare `null` / `undefined` /
    /// `void` value (and the tree is non-empty) — an initializer whose
    /// value is only nullable, whatever the null algebra.
    #[must_use]
    pub fn all_widening_nullish(&self) -> bool {
        match self {
            Self::WideningNullish => true,
            Self::Fresh | Self::Pinned => false,
            Self::PerArm(arms) => !arms.is_empty() && arms.iter().all(Self::all_widening_nullish),
        }
    }
}

/// The freshness mirror for one value expression — an assignment or
/// binding right-hand side, or a `return` argument. See
/// [`SliceFreshness`] for the alignment contract with `lower_expr`.
fn expression_freshness(expression: &Expression<'_>) -> SliceFreshness {
    match expression {
        Expression::ParenthesizedExpression(paren) => expression_freshness(&paren.expression),
        // An `await x` publishes its OPERAND's value through the lib
        // `Awaited` surface, which passes a settled literal through
        // verbatim — so the operand's freshness is the await's own. tsgo
        // types `async function f() { return await 1 }` as
        // `Promise<number>`, exactly as `return 1` is; the pinned-wins
        // fold and the per-arm rule below still apply unchanged, so two
        // awaited fresh arms keep `1 | 2`.
        Expression::AwaitExpression(awaited) => expression_freshness(&awaited.argument),
        // `!x` is the checker's FRESH `true` / `false` (or `boolean`).
        Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::LogicalNot => {
            SliceFreshness::Fresh
        }
        // An `=` assignment's value is its right-hand side's.
        Expression::AssignmentExpression(assignment)
            if assignment.operator == oxc_ast::ast::AssignmentOperator::Assign =>
        {
            expression_freshness(&assignment.right)
        }
        Expression::ConditionalExpression(conditional) => SliceFreshness::PerArm(Arc::from([
            expression_freshness(&conditional.consequent),
            expression_freshness(&conditional.alternate),
        ])),
        _ => {
            if expr_is_bare_literal(expression) {
                SliceFreshness::Fresh
            } else if expr_is_widening_nullish(expression) {
                SliceFreshness::WideningNullish
            } else {
                SliceFreshness::Pinned
            }
        }
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

/// Whether a labeled statement's body chain terminates at a loop it
/// DIRECTLY wraps: a chain of labeled statements whose terminal body is
/// the loop itself, with nothing in between. A `break` naming such a
/// label is the loop's own exit — the label's continuation IS the loop's
/// fall-through point — and a `continue` naming it is the loop's own
/// iteration edge, so neither transfers control past lowered content.
fn label_directly_wraps_loop(body: &Statement<'_>) -> bool {
    let mut body = body;
    loop {
        match body {
            Statement::LabeledStatement(labeled) => body = &labeled.body,
            Statement::DoWhileStatement(_)
            | Statement::ForInStatement(_)
            | Statement::ForOfStatement(_)
            | Statement::ForStatement(_)
            | Statement::WhileStatement(_) => return true,
            _ => return false,
        }
    }
}

/// Whether the loop statement's tree contains a labeled `break` /
/// `continue` whose target resolves OUTSIDE the loop: the label is
/// neither defined within the walked tree nor one of `direct_labels`
/// (the labels directly wrapping the walked loop — see
/// [`label_directly_wraps_loop`]). Such a jump exits THROUGH the
/// transparent summary into an enclosing lowered construct: the loop's
/// body vanishes with the lowering, so the labeled exit edge would
/// vanish with it — the enclosing `Labeled`'s `may_break` never records
/// the exit, contributors reachable only through the break are dropped,
/// and code the break skips is treated reachable (measured:
/// `outer: { for (;;) { break outer } return 0 } return x` on
/// `x: string | null` is `string | 0 | null`; transparency published
/// `number`). The loop takes the typed refusal instead, exactly like a
/// return-bearing one — this deliberately does not carry the edge.
///
/// Nested function/class frames are never entered, and need not be: a
/// label cannot cross a function boundary, so every labeled jump this
/// walk can see belongs to the walked frame, and a jump inside a nested
/// frame can only target a label inside that frame. Unlabeled jumps
/// always bind within the loop (the loop itself, or a nested
/// loop/switch) and never escape it.
/// Whether a loop's own normal-exit edge is statically unreachable, so the
/// loop is entered and never completes normally.
///
/// This mirrors the checker's binder rule and deliberately does NOT
/// generalise it. The binder marks the exit edge unreachable when the
/// condition is the bare `true` KEYWORD, or (for `for`) absent; it neither
/// skips parentheses nor evaluates truthiness. `while (1)` and
/// `while ((true))` therefore keep a reachable exit and keep contributing
/// the implicit `undefined`, exactly as the checker does.
///
/// The exit is also reachable whenever a `break` can target THIS loop: an
/// unlabeled `break` whose innermost enclosing breakable construct is the
/// loop itself, or a labeled `break` naming a label that wraps it. A
/// `continue`, a `break` captured by a nested loop, and a `break` captured
/// by a nested `switch` all leave the exit unreachable.
fn loop_exit_edge_is_unreachable(
    loop_statement: &Statement<'_>,
    wrapping_labels: &[Arc<str>],
) -> bool {
    let body = match loop_statement {
        Statement::WhileStatement(while_stmt) => {
            if !matches!(&while_stmt.test, Expression::BooleanLiteral(literal) if literal.value) {
                return false;
            }
            &while_stmt.body
        }
        Statement::DoWhileStatement(do_while) => {
            if !matches!(&do_while.test, Expression::BooleanLiteral(literal) if literal.value) {
                return false;
            }
            &do_while.body
        }
        Statement::ForStatement(for_stmt) => {
            if for_stmt.test.is_some() {
                return false;
            }
            &for_stmt.body
        }
        // `for..in` / `for..of` complete normally once the iterated value is
        // exhausted, so their exit edge is always reachable.
        _ => return false,
    };
    !loop_body_reaches_exit(body, wrapping_labels, &mut Vec::new(), 0)
}

/// Whether `statement`, appearing inside a loop body, can transfer control
/// to that loop's exit edge.
///
/// `enclosing_breakables` counts the breakable constructs entered since the
/// loop body, so an unlabeled `break` is bound to the loop exactly at zero.
/// It is a LEXICAL binding fact, not a recursion budget: the walk is finite
/// in the statement tree and has no cutoff.
/// `nested_labels` are the labels declared between the loop body and
/// `statement`; `wrapping_labels` are the labels wrapping the loop itself.
///
/// Detection is biased toward finding a reaching `break`: an unrecognised
/// statement form is treated as possibly carrying one, which preserves the
/// fall-through answer rather than asserting divergence.
fn loop_body_reaches_exit<'a>(
    statement: &'a Statement<'a>,
    wrapping_labels: &[Arc<str>],
    nested_labels: &mut Vec<&'a str>,
    enclosing_breakables: u32,
) -> bool {
    let recurse =
        |statement: &'a Statement<'a>, nested_labels: &mut Vec<&'a str>, enclosing_breakables| {
            loop_body_reaches_exit(
                statement,
                wrapping_labels,
                nested_labels,
                enclosing_breakables,
            )
        };
    match statement {
        Statement::BreakStatement(break_stmt) => match break_stmt.label.as_ref() {
            None => enclosing_breakables == 0,
            // A label declared inside the body names an inner construct, so
            // a break naming it cannot reach the loop's own exit.
            Some(label) => {
                let name = label.name.as_str();
                !nested_labels.contains(&name)
                    && wrapping_labels.iter().any(|wrapping| &**wrapping == name)
            }
        },
        // A `continue` re-enters the loop; it never reaches the exit edge.
        Statement::ContinueStatement(_) => false,
        Statement::LabeledStatement(labeled) => {
            nested_labels.push(labeled.label.name.as_str());
            let reaches = recurse(&labeled.body, nested_labels, enclosing_breakables);
            nested_labels.pop();
            reaches
        }
        Statement::BlockStatement(block) => block
            .body
            .iter()
            .any(|statement| recurse(statement, nested_labels, enclosing_breakables)),
        Statement::IfStatement(if_stmt) => {
            recurse(&if_stmt.consequent, nested_labels, enclosing_breakables)
                || if_stmt.alternate.as_ref().is_some_and(|alternate| {
                    recurse(alternate, nested_labels, enclosing_breakables)
                })
        }
        Statement::WithStatement(with_stmt) => {
            recurse(&with_stmt.body, nested_labels, enclosing_breakables)
        }
        Statement::TryStatement(try_stmt) => {
            try_stmt
                .block
                .body
                .iter()
                .any(|statement| recurse(statement, nested_labels, enclosing_breakables))
                || try_stmt.handler.as_ref().is_some_and(|handler| {
                    handler
                        .body
                        .body
                        .iter()
                        .any(|statement| recurse(statement, nested_labels, enclosing_breakables))
                })
                || try_stmt.finalizer.as_ref().is_some_and(|finalizer| {
                    finalizer
                        .body
                        .iter()
                        .any(|statement| recurse(statement, nested_labels, enclosing_breakables))
                })
        }
        // A nested breakable construct captures an unlabeled `break`.
        Statement::SwitchStatement(switch) => switch.cases.iter().any(|case| {
            case.consequent
                .iter()
                .any(|statement| recurse(statement, nested_labels, enclosing_breakables + 1))
        }),
        Statement::DoWhileStatement(do_while) => {
            recurse(&do_while.body, nested_labels, enclosing_breakables + 1)
        }
        Statement::WhileStatement(while_stmt) => {
            recurse(&while_stmt.body, nested_labels, enclosing_breakables + 1)
        }
        Statement::ForStatement(for_stmt) => {
            recurse(&for_stmt.body, nested_labels, enclosing_breakables + 1)
        }
        Statement::ForInStatement(for_in) => {
            recurse(&for_in.body, nested_labels, enclosing_breakables + 1)
        }
        Statement::ForOfStatement(for_of) => {
            recurse(&for_of.body, nested_labels, enclosing_breakables + 1)
        }
        // Forms that cannot lexically carry a `break` bound to an enclosing
        // loop. A nested function body is excluded by the grammar.
        Statement::DebuggerStatement(_)
        | Statement::EmptyStatement(_)
        | Statement::ExpressionStatement(_)
        | Statement::ReturnStatement(_)
        | Statement::ThrowStatement(_)
        | Statement::VariableDeclaration(_)
        | Statement::FunctionDeclaration(_)
        | Statement::ClassDeclaration(_) => false,
        // Anything else is treated as possibly carrying a reaching break,
        // which keeps the exit reachable and preserves today's answer.
        _ => true,
    }
}

fn loop_transfers_to_enclosing_label(
    loop_statement: &Statement<'_>,
    direct_labels: &[Arc<str>],
) -> bool {
    fn target_escapes(
        label: Option<&oxc_ast::ast::LabelIdentifier<'_>>,
        locals: &[&str],
        direct: &[Arc<str>],
    ) -> bool {
        let Some(label) = label else {
            return false;
        };
        let name = label.name.as_str();
        !locals.contains(&name) && !direct.iter().any(|direct| &**direct == name)
    }
    fn walk_statement<'a>(
        statement: &'a Statement<'a>,
        locals: &mut Vec<&'a str>,
        direct: &[Arc<str>],
    ) -> bool {
        match statement {
            Statement::BreakStatement(break_stmt) => {
                target_escapes(break_stmt.label.as_ref(), locals, direct)
            }
            Statement::ContinueStatement(continue_stmt) => {
                target_escapes(continue_stmt.label.as_ref(), locals, direct)
            }
            Statement::LabeledStatement(labeled) => {
                locals.push(labeled.label.name.as_str());
                let escapes = walk_statement(&labeled.body, locals, direct);
                locals.pop();
                escapes
            }
            Statement::BlockStatement(block) => block
                .body
                .iter()
                .any(|statement| walk_statement(statement, locals, direct)),
            Statement::IfStatement(if_stmt) => {
                walk_statement(&if_stmt.consequent, locals, direct)
                    || if_stmt
                        .alternate
                        .as_ref()
                        .is_some_and(|alternate| walk_statement(alternate, locals, direct))
            }
            Statement::DoWhileStatement(do_while) => walk_statement(&do_while.body, locals, direct),
            Statement::WhileStatement(while_stmt) => {
                walk_statement(&while_stmt.body, locals, direct)
            }
            Statement::ForStatement(for_stmt) => walk_statement(&for_stmt.body, locals, direct),
            Statement::ForInStatement(for_in) => walk_statement(&for_in.body, locals, direct),
            Statement::ForOfStatement(for_of) => walk_statement(&for_of.body, locals, direct),
            Statement::SwitchStatement(switch) => switch.cases.iter().any(|case| {
                case.consequent
                    .iter()
                    .any(|statement| walk_statement(statement, locals, direct))
            }),
            Statement::TryStatement(try_stmt) => {
                try_stmt
                    .block
                    .body
                    .iter()
                    .any(|statement| walk_statement(statement, locals, direct))
                    || try_stmt.handler.as_ref().is_some_and(|handler| {
                        handler
                            .body
                            .body
                            .iter()
                            .any(|statement| walk_statement(statement, locals, direct))
                    })
                    || try_stmt.finalizer.as_ref().is_some_and(|finalizer| {
                        finalizer
                            .body
                            .iter()
                            .any(|statement| walk_statement(statement, locals, direct))
                    })
            }
            Statement::WithStatement(with_stmt) => walk_statement(&with_stmt.body, locals, direct),
            _ => false,
        }
    }
    walk_statement(loop_statement, &mut Vec::new(), direct_labels)
}

/// Whether entering this statement guarantees that the current function
/// reaches an authored return before normal completion.
///
/// THREE states, because two of them conflated the only distinction that
/// matters here. A pending `break` whose destination is PROVED to reach
/// the function end contributes an implicit `undefined`; a destination
/// this lowering cannot classify proves nothing, and answering "does not
/// return" for it fabricated that contributor out of a coverage gap. The
/// measured consequence, over one base program's suffix spellings: a
/// `return` / block / `if` suffix published `"a" | "b"`, while a LABELED,
/// `try`, `throw` or `switch` suffix published `"a" | undefined` — so
/// merely LABELING a block changed the answer, and the wrong answer was
/// admitted warm because nothing marked it.
///
/// [`SuffixReturn::Undecided`] is the fail-closed disposition that class
/// requires: the caller keeps the derivation's value and mints
/// [`crate::semantic_query::FlowGap::AbruptCompletion`], so the result is
/// still returned and is never admitted. Deciding those forms HERE is
/// forbidden — a syntax-only completion classifier is exactly the second
/// completion authority this substrate must not have, and the answer
/// belongs to the demanded `FunctionFlowGraph` reduction that owes the
/// abrupt-completion topology.
///
/// Deliberately stricter than the control inventory's `has_return`: a
/// conditional return does not prevent a preceding labelled break from
/// reaching function end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SuffixReturn {
    /// Proved: entering the statement reaches an authored return.
    Guaranteed,
    /// Proved: the statement completes normally without returning, so
    /// control continues to whatever follows it.
    NotGuaranteed,
    /// Not decidable from statement shape alone, and never answered as
    /// either proof.
    Undecided,
}

impl SuffixReturn {
    /// Fold the statements a destination continues into. A proved return
    /// anywhere ahead dominates — control cannot pass it — and otherwise a
    /// single undecided statement makes the whole suffix undecided.
    fn fold(statements: impl Iterator<Item = Self>) -> Self {
        let mut undecided = false;
        for statement in statements {
            match statement {
                Self::Guaranteed => return Self::Guaranteed,
                Self::Undecided => undecided = true,
                Self::NotGuaranteed => {}
            }
        }
        if undecided {
            Self::Undecided
        } else {
            Self::NotGuaranteed
        }
    }
}

fn suffix_return_of(statement: &Statement<'_>) -> SuffixReturn {
    match statement {
        Statement::ReturnStatement(_) => SuffixReturn::Guaranteed,
        Statement::BlockStatement(block) => {
            SuffixReturn::fold(block.body.iter().map(suffix_return_of))
        }
        Statement::IfStatement(branch) => {
            let consequent = suffix_return_of(&branch.consequent);
            // A missing `else` arm completes normally by definition.
            let alternate = branch
                .alternate
                .as_ref()
                .map_or(SuffixReturn::NotGuaranteed, suffix_return_of);
            match (consequent, alternate) {
                (SuffixReturn::Guaranteed, SuffixReturn::Guaranteed) => SuffixReturn::Guaranteed,
                (SuffixReturn::Undecided, _) | (_, SuffixReturn::Undecided) => {
                    SuffixReturn::Undecided
                }
                _ => SuffixReturn::NotGuaranteed,
            }
        }
        // Forms that complete normally without returning, so a destination
        // reaching one of them really does reach the function end. The one
        // residual is an expression statement whose call is proven `never`:
        // that is the typed terminator feed the graph reduction still owes,
        // and it is not decidable from the statement's shape either.
        Statement::EmptyStatement(_)
        | Statement::DebuggerStatement(_)
        | Statement::ExpressionStatement(_)
        | Statement::VariableDeclaration(_)
        | Statement::FunctionDeclaration(_)
        | Statement::ClassDeclaration(_)
        | Statement::TSTypeAliasDeclaration(_)
        | Statement::TSInterfaceDeclaration(_)
        | Statement::TSEnumDeclaration(_) => SuffixReturn::NotGuaranteed,
        // A labeled statement, a `try`, a `throw`, a `switch`, any loop,
        // any jump, and any form added later. Each carries completion this
        // lowering cannot reduce, so none of them is answered here.
        _ => SuffixReturn::Undecided,
    }
}

/// The names THIS signature's parameter list binds, paired with their
/// binding-identifier spans, read from the frame's own
/// [`FunctionBodySkeleton`] — the SAME single lexical authority every
/// other classification in this module routes through. A DESTRUCTURED
/// element is inventoried exactly like a plain binding identifier: the
/// checker resolves `typeof a` in `f({ a }: { a: number }, b: typeof a)`
/// to the destructured element.
#[cfg(test)]
pub(crate) mod capture_lookup_probe {
    use std::cell::RefCell;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    thread_local! {
        static ACTIVE: RefCell<Option<Arc<AtomicUsize>>> = const { RefCell::new(None) };
    }
    pub(crate) struct Scope(Option<Arc<AtomicUsize>>);
    impl Drop for Scope {
        fn drop(&mut self) {
            ACTIVE.with(|active| *active.borrow_mut() = self.0.take());
        }
    }
    pub(crate) fn enter(counter: Arc<AtomicUsize>) -> Scope {
        Scope(ACTIVE.with(|active| active.replace(Some(counter))))
    }
    pub(crate) fn inspect() {
        ACTIVE.with(|active| {
            if let Some(counter) = active.borrow().as_ref() {
                counter.fetch_add(1, Ordering::Relaxed);
            }
        });
    }
}

fn signature_parameter_bindings(
    skeleton: &FunctionBodySkeleton,
    anchor: u32,
) -> rustc_hash::FxHashMap<Arc<str>, u32> {
    let mut parameters = rustc_hash::FxHashMap::default();
    for binding in skeleton.bindings.iter() {
        #[cfg(test)]
        capture_lookup_probe::inspect();
        if binding.kind == SkeletonBindingKind::Param {
            let end = binding.span.to_absolute(anchor).end;
            parameters
                .entry(Arc::from(skeleton.name(binding.name)))
                .and_modify(|earliest: &mut u32| *earliest = (*earliest).min(end))
                .or_insert(end);
        }
    }
    parameters
}

/// Rebase a LIVE source span onto a function's own anchor.
fn rebase_span(anchor: u32, span: oxc_span::Span) -> FrameSpan {
    FrameSpan::rebase(anchor, span.into())
}

/// The static member path of a chain rooted at `this` (`this.a.b` is
/// `[a, b]`); `None` for any other chain.
/// One member an object literal's `this` names.
enum ObjectThisMember<'p> {
    /// A property, whose value its declaration initializes it with.
    Property(&'p Expression<'p>),
    /// A method, served as its own position.
    Method(verter_semantic::analysis::function_program::FunctionProgramKey),
    /// A getter, served as its own position.
    Getter(verter_semantic::analysis::function_program::FunctionProgramKey),
}

fn this_member_path(member: &oxc_ast::ast::StaticMemberExpression<'_>) -> Option<Vec<Arc<str>>> {
    let mut path = vec![Arc::from(member.property.name.as_str())];
    let mut object = &member.object;
    loop {
        match object {
            Expression::ThisExpression(_) => break,
            Expression::StaticMemberExpression(parent) => {
                path.push(Arc::from(parent.property.name.as_str()));
                object = &parent.object;
            }
            _ => return None,
        }
    }
    path.reverse();
    Some(path)
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
    parameters: &rustc_hash::FxHashMap<Arc<str>, u32>,
    visible_before: Option<u32>,
) -> Vec<FrameShadowedName> {
    let names = verter_type_expr::referenced_names(ty);
    let mut out: Vec<FrameShadowedName> = Vec::new();
    for root in &names.value_roots {
        #[cfg(test)]
        capture_lookup_probe::inspect();
        let bound = parameters
            .get(root.as_str())
            .is_some_and(|end| visible_before.is_none_or(|limit| *end <= limit));
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
    scope: &SignatureScope<'_>,
    skeleton: &FunctionBodySkeleton,
    bindings: &verter_semantic::analysis::flow::FlowBindingMap,
    anchor: u32,
    nullability: crate::semantic_query::NullabilityPolicy,
) -> Result<Vec<SliceParam>, verter_type_expr::facts::InferenceUnavailableReason> {
    let binders = scope.param_binders();
    let parameter_bindings = signature_parameter_bindings(skeleton, anchor);
    let mut out = Vec::with_capacity(params.items.len() + usize::from(params.rest.is_some()));
    for param in &params.items {
        let name = match &param.pattern {
            BindingPattern::BindingIdentifier(id) => Some(Arc::from(id.name.as_str())),
            _ => None,
        };
        // The modelled elements of a destructured OBJECT pattern:
        // identifier bindings (`{ label }` / `{ label = "x" }`, aliases
        // included) keyed by a static member name. Nested, computed, and
        // rest elements stay unmodelled — their reads keep the fail-closed
        // classification they have today.
        let destructured: Arc<[SliceDestructuredElement]> = match &param.pattern {
            BindingPattern::ObjectPattern(object) => object
                .properties
                .iter()
                .filter_map(|property| {
                    if property.computed {
                        return None;
                    }
                    let key = match &property.key {
                        oxc_ast::ast::PropertyKey::StaticIdentifier(id) => {
                            Arc::from(id.name.as_str())
                        }
                        oxc_ast::ast::PropertyKey::StringLiteral(literal) => {
                            Arc::from(literal.value.as_str())
                        }
                        _ => return None,
                    };
                    let (binding, binding_span, has_default) = match &property.value {
                        BindingPattern::BindingIdentifier(id) => {
                            (Arc::from(id.name.as_str()), id.span, false)
                        }
                        BindingPattern::AssignmentPattern(assignment) => {
                            match &assignment.left {
                                BindingPattern::BindingIdentifier(id) => {
                                    (Arc::from(id.name.as_str()), id.span, true)
                                }
                                // A default over an ALIASED / nested
                                // pattern is not modelled.
                                _ => return None,
                            }
                        }
                        _ => return None,
                    };
                    Some(SliceDestructuredElement {
                        binding: bindings
                            .declaration_at_span(FrameSpan::rebase(anchor, binding_span.into()))?,
                        name: binding,
                        key,
                        has_default,
                    })
                })
                .collect(),
            _ => Arc::from(Vec::new().into_boxed_slice()),
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
                        )?,
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
        // An optional (`?`) parameter is `T | undefined` inside the body
        // under `strictNullChecks`; with it off `undefined` is already a
        // member of every type and the checker adds nothing. A defaulted
        // parameter always has a value. The union rides the SAME gate
        // verdict: adding `undefined` names nothing new.
        let ty = if param.optional && param.initializer.is_none() && nullability.is_strict() {
            GatedType {
                ty: TypeExpr::union(vec![ty.ty, TypeExpr::Primitive(PrimitiveName::Undefined)]),
                shadowed: ty.shadowed,
            }
        } else {
            ty
        };
        out.push(SliceParam {
            binding: match &param.pattern {
                BindingPattern::BindingIdentifier(id) => {
                    bindings.declaration_at_span(FrameSpan::rebase(anchor, id.span.into()))
                }
                _ => None,
            },
            name,
            optional: param.optional || param.initializer.is_some(),
            rest: false,
            ty,
            destructured,
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
            binding: match &rest.rest.argument {
                BindingPattern::BindingIdentifier(id) => {
                    bindings.declaration_at_span(FrameSpan::rebase(anchor, id.span.into()))
                }
                _ => None,
            },
            name,
            optional: false,
            rest: true,
            ty,
            destructured: Arc::from(Vec::new().into_boxed_slice()),
        });
    }
    Ok(out)
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

/// The spans of the statements the checker's `extendAssignmentPosition`
/// extends an assignment to (a variable or expression statement, an `if`,
/// a loop, a `with`, a `switch`, a `try`, a class declaration), across the
/// frame's own statements — never inside a nested function.
fn collect_assignment_extent_statements(
    statements: &[Statement<'_>],
    out: &mut Vec<oxc_span::Span>,
) {
    for statement in statements {
        let listed = matches!(
            statement,
            Statement::VariableDeclaration(_)
                | Statement::ExpressionStatement(_)
                | Statement::IfStatement(_)
                | Statement::DoWhileStatement(_)
                | Statement::WhileStatement(_)
                | Statement::ForStatement(_)
                | Statement::ForInStatement(_)
                | Statement::ForOfStatement(_)
                | Statement::WithStatement(_)
                | Statement::SwitchStatement(_)
                | Statement::TryStatement(_)
                | Statement::ClassDeclaration(_)
        );
        if listed {
            out.push(statement.span());
        }
        let mut nested = |statement: &Statement<'_>| {
            collect_assignment_extent_statements(std::slice::from_ref(statement), out);
        };
        match statement {
            Statement::BlockStatement(block) => {
                collect_assignment_extent_statements(&block.body, out);
            }
            Statement::IfStatement(if_stmt) => {
                nested(&if_stmt.consequent);
                if let Some(alternate) = &if_stmt.alternate {
                    nested(alternate);
                }
            }
            Statement::DoWhileStatement(loop_stmt) => nested(&loop_stmt.body),
            Statement::WhileStatement(loop_stmt) => nested(&loop_stmt.body),
            Statement::ForStatement(loop_stmt) => nested(&loop_stmt.body),
            Statement::ForInStatement(loop_stmt) => nested(&loop_stmt.body),
            Statement::ForOfStatement(loop_stmt) => nested(&loop_stmt.body),
            Statement::WithStatement(with_stmt) => nested(&with_stmt.body),
            Statement::LabeledStatement(labeled) => nested(&labeled.body),
            Statement::SwitchStatement(switch) => {
                for case in &switch.cases {
                    collect_assignment_extent_statements(&case.consequent, out);
                }
            }
            Statement::TryStatement(try_stmt) => {
                collect_assignment_extent_statements(&try_stmt.block.body, out);
                if let Some(handler) = &try_stmt.handler {
                    collect_assignment_extent_statements(&handler.body.body, out);
                }
                if let Some(finalizer) = &try_stmt.finalizer {
                    collect_assignment_extent_statements(&finalizer.body, out);
                }
            }
            _ => {}
        }
    }
}

/// What [`Lowerer::lower_evolving_operation`] made of an expression.
enum EvolvingLowering {
    /// An operation on a selected EVOLVING array.
    Operation(SliceEvolvingOperation),
    /// An operation on an evolving array the demand did not select: its
    /// operands' effects were scanned, and nothing retypes.
    Unselected,
    /// Not an evolving-array operation.
    NotEvolving,
}

/// Whether an assignment is `x = []` — the unparenthesized empty array
/// literal that starts a new EVOLVING array when `x` is one.
fn evolving_reset_assignment(assignment: &oxc_ast::ast::AssignmentExpression<'_>) -> bool {
    assignment.operator == oxc_ast::ast::AssignmentOperator::Assign
        && verter_semantic::analysis::flow::assignment_target_binding(&assignment.left).is_some()
        && verter_semantic::analysis::flow::is_evolving_array_initializer(Some(&assignment.right))
}

/// The region lowering result: the region plus whether any nested lowering
/// hit an unsupported construct (the marker is in the tree; the flag
/// propagates so the root region stops at the same point).
/// One lowered loop and what it tells the enclosing region.
struct LoweredLoop {
    lowered: SliceLoop,
    hit_unsupported: bool,
    /// The named `break` exits the body may take past the loop.
    may_break: Vec<SliceBreakTarget>,
    /// The loop can complete normally: its test can fail, it iterates a
    /// value, or a `break` targets it.
    completes: bool,
}

struct LoweredRegion {
    region: SliceRegion,
    hit_unsupported: bool,
    /// The `break` exits a path through the region may take, not yet
    /// absorbed by the construct they target. The lowering of the target
    /// construct (a `switch` case's anonymous exit, a labeled statement's
    /// named one) absorbs its own entries; every other construct
    /// propagates them upward untouched.
    may_break: Vec<SliceBreakTarget>,
}

/// A `break` target one lowered region's path may exit to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SliceBreakTarget {
    /// The innermost ANONYMOUS breakable — a `switch` (loop bodies never
    /// lower, so a loop is never on the target stack).
    Anonymous,
    /// A labeled statement, by name.
    Named(Arc<str>),
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
    /// capture), read through its exact identity in the child input snapshot.
    Captured,
    /// A hoisted nested function declaration of this frame binds the
    /// name; it shadows every outer same-name declaration.
    NestedFunction,
    /// A resolved function-local binding this content half cannot model.
    Unmodeled,
}

/// A shared structural lexical frame. It contains no lowered annotation or
/// semantic value and is queried only for names the selected content uses.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DefiningFrameGate {
    skeleton: Arc<FunctionBodySkeleton>,
    bindings: Arc<verter_semantic::analysis::flow::FlowBindingMap>,
    type_parameters: Arc<[Arc<str>]>,
    /// The ENCLOSING declaration's clause, by name: a class member's
    /// class clause (`class C<T> { m() { … } }`), empty otherwise.
    enclosing_type_parameters: Arc<[Arc<str>]>,
    parameters: Arc<rustc_hash::FxHashMap<SkeletonBindingId, CaptureParameterLocator>>,
    parameter_names: Arc<rustc_hash::FxHashMap<Arc<str>, u32>>,
    /// The bindings a destructuring declarator binds whose whole pattern
    /// this half models ([`SlicePattern`]).
    modelled_patterns: Arc<FxHashSet<SkeletonBindingId>>,
    body_hash: [u8; 16],
    snapshot: crate::decl_lowering::SnapshotKey,
    outer: CaptureScope,
    anchor: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureParameterLocator {
    binding: SkeletonBindingId,
    ordinal: usize,
    key: Option<Arc<str>>,
    has_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapturedFrame {
    gate: Arc<DefiningFrameGate>,
    region: verter_semantic::analysis::flow::SkeletonRegionId,
}

/// What `this` reads inside a class declaration's member: the checker's
/// receiver for the member (`checkThisExpression`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SliceThis {
    /// An instance member of the class `class` (its registered
    /// declaration name) declaring `type_parameters`: the class's
    /// polymorphic `this` type, whose constraint is the class instance.
    Instance {
        class: Arc<str>,
        type_parameters: Arc<[Arc<str>]>,
    },
    /// A static member: the class constructor, `typeof class`. A top-level
    /// class is statement `contributor`; a static member read off `this`
    /// lowers from the static member it names there.
    Static {
        class: Arc<str>,
        contributor: Option<u32>,
    },
    /// A method or accessor of the object literal a variable declares:
    /// the variable's value, `typeof value`. The literal is the initializer
    /// of declarator `declarator` of top-level statement `contributor`;
    /// a member read off `this` lowers from the member it names there.
    Value {
        value: Arc<str>,
        contributor: u32,
        declarator: u32,
    },
    /// An instance member of a class EXPRESSION, or a method or accessor of
    /// an object literal: the instance (or object) the evaluator binds
    /// while it evaluates the class's members (or the literal's).
    Receiver,
}

/// The exact lexical chain at a nested function's authored position.
/// Shared frame handles avoid enumerating or copying visible declarations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CaptureScope {
    enclosing: Option<Arc<CapturedFrame>>,
}

/// Owned content-free lexical context at the exact nested function position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedFlowContext {
    captures: CaptureScope,
    /// The lexical `this` an ARROW created in a class member reads; `None`
    /// for every other nested function, whose `this` is its own.
    this: Option<SliceThis>,
}

impl NestedFlowContext {
    /// What `this` reads inside the nested function.
    pub(crate) fn this(&self) -> Option<&SliceThis> {
        self.this.as_ref()
    }
}

/// An exact source declaration eligible to provide a selected capture's type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SliceCaptureAuthorityLocator {
    pub binding: verter_semantic::analysis::function_program::FlowBindingIdentity,
    pub declaration: verter_semantic::analysis::function_program::FlowBindingIdentity,
    pub source: SliceCaptureAuthoritySource,
    parameter_ordinal: Option<usize>,
    gate: Arc<DefiningFrameGate>,
}

impl SliceCaptureAuthorityLocator {
    pub(crate) fn local_declaration(&self) -> Option<SkeletonBindingId> {
        self.gate.bindings.local(&self.declaration)
    }

    pub(crate) fn matches_snapshot(&self, snapshot: &crate::decl_lowering::SnapshotKey) -> bool {
        &self.gate.snapshot == snapshot
    }
}

impl NestedFlowContext {
    pub(crate) fn matches_snapshot(&self, snapshot: &crate::decl_lowering::SnapshotKey) -> bool {
        self.captures
            .enclosing
            .as_ref()
            .is_some_and(|frame| &frame.gate.snapshot == snapshot)
    }

    pub(crate) fn mutable_authorities(
        &self,
        identity: &verter_semantic::analysis::function_program::FlowBindingIdentity,
    ) -> Vec<SliceCaptureAuthorityLocator> {
        let mut current = self.captures.enclosing.as_deref();
        while let Some(frame) = current {
            if frame.gate.bindings.function() == &identity.defining_function {
                let Some(local) = frame.gate.bindings.local(identity) else {
                    return Vec::new();
                };
                let mut authorities: Vec<_> = frame
                    .gate
                    .bindings
                    .runtime_declarations(local)
                    .iter()
                    .copied()
                    .filter_map(|declaration| {
                        #[cfg(test)]
                        capture_lookup_probe::inspect();
                        let fact = frame.gate.skeleton.binding(declaration);
                        #[cfg(test)]
                        capture_lookup_probe::inspect();
                        let parameter = frame.gate.parameters.get(&declaration);
                        let source = match fact.kind {
                            SkeletonBindingKind::Param => {
                                let parameter = parameter?;
                                SliceCaptureAuthoritySource::Parameter {
                                    key: parameter.key.clone(),
                                    has_default: parameter.has_default,
                                }
                            }
                            SkeletonBindingKind::Let
                                if !fact.destructured && fact.annotation_span.is_some() =>
                            {
                                SliceCaptureAuthoritySource::Local(SliceBindingKind::Let)
                            }
                            SkeletonBindingKind::Var
                                if !fact.destructured && fact.annotation_span.is_some() =>
                            {
                                SliceCaptureAuthoritySource::Local(SliceBindingKind::Var)
                            }
                            _ => return None,
                        };
                        Some(SliceCaptureAuthorityLocator {
                            binding: identity.clone(),
                            declaration: frame.gate.bindings.identity(declaration)?.clone(),
                            source,
                            parameter_ordinal: parameter.map(|parameter| parameter.ordinal),
                            gate: Arc::clone(&frame.gate),
                        })
                    })
                    .collect();
                // The parameter is the declared authority of its runtime alias.
                authorities.sort_by_key(|authority| {
                    !matches!(
                        authority.source,
                        SliceCaptureAuthoritySource::Parameter { .. }
                    )
                });
                return authorities;
            }
            current = frame.gate.outer.enclosing.as_deref();
        }
        Vec::new()
    }
}

impl CaptureScope {
    fn gate(&self, ty: TypeExpr, binders: &[Arc<str>]) -> GatedType {
        let names = verter_type_expr::referenced_names(&ty);
        let mut shadowed = Vec::new();
        for name in names.value_roots {
            if !matches!(self.lookup(&name), NameBinding::Free) {
                shadowed.push(FrameShadowedName::Value(Arc::from(name)));
            }
        }
        for occurrence in names.type_names {
            let name = occurrence.head.as_str();
            let meaning = if occurrence.qualified {
                NameMeaning::Namespace
            } else {
                NameMeaning::Type
            };
            let bound = if binders.iter().any(|binder| binder.as_ref() == name) {
                occurrence.qualified
            } else {
                self.name_is_bound(name, meaning)
            };
            let entry = if occurrence.qualified {
                FrameShadowedName::Namespace(Arc::from(name))
            } else {
                FrameShadowedName::Type(Arc::from(name))
            };
            if bound && !shadowed.contains(&entry) {
                shadowed.push(entry);
            }
        }
        GatedType {
            ty,
            shadowed: Arc::from(shadowed),
        }
    }

    fn binding(&self, name: &str) -> Option<(&CapturedFrame, Vec<SkeletonBindingId>)> {
        let mut current = self.enclosing.as_deref();
        while let Some(frame) = current {
            if let Some(name) = frame.gate.skeleton.name_id(name) {
                let resolved = frame
                    .gate
                    .skeleton
                    .bindings_of_name_in_scope(name, frame.region);
                if !resolved.is_empty() {
                    return Some((frame, resolved));
                }
            }
            current = frame.gate.outer.enclosing.as_deref();
        }
        None
    }

    /// The enclosing frame that declares `identity`, with the binding's
    /// local slot there.
    fn defining_local(
        &self,
        identity: &verter_semantic::analysis::function_program::FlowBindingIdentity,
    ) -> Option<(&DefiningFrameGate, SkeletonBindingId)> {
        let mut current = self.enclosing.as_deref();
        while let Some(frame) = current {
            if frame.gate.bindings.function() == &identity.defining_function {
                let local = frame.gate.bindings.local(identity)?;
                return Some((&frame.gate, local));
            }
            current = frame.gate.outer.enclosing.as_deref();
        }
        None
    }

    fn classify_identity(
        &self,
        identity: &verter_semantic::analysis::function_program::FlowBindingIdentity,
    ) -> NameBinding {
        let mut current = self.enclosing.as_deref();
        while let Some(frame) = current {
            if frame.gate.bindings.function() == &identity.defining_function {
                let Some(local) = frame.gate.bindings.local(identity) else {
                    return NameBinding::Unmodeled;
                };
                let local = frame.gate.bindings.canonical_local(local);
                let fact = frame.gate.skeleton.binding(local);
                return if frame.gate.destructured_var_is_modelled(local)
                    && ((fact.kind == SkeletonBindingKind::Param
                        && (frame.gate.parameters.contains_key(&local)
                            || frame.gate.modelled_patterns.contains(&local)))
                        || ((!fact.destructured || frame.gate.modelled_patterns.contains(&local))
                            && matches!(
                                fact.kind,
                                SkeletonBindingKind::Const
                                    | SkeletonBindingKind::Let
                                    | SkeletonBindingKind::Var
                            ))
                        || (!fact.destructured && fact.kind == SkeletonBindingKind::CatchParam))
                {
                    NameBinding::Captured
                } else {
                    NameBinding::Unmodeled
                };
            }
            current = frame.gate.outer.enclosing.as_deref();
        }
        NameBinding::Unmodeled
    }

    fn lookup(&self, name: &str) -> NameBinding {
        let Some((frame, resolved)) = self.binding(name) else {
            return NameBinding::Free;
        };
        if resolved.iter().all(|id| {
            let binding = frame.gate.skeleton.binding(*id);
            (binding.kind == SkeletonBindingKind::Param && frame.gate.parameters.contains_key(id))
                || (!binding.destructured
                    && matches!(
                        binding.kind,
                        SkeletonBindingKind::Const
                            | SkeletonBindingKind::Let
                            | SkeletonBindingKind::Var
                    ))
        }) {
            NameBinding::Captured
        } else {
            NameBinding::Unmodeled
        }
    }

    fn name_is_bound(&self, name: &str, meaning: NameMeaning) -> bool {
        let Some(frame) = self.enclosing.as_deref() else {
            return false;
        };
        if frame.gate.skeleton.name_id(name).is_some_and(|name| {
            frame
                .gate
                .skeleton
                .declares_meaning_in_scope(name, frame.region, meaning)
        }) {
            return true;
        }
        if frame
            .gate
            .type_parameters
            .iter()
            .any(|binder| binder.as_ref() == name)
        {
            return meaning == NameMeaning::Namespace;
        }
        frame.gate.outer.name_is_bound(name, meaning)
    }

    fn binder_is_visible(&self, name: &str) -> bool {
        let Some(frame) = self.enclosing.as_deref() else {
            return false;
        };
        if frame.gate.skeleton.name_id(name).is_some_and(|name| {
            frame
                .gate
                .skeleton
                .declares_meaning_in_scope(name, frame.region, NameMeaning::Type)
                || frame.gate.skeleton.declares_meaning_in_scope(
                    name,
                    frame.region,
                    NameMeaning::Namespace,
                )
        }) {
            return false;
        }
        frame
            .gate
            .type_parameters
            .iter()
            .any(|binder| binder.as_ref() == name)
            || frame.gate.outer.binder_is_visible(name)
    }
}

impl DefiningFrameGate {
    /// Whether every destructuring declaration of `binding`'s runtime
    /// variable is a modelled pattern (vacuously, when it has none).
    fn destructured_var_is_modelled(&self, binding: SkeletonBindingId) -> bool {
        !self.bindings.runtime_shape(binding).has_destructured_var
            || self
                .bindings
                .runtime_declarations(binding)
                .iter()
                .all(|declaration| {
                    !self.skeleton.binding(*declaration).destructured
                        || self.modelled_patterns.contains(declaration)
                })
    }

    /// Lexical value visibility for type-position `typeof` names. This gate
    /// produces no runtime binding identity and accepts no runtime occurrence.
    fn value_name_is_bound(&self, name: &str, span: FrameSpan) -> bool {
        let region = self.skeleton.innermost_region_containing(span);
        self.skeleton.name_id(name).is_some_and(|name| {
            !self
                .skeleton
                .bindings_of_name_in_scope(name, region)
                .is_empty()
        }) || !matches!(self.outer.lookup(name), NameBinding::Free)
    }
    fn name_is_bound(
        &self,
        name: &str,
        span: FrameSpan,
        meaning: NameMeaning,
        binders: &[Arc<str>],
    ) -> bool {
        if binders.iter().any(|binder| binder.as_ref() == name) {
            return meaning == NameMeaning::Namespace;
        }
        let region = self.skeleton.innermost_region_containing(span);
        if self.skeleton.name_id(name).is_some_and(|name| {
            self.skeleton
                .declares_meaning_in_scope(name, region, meaning)
        }) {
            return true;
        }
        if self
            .type_parameters
            .iter()
            .any(|binder| binder.as_ref() == name)
        {
            return meaning == NameMeaning::Namespace;
        }
        self.outer.name_is_bound(name, meaning)
    }

    fn answer_names_frame_bound(
        &self,
        ty: &TypeExpr,
        span: FrameSpan,
        binders: &[Arc<str>],
    ) -> Vec<FrameShadowedName> {
        let names = verter_type_expr::referenced_names(ty);
        let region = self.skeleton.innermost_region_containing(span);
        let mut shadowed = Vec::new();
        for name in names.value_roots {
            let local = self.skeleton.name_id(&name).is_some_and(|name| {
                !self
                    .skeleton
                    .bindings_of_name_in_scope(name, region)
                    .is_empty()
            });
            if local || !matches!(self.outer.lookup(&name), NameBinding::Free) {
                shadowed.push(FrameShadowedName::Value(Arc::from(name)));
            }
        }
        for occurrence in names.type_names {
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
            if self.name_is_bound(&occurrence.head, span, meaning, binders)
                && !shadowed.contains(&entry)
            {
                shadowed.push(entry);
            }
        }
        shadowed
    }
}

/// Source-ordered syntax siblings are disjoint; only the one containing the
/// exact indexed binding can supply its annotation. Each level costs log(N).
pub(crate) fn selected_span_child<T: GetSpan>(items: &[T], target: oxc_span::Span) -> Option<&T> {
    let at = items.partition_point(|item| item.span().end <= target.start);
    items.get(at).filter(|item| {
        let span = item.span();
        span.start <= target.start && span.end >= target.end
    })
}

fn pattern_has_identifier(pattern: &BindingPattern<'_>, target: oxc_span::Span) -> bool {
    match pattern {
        BindingPattern::BindingIdentifier(id) => id.span == target,
        BindingPattern::ObjectPattern(object) => {
            selected_span_child(&object.properties, target)
                .is_some_and(|property| pattern_has_identifier(&property.value, target))
                || object
                    .rest
                    .as_ref()
                    .is_some_and(|rest| pattern_has_identifier(&rest.argument, target))
        }
        BindingPattern::ArrayPattern(array) => {
            array
                .elements
                .iter()
                .flatten()
                .any(|element| pattern_has_identifier(element, target))
                || array
                    .rest
                    .as_ref()
                    .is_some_and(|rest| pattern_has_identifier(&rest.argument, target))
        }
        BindingPattern::AssignmentPattern(assignment) => {
            pattern_has_identifier(&assignment.left, target)
        }
    }
}

struct SelectedAnnotationFinder<'s> {
    source: &'s str,
    target: oxc_span::Span,
    locator: &'s SliceCaptureAuthorityLocator,
    found: Option<Option<GatedType>>,
}

impl SelectedAnnotationFinder<'_> {
    fn contains(&self, span: oxc_span::Span) -> bool {
        self.found.is_none() && span.start <= self.target.start && span.end >= self.target.end
    }

    fn parameter(
        &mut self,
        annotation: Option<&oxc_ast::ast::TSTypeAnnotation<'_>>,
        initializer: Option<&Expression<'_>>,
    ) {
        let gate = &self.locator.gate;
        let scope = SignatureScope::Nested {
            gate: &gate.outer,
            binders: &gate.type_parameters,
        };
        let parameter_bindings = &gate.parameter_names;
        let (mut gated, visible_before) = if let Some(annotation) = annotation {
            (
                scope.gate(
                    lower_ts_type(&annotation.type_annotation, self.source),
                    &gate.type_parameters,
                ),
                None,
            )
        } else if let Some(initializer) = initializer {
            let Ok(ty) = infer_declaration_expression_type(
                initializer,
                self.source,
                TopLevelLiteralPolicy::Widen,
            ) else {
                return;
            };
            (
                scope.gate_param_default(
                    ty,
                    initializer,
                    &gate.type_parameters,
                    parameter_bindings,
                ),
                Some(initializer.span().start),
            )
        } else {
            (
                GatedType::root_signature(TypeExpr::Primitive(PrimitiveName::Any)),
                None,
            )
        };
        gated.add_shadowed(parameter_list_shadowed(
            gated.ty(),
            parameter_bindings,
            visible_before,
        ));
        self.found = Some(Some(gated));
    }
}

macro_rules! selected_annotation_list {
    ($list:ident, $item:ident, $visit:ident) => {
        fn $list(&mut self, items: &oxc_allocator::Vec<'a, oxc_ast::ast::$item<'a>>) {
            if self.found.is_none() {
                if let Some(item) = selected_span_child(items, self.target) {
                    self.$visit(item);
                }
            }
        }
    };
}

impl<'a> Visit<'a> for SelectedAnnotationFinder<'_> {
    selected_annotation_list!(visit_statements, Statement, visit_statement);
    selected_annotation_list!(
        visit_variable_declarators,
        VariableDeclarator,
        visit_variable_declarator
    );
    selected_annotation_list!(
        visit_object_property_kinds,
        ObjectPropertyKind,
        visit_object_property_kind
    );
    selected_annotation_list!(visit_class_elements, ClassElement, visit_class_element);
    selected_annotation_list!(visit_switch_cases, SwitchCase, visit_switch_case);
    selected_annotation_list!(visit_arguments, Argument, visit_argument);
    selected_annotation_list!(
        visit_array_expression_elements,
        ArrayExpressionElement,
        visit_array_expression_element
    );
    selected_annotation_list!(visit_expressions, Expression, visit_expression);
    selected_annotation_list!(
        visit_formal_parameter_list,
        FormalParameter,
        visit_formal_parameter
    );
    selected_annotation_list!(visit_decorators, Decorator, visit_decorator);
    selected_annotation_list!(
        visit_binding_properties,
        BindingProperty,
        visit_binding_property
    );

    fn visit_ts_type(&mut self, _: &TSType<'a>) {}

    fn visit_statement(&mut self, statement: &Statement<'a>) {
        if self.contains(statement.span()) {
            walk::walk_statement(self, statement);
        }
    }

    fn visit_expression(&mut self, expression: &Expression<'a>) {
        if self.contains(expression.span()) {
            walk::walk_expression(self, expression);
        }
    }

    fn visit_variable_declarator(&mut self, declarator: &oxc_ast::ast::VariableDeclarator<'a>) {
        if self.locator.parameter_ordinal.is_none()
            && matches!(&declarator.id, BindingPattern::BindingIdentifier(id) if id.span == self.target)
        {
            self.found = Some(declarator.type_annotation.as_ref().map(|annotation| {
                let ty = lower_ts_type(&annotation.type_annotation, self.source);
                let span = FrameSpan::rebase(self.locator.gate.anchor, self.target.into());
                GatedType {
                    shadowed: self
                        .locator
                        .gate
                        .answer_names_frame_bound(&ty, span, &[])
                        .into(),
                    ty,
                }
            }));
        } else if self.contains(declarator.span) {
            walk::walk_variable_declarator(self, declarator);
        }
    }

    fn visit_formal_parameter(&mut self, parameter: &oxc_ast::ast::FormalParameter<'a>) {
        if self.locator.parameter_ordinal.is_some()
            && pattern_has_identifier(&parameter.pattern, self.target)
        {
            self.parameter(
                parameter.type_annotation.as_deref(),
                parameter.initializer.as_deref(),
            );
        } else if self.contains(parameter.span) {
            walk::walk_formal_parameter(self, parameter);
        }
    }

    fn visit_formal_parameter_rest(&mut self, parameter: &oxc_ast::ast::FormalParameterRest<'a>) {
        if self.locator.parameter_ordinal.is_some()
            && pattern_has_identifier(&parameter.rest.argument, self.target)
        {
            self.parameter(parameter.type_annotation.as_deref(), None);
        } else if self.contains(parameter.span()) {
            walk::walk_formal_parameter_rest(self, parameter);
        }
    }
}

/// Hydrate one selected declaration by its exact indexed identifier span.
/// `Some(None)` is authored absence; `None` is a source/locator mismatch.
pub(crate) fn build_flow_capture_authority(
    program: &Program<'_>,
    source: &str,
    entry: &FunctionProgramEntry,
    locator: &SliceCaptureAuthorityLocator,
) -> Option<Option<SliceCaptureAuthority>> {
    if entry.key != locator.declaration.defining_function
        || entry.flow_body_exact_hash != Some(locator.gate.body_hash)
    {
        return None;
    }
    let binding = locator.gate.bindings.local(&locator.declaration)?;
    let fact = locator.gate.skeleton.binding(binding);
    let authored = entry
        .bindings
        .get(locator.declaration.binding_slot as usize)?;
    let absolute = fact.span.to_absolute(locator.gate.anchor);
    if authored.span != absolute {
        return None;
    }
    let mut finder = SelectedAnnotationFinder {
        source,
        target: oxc_span::Span::new(absolute.start, absolute.end),
        locator,
        found: None,
    };
    finder.visit_program(program);
    Some(finder.found?.map(|declared| SliceCaptureAuthority {
        binding: locator.binding.clone(),
        name: locator.declaration.name.clone(),
        declared,
        source: locator.source.clone(),
    }))
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
    /// The function's own project's `strictNullChecks` algebra. With it
    /// off a bare `null` / `undefined` / `void` value nested in an object
    /// member or an array element is the checker's widening nullable type,
    /// which the enclosing literal's widening turns into `any`.
    nullability: crate::semantic_query::NullabilityPolicy,
    /// Whether the frame's function is `async`.
    frame_is_async: bool,
    frame_gate: Arc<DefiningFrameGate>,
    bindings: &'a verter_semantic::analysis::flow::FlowBindingMap,
    index: &'a verter_semantic::analysis::function_program::FunctionProgramIndex,
    source: &'a str,
    /// The function's own start offset — the anchor the frame's
    /// [`FunctionBodySkeleton`] (and the plan derived from it) stores
    /// every span relative to.
    ///
    /// Those artifacts are content-addressed and carry no absolute
    /// source position, so a LIVE position is rebased onto this anchor
    /// before it is compared against, or looked up in, either of them.
    anchor: u32,
    /// The demand selection gating body content. `None` requests only the
    /// callable's signature and never lowers its body.
    selection: Option<&'a FlowSliceSelection>,
    params: &'a [SliceParam],
    /// This frame's OWN type-parameter names. They are TYPE-meaning
    /// binders of the frame: the evaluator's binder environment interns
    /// them, so a body answer naming one resolves to the binder and must
    /// NOT be reported as shadowed by a captured same-named `class`.
    type_param_names: &'a [Arc<str>],
    self_name: Option<&'a str>,
    /// The heritage (`extends`) context of the enclosing class member this
    /// frame is, when it is a DIRECT member of a class with an `extends`
    /// clause — what a `super.x` access inside the body resolves through.
    /// `None` for every other frame (nested callables included, mirroring
    /// the type-parameter clause rule).
    enclosing_heritage: Option<verter_semantic::analysis::function_program::EnclosingHeritage<'a>>,
    /// What `this` reads in this frame: a class declaration's member
    /// receiver, or the one an arrow inherits from the member creating it.
    /// A class expression's instance initializers read the class's own
    /// receiver while they lower.
    this: Option<SliceThis>,
    /// The `this` the NEXT nested function lowered takes in place of the
    /// one its kind implies: a class expression's member function.
    member_this: Option<Option<SliceThis>>,
    /// This frame's shared structural skeleton. Runtime references use the
    /// prepared map's exact occurrence records; type-position visibility
    /// queries use the skeleton's separate lexical meaning rules.
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
    /// The whole retained parse snapshot this frame's function lives in.
    /// The guard lowering reads SAME-FILE predicate / assertion
    /// signatures from it (`isStr(u)` / `assertStr(u);` carry their
    /// narrowing fact in the callee's declared return, not at the use
    /// site); a cross-file callee is beyond this channel and lowers to
    /// [`SliceGuard::None`].
    program: &'a Program<'a>,
    /// Whether the frame's file is PROVABLY module-scoped: the carrier
    /// projects its script block as a module (a `.vue` / `.svelte` script
    /// block compiles to one), or the retained program carries top-level
    /// module syntax. This is the one-directional proof the callee
    /// closure gate ([`Self::closed_callee_declaration`]) needs: a file
    /// that is not provably a module is treated as a SCRIPT, whose
    /// top-level functions are GLOBAL symbols merging with every other
    /// script's and every `declare global` block's same-name declarations
    /// — a set this snapshot cannot enumerate — so no callee of such a
    /// file is ever certified or selected as a predicate.
    module_scope: bool,
    /// Whether the frame's function is NAMESPACE-OWNED — lexically inside
    /// a `namespace` / `module` block (the index locator descends through
    /// one). A bare callee there binds through the enclosing block scope
    /// BEFORE the top level: a block-local declaration of the name (a
    /// function, a `const`, a class, an enum, an `import =`, an exported
    /// member of a merged sibling block, or a member an augmentation of an
    /// exported namespace merges in) shadows the top-level one, exactly
    /// as the function index binds `N.check` over the file-global `check`
    /// for a direct call. The callee closure gate
    /// ([`Self::closed_callee_declaration`]) enumerates TOP-LEVEL
    /// declarations only, so under this flag it would certify — or mint
    /// the predicate target of — the WRONG declaration; it refuses
    /// instead. Inherited by every nested frame: a namespace cannot be
    /// declared inside a function, so the block chain between a call site
    /// and the top level is fixed by the served function's own position.
    namespace_owned: bool,
    /// The namespace blocks enclosing a namespace-owned function, outermost
    /// first (empty otherwise): a free value read in the body names a
    /// block's member before the file's top level.
    namespace_scopes: &'a [NamespaceBlockScope],
    /// The contributing top-level statement the served function (and every
    /// frame enclosing it) sits in — where a class expression's enclosing
    /// clauses are named.
    contributor: u32,
    /// The first budget edge a SELECTED leaf's expression lowering hit.
    budget_failure: Option<verter_type_expr::facts::InferenceUnavailableReason>,
    /// Write effects proven unreachable by a literal control edge.
    inert_write_spans: FxHashSet<FrameSpan>,
    /// The spans of the logical expressions whose operands are being
    /// lowered, innermost last: the value SITE an assignment operand of
    /// one belongs to (the planner opens no site of its own for it).
    logical_value_sites: Vec<oxc_span::Span>,
    /// Call / construct spans of decided-above positions — see
    /// [`SliceContent::decided_above_call_spans`].
    decided_above_call_spans: Vec<verter_span::Span>,
    /// The frame-lowered argument values of each call — see
    /// [`SliceContent::call_arguments`].
    call_arguments: FxHashMap<verter_span::Span, Arc<[SliceExpr]>>,
    /// Nonzero while a call argument lowers as a WHOLE value: every
    /// position inside it is a value position, whatever the demand
    /// selected.
    whole_value_nesting: u32,
    /// The authored call spans whose [`SliceGuard::TypePredicate`] fact
    /// this lowering MINTED: evidence-backed at guard application, so the
    /// control-position recorder neither certifies them decided-above nor
    /// gaps them.
    predicate_guard_call_spans: FxHashSet<verter_span::Span>,
    /// The authored call spans in a test position that hand the callee no
    /// reference the checker could narrow: provably non-narrowing whatever
    /// the callee, so the control-position recorder certifies them.
    non_narrowing_call_spans: FxHashSet<verter_span::Span>,
    /// The parameters a type predicate inferred from the body may name —
    /// `Some` only for a function the checker may infer one for (see
    /// [`ReturnPredicateTest`]). Its single return reads its argument as a
    /// test over them.
    predicate_parameters: Option<Arc<[u32]>>,
    /// A narrowing position lowered inside the CURRENT statement — a
    /// control-position test, an assertion statement, a sequence's
    /// discarded operand — carried a fact this half can neither certify
    /// result-independent nor back with guard evidence (an unprovable
    /// control call, a predicate target the call site rebinds, an
    /// unprovable `instanceof` constructor, an unprovable discarded
    /// call): the
    /// statement loop drains this into a [`SliceStatement::Gap`]
    /// (`GuardNarrowing`) AHEAD of the statement — a typed degradation,
    /// never a silent certification. The `if` statement takes the flag
    /// itself right after lowering its test, so an arm region's own loop
    /// cannot drain it INTO the arm.
    control_test_gap: bool,
    /// While set, the `asserts` predicates of the entered calls a scan
    /// finds (outside any conditional arm) are collected here to be
    /// applied after the scanned position, instead of taking the typed
    /// gap.
    entered_assertion_sink: Option<Vec<SliceStatement>>,
    /// The same-frame `const`-kind locals whose initializer is a form the
    /// checker can bind a narrowing FACT to (a comparison, an
    /// `instanceof` / `in` test, a call, a composition of those, or
    /// another such alias). TypeScript preserves the narrowing of an
    /// aliased CONDITION, so the truthiness of one of these names is not
    /// the whole narrowing the checker applies: [`Self::lower_guard`]
    /// flags the typed gap for it rather than publishing the alias's
    /// unnarrowed subject. Populated whether or not the declaration is
    /// value-selected — an elided local is still name-resolvable — and
    /// only for `const` / `using` declarations, since the checker does
    /// not preserve an aliased condition through a reassignable binding.
    narrowing_alias_locals: FxHashSet<Arc<str>>,
    /// The ALIASED CONDITIONS of this frame: per eligible `const` alias,
    /// its initializer read as a guard, indexed by how many FURTHER alias
    /// inlines the reading may still make (the checker inlines at most
    /// [`ALIAS_INLINE_LIMIT`] levels); `None` for a reading this
    /// vocabulary cannot express. Every fact in a reading lands on a
    /// CONSTANT reference or narrows nothing.
    alias_conditions:
        rustc_hash::FxHashMap<FlowBindingRef, [Option<SliceGuard>; ALIAS_INLINE_LIMIT]>,
    /// The ALIASED DISCRIMINANTS of this frame: a `const k = u.kind` or
    /// `const { kind: k } = u` alias, with the member reference it names.
    discriminant_aliases: rustc_hash::FxHashMap<FlowBindingRef, SliceNarrowSubject>,
    /// How many alias inlines the guard classification in progress may
    /// still make.
    alias_inline_budget: usize,
    unsafe_invoked_closure_effects: FxHashSet<FrameSpan>,
    nested_free_writes: FxHashSet<SkeletonBindingId>,
    /// The statements an assignment's position extends to
    /// ([`Lowerer::is_past_last_assignment`]), as source spans.
    assignment_extent_statements: Vec<oxc_span::Span>,
    /// How many class property initializers enclose the lowering position:
    /// a callable there is contained by the property, not this frame, so
    /// no narrowing of this frame reaches it.
    class_property_initializers: u32,
    active_guard_bindings: Vec<SkeletonBindingId>,
    /// The guard bindings of every branch a condition lowering pushes,
    /// collected while an `if` test holding a write lowers as a
    /// condition — the arms it guards then capture every guarded reading.
    condition_guard_bindings: Option<Vec<SkeletonBindingId>>,
    /// How many member reads off an object-literal root enclose the
    /// lowering: the planner tracks such a read as ONE reference site, so
    /// every value of the literal it reads through is selected with it.
    open_value_rooted_reads: u32,
    /// The bindings a test of each destructured element also narrows
    /// ([`Self::record_aliased_bindings`]), by the element's canonical
    /// binding. Owned by the lowering, dropped with it.
    aliased_bindings: rustc_hash::FxHashMap<SkeletonBindingId, Arc<[SkeletonBindingId]>>,
    /// The ordinals of the parameters with an authored type annotation —
    /// the ones whose type the checker's `getTypeOfDottedName` reads.
    annotated_params: FxHashSet<u32>,
    /// The stack of breakable constructs whose bodies are currently being
    /// lowered (innermost last): `None` for a `switch`, `Some(label)` for
    /// a labeled statement. A `break` resolves against this stack — an
    /// unlabeled one targets the innermost `None` entry (labels do not
    /// accept unlabeled breaks), a labeled one the innermost matching
    /// name. Loop bodies never lower, so a loop is never an entry.
    break_targets: Vec<Option<Arc<str>>>,
    /// The labels whose statements DIRECTLY wrap the loop currently being
    /// classified for transparency: a chain of labeled statements whose
    /// terminal body is the loop itself, with nothing in between. A
    /// `break`/`continue` naming one of these is the loop's OWN exit or
    /// iteration edge — the label's continuation IS the loop's
    /// fall-through point — so it never transfers control past a lowered
    /// construct and does not defeat transparency. Distinct from
    /// [`Self::break_targets`], which also carries labels separated from
    /// the loop by an intervening statement (a block with statements after
    /// the loop): breaking to THOSE skips lowered content.
    loop_direct_labels: Vec<Arc<str>>,
    /// The labels of a label chain that directly wraps the loop about to
    /// lower — the loop takes them as the names a labeled `continue`
    /// targets it by.
    pending_loop_labels: Vec<Arc<str>>,
    /// The loops whose bodies are currently being lowered (innermost
    /// last), each with the labels naming it: an unlabeled `continue`
    /// targets the innermost one, a labeled one the loop its label names.
    continue_targets: Vec<Arc<[Arc<str>]>>,
    /// For each break target, whether the target statement has a guaranteed
    /// current-function return later in its enclosing statement list. A
    /// pending break contributes implicit `undefined` only when its
    /// destination is PROVED to reach the function end rather than that
    /// return; an undecided destination proves neither and fails closed
    /// through [`crate::semantic_query::FlowGap::AbruptCompletion`].
    break_target_followed_by_return: Vec<SuffixReturn>,
    /// The suffix fact for the statement currently being lowered; captured
    /// when that statement introduces a break target.
    current_statement_followed_by_return: SuffixReturn,
}

impl<'a> Lowerer<'a> {
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
    /// slice. Body lowering always carries a selection; None is reserved
    /// for signature-only preparation, whose body remains empty.
    fn value_span_selected(&self, span: oxc_span::Span) -> bool {
        self.whole_value_nesting > 0
            || self.open_value_rooted_reads > 0
            || self
                .selection
                .is_none_or(|selection| selection.value_span(self.rebase(span)))
    }

    /// Whether a binding slot (identified by its binding-identifier
    /// span) is value-selected by the demand slice.
    fn slot_selected(&self, span: oxc_span::Span) -> bool {
        self.selection
            .is_none_or(|selection| selection.value_slot_span(self.rebase(span)))
    }

    /// Whether one resolved binding is part of the demanded value slice.
    /// A demanded nested body carries its own selection; signature-only
    /// preparation does not visit its declarations.
    fn binding_is_selected(
        &self,
        binding: verter_semantic::analysis::flow::SkeletonBindingId,
    ) -> bool {
        self.selection
            .is_none_or(|selection| selection.value_slot_span(self.skeleton.binding(binding).span))
    }

    /// Whether one selected binding is read after `loop_span`. A slot used
    /// only by the loop's own control (for example its induction variable)
    /// cannot affect a later selected value and does not defeat transparency.
    fn binding_is_read_after_loop(
        &self,
        binding: verter_semantic::analysis::flow::SkeletonBindingId,
        loop_span: FrameSpan,
    ) -> bool {
        self.binding_is_read_after_loop_at_path(binding, &[], loop_span)
    }

    fn binding_is_read_after_loop_at_path(
        &self,
        binding: verter_semantic::analysis::flow::SkeletonBindingId,
        write_path: &[SkeletonPathSegment],
        loop_span: FrameSpan,
    ) -> bool {
        self.binding_is_selected(binding)
            && self.skeleton.expr_sites.iter().any(|site| {
                !loop_span.contains(site.span)
                    && site.span > loop_span
                    && site.reads.iter().any(|read| {
                        paths_may_overlap(write_path, &read.path)
                            && matches!(read.binding, Some(FlowBindingRef::Local(local)) if self.bindings.canonical_local(local) == self.bindings.canonical_local(binding))
                    })
            })
    }

    /// Whether the reads anywhere under `span` resolve to a selected slot
    /// that is observed after the loop. Resolution uses each skeleton site's
    /// own region, so a same-named loop local never aliases a downstream outer
    /// binding by name alone.
    fn span_reads_downstream_slot(&self, span: FrameSpan, loop_span: FrameSpan) -> bool {
        self.skeleton.expr_sites.iter().any(|site| {
            span.contains(site.span)
                && site.reads.iter().any(|read| {
                    matches!(read.binding, Some(FlowBindingRef::Local(binding)) if self.binding_is_read_after_loop(binding, loop_span))
                })
        })
    }

    /// Whether a return-free loop carries a transfer the transparent summary
    /// cannot justify for a downstream-selected binding. This deliberately
    /// does not implement loop flow: it recognizes only the unsound admission
    /// boundary and routes it to the existing typed loop refusal.
    ///
    /// Three syntax-independent skeleton facts can change a selected value
    /// past a loop: a control input whose exit establishes a guard, a call
    /// involving the slot (which may be a predicate/assertion), or a write to
    /// the slot. A loop with none of those captures stays transparent.
    fn loop_has_selected_transfer(&mut self, statement: &Statement<'_>) -> bool {
        let loop_span = self.rebase(statement.span());
        let control_guard_reads_selected =
            self.statement_has_selected_guard_transfer(statement, loop_span);
        if control_guard_reads_selected {
            return true;
        }

        let call_reads_selected = self.skeleton.expr_sites.iter().any(|site| {
            site.calls.iter().any(|call| {
                loop_span.contains(call.span)
                    && !self.span_is_in_literal_dead_branch(statement, call.span)
                    && self.span_reads_downstream_slot(call.span, loop_span)
            })
        });
        if call_reads_selected {
            return true;
        }

        if self.invoked_closure_transfers_downstream_slot(statement, loop_span) {
            return true;
        }

        self.loop_has_selected_write(statement)
    }

    /// Whether a loop writes a binding the slice reads after it (a local,
    /// or a captured EVOLVING array): the write transfer
    /// [`Self::loop_has_selected_transfer`] refuses and a whole-lowered
    /// loop ([`SliceStatement::Loop`]) resolves through each reference's
    /// loop head.
    fn loop_has_selected_write(&mut self, statement: &Statement<'_>) -> bool {
        let loop_span = self.rebase(statement.span());
        self.skeleton.writes.iter().any(|write| {
            if !loop_span.contains(write.span) {
                return false;
            }
            if self.span_is_in_literal_dead_branch(statement, write.span) {
                self.inert_write_spans.insert(write.span);
                return false;
            }
            match &write.binding {
                Some(FlowBindingRef::Local(binding)) => {
                    self.binding_is_read_after_loop_at_path(*binding, &write.path, loop_span)
                }
                // An operation on a captured EVOLVING array retypes this
                // frame's input: a transfer when the frame reads it after
                // the loop.
                Some(captured @ FlowBindingRef::Captured(_)) => {
                    self.is_evolving_binding(captured)
                        && self.skeleton.expr_sites.iter().any(|site| {
                            site.span > loop_span
                                && !loop_span.contains(site.span)
                                && site
                                    .reads
                                    .iter()
                                    .any(|read| read.binding.as_ref() == Some(captured))
                        })
                }
                None => false,
            }
        })
    }

    /// Whether a loop test carries narrowing over a downstream-selected
    /// slot. A test PROVED to establish no fact is transparent here; a
    /// modeled one and one this vocabulary cannot express both count, so
    /// the loop takes its typed refusal rather than iterating under a
    /// narrow nothing applied. Calls and writes are classified
    /// independently. The classification is consulted, not lowered: a
    /// loop test must not degrade the enclosing statement.
    fn control_test_narrows_downstream_slot(
        &mut self,
        test: &Expression<'_>,
        loop_span: FrameSpan,
    ) -> bool {
        !self.classify_guard(test).is_no_narrowing()
            && self.span_reads_downstream_slot(self.rebase(test.span()), loop_span)
    }

    /// Search the loop's control tree without entering nested function/class
    /// frames. Only tests the shared guard lowerer can model count as narrowing
    /// transfers; a switch discriminant is separately control-bearing because
    /// its case dispatch can select a surviving edge.
    fn statement_has_selected_guard_transfer(
        &mut self,
        statement: &Statement<'_>,
        loop_span: FrameSpan,
    ) -> bool {
        match statement {
            Statement::BlockStatement(block) => block
                .body
                .iter()
                .any(|statement| self.statement_has_selected_guard_transfer(statement, loop_span)),
            Statement::IfStatement(if_stmt) => {
                let test_transfer =
                    self.control_test_narrows_downstream_slot(&if_stmt.test, loop_span);
                match literal_boolean_value(&if_stmt.test) {
                    Some(true) => {
                        test_transfer
                            || self.statement_has_selected_guard_transfer(
                                &if_stmt.consequent,
                                loop_span,
                            )
                    }
                    Some(false) => {
                        test_transfer
                            || if_stmt.alternate.as_ref().is_some_and(|alternate| {
                                self.statement_has_selected_guard_transfer(alternate, loop_span)
                            })
                    }
                    None => {
                        test_transfer
                            || self.statement_has_selected_guard_transfer(
                                &if_stmt.consequent,
                                loop_span,
                            )
                            || if_stmt.alternate.as_ref().is_some_and(|alternate| {
                                self.statement_has_selected_guard_transfer(alternate, loop_span)
                            })
                    }
                }
            }
            Statement::ForStatement(for_stmt) => {
                for_stmt
                    .test
                    .as_ref()
                    .is_some_and(|test| self.control_test_narrows_downstream_slot(test, loop_span))
                    || self.statement_has_selected_guard_transfer(&for_stmt.body, loop_span)
            }
            Statement::WhileStatement(while_stmt) => {
                self.control_test_narrows_downstream_slot(&while_stmt.test, loop_span)
                    || self.statement_has_selected_guard_transfer(&while_stmt.body, loop_span)
            }
            Statement::DoWhileStatement(do_stmt) => {
                self.control_test_narrows_downstream_slot(&do_stmt.test, loop_span)
                    || self.statement_has_selected_guard_transfer(&do_stmt.body, loop_span)
            }
            Statement::ForInStatement(for_stmt) => {
                self.statement_has_selected_guard_transfer(&for_stmt.body, loop_span)
            }
            Statement::ForOfStatement(for_stmt) => {
                self.statement_has_selected_guard_transfer(&for_stmt.body, loop_span)
            }
            Statement::SwitchStatement(switch) => {
                self.span_reads_downstream_slot(self.rebase(switch.discriminant.span()), loop_span)
                    || switch.cases.iter().any(|case| {
                        case.consequent.iter().any(|statement| {
                            self.statement_has_selected_guard_transfer(statement, loop_span)
                        })
                    })
            }
            Statement::TryStatement(try_stmt) => {
                try_stmt.block.body.iter().any(|statement| {
                    self.statement_has_selected_guard_transfer(statement, loop_span)
                }) || try_stmt.handler.as_ref().is_some_and(|handler| {
                    handler.body.body.iter().any(|statement| {
                        self.statement_has_selected_guard_transfer(statement, loop_span)
                    })
                }) || try_stmt.finalizer.as_ref().is_some_and(|finalizer| {
                    finalizer.body.iter().any(|statement| {
                        self.statement_has_selected_guard_transfer(statement, loop_span)
                    })
                })
            }
            Statement::LabeledStatement(labeled) => {
                self.statement_has_selected_guard_transfer(&labeled.body, loop_span)
            }
            Statement::WithStatement(with_stmt) => {
                self.statement_has_selected_guard_transfer(&with_stmt.body, loop_span)
            }
            _ => false,
        }
    }

    /// Fail-closed closure boundary for directly invoked callees under a
    /// return-free loop. A function passed as an argument is only a value; the
    /// call does not establish that the callback runs. A direct closure callee
    /// is inspected for captured writes and control/call reads that can change
    /// downstream-selected flow.
    fn invoked_closure_transfers_downstream_slot(
        &self,
        statement: &Statement<'_>,
        loop_span: FrameSpan,
    ) -> bool {
        let mut transfers = false;
        for_each_call_expression(std::slice::from_ref(statement), |call| {
            if transfers {
                return;
            }
            if self.span_is_in_literal_dead_branch(statement, self.rebase(call.span)) {
                return;
            }
            let mut inspect = |node: FunctionNode<'_>| {
                if self.nested_function_transfers_downstream_slot(&node, loop_span) {
                    transfers = true;
                }
            };
            match unwrap_parenthesized(&call.callee) {
                Expression::FunctionExpression(function) => {
                    inspect(FunctionNode::Function(function));
                }
                Expression::ArrowFunctionExpression(arrow) => {
                    inspect(FunctionNode::Arrow(arrow));
                }
                _ => {}
            }
        });
        transfers
    }

    fn index_unsafe_invoked_closure_effects(
        &self,
        statements: &[Statement<'_>],
    ) -> FxHashSet<FrameSpan> {
        let mut unsafe_calls = FxHashSet::default();
        for statement in statements {
            for_each_call_expression(std::slice::from_ref(statement), |call| {
                let call_span = self.rebase(call.span);
                if self.span_is_in_literal_dead_branch(statement, call_span) {
                    return;
                }
                let node = match unwrap_parenthesized(&call.callee) {
                    Expression::FunctionExpression(function) => FunctionNode::Function(function),
                    Expression::ArrowFunctionExpression(arrow) => FunctionNode::Arrow(arrow),
                    _ => return,
                };
                if self.nested_function_transfers_downstream_slot(&node, call_span) {
                    unsafe_calls.insert(call_span);
                }
            });
        }
        unsafe_calls
    }

    fn span_contains_unsafe_invoked_closure(&self, span: oxc_span::Span) -> bool {
        let span = self.rebase(span);
        self.unsafe_invoked_closure_effects
            .iter()
            .any(|call| span.contains(*call))
    }

    /// The parameters and mutable locals (`let`, `catch` parameter — the
    /// checker's `isParameterOrMutableLocalVariable`) some nested function
    /// assigns: none of them is ever past its last assignment.
    fn build_nested_free_writes(&self) -> FxHashSet<SkeletonBindingId> {
        self.index
            .get(self.bindings.function())
            .into_iter()
            .flat_map(|entry| entry.entry().descendant_writes.iter())
            .filter_map(|identity| self.bindings.local(identity))
            .filter(|binding| {
                matches!(
                    self.skeleton.binding(*binding).kind,
                    SkeletonBindingKind::Param
                        | SkeletonBindingKind::Let
                        | SkeletonBindingKind::CatchParam
                )
            })
            .collect()
    }

    /// Whether this frame writes `binding`'s runtime variable anywhere.
    fn binding_is_written(&self, binding: SkeletonBindingId) -> bool {
        let runtime = self.bindings.canonical_local(binding);
        self.skeleton.writes.iter().any(|write| {
            matches!(write.binding, Some(FlowBindingRef::Local(local))
                if self.bindings.canonical_local(local) == runtime)
        })
    }

    /// Whether this frame assigns `binding` at or after a function created
    /// at `creation_span` — the checker's `isPastLastAssignment` negated.
    /// An assignment's position extends to the end of the outermost
    /// statement holding it that begins after the binding's declaration
    /// (`extendAssignmentPosition`): `if (c) { x = 1; const f = () => x; }`
    /// assigns `x` at the end of the `if`, after `f` is created.
    fn binding_has_write_after(
        &self,
        binding: SkeletonBindingId,
        creation_span: oxc_span::Span,
    ) -> bool {
        let creation = self.rebase(creation_span);
        let canonical = self.bindings.canonical_local(binding);
        let declaration = self.skeleton.binding(canonical).span;
        self.skeleton.writes.iter().any(|write| {
            if !matches!(write.binding, Some(FlowBindingRef::Local(local))
                if self.bindings.canonical_local(local) == canonical)
            {
                return false;
            }
            let mut extent = AssignmentExtent {
                anchor: self.anchor,
                write: write.span,
                declaration,
                found: None,
            };
            extent.visit_program(self.program);
            let position = extent.found.unwrap_or(write.span);
            position.contains(creation) || position > creation
        })
    }

    fn binding_has_write_before(
        &self,
        binding: SkeletonBindingId,
        creation_span: oxc_span::Span,
    ) -> bool {
        let creation = self.rebase(creation_span);
        self.skeleton.writes.iter().any(|write| {
            write.span < creation
                && matches!(write.binding, Some(FlowBindingRef::Local(local))
                    if self.bindings.canonical_local(local) == self.bindings.canonical_local(binding))
        })
    }

    fn binding_has_write_within(&self, binding: SkeletonBindingId, range: oxc_span::Span) -> bool {
        let range = self.rebase(range);
        self.skeleton.writes.iter().any(|write| {
            range.contains(write.span)
                && matches!(write.binding, Some(FlowBindingRef::Local(local))
                    if self.bindings.canonical_local(local) == self.bindings.canonical_local(binding))
        })
    }

    /// Whether a capture outside its extended container reads exactly what
    /// the evaluator supplies — the checker's declared type: a whole
    /// parameter or annotated `let` / `var` reads its declared authority,
    /// and an unannotated whole `let` / `var` its initializer's widened
    /// type (`any` for an auto-typed one).
    fn capture_reads_declared_type(&self, binding: SkeletonBindingId) -> bool {
        let fact = self.skeleton.binding(binding);
        !fact.destructured
            && matches!(
                fact.kind,
                SkeletonBindingKind::Param | SkeletonBindingKind::Let | SkeletonBindingKind::Var
            )
    }

    fn guard_bindings(&self, guard: &SliceGuard, _at: oxc_span::Span) -> Vec<SkeletonBindingId> {
        let mut bindings = Vec::new();
        collect_guard_subjects(guard, &mut |subject| {
            self.extend_subject_bindings(subject, &mut bindings)
        });
        bindings
    }

    fn subject_bindings(
        &self,
        subject: &SliceNarrowSubject,
        _at: oxc_span::Span,
    ) -> Vec<SkeletonBindingId> {
        let mut bindings = Vec::new();
        self.extend_subject_bindings(subject, &mut bindings);
        bindings
    }

    fn extend_subject_bindings(
        &self,
        subject: &SliceNarrowSubject,
        bindings: &mut Vec<SkeletonBindingId>,
    ) {
        let local = match &subject.root {
            SliceNarrowRoot::Param { binding, .. }
            | SliceNarrowRoot::Local {
                binding: FlowBindingRef::Local(binding),
                ..
            } => *binding,
            SliceNarrowRoot::Local {
                binding: FlowBindingRef::Captured(_),
                ..
            } => return,
        };
        let local = self.bindings.canonical_local(local);
        if !bindings.contains(&local) {
            bindings.push(local);
        }
        // A test of a destructured element also narrows the references it
        // aliases (its correlated siblings, its destructured source).
        if let Some(aliased) = self.aliased_bindings.get(&local) {
            for binding in aliased.iter() {
                if !bindings.contains(binding) {
                    bindings.push(*binding);
                }
            }
        }
    }

    /// Record the bindings a destructured declaration's elements alias
    /// ([`SliceStatement::Destructure`]'s `correlated` and `source`): a
    /// test of one element narrows its siblings and the source, so a
    /// closure an arm creates over any of them captures a guarded reading.
    fn record_aliased_bindings(
        &mut self,
        pattern: &SlicePattern,
        correlated: bool,
        source: Option<&SliceNarrowSubject>,
    ) {
        let elements: Vec<SkeletonBindingId> = pattern
            .bindings()
            .iter()
            .map(|(binding, _)| self.bindings.canonical_local(*binding))
            .collect();
        let mut aliased: Vec<SkeletonBindingId> = if correlated {
            elements.clone()
        } else {
            Vec::new()
        };
        if let Some(source) = source {
            let mut source_bindings = Vec::new();
            match &source.root {
                SliceNarrowRoot::Param { binding, .. }
                | SliceNarrowRoot::Local {
                    binding: FlowBindingRef::Local(binding),
                    ..
                } => source_bindings.push(self.bindings.canonical_local(*binding)),
                SliceNarrowRoot::Local { .. } => {}
            }
            aliased.extend(source_bindings);
        }
        if aliased.is_empty() {
            return;
        }
        let aliased: Arc<[SkeletonBindingId]> = Arc::from(aliased.into_boxed_slice());
        for element in elements {
            self.aliased_bindings.insert(element, Arc::clone(&aliased));
        }
    }

    /// [`Self::record_aliased_bindings`] for every destructured parameter
    /// the entry prologue binds as a pattern, ahead of the body: the
    /// pattern correlates its elements when none of them is assigned.
    fn record_parameter_pattern_aliases(&mut self, params: &[oxc_ast::ast::FormalParameter<'_>]) {
        for param in params {
            let pattern = match &param.pattern {
                BindingPattern::AssignmentPattern(assignment) => &assignment.left,
                other => other,
            };
            if matches!(pattern, BindingPattern::BindingIdentifier(_))
                || param_pattern_is_flat(pattern)
            {
                continue;
            }
            let mut spans = Vec::new();
            collect_pattern_identifier_spans(pattern, &mut spans);
            let bindings: Vec<SkeletonBindingId> = spans
                .iter()
                .filter_map(|span| match self.binding_at(*span) {
                    Some(FlowBindingRef::Local(binding)) => {
                        Some(self.bindings.canonical_local(binding))
                    }
                    _ => None,
                })
                .collect();
            let assigned = bindings.iter().any(|binding| {
                self.binding_is_written(*binding) || self.nested_free_writes.contains(binding)
            });
            if assigned || bindings.is_empty() {
                continue;
            }
            let aliased: Arc<[SkeletonBindingId]> = Arc::from(bindings.clone().into_boxed_slice());
            for binding in bindings {
                self.aliased_bindings.insert(binding, Arc::clone(&aliased));
            }
        }
    }

    fn nested_function_transfers_downstream_slot(
        &self,
        node: &FunctionNode<'_>,
        loop_span: FrameSpan,
    ) -> bool {
        use verter_semantic::analysis::function_program::FunctionWriteTarget;
        let Some(nested) = self
            .index
            .nested_at(self.bindings.function(), node_span(node).into())
        else {
            return false;
        };
        let nested = nested.entry();
        let targets_downstream =
            |identity: &verter_semantic::analysis::function_program::FlowBindingIdentity| {
                self.bindings
                    .local(identity)
                    .is_some_and(|binding| self.binding_is_read_after_loop(binding, loop_span))
            };
        nested
            .writes
            .iter()
            .flat_map(|write| write.targets.iter())
            .any(|target| {
                let FunctionWriteTarget::Binding { reference, .. } = target else {
                    return false;
                };
                reference
                    .binding
                    .resolved()
                    .is_some_and(&targets_downstream)
            })
            || nested.references.iter().any(|reference| {
                reference
                    .read_role
                    .is_some_and(|role| role.is_effect_input())
                    && reference
                        .binding
                        .resolved()
                        .is_some_and(&targets_downstream)
            })
    }

    /// Whether `target` lies under an `if` branch whose literal test proves
    /// that branch unreachable. This is deliberately a small, syntactic
    /// reachability authority: it filters facts that cannot execute without
    /// pretending to solve general control flow.
    fn span_is_in_literal_dead_branch(&self, statement: &Statement<'_>, target: FrameSpan) -> bool {
        if !self.rebase(statement.span()).contains(target) {
            return false;
        }
        let contains = |statement: &Statement<'_>| self.rebase(statement.span()).contains(target);
        match statement {
            Statement::BlockStatement(block) => block
                .body
                .iter()
                .find(|statement| contains(statement))
                .is_some_and(|statement| self.span_is_in_literal_dead_branch(statement, target)),
            Statement::IfStatement(if_stmt) => {
                let consequent_contains = contains(&if_stmt.consequent);
                let alternate_contains = if_stmt.alternate.as_ref().is_some_and(&contains);
                match literal_boolean_value(&if_stmt.test) {
                    Some(false) if consequent_contains => true,
                    Some(true) if alternate_contains => true,
                    _ if consequent_contains => {
                        self.span_is_in_literal_dead_branch(&if_stmt.consequent, target)
                    }
                    _ if alternate_contains => {
                        if_stmt.alternate.as_ref().is_some_and(|alternate| {
                            self.span_is_in_literal_dead_branch(alternate, target)
                        })
                    }
                    _ => false,
                }
            }
            Statement::DoWhileStatement(loop_stmt) => {
                self.span_is_in_literal_dead_branch(&loop_stmt.body, target)
            }
            Statement::WhileStatement(loop_stmt) => {
                (contains(&loop_stmt.body) && literal_boolean_value(&loop_stmt.test) == Some(false))
                    || self.span_is_in_literal_dead_branch(&loop_stmt.body, target)
            }
            Statement::ForStatement(loop_stmt) => {
                self.span_is_in_literal_dead_branch(&loop_stmt.body, target)
            }
            Statement::ForInStatement(loop_stmt) => {
                self.span_is_in_literal_dead_branch(&loop_stmt.body, target)
            }
            Statement::ForOfStatement(loop_stmt) => {
                self.span_is_in_literal_dead_branch(&loop_stmt.body, target)
            }
            Statement::SwitchStatement(switch) => switch.cases.iter().any(|case| {
                case.consequent
                    .iter()
                    .find(|statement| contains(statement))
                    .is_some_and(|statement| self.span_is_in_literal_dead_branch(statement, target))
            }),
            Statement::TryStatement(try_stmt) => try_stmt
                .block
                .body
                .iter()
                .chain(
                    try_stmt
                        .handler
                        .iter()
                        .flat_map(|handler| handler.body.body.iter()),
                )
                .chain(
                    try_stmt
                        .finalizer
                        .iter()
                        .flat_map(|finalizer| finalizer.body.iter()),
                )
                .find(|statement| contains(statement))
                .is_some_and(|statement| self.span_is_in_literal_dead_branch(statement, target)),
            Statement::LabeledStatement(labeled) => {
                self.span_is_in_literal_dead_branch(&labeled.body, target)
            }
            Statement::WithStatement(with_stmt) => {
                self.span_is_in_literal_dead_branch(&with_stmt.body, target)
            }
            _ => false,
        }
    }

    /// An equality or `case` operand naming a value by a static member path
    /// whose root the frame leaves free — `E.A`, `NS.E.A` — the operand an
    /// enum member's comparison is spelled with. A root the frame binds
    /// (or cannot classify) names no module value, so the operand is not
    /// one.
    fn guard_value_path_of(&self, expression: &Expression<'_>) -> Option<SliceGuardLiteral> {
        let mut segments: Vec<Arc<str>> = Vec::new();
        let mut current = unwrap_parenthesized(expression);
        let root = loop {
            match current {
                Expression::StaticMemberExpression(member) => {
                    segments.push(Arc::from(member.property.name.as_str()));
                    current = unwrap_parenthesized(&member.object);
                }
                Expression::Identifier(root) => break root,
                _ => return None,
            }
        };
        if segments.is_empty() || !matches!(self.classify_occurrence(root.span), NameBinding::Free)
        {
            return None;
        }
        segments.push(Arc::from(root.name.as_str()));
        segments.reverse();
        Some(SliceGuardLiteral::Value(Arc::from(
            segments.into_boxed_slice(),
        )))
    }

    /// Classify the exact prepared occurrence. Missing evidence is never free.
    fn classify_occurrence(&self, span: oxc_span::Span) -> NameBinding {
        use verter_semantic::analysis::flow::FlowBindingOccurrence;
        match self.bindings.occurrence(self.rebase(span)) {
            FlowBindingOccurrence::Resolved(FlowBindingRef::Local(binding)) => {
                self.classify_binding(*binding)
            }
            FlowBindingOccurrence::Resolved(FlowBindingRef::Captured(identity)) => {
                self.captures.classify_identity(identity)
            }
            FlowBindingOccurrence::Free => NameBinding::Free,
            FlowBindingOccurrence::UnmodeledLocal | FlowBindingOccurrence::Missing => {
                NameBinding::Unmodeled
            }
        }
    }

    fn binding_at(&self, span: oxc_span::Span) -> Option<FlowBindingRef> {
        use verter_semantic::analysis::flow::FlowBindingOccurrence;
        match self.bindings.occurrence(self.rebase(span)) {
            FlowBindingOccurrence::Resolved(binding) => Some(binding.clone()),
            FlowBindingOccurrence::Free
            | FlowBindingOccurrence::UnmodeledLocal
            | FlowBindingOccurrence::Missing => None,
        }
    }

    fn narrow_root(
        &self,
        name: &str,
        span: oxc_span::Span,
        ordinal: Option<u32>,
    ) -> Option<SliceNarrowRoot> {
        let binding = self.binding_at(span)?;
        match (ordinal, binding) {
            (Some(ordinal), verter_semantic::analysis::flow::FlowBindingRef::Local(binding)) => {
                Some(SliceNarrowRoot::Param { ordinal, binding })
            }
            (_, binding) => Some(SliceNarrowRoot::Local {
                name: Arc::from(name),
                binding,
            }),
        }
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
        self.frame_gate
            .name_is_bound(name, self.rebase(span), meaning, binders)
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

    /// Runtime aliases retain one canonical slot and constant-size source
    /// shape facts. Classification never scans the authored alias group.
    fn classify_binding(&self, binding: SkeletonBindingId) -> NameBinding {
        let binding = self.bindings.canonical_local(binding);
        let shape = self.bindings.runtime_shape(binding);
        #[cfg(test)]
        capture_lookup_probe::inspect();
        if shape.has_destructured_var && !self.frame_gate.destructured_var_is_modelled(binding) {
            return NameBinding::Unmodeled;
        }
        let fact = self.skeleton.binding(binding);
        match fact.kind {
            SkeletonBindingKind::Param => match self.frame_gate.parameters.get(&binding) {
                Some(_) if fact.destructured => NameBinding::Local(None),
                None if fact.destructured
                    && self.frame_gate.modelled_patterns.contains(&binding) =>
                {
                    NameBinding::Local(None)
                }
                Some(parameter) if shape.has_var => {
                    NameBinding::Local(Some(parameter.ordinal as u32))
                }
                Some(parameter) => NameBinding::Param(parameter.ordinal as u32),
                None => NameBinding::Unmodeled,
            },
            SkeletonBindingKind::Const | SkeletonBindingKind::Let | SkeletonBindingKind::Var
                if !fact.destructured || self.frame_gate.modelled_patterns.contains(&binding) =>
            {
                NameBinding::Local(None)
            }
            // A plain `catch` variable is bound at the clause's entry.
            SkeletonBindingKind::CatchParam if !fact.destructured => NameBinding::Local(None),
            SkeletonBindingKind::NestedFunction => NameBinding::NestedFunction,
            _ => NameBinding::Unmodeled,
        }
    }

    /// Retain the shared defining frame and the exact lexical region only.
    /// Captured values and annotation locators are selected by the child graph.
    fn capture_scope_for(&self, function_span: oxc_span::Span) -> CaptureScope {
        CaptureScope {
            enclosing: Some(Arc::new(CapturedFrame {
                gate: Arc::clone(&self.frame_gate),
                region: self
                    .skeleton
                    .innermost_region_containing(self.rebase(function_span)),
            })),
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
    ///
    /// The checker still aggregates every `return` and `yield` of the body
    /// that no path reaches, its references reading their declared types:
    /// dropped statements holding one lower into a trailing
    /// [`SliceStatement::Unreachable`] region the evaluator reads for those
    /// contributions alone. One whose lowering hits an unsupported
    /// construct takes the typed `AbruptCompletion` gap at the region's
    /// head instead.
    fn lower_region(&mut self, statements: &[Statement<'_>]) -> LoweredRegion {
        let enclosing_followed_by_return = self.current_statement_followed_by_return;
        let mut out: Vec<SliceStatement> = Vec::new();
        let mut can_fall_through = true;
        let mut hit_unsupported = false;
        let mut may_break: Vec<SliceBreakTarget> = Vec::new();
        for (index, statement) in statements.iter().enumerate() {
            if !can_fall_through {
                if !hit_unsupported && unreachable_statements_contribute(&statements[index..]) {
                    let unreachable = self.lower_region(&statements[index..]);
                    if unreachable.hit_unsupported {
                        out.insert(
                            0,
                            SliceStatement::Gap(crate::semantic_query::FlowGap::AbruptCompletion),
                        );
                    } else {
                        out.push(SliceStatement::Unreachable(Box::new(unreachable.region)));
                    }
                }
                break;
            }
            self.current_statement_followed_by_return = SuffixReturn::fold(
                std::iter::once(enclosing_followed_by_return)
                    .chain(statements[index + 1..].iter().map(suffix_return_of)),
            );
            if self.span_contains_unsafe_invoked_closure(statement.span()) {
                out.push(SliceStatement::Unsupported(
                    SliceUnsupported::InvokedClosureEffect,
                ));
                hit_unsupported = true;
                can_fall_through = false;
                break;
            }
            let statement_start = out.len();
            match statement {
                Statement::ReturnStatement(ret) => {
                    // A bare call of the frame's own declaration contributes
                    // nothing to the return type: `never`, which leaves the
                    // freshness of every other return untouched.
                    let bare_self_call = ret
                        .argument
                        .as_ref()
                        .is_some_and(|arg| self.returns_bare_self_call(arg));
                    let freshness = if bare_self_call {
                        SliceFreshness::Fresh
                    } else {
                        ret.argument
                            .as_ref()
                            .map_or(SliceFreshness::Pinned, expression_freshness)
                    };
                    let mut predicate_test = None;
                    let argument = ret.argument.as_ref().map(|arg| {
                        if bare_self_call {
                            SliceExpr::Type(GatedLeaf(
                                TypeExpr::Primitive(PrimitiveName::Never),
                                None,
                            ))
                        } else if self.value_span_selected(arg.span()) {
                            let lowered = self.lower_expr(arg, ExprMode::Return);
                            predicate_test = self.return_predicate_test(arg);
                            lowered
                        } else {
                            // An unselected return argument still RUNS:
                            // scan its effects like every elided position.
                            self.scan_unmodeled_position_effects(arg);
                            SliceExpr::Elided
                        }
                    });
                    out.push(SliceStatement::Return {
                        argument,
                        freshness,
                        predicate_test,
                    });
                    can_fall_through = false;
                }
                Statement::BlockStatement(block) => {
                    let child = self.lower_region(&block.body);
                    can_fall_through = child
                        .region
                        .can_fall_through
                        .reaches_end(CompletionDischarge::RegionComposition);
                    hit_unsupported = child.hit_unsupported;
                    // A block absorbs no `break` — an exit targeting an
                    // enclosing switch / labeled statement passes through.
                    may_break.extend(child.may_break);
                    out.push(SliceStatement::Block(child.region));
                }
                Statement::IfStatement(if_stmt) if discarded_value_holds_write(&if_stmt.test) => {
                    let lowered = self.lower_if_with_test_writes(if_stmt);
                    can_fall_through = lowered
                        .region
                        .can_fall_through
                        .reaches_end(CompletionDischarge::RegionComposition);
                    hit_unsupported = lowered.hit_unsupported;
                    may_break.extend(lowered.may_break);
                    out.extend(lowered.region.statements.iter().cloned());
                }
                Statement::IfStatement(if_stmt) => {
                    // The evaluator never consumes the test's VALUE, so no
                    // test content lowers — but its narrowing facts do,
                    // through the ONE guard authority both control
                    // spellings share.
                    let guard = self.lower_guard(&if_stmt.test);
                    // A guard form the lowering REFUSED (an unprovable
                    // `instanceof` constructor) flagged the gap while the
                    // test lowered. Take it here, ahead of the arms: an arm
                    // region's own statement loop would otherwise drain it
                    // INTO the arm.
                    let unprovable_guard = std::mem::take(&mut self.control_test_gap);
                    // A call in the TEST is decided above ONLY when its
                    // result provably cannot control the arms' narrowing;
                    // a predicate call takes evaluator evidence at guard
                    // application, and an unprovable callee degrades the
                    // demand through the typed guard-narrowing gap below.
                    // An entered `asserts` call in the test narrows once
                    // the test has run, ahead of both arms.
                    let mut unprovable_control_call = false;
                    let test_assertions = self.collecting_entered_assertions(|this| {
                        unprovable_control_call = this.record_control_position_calls(&if_stmt.test);
                    });
                    out.extend(test_assertions);
                    let active_guard_base = self.active_guard_bindings.len();
                    let guard_bindings = self.guard_bindings(&guard, if_stmt.test.span());
                    self.active_guard_bindings
                        .extend(guard_bindings.iter().copied());
                    let consequent = self.lower_arm(&if_stmt.consequent);
                    self.active_guard_bindings.truncate(active_guard_base);
                    let alternate = if_stmt.alternate.as_ref().map(|alternate| {
                        self.active_guard_bindings
                            .extend(guard_bindings.iter().copied());
                        let lowered = self.lower_arm(alternate);
                        self.active_guard_bindings.truncate(active_guard_base);
                        lowered
                    });
                    can_fall_through = consequent
                        .region
                        .can_fall_through
                        .reaches_end(CompletionDischarge::RegionComposition)
                        || alternate
                            .as_ref()
                            .map(|region| {
                                region
                                    .region
                                    .can_fall_through
                                    .reaches_end(CompletionDischarge::RegionComposition)
                            })
                            .unwrap_or(true);
                    hit_unsupported = consequent.hit_unsupported
                        || alternate
                            .as_ref()
                            .is_some_and(|region| region.hit_unsupported);
                    // An `if` absorbs no `break` either: a conditional exit
                    // (`if (f) break;`) is still an exit of the region.
                    may_break.extend(consequent.may_break);
                    // A call in the TEST is a throw point BEFORE either
                    // arm — the test lowers to guard facts only, so the
                    // marker carries the point (ahead of the `if`, where
                    // the test evaluates).
                    if unprovable_control_call || unprovable_guard {
                        out.push(SliceStatement::Gap(
                            crate::semantic_query::FlowGap::GuardNarrowing,
                        ));
                    }
                    if verter_semantic::analysis::flow::expression_contains_call(&if_stmt.test) {
                        out.push(SliceStatement::ThrowPoint);
                    }
                    self.lower_test_updates(&if_stmt.test, &mut out);
                    if let Some(alternate) = alternate {
                        may_break.extend(alternate.may_break);
                        out.push(SliceStatement::If {
                            guard,
                            consequent: Box::new(consequent.region),
                            alternate: Some(Box::new(alternate.region)),
                        });
                    } else {
                        out.push(SliceStatement::If {
                            guard,
                            consequent: Box::new(consequent.region),
                            alternate: None,
                        });
                    }
                }
                Statement::VariableDeclaration(decl) => {
                    self.lower_variable_declaration(decl, &mut out)
                }
                // An expression statement's value is never consumed by the
                // evaluator; its evaluation effects ride the slice's typed
                // effect obligations, not this content tree — EXCEPT the
                // two value-neutral forms whose effect IS the point: a
                // whole-binding `=` write the evaluator can apply in
                // source order, and a same-file assertion call whose
                // narrowing persists.
                Statement::ExpressionStatement(expression) => {
                    // A statement-position `yield x` is the generator's
                    // yield-parameter contributor: the argument lowers like
                    // a return argument (unconditionally — the yield
                    // parameter is demanded by the generator's own return
                    // wrap, so over-selecting here is the safe asymmetry the
                    // shared classifier documents). `yield*` delegation
                    // keeps the fail-closed marker: the delegated
                    // sequence's yield surface is not modelled.
                    if let Expression::YieldExpression(yield_expr) =
                        unwrap_parenthesized(&expression.expression)
                    {
                        let argument = yield_expr.argument.as_ref().map(|arg| {
                            if yield_expr.delegate {
                                SliceExpr::Gap(crate::semantic_query::FlowGap::UnmodeledExpression)
                            } else {
                                self.lower_expr(arg, ExprMode::Return)
                            }
                        });
                        let freshness = yield_expr
                            .argument
                            .as_ref()
                            .map_or(SliceFreshness::Pinned, expression_freshness);
                        out.push(SliceStatement::Yield {
                            argument,
                            freshness,
                        });
                    } else if let Some(statement) =
                        self.lower_effect_statement(&expression.expression)
                    {
                        // A statement-position call to a callee proven
                        // never to return ends the path exactly as an
                        // authored `throw` does: the statements after it
                        // are unreachable and contribute nothing.
                        if statement_ends_path(&statement) {
                            can_fall_through = false;
                        }
                        out.push(statement);
                    }
                }
                // A `throw` terminates the region path without contributing
                // a return arm; the marker carries the throw POINT to the
                // evaluator (a `catch` is entered from it too). The
                // argument still EVALUATES first: an effect there runs
                // before the region ends, so it takes the same fail-closed
                // scan.
                Statement::ThrowStatement(throw_stmt) => {
                    // An entered `asserts` call in it narrows before the
                    // throw point.
                    let entered = self.collecting_entered_assertions(|this| {
                        this.scan_unmodeled_position_effects(&throw_stmt.argument)
                    });
                    out.extend(entered);
                    out.push(SliceStatement::Throw);
                    can_fall_through = false;
                }
                Statement::DoWhileStatement(_)
                | Statement::ForInStatement(_)
                | Statement::ForOfStatement(_)
                | Statement::ForStatement(_)
                | Statement::WhileStatement(_) => {
                    // A return-free loop is fall-through TRANSPARENT only
                    // while it binds nothing that outlives it, transfers
                    // no control past a lowered construct, and carries no
                    // unmodelled transfer for a downstream-selected slot. A
                    // `var` declaration escapes the loop; a `break`/
                    // `continue` naming an enclosing label exits an edge
                    // the vanished body can no longer record; a selected
                    // guard, call/assertion, or write depends on iteration
                    // flow. Every such loop lowers structurally and the
                    // evaluator iterates it to the checker's fixed point
                    // ([`SliceLoop`]); a shape that does not lower keeps
                    // the typed loop refusal.
                    let labels = std::mem::take(&mut self.pending_loop_labels);
                    if self.control_has_return(statement)
                        || statement_yields_in_own_frame(statement)
                        || declares_var(statement)
                        || loop_transfers_to_enclosing_label(statement, &self.loop_direct_labels)
                        || self.loop_has_selected_transfer(statement)
                    {
                        match self.lower_loop(statement, labels) {
                            Some(lowered) => {
                                hit_unsupported = lowered.hit_unsupported;
                                may_break.extend(lowered.may_break);
                                if !lowered.completes || hit_unsupported {
                                    can_fall_through = false;
                                }
                                out.push(SliceStatement::Loop(Box::new(lowered.lowered)));
                            }
                            None => {
                                out.push(SliceStatement::Unsupported(SliceUnsupported::Loop));
                                hit_unsupported = true;
                                can_fall_through = false;
                            }
                        }
                    } else if loop_exit_edge_is_unreachable(statement, &self.loop_direct_labels) {
                        // The loop never completes normally, so it ends the
                        // region's normal path exactly as an authored `throw`
                        // does. The statements after it are unreachable and
                        // contribute nothing, and the body no longer
                        // contributes the fall-through `undefined`.
                        out.push(SliceStatement::DivergentLoop);
                        can_fall_through = false;
                    } else {
                        out.push(SliceStatement::TransparentLoop);
                    }
                }
                Statement::LabeledStatement(labeled) => {
                    // The label is a break target for its OWN body in both
                    // paths: a `break` naming it exits to after the
                    // statement, which the absorption below folds into the
                    // statement's reachability.
                    let label: Arc<str> = Arc::from(labeled.label.name.as_str());
                    self.break_targets.push(Some(Arc::clone(&label)));
                    self.break_target_followed_by_return
                        .push(self.current_statement_followed_by_return);
                    // A label chain directly wrapping a loop names the
                    // loop's own exit/iteration edge: record it so the
                    // loop's transparency classification treats a jump to
                    // it as local rather than an escaping transfer.
                    let direct_wrap = label_directly_wraps_loop(&labeled.body);
                    let pending_base = self.pending_loop_labels.len();
                    if direct_wrap {
                        self.loop_direct_labels.push(Arc::clone(&label));
                        self.pending_loop_labels.push(Arc::clone(&label));
                    }
                    let child = self.lower_arm(&labeled.body);
                    self.pending_loop_labels.truncate(pending_base);
                    if direct_wrap {
                        self.loop_direct_labels.pop();
                    }
                    self.break_targets.pop();
                    self.break_target_followed_by_return.pop();
                    let mut absorbed = false;
                    for target in child.may_break {
                        match target {
                            SliceBreakTarget::Named(name) if name == label => absorbed = true,
                            other => may_break.push(other),
                        }
                    }
                    // The body lowers identically whether or not it bears a
                    // return: the label wraps an ordinary statement whose
                    // own rail decides (a block's hoisted `var`s, a loop's
                    // escaping `var` fail-close, an `if` arm's conditional
                    // binding, `switch` / `try` / `with` unsupported), and
                    // the EVALUATOR needs the label's name either way —
                    // the absorbed `break` is what lets execution reach
                    // past the statement even when the body itself cannot,
                    // and its captured state is that edge's layer state.
                    can_fall_through = child
                        .region
                        .can_fall_through
                        .reaches_end(CompletionDischarge::RegionComposition)
                        || absorbed;
                    hit_unsupported = child.hit_unsupported;
                    out.push(SliceStatement::Labeled {
                        label,
                        body: Box::new(child.region),
                    });
                }
                Statement::SwitchStatement(switch) => {
                    // Each case clause lowers as its own region with the
                    // switch on the break-target stack: a `break` ends the
                    // case's path and is absorbed into the clause's
                    // `breaks` flag; a `break` naming an OUTER labeled
                    // statement propagates through the switch untouched.
                    // The discriminant lowers no value content; when it is
                    // a narrowable reference it rides the statement so the
                    // evaluator can narrow it per dispatch edge, and each
                    // literal case test rides its clause for the same
                    // purpose (a non-literal test narrows nothing).
                    //
                    // Both positions still EXECUTE, so their effects take
                    // the fail-closed scan: the discriminant's value feeds
                    // the dispatch but never the demanded answer (the
                    // discarded-operand discipline — only an `asserts`
                    // callee narrows what follows), and each case TEST is
                    // a control position exactly as an `if` test is —
                    // `switch (true) { case isString(x): … }` narrows `x`
                    // inside the clause in the checker.
                    //
                    // The scan alone is NOT completeness evidence, because
                    // a case relation narrows with no call and no write at
                    // all: `switch (true) { case typeof x === "string": }`
                    // narrows `x` inside the clause, `case K:` against a
                    // literal-typed `const` narrows the discriminant, and a
                    // bare-identifier test aliasing a narrowing expression
                    // narrows through the alias. The MODELED
                    // dispatch is therefore exactly one shape — a
                    // represented discriminant against a LITERAL case
                    // relation, the only pair this lowering carries to the
                    // evaluator — and every other clause with a test mints
                    // the typed gap.
                    //
                    // The gap belongs AHEAD of the switch: a case test
                    // evaluates whether or not its clause is entered, so
                    // the flag must not drain into a clause region's own
                    // statement loop.
                    let mut unprovable_switch_effect =
                        self.record_discarded_operand_calls(&switch.discriminant);
                    let has_default = switch.cases.iter().any(|case| case.test.is_none());
                    let discriminant = self.narrow_subject_of(&switch.discriminant);
                    self.break_targets.push(None);
                    self.break_target_followed_by_return
                        .push(SuffixReturn::NotGuaranteed);
                    // A clause body evaluates under the dispatch narrow
                    // of the discriminant, so a closure created there
                    // takes the closure-capture rail the `if` arms and
                    // the ternary's arms take.
                    let active_guard_base = self.active_guard_bindings.len();
                    if let Some(subject) = discriminant.as_ref() {
                        let bindings = self.subject_bindings(subject, switch.discriminant.span());
                        self.active_guard_bindings.extend(bindings);
                    }
                    let mut cases = Vec::with_capacity(switch.cases.len());
                    for case in &switch.cases {
                        if let Some(test) = case.test.as_ref() {
                            unprovable_switch_effect |= self.record_control_position_calls(test);
                        }
                        // The MODELED dispatch is exactly one pair: a
                        // represented discriminant against a LITERAL case
                        // relation. Anything else — a `typeof` or
                        // equality relation under a non-reference
                        // discriminant, a case test naming a constant, a
                        // template case, a discriminant this half cannot
                        // represent — establishes a clause narrow the
                        // checker applies and this lowering carries
                        // nothing for, so the switch degrades. The
                        // unrecognized clause keeps its OWN carrier: it
                        // must never be dispatched as the default edge.
                        let test = match case.test.as_ref() {
                            None => SliceSwitchTest::Default,
                            Some(test) => {
                                match guard_literal_of(test, self.source)
                                    .or_else(|| self.guard_value_path_of(test))
                                    .filter(|_| discriminant.is_some())
                                {
                                    Some(literal) => SliceSwitchTest::Literal(literal),
                                    None => {
                                        unprovable_switch_effect = true;
                                        SliceSwitchTest::Unmodeled
                                    }
                                }
                            }
                        };
                        let lowered = self.lower_region(&case.consequent);
                        hit_unsupported |= lowered.hit_unsupported;
                        let mut breaks = false;
                        for target in lowered.may_break {
                            match target {
                                SliceBreakTarget::Anonymous => breaks = true,
                                named => may_break.push(named),
                            }
                        }
                        cases.push(SliceSwitchCase {
                            region: lowered.region,
                            breaks: NormalCompletion::minted(
                                breaks,
                                CompletionConstruction::SwitchCaseBreak,
                            ),
                            test,
                        });
                    }
                    self.active_guard_bindings.truncate(active_guard_base);
                    self.break_targets.pop();
                    self.break_target_followed_by_return.pop();
                    // Past the switch is reachable when no `default`
                    // exists (a non-matching discriminant skips every
                    // case), when the LAST clause falls off the end of the
                    // switch, or when any clause exits via `break`.
                    can_fall_through = !has_default
                        || cases.last().is_some_and(|case| {
                            case.region
                                .can_fall_through
                                .reaches_end(CompletionDischarge::RegionComposition)
                        })
                        || cases.iter().any(|case| {
                            case.breaks
                                .reaches_end(CompletionDischarge::RegionComposition)
                        });
                    if unprovable_switch_effect {
                        out.push(SliceStatement::Gap(
                            crate::semantic_query::FlowGap::GuardNarrowing,
                        ));
                    }
                    out.push(SliceStatement::Switch {
                        discriminant,
                        cases: Arc::from(cases.into_boxed_slice()),
                        has_default,
                    });
                }
                Statement::TryStatement(try_stmt) => {
                    let block = self.lower_region(&try_stmt.block.body);
                    hit_unsupported |= block.hit_unsupported;
                    let mut clause_may_break = block.may_break;
                    let block = block.region;
                    let catch = try_stmt.handler.as_ref().map(|handler| {
                        let param = handler.param.as_ref().and_then(|param| {
                            match &param.pattern {
                                BindingPattern::BindingIdentifier(id) => {
                                    Some(Arc::from(id.name.as_str()))
                                }
                                // A destructured catch parameter binds
                                // nothing this frame can name.
                                _ => None,
                            }
                        });
                        let region = self.lower_region(&handler.body.body);
                        hit_unsupported |= region.hit_unsupported;
                        clause_may_break.extend(region.may_break);
                        let declared = handler.param.as_ref().and_then(|param| {
                            param.type_annotation.as_ref().map(|annotation| {
                                self.gate(
                                    lower_ts_type(&annotation.type_annotation, self.source),
                                    param.pattern.span(),
                                    &[],
                                )
                            })
                        });
                        Box::new(SliceCatchClause {
                            declared,
                            binding: handler.param.as_ref().and_then(|param| {
                                match &param.pattern {
                                    BindingPattern::BindingIdentifier(id) => {
                                        self.bindings.declaration_at_span(self.rebase(id.span))
                                    }
                                    _ => None,
                                }
                            }),
                            param,
                            region: region.region,
                        })
                    });
                    let finally = try_stmt.finalizer.as_ref().map(|finalizer| {
                        let region = self.lower_region(&finalizer.body);
                        hit_unsupported |= region.hit_unsupported;
                        (Box::new(region.region), region.may_break)
                    });
                    // A `finally` that CANNOT fall through completes
                    // abruptly on every path, and abrupt completion
                    // discards the try/catch's pending exits — pending
                    // returns AND pending `break`s alike. A finally that
                    // CAN fall through overrides nothing on that path: a
                    // pending break proceeds past the try when the finally
                    // does not return, so the try/catch clauses' break
                    // exits propagate whenever the finally has a
                    // fall-through path (or does not exist). The finally
                    // clause's OWN break exits always propagate: they fire
                    // after every override decision, they are never
                    // pending.
                    let finally_blocks_exits = finally.as_ref().is_some_and(|(region, _)| {
                        !region
                            .can_fall_through
                            .reaches_end(CompletionDischarge::RegionComposition)
                    });
                    // A named break crossing this try for any enclosing
                    // label remains an authored return-inference path even
                    // when blocks or inner labels wrap the try. An abrupt
                    // finally replaces the runtime edge, but not that
                    // implicit-`undefined` inference contribution.
                    let target_followed_by_return = |name: &Arc<str>| {
                        self.break_targets
                            .iter()
                            .zip(self.break_target_followed_by_return.iter())
                            .rev()
                            .find(|(entry, _)| entry.as_ref() == Some(name))
                            .map(|(_, followed_by_return)| *followed_by_return)
                    };
                    // An anonymous break's destination is the innermost
                    // anonymous breakable's continuation.
                    let anonymous_followed_by_return = || {
                        self.break_targets
                            .iter()
                            .zip(self.break_target_followed_by_return.iter())
                            .rev()
                            .find(|(entry, _)| entry.is_none())
                            .map(|(_, followed_by_return)| *followed_by_return)
                    };
                    // The destination decides the contribution, and an
                    // UNDECIDED destination decides nothing: the value keeps
                    // the derivation it always had, and the gap below makes
                    // the result return without ever being admitted.
                    let pending_break_destination = |state: SuffixReturn| {
                        finally_blocks_exits
                            && clause_may_break.iter().any(|target| match target {
                                SliceBreakTarget::Named(name) => {
                                    target_followed_by_return(name) == Some(state)
                                }
                                SliceBreakTarget::Anonymous => {
                                    anonymous_followed_by_return() == Some(state)
                                }
                            })
                    };
                    let pending_break_destination_undecided =
                        pending_break_destination(SuffixReturn::Undecided);
                    let pending_break_contributes_undefined =
                        pending_break_destination(SuffixReturn::NotGuaranteed)
                            || pending_break_destination_undecided;
                    let mut pending_break_following_return_targets: Vec<Arc<str>> = Vec::new();
                    if finally_blocks_exits {
                        for target in &clause_may_break {
                            let SliceBreakTarget::Named(name) = target else {
                                continue;
                            };
                            if target_followed_by_return(name) == Some(SuffixReturn::Guaranteed)
                                && !pending_break_following_return_targets.contains(name)
                            {
                                pending_break_following_return_targets.push(Arc::clone(name));
                            }
                        }
                    }
                    if !finally_blocks_exits {
                        may_break.extend(clause_may_break);
                    } else {
                        // When the crossed target is followed by a guaranteed
                        // return, inference keeps that suffix return instead
                        // of the implicit-undefined contribution. Propagate
                        // the named exit until its label absorbs it; the
                        // qualifier is inherited through intervening labels
                        // and blocks by `lower_region`.
                        may_break.extend(
                            pending_break_following_return_targets
                                .iter()
                                .cloned()
                                .map(SliceBreakTarget::Named),
                        );
                    }
                    if let Some((_, finally_may_break)) = &finally {
                        may_break.extend(finally_may_break.iter().cloned());
                    }
                    let pre_finally_fall_through = block
                        .can_fall_through
                        .reaches_end(CompletionDischarge::RegionComposition)
                        || catch.as_ref().is_some_and(|catch| {
                            catch
                                .region
                                .can_fall_through
                                .reaches_end(CompletionDischarge::RegionComposition)
                        });
                    can_fall_through = pre_finally_fall_through
                        && finally.as_ref().is_none_or(|(region, _)| {
                            region
                                .can_fall_through
                                .reaches_end(CompletionDischarge::RegionComposition)
                        });
                    if pending_break_destination_undecided {
                        // A pending break whose destination this lowering
                        // cannot classify. The contribution above is a
                        // derivation, not a proof, so the slice carries the
                        // typed gap ahead of the try: the evaluation returns
                        // the value and refuses to warm it.
                        out.push(SliceStatement::Gap(
                            crate::semantic_query::FlowGap::AbruptCompletion,
                        ));
                    }
                    out.push(SliceStatement::Try {
                        block: Box::new(block),
                        catch,
                        finally: finally.map(|(region, _)| region),
                        pending_break_contributes_undefined,
                        pending_break_following_return_targets: Arc::from(
                            pending_break_following_return_targets.into_boxed_slice(),
                        ),
                    });
                }
                Statement::WithStatement(_) => {
                    out.push(SliceStatement::Unsupported(SliceUnsupported::With));
                    hit_unsupported = true;
                    can_fall_through = false;
                }
                Statement::BreakStatement(break_stmt) => {
                    // A `break` whose target is being lowered ends this
                    // region's path and records the exit; the target's own
                    // lowering absorbs it. Any other `break` stays the
                    // typed jump failure it always was.
                    let target = match break_stmt.label.as_ref() {
                        Some(label) => {
                            let name: Arc<str> = Arc::from(label.name.as_str());
                            self.break_targets
                                .iter()
                                .rev()
                                .any(|entry| entry.as_ref() == Some(&name))
                                .then_some(SliceBreakTarget::Named(name))
                        }
                        // An unlabeled break targets the innermost
                        // ANONYMOUS breakable (a switch) — a labeled
                        // statement does not accept it.
                        None => self
                            .break_targets
                            .iter()
                            .rev()
                            .any(|entry| entry.is_none())
                            .then_some(SliceBreakTarget::Anonymous),
                    };
                    match target {
                        Some(target) => {
                            may_break.push(target.clone());
                            // The marker lets the evaluator capture the
                            // layer state AT the break point — the edge
                            // past the absorbing construct is that state,
                            // and the rest of this region is unreachable.
                            out.push(SliceStatement::Break {
                                target: match target {
                                    SliceBreakTarget::Anonymous => None,
                                    SliceBreakTarget::Named(name) => Some(name),
                                },
                            });
                            can_fall_through = false;
                        }
                        None => {
                            out.push(SliceStatement::Unsupported(SliceUnsupported::Jump));
                            hit_unsupported = true;
                            can_fall_through = false;
                        }
                    }
                }
                Statement::ContinueStatement(continue_stmt) => {
                    // A `continue` of a loop whose body is being lowered
                    // ends this region's path and records a back edge of
                    // that loop; any other stays the typed jump failure.
                    let target = match continue_stmt.label.as_ref() {
                        Some(label) => {
                            let name: Arc<str> = Arc::from(label.name.as_str());
                            self.continue_targets
                                .iter()
                                .any(|labels| labels.contains(&name))
                                .then_some(Some(name))
                        }
                        None => (!self.continue_targets.is_empty()).then_some(None),
                    };
                    match target {
                        Some(target) => out.push(SliceStatement::Continue { target }),
                        None => {
                            out.push(SliceStatement::Unsupported(SliceUnsupported::Jump));
                            hit_unsupported = true;
                        }
                    }
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
                // A class declaration is NOT a no-op: its decorators,
                // `super_class` heritage expression, computed member keys,
                // member decorators, static blocks, and static property /
                // accessor initializers evaluate in THIS frame at the
                // statement. The ONE class discipline the leaf path applies
                // to a class EXPRESSION answers for it: provably
                // non-narrowing calls are decided above, every other call
                // flags the enclosing statement's typed gap, and a
                // whole-binding WRITE to a frame-owned target — invisible
                // to the slice's effect ledger, which no class subtree
                // feeds — takes the same typed gap.
                // Deferred bodies (a method runs when called, an instance
                // property initializer at construction) keep the
                // nested-frame blanket treatment.
                Statement::ClassDeclaration(class) => {
                    let mut scanner = LeafCallScanner::default();
                    scanner.visit_class(class);
                    self.drain_leaf_call_scanner(scanner);
                }
                // Declaration / no-op statements: transparent (no return
                // contribution, no content statement) — EXCEPT the enum:
                // a non-ambient enum's member initializers EVALUATE in
                // this frame at the statement (`enum E { A =
                // (assertString(x), 1) }` narrows `x` for every read that
                // follows in the checker), so its effectful initializers
                // take the same fail-closed scan every unmodeled position
                // gets. A `declare`d enum is ambient: nothing runs.
                Statement::TSEnumDeclaration(enumeration) => {
                    if !enumeration.declare {
                        for member in &enumeration.body.members {
                            if let Some(initializer) = member.initializer.as_ref() {
                                // An entered `asserts` call narrows once the
                                // initializer has run.
                                let entered = self.collecting_entered_assertions(|this| {
                                    this.scan_unmodeled_position_effects(initializer)
                                });
                                out.extend(entered);
                            }
                        }
                    }
                }
                // Declaration / no-op statements: transparent (no return
                // contribution, no content statement) — EXCEPT the
                // executable declarations: a non-ambient namespace body
                // RUNS its statements at this statement.
                Statement::TSModuleDeclaration(module) => {
                    // A string-named `module "…"` block is an ambient
                    // module augmentation: it evaluates nothing.
                    if !module.declare
                        && matches!(
                            module.id,
                            oxc_ast::ast::TSModuleDeclarationName::Identifier(_)
                        )
                    {
                        self.scan_module_declaration_effects(module);
                    }
                }
                Statement::DebuggerStatement(_)
                | Statement::EmptyStatement(_)
                | Statement::FunctionDeclaration(_)
                | Statement::TSTypeAliasDeclaration(_)
                | Statement::TSInterfaceDeclaration(_)
                | Statement::TSGlobalDeclaration(_)
                | Statement::TSImportEqualsDeclaration(_) => {}
            }
            // A ternary test lowered INSIDE this statement carried a
            // control call this half could neither certify nor evidence:
            // the typed guard-narrowing gap lands AHEAD of the statement,
            // so a terminal statement (a `return` of the ternary) cannot
            // strand it unreachable.
            if std::mem::take(&mut self.control_test_gap) {
                out.insert(
                    statement_start,
                    SliceStatement::Gap(crate::semantic_query::FlowGap::GuardNarrowing),
                );
            }
            if hit_unsupported {
                can_fall_through = false;
            }
        }
        self.current_statement_followed_by_return = enclosing_followed_by_return;
        LoweredRegion {
            region: SliceRegion {
                statements: Arc::from(out.into_boxed_slice()),
                can_fall_through: NormalCompletion::minted(
                    can_fall_through,
                    CompletionConstruction::RegionAccumulator,
                ),
            },
            hit_unsupported,
            may_break,
        }
    }

    /// Lower a loop the evaluator iterates to the checker's fixed point
    /// ([`SliceLoop`]). `None` for a shape it does not model — `for
    /// await`, or a `for…of` / `for…in` declaring more than one binding
    /// — which keeps the typed loop refusal.
    fn lower_loop(
        &mut self,
        statement: &Statement<'_>,
        labels: Vec<Arc<str>>,
    ) -> Option<LoweredLoop> {
        let (test, update, body, test_after, element_source) = match statement {
            Statement::WhileStatement(while_stmt) => {
                (Some(&while_stmt.test), None, &while_stmt.body, false, None)
            }
            Statement::DoWhileStatement(do_while) => {
                (Some(&do_while.test), None, &do_while.body, true, None)
            }
            Statement::ForStatement(for_stmt) => (
                for_stmt.test.as_ref(),
                for_stmt.update.as_ref(),
                &for_stmt.body,
                false,
                None,
            ),
            Statement::ForOfStatement(for_of) if !for_of.r#await => (
                None,
                None,
                &for_of.body,
                false,
                Some((&for_of.left, &for_of.right, false)),
            ),
            Statement::ForInStatement(for_in) => (
                None,
                None,
                &for_in.body,
                false,
                Some((&for_in.left, &for_in.right, true)),
            ),
            _ => return None,
        };
        let constant = test.and_then(literal_boolean_value);
        // The element binding: one declared identifier. A destructuring
        // pattern or an existing assignment target binds nothing the
        // evaluator models — its names read as unmodeled bindings and its
        // writes keep the typed unapplied-write degradation.
        let element_binding = match element_source {
            Some((oxc_ast::ast::ForStatementLeft::VariableDeclaration(decl), _, _)) => {
                match (decl.declarations.as_slice(), decl.kind) {
                    ([declarator], kind) => match &declarator.id {
                        BindingPattern::BindingIdentifier(id) => {
                            let span = self.rebase(id.span);
                            let binding = self.bindings.declaration_at_span(span)?;
                            let kind = match kind {
                                VariableDeclarationKind::Let => SliceBindingKind::Let,
                                VariableDeclarationKind::Var => SliceBindingKind::Var,
                                VariableDeclarationKind::Const
                                | VariableDeclarationKind::Using
                                | VariableDeclarationKind::AwaitUsing => SliceBindingKind::Const,
                            };
                            // An element the demand never reads binds
                            // nothing the evaluation holds a product for.
                            self.binding_is_selected(binding)
                                .then_some(SliceLoopBinding {
                                    binding,
                                    kind,
                                    span,
                                })
                        }
                        _ => None,
                    },
                    _ => return None,
                }
            }
            _ => None,
        };
        // A declared destructuring element binds its pattern at every
        // iteration.
        let element_pattern = match element_source {
            Some((oxc_ast::ast::ForStatementLeft::VariableDeclaration(decl), _, _))
                if element_binding.is_none() =>
            {
                match decl.declarations.as_slice() {
                    [declarator] => self.lower_pattern(&declarator.id).map(|pattern| {
                        let kind = match decl.kind {
                            VariableDeclarationKind::Let => SliceBindingKind::Let,
                            VariableDeclarationKind::Var => SliceBindingKind::Var,
                            VariableDeclarationKind::Const
                            | VariableDeclarationKind::Using
                            | VariableDeclarationKind::AwaitUsing => SliceBindingKind::Const,
                        };
                        self.record_aliased_bindings(
                            &pattern,
                            kind == SliceBindingKind::Const,
                            None,
                        );
                        (pattern, kind)
                    }),
                    _ => None,
                }
            }
            _ => None,
        };
        let completes = !loop_exit_edge_is_unreachable(statement, &self.loop_direct_labels);
        // The loop's own region: the `for` initializer, and the iterated
        // expression, evaluated once when the loop is entered.
        let mut init: Vec<SliceStatement> = Vec::new();
        if let Statement::ForStatement(for_stmt) = statement {
            match &for_stmt.init {
                Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(decl)) => {
                    self.lower_variable_declaration(decl, &mut init);
                }
                Some(other) => {
                    if let Some(expression) = other.as_expression() {
                        init.extend(self.lower_effect_statement(expression));
                    }
                }
                None => {}
            }
        }
        let element = element_source.map(|(_, right, keys)| SliceLoopElement {
            binding: element_binding,
            pattern: element_pattern,
            iterable: self.lower_expr(right, ExprMode::Return),
            keys,
        });
        // The test lowers to its narrowing facts through the one guard
        // authority; a test this vocabulary cannot express, or a control
        // call it cannot certify, takes the typed gap ahead of the loop.
        let mut test_gap = false;
        let mut test_throws = false;
        let mut active_guard = None;
        let mut test_effects: Vec<SliceStatement> = Vec::new();
        let test = match test {
            Some(test) => {
                let guard = self.lower_guard(test);
                test_gap |= std::mem::take(&mut self.control_test_gap);
                test_gap |= self.record_control_position_calls(test);
                test_throws = verter_semantic::analysis::flow::expression_contains_call(test);
                self.lower_test_updates(test, &mut test_effects);
                if !test_after {
                    active_guard = Some(self.guard_bindings(&guard, test.span()));
                }
                if test_after {
                    SliceLoopTest::After { guard, constant }
                } else {
                    SliceLoopTest::Before { guard, constant }
                }
            }
            None if element.is_some() => SliceLoopTest::Exhausted,
            None => SliceLoopTest::Never,
        };
        // The body: an unlabeled `break` and `continue` target this loop,
        // and a label chain wrapping the loop wraps nothing inside it.
        let enclosing_direct_labels = std::mem::take(&mut self.loop_direct_labels);
        // A `break` leaves to the statement after the loop.
        self.break_targets.push(None);
        self.break_target_followed_by_return
            .push(self.current_statement_followed_by_return);
        let labels: Arc<[Arc<str>]> = Arc::from(labels.into_boxed_slice());
        self.continue_targets.push(Arc::clone(&labels));
        let active_guard_base = self.active_guard_bindings.len();
        if let Some(bindings) = &active_guard {
            self.active_guard_bindings.extend(bindings.iter().copied());
        }
        let lowered_body = self.lower_arm(body);
        self.active_guard_bindings.truncate(active_guard_base);
        self.continue_targets.pop();
        self.break_target_followed_by_return.pop();
        self.break_targets.pop();
        self.loop_direct_labels = enclosing_direct_labels;
        let mut update_statements: Vec<SliceStatement> = Vec::new();
        if let Some(update) = update {
            update_statements.extend(self.lower_effect_statement(update));
        }
        if test_gap {
            self.control_test_gap = true;
        }
        let region = |statements: Vec<SliceStatement>| SliceRegion {
            statements: Arc::from(statements.into_boxed_slice()),
            can_fall_through: NormalCompletion::minted(
                true,
                CompletionConstruction::SynthesizedRegion,
            ),
        };
        // The loop's own anonymous break is absorbed here; a named one
        // travels to its labeled statement.
        let may_break = lowered_body
            .may_break
            .into_iter()
            .filter(|target| !matches!(target, SliceBreakTarget::Anonymous))
            .collect();
        let (writes, inferred) = self.loop_dependencies(statement);
        Some(LoweredLoop {
            lowered: SliceLoop {
                init: region(init),
                test,
                test_effects: region(test_effects),
                element,
                body: lowered_body.region,
                update: region(update_statements),
                labels,
                test_throws,
                writes,
                inferred,
            },
            hit_unsupported: lowered_body.hit_unsupported,
            may_break,
            completes,
        })
    }

    /// The dependencies the checker's loop analysis follows: every write
    /// inside the loop that retypes a binding, with the bindings its value
    /// reads ([`SliceLoop::writes`]), and every binding the loop declares
    /// with an inferred type, with the bindings its initializer reads
    /// ([`SliceLoop::inferred`]).
    fn loop_dependencies(
        &self,
        statement: &Statement<'_>,
    ) -> (Arc<[SliceLoopWrite]>, Arc<[SliceLoopDependency]>) {
        let loop_span = self.rebase(statement.span());
        let inferred: Vec<SliceLoopDependency> = self
            .skeleton
            .bindings
            .iter()
            .filter(|binding| {
                loop_span.contains(binding.span)
                    && !binding.destructured
                    && binding.annotation_span.is_none()
                    && matches!(
                        binding.kind,
                        SkeletonBindingKind::Let
                            | SkeletonBindingKind::Const
                            | SkeletonBindingKind::Var
                    )
            })
            .filter_map(|binding| {
                let initializer = binding.initializer?;
                let declared = self.bindings.declaration_at_span(binding.span)?;
                Some(SliceLoopDependency {
                    binding: self.bindings.canonical_local(declared),
                    reads: self.site_binding_reads(initializer),
                })
            })
            .collect();
        // A write with no value site (`x++`) reads what its own site does.
        let writes: Vec<SliceLoopWrite> = self
            .skeleton
            .writes
            .iter()
            .filter(|write| loop_span.contains(write.span))
            .filter_map(|write| {
                let binding = write.binding.as_ref()?;
                (write.path.is_empty() || self.is_evolving_binding(binding)).then(|| {
                    SliceLoopWrite {
                        binding: self.canonical_binding_ref(binding),
                        reads: self.site_reads(write.value.unwrap_or(write.site)),
                    }
                })
            })
            .collect();
        (
            Arc::from(writes.into_boxed_slice()),
            Arc::from(inferred.into_boxed_slice()),
        )
    }

    /// Every binding the sites under `site` read — a frame local by its
    /// canonical binding, a captured one by its identity — deduplicated.
    fn site_reads(
        &self,
        site: verter_semantic::analysis::flow::SkeletonExprSiteId,
    ) -> Arc<[FlowBindingRef]> {
        let span = self.skeleton.expr_site(site).span;
        let mut reads: Vec<FlowBindingRef> = Vec::new();
        for read in self
            .skeleton
            .expr_sites
            .iter()
            .filter(|candidate| span.contains(candidate.span))
            .flat_map(|candidate| candidate.reads.iter())
        {
            if let Some(binding) = read.binding.as_ref() {
                let binding = self.canonical_binding_ref(binding);
                if !reads.contains(&binding) {
                    reads.push(binding);
                }
            }
        }
        Arc::from(reads.into_boxed_slice())
    }

    /// A binding reference with a frame local named by its canonical
    /// binding.
    fn canonical_binding_ref(&self, binding: &FlowBindingRef) -> FlowBindingRef {
        match binding {
            FlowBindingRef::Local(local) => {
                FlowBindingRef::Local(self.bindings.canonical_local(*local))
            }
            captured @ FlowBindingRef::Captured(_) => captured.clone(),
        }
    }

    /// The frame bindings every site under `site` reads, canonical and
    /// deduplicated.
    fn site_binding_reads(
        &self,
        site: verter_semantic::analysis::flow::SkeletonExprSiteId,
    ) -> Arc<[SkeletonBindingId]> {
        let span = self.skeleton.expr_site(site).span;
        let mut reads: Vec<SkeletonBindingId> = self
            .skeleton
            .expr_sites
            .iter()
            .filter(|candidate| span.contains(candidate.span))
            .flat_map(|candidate| candidate.reads.iter())
            .filter_map(|read| match read.binding {
                Some(FlowBindingRef::Local(binding)) => {
                    Some(self.bindings.canonical_local(binding))
                }
                _ => None,
            })
            .collect();
        reads.sort_unstable_by_key(|binding| binding.index());
        reads.dedup();
        Arc::from(reads.into_boxed_slice())
    }

    /// Lower one variable declaration's declarators into `out` — a
    /// statement's, or a `for` loop initializer's.
    fn lower_variable_declaration(
        &mut self,
        decl: &oxc_ast::ast::VariableDeclaration<'_>,
        out: &mut Vec<SliceStatement>,
    ) {
        let kind = match decl.kind {
            VariableDeclarationKind::Const
            | VariableDeclarationKind::Using
            | VariableDeclarationKind::AwaitUsing => SliceBindingKind::Const,
            VariableDeclarationKind::Let => SliceBindingKind::Let,
            VariableDeclarationKind::Var => SliceBindingKind::Var,
        };
        for declarator in &decl.declarations {
            // An eligible alias of a CONDITION or a
            // DISCRIMINANT re-establishes its initializer's
            // fact at every later test of the bound name,
            // whether or not the demand value-selected the
            // declaration — the name still resolves either
            // way, and a destructured discriminant aliases
            // exactly as a whole-binding one does.
            if matches!(declarator.id, BindingPattern::BindingIdentifier(_)) {
                self.record_narrowing_aliases(&decl.kind, declarator);
            }
            let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                // A modelled destructuring pattern whose demand selected
                // one of its bindings binds each element from the
                // declarator's annotation or initializer — and the
                // evaluator carries the narrowings its elements alias.
                if let Some(statement) = self.lower_destructuring_declarator(declarator, kind) {
                    out.push(statement);
                    continue;
                }
                self.record_narrowing_aliases(&decl.kind, declarator);
                // Any other destructuring declarator binds nothing this
                // half models — but the initializer still RUNS at the
                // statement, so its effects take the same fail-closed
                // scan every unmodeled position gets; an entered `asserts`
                // call in it narrows once the initializer has run.
                if let Some(init) = declarator.init.as_ref() {
                    let entered = self.collecting_entered_assertions(|this| {
                        this.scan_unmodeled_position_effects(init)
                    });
                    out.extend(entered);
                }
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
            //
            // The elided initializer still RUNS at the
            // statement, though: an assertion call or a
            // whole-binding write inside it narrows / retypes
            // what follows in the checker while the slice's
            // obligations never reach the position — scan it
            // with the same fail-closed discipline (a
            // pure-literal initializer carries no effect and
            // stays silent).
            //
            // A whole-binding write the initializer holds
            // applies in evaluation order instead
            // ([`Self::lower_discarded_effects`]).
            if !self.slot_selected(id.span) {
                if let Some(init) = declarator.init.as_ref() {
                    self.lower_discarded_effects(init, DiscardedContext::Initializer, out);
                }
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
            // A class expression that directly initializes a
            // variable is named after it (`const C = class {}`
            // is the checker's `C`).
            let init = declarator.init.as_ref().map(|expr| {
                self.lower_assigned_value(
                    expr,
                    id.name.as_str(),
                    ExprMode::BindingInit { preserve_literal },
                )
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
            // The initializer's FRESHNESS shape for an
            // unannotated `const` — the evaluator's widening
            // membership input. An all-fresh tree (a bare
            // literal, or a conditional whose EVERY leaf is
            // one — `f ? 1 : "s"`) is the classic
            // widening-literal binding: the checker widens
            // every arm at a widening read (`{ label: v }`
            // reads `string | number`). A MIXED tree (an
            // `as const` arm, a call, a reference beside a
            // fresh leaf) carries per-arm verdicts so the
            // evaluator widens exactly the fresh arms and
            // keeps the authored pins. `let` / `var`
            // initializers already widened at `BindingInit`
            // lowering and stay `Pinned` here; an annotated
            // declarator's shape is the declared union
            // reduction's fresh-boolean input.
            // An unannotated `let` / `var` whose initializer is
            // a bare `null` / `undefined` / `void` value carries
            // that shape too: it is the checker's auto-typed
            // variable, reading the widening nullable type
            // until a later write retypes it.
            let freshness = match (declared.as_ref(), declarator.init.as_ref()) {
                (Some(_), Some(init)) => expression_freshness(init),
                (None, Some(init)) if kind == SliceBindingKind::Const => expression_freshness(init),
                (None, Some(init)) if expr_is_widening_nullish(init) => expression_freshness(init),
                _ => SliceFreshness::Pinned,
            };
            let auto_typed_form = declared.is_none()
                && kind != SliceBindingKind::Const
                && declarator
                    .init
                    .as_ref()
                    .is_none_or(|init| self.is_null_or_undefined_keyword(init));
            let Some(binding) = self.bindings.declaration_at_span(self.rebase(id.span)) else {
                out.push(SliceStatement::Gap(
                    crate::semantic_query::FlowGap::UnmodeledExpression,
                ));
                continue;
            };
            let evolving_array = self.skeleton.binding(binding).evolving_array;
            out.push(SliceStatement::Binding {
                binding,
                name: Arc::from(id.name.as_str()),
                kind,
                init,
                declared,
                freshness,
                auto_typed_form,
                evolving_array,
            });
        }
    }

    /// Lower one destructuring declarator as a [`SliceStatement::Destructure`]:
    /// `None` when its pattern is not modelled or the demand selected none
    /// of its bindings.
    fn lower_destructuring_declarator(
        &mut self,
        declarator: &oxc_ast::ast::VariableDeclarator<'_>,
        kind: SliceBindingKind,
    ) -> Option<SliceStatement> {
        let pattern = self.lower_pattern(&declarator.id)?;
        let bindings = pattern.bindings();
        if !bindings.iter().any(|(binding, _)| {
            self.frame_gate.modelled_patterns.contains(binding)
                && self.binding_is_selected(*binding)
        }) {
            return None;
        }
        let declared = declarator.type_annotation.as_ref().map(|annotation| {
            self.gate(
                lower_ts_type(&annotation.type_annotation, self.source),
                declarator.id.span(),
                &[],
            )
        });
        let init = declarator.init.as_ref().map(|init| {
            self.lower_expr(
                init,
                ExprMode::BindingInit {
                    preserve_literal: true,
                },
            )
        });
        let correlated = kind == SliceBindingKind::Const;
        let source = if correlated && declarator.type_annotation.is_none() {
            declarator.init.as_ref().and_then(|init| {
                matches!(
                    unwrap_parenthesized(init),
                    Expression::Identifier(_)
                        | Expression::StaticMemberExpression(_)
                        | Expression::ComputedMemberExpression(_)
                )
                .then(|| self.narrow_subject_of(init))
                .flatten()
            })
        } else {
            None
        };
        self.record_aliased_bindings(&pattern, correlated, source.as_ref());
        Some(SliceStatement::Destructure {
            pattern,
            kind,
            init,
            declared,
            annotated: false,
            correlated,
            source,
        })
    }

    /// Lower one destructuring pattern ([`SlicePattern`]); `None` for a
    /// form it does not model (a computed key that is not a literal, a
    /// rest element that is itself a pattern).
    fn lower_pattern(&mut self, pattern: &BindingPattern<'_>) -> Option<SlicePattern> {
        match pattern {
            BindingPattern::BindingIdentifier(id) => {
                let span = self.rebase(id.span);
                Some(SlicePattern::Binding {
                    binding: self.bindings.declaration_at_span(span)?,
                    span,
                })
            }
            BindingPattern::ObjectPattern(object) => {
                let mut properties = Vec::with_capacity(object.properties.len());
                for property in &object.properties {
                    let key = match pattern_property_key(&property.key, property.computed) {
                        Some(name) => SlicePatternKey::Named(name),
                        None => SlicePatternKey::Computed(Box::new(self.lower_expr(
                            property.key.as_expression()?,
                            ExprMode::BindingInit {
                                preserve_literal: true,
                            },
                        ))),
                    };
                    properties.push((key, self.lower_pattern_element(&property.value)?));
                }
                let rest = match object.rest.as_ref() {
                    Some(rest) => Some(self.lower_rest_binding(&rest.argument)?),
                    None => None,
                };
                Some(SlicePattern::Object {
                    properties: Arc::from(properties.into_boxed_slice()),
                    rest,
                })
            }
            BindingPattern::ArrayPattern(array) => {
                let mut elements = Vec::with_capacity(array.elements.len());
                for element in &array.elements {
                    elements.push(match element {
                        Some(element) => Some(self.lower_pattern_element(element)?),
                        None => None,
                    });
                }
                let rest = match array.rest.as_ref() {
                    Some(rest) => Some(self.lower_rest_binding(&rest.argument)?),
                    None => None,
                };
                Some(SlicePattern::Array {
                    elements: Arc::from(elements.into_boxed_slice()),
                    rest,
                })
            }
            BindingPattern::AssignmentPattern(_) => None,
        }
    }

    fn lower_pattern_element(
        &mut self,
        element: &BindingPattern<'_>,
    ) -> Option<SlicePatternElement> {
        match element {
            BindingPattern::AssignmentPattern(assignment) => Some(SlicePatternElement {
                pattern: self.lower_pattern(&assignment.left)?,
                default: Some(Box::new(self.lower_expr(
                    &assignment.right,
                    ExprMode::BindingInit {
                        preserve_literal: true,
                    },
                ))),
                default_fresh: expr_is_bare_literal(&assignment.right),
            }),
            other => Some(SlicePatternElement {
                pattern: self.lower_pattern(other)?,
                default: None,
                default_fresh: false,
            }),
        }
    }

    /// Lower a destructuring ASSIGNMENT target as a [`SlicePattern`] whose
    /// leaves are write targets; `None` for a form it does not model (a
    /// member or rest target, a key that is not a literal).
    fn lower_assignment_pattern(
        &mut self,
        target: &oxc_ast::ast::AssignmentTarget<'_>,
    ) -> Option<SlicePattern> {
        use oxc_ast::ast::{AssignmentTarget, AssignmentTargetProperty};
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                let root = self.write_target_root(identifier)?;
                Some(SlicePattern::Target {
                    target: SliceNarrowSubject {
                        root,
                        path: Arc::from(Vec::new().into_boxed_slice()),
                    },
                    span: self.rebase(identifier.span),
                })
            }
            AssignmentTarget::ObjectAssignmentTarget(object) => {
                if object.rest.is_some() {
                    return None;
                }
                let mut properties = Vec::with_capacity(object.properties.len());
                for property in &object.properties {
                    match property {
                        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(shorthand) => {
                            let root = self.write_target_root(&shorthand.binding)?;
                            properties.push((
                                SlicePatternKey::Named(Arc::from(shorthand.binding.name.as_str())),
                                SlicePatternElement {
                                    pattern: SlicePattern::Target {
                                        target: SliceNarrowSubject {
                                            root,
                                            path: Arc::from(Vec::new().into_boxed_slice()),
                                        },
                                        span: self.rebase(shorthand.span),
                                    },
                                    default: shorthand.init.as_ref().map(|init| {
                                        Box::new(self.lower_expr(
                                            init,
                                            ExprMode::BindingInit {
                                                preserve_literal: true,
                                            },
                                        ))
                                    }),
                                    default_fresh: shorthand
                                        .init
                                        .as_ref()
                                        .is_some_and(expr_is_bare_literal),
                                },
                            ));
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(property) => {
                            let key = pattern_property_key(&property.name, property.computed)?;
                            let element = self.lower_assignment_element(&property.binding)?;
                            properties.push((SlicePatternKey::Named(key), element));
                        }
                    }
                }
                Some(SlicePattern::Object {
                    properties: Arc::from(properties.into_boxed_slice()),
                    rest: None,
                })
            }
            AssignmentTarget::ArrayAssignmentTarget(array) => {
                if array.rest.is_some() {
                    return None;
                }
                let mut elements = Vec::with_capacity(array.elements.len());
                for element in &array.elements {
                    elements.push(match element {
                        Some(element) => Some(self.lower_assignment_element(element)?),
                        None => None,
                    });
                }
                Some(SlicePattern::Array {
                    elements: Arc::from(elements.into_boxed_slice()),
                    rest: None,
                })
            }
            _ => None,
        }
    }

    fn lower_assignment_element(
        &mut self,
        element: &oxc_ast::ast::AssignmentTargetMaybeDefault<'_>,
    ) -> Option<SlicePatternElement> {
        match element {
            oxc_ast::ast::AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(
                with_default,
            ) => Some(SlicePatternElement {
                pattern: self.lower_assignment_pattern(&with_default.binding)?,
                default: Some(Box::new(self.lower_expr(
                    &with_default.init,
                    ExprMode::BindingInit {
                        preserve_literal: true,
                    },
                ))),
                default_fresh: expr_is_bare_literal(&with_default.init),
            }),
            other => Some(SlicePatternElement {
                pattern: self.lower_assignment_pattern(other.to_assignment_target())?,
                default: None,
                default_fresh: false,
            }),
        }
    }

    fn lower_rest_binding(
        &mut self,
        rest: &BindingPattern<'_>,
    ) -> Option<(SkeletonBindingId, FrameSpan)> {
        let BindingPattern::BindingIdentifier(id) = rest else {
            return None;
        };
        let span = self.rebase(id.span);
        Some((self.bindings.declaration_at_span(span)?, span))
    }

    /// Lower one `if` arm: a block arm lowers its statement list directly;
    /// any other statement is a one-statement region.
    fn lower_arm(&mut self, statement: &Statement<'_>) -> LoweredRegion {
        match statement {
            Statement::BlockStatement(block) => self.lower_region(&block.body),
            _ => self.lower_region(std::slice::from_ref(statement)),
        }
    }

    // ── Guards ──────────────────────────────────────────────────────

    /// THE guard lowering — the single authority over conditional-test
    /// forms, shared by the ternary's branch join and the `if`
    /// statement's arms.
    ///
    /// The output is a structural description of the narrowing facts the
    /// test establishes; it evaluates nothing (this half has no
    /// resolver). Negation is pushed to the leaves at lowering time (De
    /// Morgan), so the evaluator only ever asks a guard for its positive
    /// or its negated reading.
    ///
    /// [`SliceGuard::None`] means PROVED NON-NARROWING, and nothing else:
    /// it is what the evaluator applies when the test establishes no fact
    /// at all. A form this vocabulary cannot express is therefore never
    /// spelled as `None` alone — [`Self::classify_guard`] answers with the
    /// third disposition, and this conversion flags
    /// [`Lowerer::control_test_gap`] so the caller emits the typed
    /// `GuardNarrowing` gap ahead of the construct: a degraded success,
    /// `ReturnOnly`, never a silently published superset.
    fn lower_guard(&mut self, test: &Expression<'_>) -> SliceGuard {
        match self.classify_guard(test) {
            GuardDisposition::Modeled(guard) => *guard,
            GuardDisposition::NoNarrowing => SliceGuard::None,
            GuardDisposition::Unexpressible => {
                self.control_test_gap = true;
                SliceGuard::None
            }
        }
    }

    /// The single returned expression of a function that may infer a type
    /// predicate, read as a test through the ONE guard authority
    /// ([`ReturnPredicateTest`]); `None` for every other function. An
    /// unexpressible test raises no gap here: only a `boolean` return
    /// makes the unknown predicate matter, and only the evaluator sees
    /// the returned type.
    fn return_predicate_test(&mut self, argument: &Expression<'_>) -> Option<ReturnPredicateTest> {
        let parameters = self.predicate_parameters.clone()?;
        let guard = match self.classify_guard(argument) {
            GuardDisposition::Modeled(guard) => *guard,
            GuardDisposition::NoNarrowing => SliceGuard::None,
            // A test this vocabulary cannot express still infers no
            // predicate when it provably narrows no parameter.
            GuardDisposition::Unexpressible if !self.test_may_narrow_parameter(argument) => {
                SliceGuard::None
            }
            GuardDisposition::Unexpressible => return Some(ReturnPredicateTest::Unexpressible),
        };
        if self.holds_unprovable_narrowing_call(argument) {
            return Some(ReturnPredicateTest::Unexpressible);
        }
        Some(ReturnPredicateTest::Guard {
            guard: Box::new(guard),
            parameters,
        })
    }

    /// Whether a test could narrow a PARAMETER itself — the only
    /// references an inferred predicate talks about. The checker narrows a
    /// reference the test names and, through a discriminant member access,
    /// that access's object: so a parameter is narrowed only by a test of
    /// the parameter, of one member access directly on it (a static name, a
    /// literal key, or a `const` binding's literal key), of an alias local,
    /// or of a call. A member chain of two or more steps, or an element
    /// access by a key that names no member (a parameter or `let` key),
    /// narrows no parameter. Conservative: any form not listed may.
    fn test_may_narrow_parameter(&self, test: &Expression<'_>) -> bool {
        match unwrap_reference_transparent(test) {
            Expression::UnaryExpression(unary) => self.test_may_narrow_parameter(&unary.argument),
            Expression::LogicalExpression(logical) => {
                self.test_may_narrow_parameter(&logical.left)
                    || self.test_may_narrow_parameter(&logical.right)
            }
            Expression::BinaryExpression(binary) => {
                self.test_may_narrow_parameter(&binary.left)
                    || self.test_may_narrow_parameter(&binary.right)
            }
            Expression::BooleanLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::BigIntLiteral(_) => false,
            Expression::Identifier(identifier) => {
                !matches!(
                    self.classify_occurrence(identifier.span),
                    NameBinding::Free | NameBinding::NestedFunction
                ) || self
                    .narrowing_alias_locals
                    .contains(identifier.name.as_str())
            }
            Expression::ComputedMemberExpression(member) => {
                let key_names_member = literal_member_key(&member.expression).is_some()
                    || match unwrap_parenthesized(&member.expression) {
                        Expression::Identifier(key) => !matches!(
                            self.binding_at(key.span),
                            Some(FlowBindingRef::Local(binding))
                                if self.skeleton.binding(self.bindings.canonical_local(binding)).kind
                                    != SkeletonBindingKind::Const
                        ),
                        _ => true,
                    };
                (key_names_member && self.object_is_parameter(&member.object))
                    || self.key_may_narrow_parameter(&member.expression)
            }
            Expression::StaticMemberExpression(member) => self.object_is_parameter(&member.object),
            _ => true,
        }
    }

    /// Whether an element-access key is itself a test that could narrow a
    /// parameter — a key is evaluated, never tested, so only a key holding
    /// a call can.
    fn key_may_narrow_parameter(&self, key: &Expression<'_>) -> bool {
        verter_semantic::analysis::flow::expression_contains_call(key)
    }

    /// Whether an expression names a parameter binding itself (through the
    /// reference-transparent wrappers) — or an alias local, which may
    /// stand for one.
    fn object_is_parameter(&self, object: &Expression<'_>) -> bool {
        match unwrap_reference_transparent(object) {
            Expression::Identifier(identifier) => {
                matches!(
                    self.classify_occurrence(identifier.span),
                    NameBinding::Param(_) | NameBinding::Local(Some(_))
                ) || self
                    .narrowing_alias_locals
                    .contains(identifier.name.as_str())
                    || matches!(
                        self.binding_at(identifier.span),
                        Some(FlowBindingRef::Local(binding))
                            if self.skeleton.binding(self.bindings.canonical_local(binding)).kind
                                == SkeletonBindingKind::Param
                    )
            }
            Expression::StaticMemberExpression(_)
            | Expression::ComputedMemberExpression(_)
            | Expression::PrivateFieldExpression(_) => false,
            _ => true,
        }
    }

    /// Whether a returned expression holds a call whose result could
    /// narrow a parameter beyond what the guard lowering minted. A call
    /// narrows only a reference it is handed — an argument or the
    /// receiver — so only a call reaching a parameter that way counts. The
    /// guard vocabulary reads a call as a fact only for a provably closed
    /// same-file predicate callee, so every other such call
    /// (`Array.isArray(x)`, an imported guard) is proved inert only when
    /// its callee is a closed same-file declaration with a non-predicate
    /// return annotation — the control-test rule, read here without
    /// recording anything.
    /// Whether an entered call could carry an `asserts` narrowing of a
    /// frame-owned binding: a parameter or local among its assertion
    /// subjects, and a callee that is not a provably closed same-file
    /// declaration whose return annotation rules an assertion out.
    fn call_may_assert_a_frame_binding(&self, call: &oxc_ast::ast::CallExpression<'_>) -> bool {
        let ControlCall::Call {
            callee,
            assertion_subject_roots,
            ..
        } = ControlCall::of_call(call)
        else {
            return false;
        };
        assertion_subject_roots.iter().any(|(_, root)| {
            matches!(
                self.classify_occurrence(*root),
                NameBinding::Param(_) | NameBinding::Local(_)
            )
        }) && !callee.as_ref().is_some_and(|(name, callee_span)| {
            matches!(self.classify_occurrence(*callee_span), NameBinding::Free)
                && self
                    .closed_callee_declaration(name)
                    .is_some_and(|function| {
                        ResultIndependentPosition::DiscardedOperand
                            .certifies_closed_return(function.return_type.as_deref())
                    })
        })
    }

    fn holds_unprovable_narrowing_call(&self, argument: &Expression<'_>) -> bool {
        let mut spine = Vec::new();
        narrowing_spine_calls(argument, &mut spine);
        spine
            .into_iter()
            .any(|call| match &ControlCall::of_call(call) {
                ControlCall::Construct(_) | ControlCall::TaggedTemplate(_) => false,
                ControlCall::Call {
                    span,
                    callee,
                    assertion_subject_roots,
                } => {
                    assertion_subject_roots.iter().any(|(_, root)| {
                        matches!(self.classify_occurrence(*root), NameBinding::Param(_))
                    }) && !self.predicate_guard_call_spans.contains(span)
                        && !self.non_narrowing_call_spans.contains(span)
                        && !callee.as_ref().is_some_and(|(name, callee_span)| {
                            matches!(self.classify_occurrence(*callee_span), NameBinding::Free)
                                && self
                                    .closed_callee_declaration(name)
                                    .is_some_and(|function| {
                                        ResultIndependentPosition::ControlTest
                                            .certifies_closed_return(
                                                function.return_type.as_deref(),
                                            )
                                    })
                        })
                }
            })
    }

    /// The tri-state classification behind [`Self::lower_guard`] — the
    /// ONE authority over what a control test establishes. Every
    /// composing form (`!`, `&&`, `||`, `??`, a conditional, a sequence,
    /// an assignment, parentheses) recurses through THIS function and
    /// composes the disposition; none of them collapses an unexpressible
    /// operand into "no narrowing".
    ///
    /// The vocabulary is deliberately smaller than the checker's, so the
    /// classification is positional: a form that establishes a narrow at
    /// a slot this half MODELS — a parameter or a modelable same-frame
    /// local, with or without an access path under it — is
    /// [`GuardDisposition::Unexpressible`] when the vocabulary cannot
    /// carry it. A form that reaches no such slot narrows nothing this
    /// half could have applied and is
    /// [`GuardDisposition::NoNarrowing`].
    ///
    /// This function has no gap side effect: it is also the probe the
    /// loop-transparency rule consults, which must classify a test
    /// WITHOUT degrading the enclosing statement.
    fn classify_guard(&mut self, test: &Expression<'_>) -> GuardDisposition {
        // The whole test rides the same reference-transparent wrappers a
        // leaf reference does — parentheses and the postfix non-null
        // assertion ONLY: `(typeof x === "string")!` still establishes
        // the inner fact, so the entry must peel them before dispatching
        // or the composing forms behind one collapse to a proved absence
        // of narrowing. `satisfies` and the `as` / angle-bracket type
        // assertion are deliberately NOT peeled: neither is a matching
        // reference for narrowing (measured, `typeof (x satisfies string
        // | number) === "string"` narrows nothing), and peeling one would
        // narrow where the checker does not — a SUBSET of the checker's
        // type, which drops a real contributor. See
        // [`unwrap_reference_transparent`].
        match unwrap_reference_transparent(test) {
            Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::LogicalNot => {
                self.classify_guard(&unary.argument).negated()
            }
            Expression::LogicalExpression(logical) => {
                let left = self.classify_guard(&logical.left);
                let right = self.classify_guard(&logical.right);
                match logical.operator {
                    // A conjunct / disjunct this half PROVED inert stays
                    // in the tree as an explicit `None` alternative (the
                    // false edge of `a && b` is a disjunction of
                    // negations, so an inert operand blocks the other's
                    // negation there); an UNEXPRESSIBLE operand degrades
                    // the whole test.
                    LogicalOperator::And | LogicalOperator::Or => {
                        if left.is_unexpressible() || right.is_unexpressible() {
                            return GuardDisposition::Unexpressible;
                        }
                        if left.is_no_narrowing() && right.is_no_narrowing() {
                            return GuardDisposition::NoNarrowing;
                        }
                        let composed = if logical.operator == LogicalOperator::And {
                            and_guard(left.into_guard(), right.into_guard())
                        } else {
                            or_guard(left.into_guard(), right.into_guard())
                        };
                        GuardDisposition::modeled(composed)
                    }
                    // `a ?? b` tests nullishness of `a`, but its result
                    // is the OPERAND's value, not a boolean fact over the
                    // arms — a narrow taken from either operand would
                    // apply to the wrong reference, and this half has no
                    // branch/merge composition to prove which survives.
                    // Silence is owed only when NEITHER operand carries a
                    // fact at all.
                    LogicalOperator::Coalesce => {
                        if left.is_no_narrowing() && right.is_no_narrowing() {
                            GuardDisposition::NoNarrowing
                        } else {
                            GuardDisposition::Unexpressible
                        }
                    }
                }
            }
            Expression::BinaryExpression(binary) => self.classify_binary_guard(binary),
            // A call's narrowing lives in its CALLEE's declared return,
            // and the control-position call rail owns it entirely
            // ([`Self::record_control_position_calls`] certifies a
            // provably non-narrowing callee and degrades every other).
            // Answering `Unexpressible` here would degrade the very tests
            // that rail proves silent and destroy the certification.
            // A call the executor cannot resolve here (a callee rooted at a
            // free name outside a module scope, say) still narrows by its
            // callee's declared signatures when the file closes their set.
            Expression::CallExpression(call) => match self.lower_predicate_guard(call) {
                SliceGuard::None => match self.classify_call_predicate(call) {
                    GuardDisposition::Unexpressible => match unwrap_parenthesized(&call.callee) {
                        Expression::Identifier(callee)
                            if matches!(
                                self.classify_occurrence(callee.span),
                                NameBinding::Free
                            ) && self
                                .closed_callee_declaration(callee.name.as_str())
                                .is_none() =>
                        {
                            match self.lower_callee_signature_guard(call, callee) {
                                SliceGuard::None => GuardDisposition::Unexpressible,
                                guard => GuardDisposition::modeled(guard),
                            }
                        }
                        _ => GuardDisposition::Unexpressible,
                    },
                    disposition => disposition,
                },
                guard => GuardDisposition::modeled(guard),
            },
            // The test's VALUE is one of the branches; this half carries
            // no branch/merge composition for a guard, so a branch that
            // establishes anything degrades the test.
            Expression::ConditionalExpression(conditional) => {
                let consequent = self.classify_guard(&conditional.consequent);
                let alternate = self.classify_guard(&conditional.alternate);
                if consequent.is_no_narrowing() && alternate.is_no_narrowing() {
                    GuardDisposition::NoNarrowing
                } else {
                    GuardDisposition::Unexpressible
                }
            }
            // A sequence's VALUE is its last operand, and the checker
            // narrows through it (`narrowType` reads a comma's right
            // operand); the earlier operands only run first, and their
            // calls take the discarded-operand rail.
            Expression::SequenceExpression(sequence) => match sequence.expressions.last() {
                Some(last) => self.classify_guard(last),
                None => GuardDisposition::NoNarrowing,
            },
            // An assignment used as a test narrows the binding it WROTE
            // (the checker takes the target as the reference and the
            // right-hand side's truthiness as the fact): a plain `=` to a
            // narrowable binding whose right-hand side narrows nothing is
            // the truthiness of the target, read after the write. Where
            // the write is applied before the test's edges (a logical
            // operand's), that is the checker's narrow; where it is not,
            // the unapplied write already degrades the frame.
            Expression::AssignmentExpression(assignment)
                if assignment.operator == oxc_ast::ast::AssignmentOperator::Assign
                    && matches!(
                        assignment.left,
                        oxc_ast::ast::AssignmentTarget::AssignmentTargetIdentifier(_)
                    ) =>
            {
                let oxc_ast::ast::AssignmentTarget::AssignmentTargetIdentifier(target) =
                    &assignment.left
                else {
                    unreachable!("the guard admits an identifier target");
                };
                let name = target.name.as_str();
                let subject = match self.classify_occurrence(target.span) {
                    NameBinding::Param(ordinal) => {
                        self.narrow_root(name, target.span, Some(ordinal))
                    }
                    NameBinding::Local(_) => self.narrow_root(name, target.span, None),
                    _ => None,
                }
                .map(|root| SliceNarrowSubject {
                    root,
                    path: Arc::from(Vec::new().into_boxed_slice()),
                });
                match subject {
                    Some(subject)
                        if !self.subject_root_carries_an_unmentioned_narrowing(&subject)
                            && self.classify_guard(&assignment.right).is_no_narrowing() =>
                    {
                        GuardDisposition::modeled(SliceGuard::Truthy {
                            subject,
                            negated: false,
                        })
                    }
                    _ if self.identifier_roots_a_narrow_destination(name, target.span)
                        || !self.classify_guard(&assignment.right).is_no_narrowing() =>
                    {
                        GuardDisposition::Unexpressible
                    }
                    _ => GuardDisposition::NoNarrowing,
                }
            }
            Expression::AssignmentExpression(assignment) => {
                let target_reaches_slot = match &assignment.left {
                    oxc_ast::ast::AssignmentTarget::AssignmentTargetIdentifier(target) => self
                        .identifier_roots_a_narrow_destination(target.name.as_str(), target.span),
                    other => other
                        .as_member_expression()
                        .is_some_and(|member| self.member_root_is_represented(member)),
                };
                if target_reaches_slot || !self.classify_guard(&assignment.right).is_no_narrowing()
                {
                    GuardDisposition::Unexpressible
                } else {
                    GuardDisposition::NoNarrowing
                }
            }
            // `#field in obj` is the private-name brand check: the
            // checker selects the subject's union arms by whether the
            // class installed the field, exactly as the string-key form
            // does. This vocabulary carries only a string key, so a
            // subject reaching a modeled slot degrades.
            Expression::PrivateInExpression(private_in) => {
                match self.narrow_destination_of(&private_in.right) {
                    NarrowDestination::Absent => GuardDisposition::NoNarrowing,
                    _ => GuardDisposition::Unexpressible,
                }
            }
            other => self.classify_truthiness_guard(other),
        }
    }

    /// The bare-truthiness classification: the test's own value decides
    /// the arms, so the fact lands on the reference the expression NAMES.
    ///
    /// The reference is read through the checker's transparent wrappers
    /// only — parentheses and the postfix non-null assertion.
    /// `satisfies` and the `as` / angle-bracket type assertion are NOT
    /// among them: neither is a matching reference for narrowing
    /// (measured — `if ((x satisfies string | undefined))` leaves
    /// `undefined` in the result, exactly like its `as` twin), so a test
    /// behind one establishes nothing and is proved inert rather than
    /// degraded. Peeling either would narrow where the checker does not —
    /// a SUBSET of the checker's type, worse than the superset a missing
    /// narrow produces. See [`unwrap_reference_transparent`].
    fn classify_truthiness_guard(&mut self, expression: &Expression<'_>) -> GuardDisposition {
        let reference = unwrap_reference_transparent(expression);
        if let Some(disposition) = self.classify_aliased_condition(reference) {
            return disposition;
        }
        match self.narrow_subject_of(reference) {
            Some(subject) => {
                if self.subject_root_carries_an_unmentioned_narrowing(&subject) {
                    GuardDisposition::Unexpressible
                } else {
                    GuardDisposition::modeled(SliceGuard::Truthy {
                        subject,
                        negated: false,
                    })
                }
            }
            None => match self.narrow_destination_of(reference) {
                NarrowDestination::Absent => GuardDisposition::NoNarrowing,
                // A reference rooted at a slot this half models whose
                // ACCESS it cannot express — a computed member, an
                // optional step, a private field — still has a narrowing
                // destination: the checker binds the fact there.
                // (`Represented` cannot reach this arm — the subject
                // lowering above already answered for it.)
                NarrowDestination::Represented | NarrowDestination::Unrepresented => {
                    GuardDisposition::Unexpressible
                }
            },
        }
    }

    /// A call whose callee is not a closed same-file predicate, read as a
    /// [`SliceGuard::CallPredicate`]: the evaluator resolves its signature
    /// and applies the predicate, if any, to the argument (or receiver) it
    /// names. The checker narrows only a reference the call hands over as
    /// it is (`isMatchingReference`: through parentheses and `!`, never
    /// through `as` or `satisfies`), so a call handing no such reference
    /// provably narrows nothing and the control-position rail certifies
    /// it. A reference this half cannot carry (a computed or optional
    /// access, an assignment or a sequence), a spread argument or a callee
    /// form this half cannot lower as a value leaves the predicate's
    /// argument unknown, so the test is unexpressible. A modeled call is
    /// evidence-backed at guard application.
    fn classify_call_predicate(
        &mut self,
        call: &oxc_ast::ast::CallExpression<'_>,
    ) -> GuardDisposition {
        // A closed same-file callee whose return is not a predicate is
        // the control-position rail's own certificate.
        if let Expression::Identifier(callee) = unwrap_parenthesized(&call.callee) {
            if matches!(self.classify_occurrence(callee.span), NameBinding::Free)
                && self
                    .closed_callee_declaration(callee.name.as_str())
                    .is_some_and(|function| {
                        ResultIndependentPosition::ControlTest
                            .certifies_closed_return(function.return_type.as_deref())
                    })
            {
                return GuardDisposition::NoNarrowing;
            }
        }
        let mut unrepresented = false;
        let receiver = match unwrap_parenthesized(&call.callee) {
            Expression::StaticMemberExpression(member) => {
                let receiver = self.narrow_subject_of(&member.object);
                unrepresented |=
                    receiver.is_none() && self.argument_may_match_a_reference(&member.object);
                receiver
            }
            Expression::PrivateFieldExpression(member) => {
                unrepresented |= self.argument_may_match_a_reference(&member.object);
                None
            }
            _ => None,
        };
        let mut spread = false;
        let arguments: Arc<[Option<SliceNarrowSubject>]> = call
            .arguments
            .iter()
            .map(|argument| match argument.as_expression() {
                Some(expression) => {
                    let subject = self.narrow_subject_of(expression);
                    unrepresented |=
                        subject.is_none() && self.argument_may_match_a_reference(expression);
                    subject
                }
                None => {
                    spread = true;
                    None
                }
            })
            .collect();
        if unrepresented {
            return GuardDisposition::Unexpressible;
        }
        if receiver.is_none() && arguments.iter().all(Option::is_none) {
            self.non_narrowing_call_spans.insert(call.span.into());
            return GuardDisposition::NoNarrowing;
        }
        let callee_is_reference = matches!(
            unwrap_parenthesized(&call.callee),
            Expression::Identifier(_) | Expression::StaticMemberExpression(_)
        );
        if spread || call.optional || !callee_is_reference {
            return GuardDisposition::Unexpressible;
        }
        // A callee rooted at a name this frame does not bind is read in
        // the owner scope, where the resolver serves a module's binding with
        // its checker-visible signature set — an exported function's value
        // merges the overloads every augmenting `declare module` block adds.
        // A script's globals merge across files and a namespace block binds
        // the name before the top level, which the owner scope does not see.
        if let Some(root) = chain_root_identifier(&call.callee) {
            if matches!(self.classify_occurrence(root.span), NameBinding::Free)
                && (!self.module_scope || self.namespace_owned)
            {
                return GuardDisposition::Unexpressible;
            }
        }
        let callee = self.lower_expr(&call.callee, ExprMode::Return);
        self.predicate_guard_call_spans.insert(call.span.into());
        GuardDisposition::modeled(SliceGuard::CallPredicate {
            callee: Box::new(callee),
            site: call_site(call),
            arguments,
            receiver,
            negated: false,
        })
    }

    /// Whether a call argument (or receiver) the subject lowering cannot
    /// carry may still be a reference the checker matches — `this`, a
    /// computed, private or optional access rooted at a modeled slot, or
    /// an assignment or sequence whose value is one.
    fn argument_may_match_a_reference(&self, expression: &Expression<'_>) -> bool {
        match unwrap_reference_transparent(expression) {
            Expression::AssignmentExpression(_) | Expression::ThisExpression(_) => true,
            Expression::SequenceExpression(sequence) => {
                sequence.expressions.last().is_some_and(|last| {
                    self.narrow_subject_of(last).is_some()
                        || self.argument_may_match_a_reference(last)
                })
            }
            other => !matches!(self.narrow_destination_of(other), NarrowDestination::Absent),
        }
    }

    /// The binding a bare identifier names, when it is a frame local.
    fn alias_binding_of(&self, expression: &Expression<'_>) -> Option<FlowBindingRef> {
        let Expression::Identifier(identifier) = unwrap_reference_transparent(expression) else {
            return None;
        };
        self.binding_at(identifier.span)
    }

    /// A test of an ALIASED CONDITION (`const isStr = typeof x ===
    /// "string"; if (isStr) …`): the alias narrows itself, and the checker
    /// inlines its initializer (`narrowType`, up to
    /// [`ALIAS_INLINE_LIMIT`] levels deep) to narrow the constant
    /// references that initializer names. `None` when `reference` is no
    /// aliased condition.
    fn classify_aliased_condition(
        &mut self,
        reference: &Expression<'_>,
    ) -> Option<GuardDisposition> {
        let binding = self.alias_binding_of(reference)?;
        let readings = self.alias_conditions.get(&binding)?;
        let inlined = match self.alias_inline_budget.checked_sub(1) {
            Some(index) => readings[index].clone(),
            None => Some(SliceGuard::None),
        };
        let Some(inlined) = inlined else {
            return Some(GuardDisposition::Unexpressible);
        };
        let own = self
            .narrow_subject_of(reference)
            .map(|subject| SliceGuard::Truthy {
                subject,
                negated: false,
            })
            .unwrap_or(SliceGuard::None);
        Some(GuardDisposition::modeled(SliceGuard::Both(Arc::from(
            vec![own, inlined].into_boxed_slice(),
        ))))
    }

    /// The binary-operator guard forms: strict (in)equality — including
    /// the `typeof x === "kind"` spelling — `instanceof`, and `in`.
    ///
    /// Each family models an EXACT pair and degrades everything else that
    /// still reaches a modeled slot: an equality whose reference operand
    /// is a member path or an access this half cannot express, against a
    /// value that is neither a reference nor a literal, against a
    /// `typeof` this half cannot resolve, or wrapping a nested guard in a
    /// boolean comparison; an
    /// `in` with a non-literal key or an inexpressible subject access; an
    /// `instanceof` over an inexpressible subject or an unprovable
    /// constructor. Relational and arithmetic operators establish no
    /// narrowing in the checker and are proved inert.
    fn classify_binary_guard(
        &mut self,
        binary: &oxc_ast::ast::BinaryExpression<'_>,
    ) -> GuardDisposition {
        use oxc_ast::ast::BinaryOperator;
        match binary.operator {
            BinaryOperator::StrictEquality | BinaryOperator::StrictInequality => {
                let negated = matches!(binary.operator, BinaryOperator::StrictInequality);
                // `typeof x === "string"` (either operand order).
                if let Some(guard) = self.typeof_guard(&binary.left, &binary.right, negated) {
                    return GuardDisposition::modeled(guard);
                }
                if let Some(guard) = self.typeof_guard(&binary.right, &binary.left, negated) {
                    return GuardDisposition::modeled(guard);
                }
                // `subject === literal` (either operand order).
                for (subject_side, literal_side) in
                    [(&binary.left, &binary.right), (&binary.right, &binary.left)]
                {
                    // An ALIASED DISCRIMINANT (`const k = u.kind`, `const {
                    // kind } = u`) compares the member it names: the
                    // checker narrows the member's parent through it
                    // (`getCandidateDiscriminantPropertyAccess`), beside
                    // the alias itself.
                    if let Some(member) = self
                        .alias_binding_of(subject_side)
                        .and_then(|binding| self.discriminant_aliases.get(&binding).cloned())
                    {
                        if let Some(literal) = guard_literal_of(literal_side, self.source) {
                            let own = self
                                .narrow_subject_of(subject_side)
                                .map(|subject| SliceGuard::EqLiteral {
                                    subject,
                                    literal: literal.clone(),
                                    negated,
                                    loose: false,
                                })
                                .unwrap_or(SliceGuard::None);
                            let aliased = SliceGuard::EqLiteral {
                                subject: member,
                                literal,
                                negated,
                                loose: false,
                            };
                            return GuardDisposition::modeled(SliceGuard::Both(Arc::from(
                                vec![own, aliased].into_boxed_slice(),
                            )));
                        }
                    }
                    let Some(subject) = self.narrow_subject_of(subject_side) else {
                        continue;
                    };
                    let Some(literal) = guard_literal_of(literal_side, self.source)
                        .or_else(|| self.guard_value_path_of(literal_side))
                    else {
                        continue;
                    };
                    // An aliased DISCRIMINANT carries its own fact: the
                    // checker re-establishes what the alias's initializer
                    // decided, on the reference that initializer named,
                    // which this relation never mentions.
                    if self.subject_root_carries_an_unmentioned_narrowing(&subject) {
                        return GuardDisposition::Unexpressible;
                    }
                    return GuardDisposition::modeled(SliceGuard::EqLiteral {
                        subject,
                        literal,
                        negated,
                        loose: false,
                    });
                }
                // A reference compared with a value that is not a literal —
                // another reference, a free name, a member path or a call —
                // narrows by the value's type through the comparable
                // relation ([`SliceGuard::EqReference`]); two operands that
                // guard cannot carry (a literal beside a reference it does not
                // name, say) narrow each other ([`SliceGuard::EqValue`]).
                if let Some(guard) = self.eq_value_guard(binary, negated, false) {
                    return GuardDisposition::modeled(guard);
                }
                if let Some(disposition) = self.equality_value_guard(binary, false, negated) {
                    return disposition;
                }
                self.classify_unexpressible_comparison(binary)
            }
            // Loose (in)equality. A `typeof` comparison narrows as the
            // strict one does. With `null` or `undefined` it selects BOTH
            // nullish arms (`x == null` is `x === null || x === undefined`,
            // and `x != null` its conjunction of negations — the checker's
            // `EQUndefinedOrNull` / `NEUndefinedOrNull` facts). With any
            // other literal it is the loose [`SliceGuard::EqLiteral`].
            // A comparison of two values, neither of them a literal, is the
            // loose two-value equality ([`SliceGuard::EqValue`]).
            BinaryOperator::Equality | BinaryOperator::Inequality => {
                let negated = matches!(binary.operator, BinaryOperator::Inequality);
                if let Some(guard) = self.typeof_guard(&binary.left, &binary.right, negated) {
                    return GuardDisposition::modeled(guard);
                }
                if let Some(guard) = self.typeof_guard(&binary.right, &binary.left, negated) {
                    return GuardDisposition::modeled(guard);
                }
                for (subject_side, literal_side) in
                    [(&binary.left, &binary.right), (&binary.right, &binary.left)]
                {
                    let Some(subject) = self.narrow_subject_of(subject_side) else {
                        continue;
                    };
                    let Some(literal) = guard_literal_of(literal_side, self.source) else {
                        continue;
                    };
                    if self.subject_root_carries_an_unmentioned_narrowing(&subject) {
                        return GuardDisposition::Unexpressible;
                    }
                    if !matches!(
                        literal,
                        SliceGuardLiteral::Null | SliceGuardLiteral::Undefined
                    ) {
                        return GuardDisposition::modeled(SliceGuard::EqLiteral {
                            subject,
                            literal,
                            negated,
                            loose: true,
                        });
                    }
                    let arms: Arc<[SliceGuard]> = Arc::from(
                        [SliceGuardLiteral::Null, SliceGuardLiteral::Undefined]
                            .into_iter()
                            .map(|literal| SliceGuard::EqLiteral {
                                subject: subject.clone(),
                                literal,
                                negated,
                                loose: false,
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    );
                    return GuardDisposition::modeled(if negated {
                        SliceGuard::And(arms)
                    } else {
                        SliceGuard::Or(arms)
                    });
                }
                // A reference compared with a value that is not a literal —
                // another reference, a free name, a member path or a call —
                // narrows by the value's type through the comparable
                // relation ([`SliceGuard::EqReference`]); two operands that
                // guard cannot carry (a literal beside a reference it does not
                // name, say) narrow each other ([`SliceGuard::EqValue`]).
                if let Some(guard) = self.eq_value_guard(binary, negated, true) {
                    return GuardDisposition::modeled(guard);
                }
                if let Some(disposition) = self.equality_value_guard(binary, true, negated) {
                    return disposition;
                }
                self.classify_unexpressible_comparison(binary)
            }
            BinaryOperator::Instanceof => {
                let Some(subject) = self.narrow_subject_of(&binary.left) else {
                    // A subject whose ACCESS this half cannot express is
                    // still a narrowing destination; one with no
                    // represented root has none.
                    return match self.narrow_destination_of(&binary.left) {
                        NarrowDestination::Absent => GuardDisposition::NoNarrowing,
                        _ => GuardDisposition::Unexpressible,
                    };
                };
                // The right-hand side is the VALUE the test compares
                // against at run time, and the evaluator lowers its NAME
                // as an owner-scope TYPE reference — which is that value's
                // instance type ONLY when the bare name provably denotes
                // the module's single same-file `class` declaration at
                // this call site ([`Self::closed_instanceof_constructor`]).
                // Every other spelling — a frame-bound name, a
                // namespace-owned site, a non-class value, an import, a
                // member or call expression — names a constructor this
                // half cannot prove, so the test degrades rather than
                // narrowing through the wrong binding.
                match unwrap_parenthesized(&binary.right) {
                    Expression::Identifier(ctor)
                        if self.closed_instanceof_constructor(ctor.name.as_str(), ctor.span) =>
                    {
                        GuardDisposition::modeled(SliceGuard::Instanceof {
                            subject,
                            ctor: Arc::from(ctor.name.as_str()),
                            negated: false,
                        })
                    }
                    _ => GuardDisposition::Unexpressible,
                }
            }
            BinaryOperator::In => {
                let key = match unwrap_parenthesized(&binary.left) {
                    Expression::StringLiteral(key) => Some(Arc::<str>::from(key.value.as_str())),
                    _ => None,
                };
                match (key, self.narrow_subject_of(&binary.right)) {
                    // The checker narrows by `in` only the reference the
                    // test names (`narrowTypeByInKeyword` never inlines an
                    // alias's initializer nor retypes a destructured
                    // sibling), so a subject rooted at a narrowing alias
                    // or a correlated element carries no fact the guard
                    // leaves unmentioned.
                    (Some(key), Some(subject)) => GuardDisposition::modeled(SliceGuard::In {
                        key,
                        subject,
                        negated: false,
                    }),
                    // Anything outside that exact pair — a computed or
                    // dynamic key, a private name, an inexpressible
                    // subject access — still selects the subject's union
                    // arms in the checker whenever the subject reaches a
                    // modeled slot.
                    _ => match self.narrow_destination_of(&binary.right) {
                        NarrowDestination::Absent => GuardDisposition::NoNarrowing,
                        _ => GuardDisposition::Unexpressible,
                    },
                }
            }
            _ => GuardDisposition::NoNarrowing,
        }
    }

    /// `subject === value` against a VALUE that is not a literal, in
    /// either operand order: another reference a narrow can land on —
    /// then both narrow, each by the other's type — or a name the frame
    /// leaves free, a static member path rooted at one, or a call whose
    /// callee is one. The checker narrows each matching reference by the
    /// other operand's type (`narrowTypeByEquality`), and a member
    /// reference's parent as a discriminant, so the value lowers as the
    /// flow expression it is and the evaluator reads its type
    /// ([`SliceGuard::EqReference`]). `None` when neither side is a
    /// represented reference, or when a side reaches a narrowing
    /// destination this vocabulary cannot spell.
    fn eq_value_guard(
        &mut self,
        binary: &oxc_ast::ast::BinaryExpression<'_>,
        negated: bool,
        loose: bool,
    ) -> Option<SliceGuard> {
        for (subject_side, value_side) in
            [(&binary.left, &binary.right), (&binary.right, &binary.left)]
        {
            let Some(subject) = self.narrow_subject_of(subject_side) else {
                continue;
            };
            if self.subject_root_carries_an_unmentioned_narrowing(&subject) {
                return None;
            }
            let value = if let Some(reference) = self.narrow_subject_of(value_side) {
                if self.subject_root_carries_an_unmentioned_narrowing(&reference) {
                    return None;
                }
                SliceEqOther::Reference(reference)
            } else {
                let value = unwrap_parenthesized(value_side);
                if self.operand_reaches_narrow_subject(value) || !self.free_rooted_value(value) {
                    return None;
                }
                SliceEqOther::Value(Box::new(self.lower_expr(value, ExprMode::Return)))
            };
            return Some(SliceGuard::EqReference {
                subject,
                value,
                negated,
                loose,
            });
        }
        None
    }

    /// Whether `expression` is a value whose read has no effect of its
    /// own beyond a call's: a name the frame leaves free (never
    /// `undefined`, which is a literal operand), a static member path
    /// rooted at one, or a call whose callee is one. A call's own effects
    /// are the control test's to certify.
    fn free_rooted_value(&self, expression: &Expression<'_>) -> bool {
        match unwrap_parenthesized(expression) {
            Expression::Identifier(identifier) => {
                identifier.name.as_str() != "undefined"
                    && matches!(self.classify_occurrence(identifier.span), NameBinding::Free)
            }
            Expression::StaticMemberExpression(member) => {
                self.free_rooted_value(&member.object)
                    && !matches!(
                        unwrap_parenthesized(&member.object),
                        Expression::CallExpression(_)
                    )
            }
            Expression::CallExpression(call) => {
                !call.optional
                    && matches!(
                        unwrap_parenthesized(&call.callee),
                        Expression::Identifier(_) | Expression::StaticMemberExpression(_)
                    )
                    && self.free_rooted_value(&call.callee)
            }
            _ => false,
        }
    }

    /// Classify an (in)equality this vocabulary could not express.
    ///
    /// The detection is RECURSIVE, not a direct operand match: a relation
    /// degrades when either side names a narrowing destination (a
    /// represented reference, an access rooted at one, or the `typeof` of
    /// either), OR when a nested guard is wrapped in a boolean comparison
    /// (`(typeof x === "string") === true`, which the checker unwraps and
    /// this half does not). A relation between two positions that reach
    /// no modeled slot narrows nothing it could have applied.
    fn classify_unexpressible_comparison(
        &mut self,
        binary: &oxc_ast::ast::BinaryExpression<'_>,
    ) -> GuardDisposition {
        for (side, other) in [(&binary.left, &binary.right), (&binary.right, &binary.left)] {
            if self.operand_reaches_narrow_subject(side) {
                return GuardDisposition::Unexpressible;
            }
            if literal_boolean_value(other).is_some()
                && !self.classify_guard(side).is_no_narrowing()
            {
                return GuardDisposition::Unexpressible;
            }
        }
        GuardDisposition::NoNarrowing
    }

    /// An equality between two values ([`SliceGuard::EqValue`]): both
    /// operands references or literals, at least one a represented whole
    /// binding. The checker narrows EVERY reference operand by the other
    /// operand's type, so a member reference (narrowing its parent as a
    /// discriminant), an aliased discriminant, and a reference this half
    /// cannot express are unexpressible. `None` when an operand is neither
    /// a reference nor a literal, or no operand is a represented
    /// reference: the caller's own classification decides those.
    fn equality_value_guard(
        &mut self,
        binary: &oxc_ast::ast::BinaryExpression<'_>,
        loose: bool,
        negated: bool,
    ) -> Option<GuardDisposition> {
        if !is_equality_value_operand(&binary.left) || !is_equality_value_operand(&binary.right) {
            return None;
        }
        let mut operands: Vec<SliceEqOperand> = Vec::with_capacity(2);
        for operand in [&binary.left, &binary.right] {
            let subject = match self.narrow_destination_of(operand) {
                NarrowDestination::Represented => {
                    let subject = self.narrow_subject_of(operand)?;
                    let aliased_discriminant = self
                        .alias_binding_of(operand)
                        .is_some_and(|binding| self.discriminant_aliases.contains_key(&binding));
                    if !subject.path.is_empty()
                        || aliased_discriminant
                        || self.subject_root_carries_an_unmentioned_narrowing(&subject)
                    {
                        return Some(GuardDisposition::Unexpressible);
                    }
                    Some(subject)
                }
                NarrowDestination::Unrepresented => return Some(GuardDisposition::Unexpressible),
                NarrowDestination::Absent => None,
            };
            let value = self.lower_expr(operand, ExprMode::Return);
            operands.push(SliceEqOperand { value, subject });
        }
        if operands.iter().all(|operand| operand.subject.is_none()) {
            return None;
        }
        let right = operands.pop()?;
        let left = operands.pop()?;
        Some(GuardDisposition::modeled(SliceGuard::EqValue {
            left: Box::new(left),
            right: Box::new(right),
            loose,
            negated,
        }))
    }

    /// Whether one COMPARISON operand names a position a narrow could
    /// land on: a narrowing destination, or the `typeof` of one. Both
    /// spellings put a modeled slot on the relation.
    fn operand_reaches_narrow_subject(&self, expression: &Expression<'_>) -> bool {
        let operand = reference_candidate(expression);
        let destination = match operand {
            Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::Typeof => {
                self.narrow_destination_of(&unary.argument)
            }
            other => self.narrow_destination_of(other),
        };
        !matches!(destination, NarrowDestination::Absent)
    }

    /// Where a narrow established by a test would LAND.
    fn narrow_destination_of(&self, expression: &Expression<'_>) -> NarrowDestination {
        if self.narrow_subject_of(expression).is_some() {
            NarrowDestination::Represented
        } else if self.reference_root_is_represented(expression) {
            NarrowDestination::Unrepresented
        } else {
            NarrowDestination::Absent
        }
    }

    /// Whether an expression is a REFERENCE route — the transparent
    /// wrappers, static / computed / private member steps, and
    /// optional-chain steps — whose ROOT is a parameter or modelable
    /// same-frame local.
    ///
    /// It never descends into a call's ARGUMENTS or a computed member's
    /// KEY: those are separate positions with their own owners (the call
    /// rail, the key's own lowering), and treating them as narrowing
    /// destinations would degrade every test that merely mentions a
    /// frame binding.
    fn reference_root_is_represented(&self, expression: &Expression<'_>) -> bool {
        match reference_candidate(expression) {
            Expression::Identifier(identifier) => self
                .identifier_roots_a_narrow_destination(identifier.name.as_str(), identifier.span),
            Expression::StaticMemberExpression(member) => {
                self.reference_root_is_represented(&member.object)
            }
            // An element access is a reference only when its key NAMES a
            // property (`getAccessedPropertyName`, with constant-index
            // narrowing): a literal, or an entity name that may be a
            // constant reference — a `const`, an enum member, a parameter
            // or local the function never writes. A key computed any other
            // way names none.
            Expression::ComputedMemberExpression(member) => {
                let key_may_name = match unwrap_parenthesized(&member.expression) {
                    Expression::StringLiteral(_) | Expression::NumericLiteral(_) => true,
                    // A key read from a binding this frame writes is not a
                    // constant reference and names nothing.
                    Expression::Identifier(key) => !matches!(
                        self.binding_at(key.span),
                        Some(FlowBindingRef::Local(binding))
                            if self.binding_is_written(binding)
                    ),
                    Expression::StaticMemberExpression(_) => true,
                    _ => false,
                };
                key_may_name && self.reference_root_is_represented(&member.object)
            }
            Expression::PrivateFieldExpression(member) => {
                self.reference_root_is_represented(&member.object)
            }
            Expression::ChainExpression(chain) => chain_element_root_identifier(&chain.expression)
                .is_some_and(|root| {
                    self.identifier_roots_a_narrow_destination(root.name.as_str(), root.span)
                }),
            _ => false,
        }
    }

    /// [`Self::reference_root_is_represented`] for an assignment target's
    /// member form.
    fn member_root_is_represented(&self, member: &oxc_ast::ast::MemberExpression<'_>) -> bool {
        self.reference_root_is_represented(member.object())
    }

    /// Whether a bare name at `span` roots a reference a narrow could
    /// land on — the same lexical authority
    /// [`Self::narrow_subject_of`] applies to a chain's root, PLUS the
    /// narrowing aliases.
    ///
    /// An alias is included even when this frame cannot otherwise model
    /// the binding (a destructured discriminant is not a simple reaching
    /// definition, so the name resolves to no modeled slot): the checker
    /// still re-establishes the aliased fact on the reference the
    /// initializer named, and that reference IS modeled — so the
    /// destination exists even though this half cannot express the
    /// route to it.
    fn identifier_roots_a_narrow_destination(&self, name: &str, span: oxc_span::Span) -> bool {
        matches!(
            self.classify_occurrence(span),
            // A CAPTURED binding is a landing slot like any other: the
            // evaluator resolves a nested read of an enclosing frame's
            // binding, and this half already WRITES through captured
            // names. A guard over one therefore establishes a fact the
            // checker applies and this lowering does not carry, so it is
            // unrepresented rather than proved absent.
            NameBinding::Param(_) | NameBinding::Local(_) | NameBinding::Captured
        ) || self.narrowing_alias_locals.contains(name)
    }

    /// Whether a modeled subject's ROOT carries a narrowing fact the
    /// guard over it never mentions — two sources, one consequence.
    ///
    /// An ALIAS local: TypeScript re-establishes the fact the alias's
    /// initializer decided, on the reference that initializer named, so
    /// a guard over the alias carries strictly less than the checker
    /// applies at any access path under it.
    ///
    /// A CORRELATED DESTRUCTURED PARAMETER ELEMENT: when the parameter's
    /// declared type could be a discriminated union, a relation over one
    /// element selects the OBJECT's arms and retypes the element's
    /// SIBLINGS — `f({ kind, payload }: P) { if (kind === "a") return
    /// payload }` publishes `payload`'s whole union while the checker
    /// publishes the `"a"` arm's. This half narrows neither the object
    /// nor the siblings, so the relation degrades.
    fn subject_root_carries_an_unmentioned_narrowing(&self, subject: &SliceNarrowSubject) -> bool {
        match &subject.root {
            SliceNarrowRoot::Local { name, .. } => {
                self.narrowing_alias_locals.contains(name)
                    || self.destructured_element_may_correlate(name)
            }
            SliceNarrowRoot::Param { .. } => false,
        }
    }

    /// Whether a name is a destructured element of a parameter whose
    /// declared type could make its elements CORRELATED.
    ///
    /// Correlation is a property of the parameter's TYPE, not of the
    /// pattern: only a discriminated union ties one element's narrow to
    /// its siblings' types. A parameter whose annotation lowers to a
    /// single object (or an intersection of objects, or a primitive /
    /// literal) cannot be one, so narrowing an element there retypes
    /// exactly that element — which this half already models. A union,
    /// or any annotation whose shape this half cannot see (a bare name,
    /// an unresolved form), leaves correlation possible.
    fn destructured_element_may_correlate(&self, name: &str) -> bool {
        self.params.iter().any(|param| {
            param
                .destructured
                .iter()
                .any(|element| element.name.as_ref() == name)
                && !param_type_forbids_correlation(param.ty.ty())
        })
    }

    /// Record every name one declarator binds as a narrowing ALIAS, when
    /// the declaration is one the checker preserves a narrowing through.
    ///
    /// Eligibility mirrors the checker's own alias rule and is decided
    /// from PROVEN disqualifications only — the declaration must be a
    /// constant binding (`const` / `using`), it must carry an
    /// initializer, and it must have NO type annotation (an annotated
    /// declaration takes its declared type and the alias is not
    /// inlined). Anything not positively disqualified, and whose
    /// initializer carries a fact, is treated as an alias. Both the
    /// whole-binding and the destructured spellings bind aliases: a
    /// destructured discriminant re-establishes its source's fact
    /// exactly as a named one does.
    fn record_narrowing_aliases(
        &mut self,
        kind: &VariableDeclarationKind,
        declarator: &oxc_ast::ast::VariableDeclarator<'_>,
    ) {
        if !matches!(
            kind,
            VariableDeclarationKind::Const
                | VariableDeclarationKind::Using
                | VariableDeclarationKind::AwaitUsing
        ) {
            return;
        }
        if declarator.type_annotation.is_some() {
            return;
        }
        let Some(init) = declarator.init.as_ref() else {
            return;
        };
        if !self.initializer_carries_a_narrowing(init) {
            return;
        }
        let mut names: FxHashSet<&str> = FxHashSet::default();
        collect_binding_pattern_names(&declarator.id, &mut names);
        for name in names {
            self.narrowing_alias_locals.insert(Arc::from(name));
        }
        match &declarator.id {
            BindingPattern::BindingIdentifier(id) => {
                let Some(binding) = self
                    .bindings
                    .declaration_at_span(self.rebase(id.span))
                    .map(FlowBindingRef::Local)
                else {
                    return;
                };
                // `const k = u.kind` aliases the DISCRIMINANT `u.kind`.
                if let Some(member) = self
                    .narrow_subject_of(init)
                    .filter(|member| !member.path.is_empty())
                {
                    if matches!(
                        unwrap_reference_transparent(init),
                        Expression::StaticMemberExpression(_)
                    ) {
                        self.discriminant_aliases.insert(binding, member);
                        return;
                    }
                }
                // Any other initializer is an aliased CONDITION, read once
                // per remaining inline budget.
                let saved = self.alias_inline_budget;
                let readings: [Option<SliceGuard>; ALIAS_INLINE_LIMIT] =
                    std::array::from_fn(|budget| {
                        self.alias_inline_budget = budget;
                        match self.classify_guard(init) {
                            GuardDisposition::Modeled(guard) => {
                                self.over_constant_references(*guard)
                            }
                            GuardDisposition::NoNarrowing => Some(SliceGuard::None),
                            GuardDisposition::Unexpressible => None,
                        }
                    });
                self.alias_inline_budget = saved;
                self.alias_conditions.insert(binding, readings);
            }
            // `const { kind } = u` / `const { kind: k } = u` alias the
            // discriminant `u.kind`.
            BindingPattern::ObjectPattern(object) => {
                let Some(source) = self.narrow_subject_of(init) else {
                    return;
                };
                for property in &object.properties {
                    if property.computed {
                        continue;
                    }
                    let key = match &property.key {
                        oxc_ast::ast::PropertyKey::StaticIdentifier(id) => id.name.as_str(),
                        oxc_ast::ast::PropertyKey::StringLiteral(literal) => literal.value.as_str(),
                        _ => continue,
                    };
                    let BindingPattern::BindingIdentifier(id) = &property.value else {
                        continue;
                    };
                    let Some(binding) = self
                        .bindings
                        .declaration_at_span(self.rebase(id.span))
                        .map(FlowBindingRef::Local)
                    else {
                        continue;
                    };
                    let mut path: Vec<Arc<str>> = source.path.to_vec();
                    path.push(Arc::from(key));
                    self.discriminant_aliases.insert(
                        binding,
                        SliceNarrowSubject {
                            root: source.root.clone(),
                            path: Arc::from(path.into_boxed_slice()),
                        },
                    );
                }
            }
            _ => {}
        }
    }

    /// `guard` with every fact on a reference the checker does not narrow
    /// through an alias (`isConstantReference`) replaced by "no narrowing":
    /// only a `const`, and a parameter or `let` / `var` local nothing ever
    /// assigns (a closure's write included), is narrowed through an
    /// aliased condition. `None` when a fact lands on a member reference —
    /// narrowed through an alias only when READONLY, which this lowering
    /// cannot see.
    fn over_constant_references(&self, guard: SliceGuard) -> Option<SliceGuard> {
        let compose = |this: &Self, parts: &Arc<[SliceGuard]>| -> Option<Arc<[SliceGuard]>> {
            parts
                .iter()
                .map(|part| this.over_constant_references(part.clone()))
                .collect::<Option<Vec<_>>>()
                .map(|parts| Arc::from(parts.into_boxed_slice()))
        };
        let subject = match &guard {
            SliceGuard::None => return Some(guard),
            SliceGuard::And(parts) => return compose(self, parts).map(SliceGuard::And),
            SliceGuard::Or(parts) => return compose(self, parts).map(SliceGuard::Or),
            SliceGuard::Both(parts) => return compose(self, parts).map(SliceGuard::Both),
            // A reference operand narrowed in turn must be constant too.
            SliceGuard::EqReference { subject, value, .. } => {
                if let SliceEqOther::Reference(reference) = value {
                    if !reference.path.is_empty() || !self.is_constant_root(&reference.root) {
                        return None;
                    }
                }
                subject
            }
            SliceGuard::Typeof { subject, .. }
            | SliceGuard::Truthy { subject, .. }
            | SliceGuard::EqLiteral { subject, .. }
            | SliceGuard::Instanceof { subject, .. }
            | SliceGuard::In { subject, .. }
            | SliceGuard::TypePredicate { subject, .. } => subject,
            // Each reference an equality narrows must itself be constant;
            // a non-constant one keeps only its value.
            SliceGuard::EqValue {
                left,
                right,
                loose,
                negated,
            } => {
                let constant = |this: &Self, operand: &SliceEqOperand| SliceEqOperand {
                    value: operand.value.clone(),
                    subject: operand
                        .subject
                        .as_ref()
                        .filter(|subject| {
                            subject.path.is_empty() && this.is_constant_root(&subject.root)
                        })
                        .cloned(),
                };
                let (left, right) = (constant(self, left), constant(self, right));
                if left.subject.is_none() && right.subject.is_none() {
                    return Some(SliceGuard::None);
                }
                return Some(SliceGuard::EqValue {
                    left: Box::new(left),
                    right: Box::new(right),
                    loose: *loose,
                    negated: *negated,
                });
            }
            // Each reference a call predicate may narrow must itself be
            // constant; a non-constant one is dropped from the guard.
            SliceGuard::CallPredicate {
                callee,
                site,
                arguments,
                receiver,
                negated,
            } => {
                let keep = |this: &Self, subject: &Option<SliceNarrowSubject>| {
                    subject.as_ref().and_then(|subject| {
                        (subject.path.is_empty() && this.is_constant_root(&subject.root))
                            .then(|| subject.clone())
                    })
                };
                if arguments
                    .iter()
                    .chain(std::iter::once(receiver))
                    .any(|subject| subject.as_ref().is_some_and(|s| !s.path.is_empty()))
                {
                    return None;
                }
                return Some(SliceGuard::CallPredicate {
                    callee: callee.clone(),
                    site: *site,
                    arguments: arguments
                        .iter()
                        .map(|subject| keep(self, subject))
                        .collect(),
                    receiver: keep(self, receiver),
                    negated: *negated,
                });
            }
            // A call through a callee's declared signatures: the same rule.
            SliceGuard::CalleePredicate {
                callee,
                arguments,
                negated,
                call,
            } => {
                if arguments
                    .iter()
                    .any(|subject| subject.as_ref().is_some_and(|s| !s.path.is_empty()))
                {
                    return None;
                }
                return Some(SliceGuard::CalleePredicate {
                    callee: callee.clone(),
                    arguments: arguments
                        .iter()
                        .map(|subject| {
                            subject
                                .as_ref()
                                .filter(|subject| self.is_constant_root(&subject.root))
                                .cloned()
                        })
                        .collect(),
                    negated: *negated,
                    call: *call,
                });
            }
        };
        // A one-segment discriminant narrows the ROOT reference; every
        // other member path narrows the member reference itself.
        let root_only = subject.path.is_empty()
            || (subject.path.len() == 1 && matches!(guard, SliceGuard::EqLiteral { .. }));
        if !root_only {
            return None;
        }
        Some(if self.is_constant_root(&subject.root) {
            guard
        } else {
            SliceGuard::None
        })
    }

    /// Whether a narrowable root is a constant reference: a `const`, or
    /// a parameter or `let` / `var` local nothing ever assigns.
    fn is_constant_root(&self, root: &SliceNarrowRoot) -> bool {
        match root {
            SliceNarrowRoot::Param { binding, .. } => !self.binding_is_assigned(*binding),
            SliceNarrowRoot::Local {
                binding: FlowBindingRef::Local(binding),
                ..
            } => match self.skeleton.binding(*binding).kind {
                SkeletonBindingKind::Const => true,
                SkeletonBindingKind::Let | SkeletonBindingKind::Var => {
                    !self.binding_is_assigned(*binding)
                }
                _ => false,
            },
            SliceNarrowRoot::Local { .. } => false,
        }
    }

    /// Whether anything in this function ever writes the whole binding —
    /// an assignment, an update, or a nested closure's write (the
    /// checker's `isSymbolAssigned`).
    fn binding_is_assigned(&self, binding: SkeletonBindingId) -> bool {
        binding_is_assigned(
            self.skeleton,
            self.bindings,
            self.index
                .get(self.bindings.function())
                .map(|entry| entry.entry()),
            binding,
        )
    }

    /// Whether a declarator initializer is a form the checker can bind a
    /// narrowing FACT to, so that an eligible `const` alias of it
    /// re-establishes that fact at every later test of the name.
    ///
    /// This classifies a DECLARATION, not a control test, and it is
    /// deliberately PURE: it decides the FORM and never lowers the
    /// expression, because lowering a guard has effects (it records
    /// predicate call spans) that a declaration position must not
    /// acquire. A CALL counts unconditionally — a type-predicate callee
    /// is syntactically indistinguishable from any other at this
    /// altitude, and the value-free certification the initializer
    /// position takes proves only that a DISCARDED result narrows
    /// nothing, which an alias's retained result is not. A REFERENCE
    /// counts too: that is the aliased-discriminant form. Literals,
    /// arithmetic and structural values carry no fact.
    fn initializer_carries_a_narrowing(&self, expression: &Expression<'_>) -> bool {
        use oxc_ast::ast::BinaryOperator;
        match unwrap_reference_transparent(expression) {
            Expression::BinaryExpression(binary) => matches!(
                binary.operator,
                BinaryOperator::StrictEquality
                    | BinaryOperator::StrictInequality
                    | BinaryOperator::Equality
                    | BinaryOperator::Inequality
                    | BinaryOperator::Instanceof
                    | BinaryOperator::In
            ),
            Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::LogicalNot => {
                self.initializer_carries_a_narrowing(&unary.argument)
            }
            Expression::LogicalExpression(logical) => {
                self.initializer_carries_a_narrowing(&logical.left)
                    || self.initializer_carries_a_narrowing(&logical.right)
            }
            Expression::ConditionalExpression(conditional) => {
                self.initializer_carries_a_narrowing(&conditional.consequent)
                    || self.initializer_carries_a_narrowing(&conditional.alternate)
            }
            Expression::CallExpression(_) => true,
            other => !matches!(self.narrow_destination_of(other), NarrowDestination::Absent),
        }
    }

    /// The `typeof subject === "kind"` form: `side` is the `typeof …`
    /// unary, `other` the compared string literal.
    fn typeof_guard(
        &self,
        side: &Expression<'_>,
        other: &Expression<'_>,
        negated: bool,
    ) -> Option<SliceGuard> {
        let Expression::UnaryExpression(unary) = unwrap_parenthesized(side) else {
            return None;
        };
        if unary.operator != UnaryOperator::Typeof {
            return None;
        }
        let Expression::StringLiteral(literal) = unwrap_parenthesized(other) else {
            return None;
        };
        let kind = match literal.value.as_str() {
            "string" => SliceTypeofKind::String,
            "number" => SliceTypeofKind::Number,
            "bigint" => SliceTypeofKind::BigInt,
            "boolean" => SliceTypeofKind::Boolean,
            "symbol" => SliceTypeofKind::Symbol,
            "undefined" => SliceTypeofKind::Undefined,
            "object" => SliceTypeofKind::Object,
            "function" => SliceTypeofKind::Function,
            _ => return None,
        };
        let subject = self.narrow_subject_of(&unary.argument)?;
        Some(SliceGuard::Typeof {
            subject,
            kind,
            negated,
        })
    }

    /// The user-defined type predicate form: `isStr(u)`, where the
    /// narrowing fact lives in the CALLEE's declared return (`x is T`),
    /// not at the use site. Only a callee with a PROVABLY CLOSED
    /// same-file declaration ([`Self::closed_callee_declaration`])
    /// carries its authored signature through this channel — a
    /// frame-local shadow names a different function, a cross-file
    /// callee's annotation is beyond the retained snapshot this half
    /// reads, a script global or an exported binding has a
    /// checker-visible signature set this file cannot enumerate, and a
    /// namespace-owned call site binds its callee through a block scope
    /// the gate does not enumerate.
    fn lower_predicate_guard(&mut self, call: &oxc_ast::ast::CallExpression<'_>) -> SliceGuard {
        let Expression::Identifier(callee) = unwrap_parenthesized(&call.callee) else {
            return SliceGuard::None;
        };
        let name = callee.name.as_str();
        if !matches!(self.classify_occurrence(callee.span), NameBinding::Free) {
            return SliceGuard::None;
        }
        let Some((ordinal, Some(target))) = self.same_file_predicate(name, false, call.span) else {
            return SliceGuard::None;
        };
        let Some(argument) = call
            .arguments
            .get(ordinal)
            .and_then(|argument| argument.as_expression())
        else {
            return SliceGuard::None;
        };
        let Some(subject) = self.narrow_subject_of(argument) else {
            return SliceGuard::None;
        };
        let span: verter_span::Span = call.span.into();
        self.predicate_guard_call_spans.insert(span);
        SliceGuard::TypePredicate {
            subject,
            target,
            negated: false,
            call: span,
        }
    }

    /// The [`SliceGuard::CalleePredicate`] of a control call whose bare
    /// callee resolves free to a value that is not ONE closed same-file
    /// function: the evaluator reads its declared signatures. A call site
    /// inside a namespace block (whose bare names bind through the block
    /// first), explicit type arguments and a spread argument keep the
    /// control-call rail.
    fn lower_callee_signature_guard(
        &mut self,
        call: &oxc_ast::ast::CallExpression<'_>,
        callee: &oxc_ast::ast::IdentifierReference<'_>,
    ) -> SliceGuard {
        if self.namespace_owned
            || call.type_arguments.is_some()
            || call.arguments.iter().any(|argument| argument.is_spread())
            || !self.callee_declaration_set_closed(callee.name.as_str())
        {
            return SliceGuard::None;
        }
        let ty = TypeExpr::TypeOf(verter_type_expr::ValueRef {
            path: vec![callee.name.to_string()],
            type_args: Vec::new(),
        });
        let callee = self.gate(ty, callee.span, &[]);
        let arguments: Vec<Option<SliceNarrowSubject>> = call
            .arguments
            .iter()
            .map(|argument| {
                argument
                    .as_expression()
                    .and_then(|argument| self.narrow_subject_of(argument))
            })
            .collect();
        let span: verter_span::Span = call.span.into();
        self.predicate_guard_call_spans.insert(span);
        SliceGuard::CalleePredicate {
            callee,
            arguments: Arc::from(arguments.into_boxed_slice()),
            negated: false,
            call: span,
        }
    }

    /// Whether the checker-visible declaration set of the top-level value
    /// `name` is exactly this file's own declarations of it, so its
    /// declared signatures are the ones the owner-scope lowering reads. A
    /// variable (`const` / `let` / `var`, declared or not, exported or not)
    /// never merges with another declaration; a function merges with every
    /// same-name global in a script and with any augmentation of an
    /// exported one, so its set is closed only when it is a module's
    /// unexported function (an overload group included). An import, a
    /// class, an enum or a namespace of the name, and a name this file does
    /// not declare, are not closed here.
    fn callee_declaration_set_closed(&self, name: &str) -> bool {
        self.closed_callee_declaration_kind(name).is_some()
    }

    /// [`Self::callee_declaration_set_closed`], answering whether the closed
    /// declaration carries an EXPLICIT type (`getExplicitTypeOfSymbol`): a
    /// function always does, a variable only through its annotation. Only
    /// an explicitly typed callee gives a call statement an effect.
    fn closed_callee_declaration_kind(&self, name: &str) -> Option<ClosedCalleeKind> {
        use oxc_ast::ast::{Declaration, ImportDeclarationSpecifier};
        let mut variable = false;
        let mut annotated_variable = false;
        let mut functions = 0usize;
        let mut other = false;
        for statement in &self.program.body {
            let declaration = match statement {
                Statement::ExportNamedDeclaration(export) => match &export.declaration {
                    Some(declaration) => declaration,
                    None => continue,
                },
                Statement::ImportDeclaration(import) => {
                    other |= import.specifiers.iter().flatten().any(|specifier| {
                        let local = match specifier {
                            ImportDeclarationSpecifier::ImportSpecifier(s) => &s.local,
                            ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => &s.local,
                            ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => &s.local,
                        };
                        local.name.as_str() == name
                    });
                    continue;
                }
                Statement::ExportDefaultDeclaration(_)
                | Statement::TSImportEqualsDeclaration(_) => {
                    continue;
                }
                statement => match statement.as_declaration() {
                    Some(declaration) => declaration,
                    None => continue,
                },
            };
            match declaration {
                Declaration::VariableDeclaration(variables) => {
                    for declarator in variables.declarations.iter() {
                        if matches!(&declarator.id, BindingPattern::BindingIdentifier(id)
                            if id.name.as_str() == name)
                        {
                            variable = true;
                            annotated_variable |= declarator.type_annotation.is_some();
                        }
                    }
                }
                Declaration::FunctionDeclaration(function) => {
                    if function
                        .id
                        .as_ref()
                        .is_some_and(|id| id.name.as_str() == name)
                    {
                        functions += 1;
                    }
                }
                Declaration::ClassDeclaration(class) => {
                    other |= class.id.as_ref().is_some_and(|id| id.name.as_str() == name);
                }
                Declaration::TSEnumDeclaration(declaration) => {
                    other |= declaration.id.name.as_str() == name;
                }
                // A namespace of the name merges with a function of it; a
                // `declare global` block declares globals this module-local
                // binding shadows.
                Declaration::TSModuleDeclaration(module) => {
                    other |= matches!(&module.id,
                        oxc_ast::ast::TSModuleDeclarationName::Identifier(id) if id.name.as_str() == name);
                }
                Declaration::TSGlobalDeclaration(_) => {}
                Declaration::TSImportEqualsDeclaration(declaration) => {
                    other |= declaration.id.name.as_str() == name;
                }
                Declaration::TSTypeAliasDeclaration(_) | Declaration::TSInterfaceDeclaration(_) => {
                }
            }
        }
        if other {
            return None;
        }
        match (variable, functions) {
            (true, 0) => Some(ClosedCalleeKind {
                explicit: annotated_variable,
            }),
            (false, 1..) if self.module_scope && !self.top_level_name_is_exported(name) => {
                Some(ClosedCalleeKind { explicit: true })
            }
            _ => None,
        }
    }

    /// Whether every signature the top-level declarations of `name` declare
    /// returns `never` by its authored annotation: a function declaration
    /// group (its implementation signature aside when overloads precede
    /// it), or an annotated variable whose annotation is a function type.
    /// Any other annotation, and a group mixing `never` with another
    /// return, is `false` — which overload a call selects is the
    /// evaluator's question.
    fn closed_callee_declares_never(&self, name: &str) -> bool {
        use oxc_ast::ast::Declaration;
        let returns_never = |annotation: Option<&oxc_ast::ast::TSTypeAnnotation<'_>>| {
            annotation.is_some_and(|annotation| {
                matches!(annotation.type_annotation, TSType::TSNeverKeyword(_))
            })
        };
        let mut signatures: Vec<bool> = Vec::new();
        let mut overloads: Vec<bool> = Vec::new();
        for statement in &self.program.body {
            let declaration = match statement {
                Statement::ExportNamedDeclaration(export) => match &export.declaration {
                    Some(declaration) => declaration,
                    None => continue,
                },
                statement => match statement.as_declaration() {
                    Some(declaration) => declaration,
                    None => continue,
                },
            };
            match declaration {
                Declaration::FunctionDeclaration(function)
                    if function
                        .id
                        .as_ref()
                        .is_some_and(|id| id.name.as_str() == name) =>
                {
                    let never = returns_never(function.return_type.as_deref());
                    if function.body.is_some() && !overloads.is_empty() {
                        // The implementation signature is not visible
                        // beside its overloads.
                        signatures.append(&mut overloads);
                    } else if function.body.is_some() {
                        signatures.push(never);
                    } else {
                        overloads.push(never);
                    }
                }
                Declaration::VariableDeclaration(variables) => {
                    for declarator in variables.declarations.iter() {
                        if !matches!(&declarator.id, BindingPattern::BindingIdentifier(id)
                            if id.name.as_str() == name)
                        {
                            continue;
                        }
                        match declarator
                            .type_annotation
                            .as_deref()
                            .map(|annotation| &annotation.type_annotation)
                        {
                            Some(TSType::TSFunctionType(function)) => {
                                signatures.push(returns_never(Some(&function.return_type)));
                            }
                            _ => return false,
                        }
                    }
                }
                _ => {}
            }
        }
        signatures.append(&mut overloads);
        !signatures.is_empty() && signatures.iter().all(|never| *never)
    }

    /// The [`SliceStatement::CalleeEffect`] of a statement call whose bare
    /// callee resolves free to a value that is not ONE closed same-file
    /// function, under the same refusals as
    /// [`Self::lower_callee_signature_guard`].
    fn lower_callee_effect_statement(
        &mut self,
        call: &oxc_ast::ast::CallExpression<'_>,
    ) -> Option<SliceStatement> {
        let Expression::Identifier(callee) = unwrap_parenthesized(&call.callee) else {
            return None;
        };
        if !matches!(self.classify_occurrence(callee.span), NameBinding::Free)
            || self
                .closed_callee_declaration(callee.name.as_str())
                .is_some()
        {
            return None;
        }
        // A callee with no explicit type — a variable without an annotation
        // — gives the call no effect at all, whatever function it holds.
        if !self.namespace_owned
            && self
                .closed_callee_declaration_kind(callee.name.as_str())
                .is_some_and(|kind| !kind.explicit)
        {
            return Some(SliceStatement::ThrowPoint);
        }
        // An explicitly typed callee every declared signature of which
        // returns `never` ends the path, as an authored `throw` does.
        if !self.namespace_owned
            && self.callee_declaration_set_closed(callee.name.as_str())
            && self.closed_callee_declares_never(callee.name.as_str())
        {
            return Some(SliceStatement::Throw);
        }
        match self.lower_callee_signature_guard(call, callee) {
            SliceGuard::CalleePredicate {
                callee, arguments, ..
            } => Some(SliceStatement::CalleeEffect { callee, arguments }),
            _ => None,
        }
    }

    /// THE constructor-closure gate for `instanceof`: whether the bare
    /// right-hand side `name`, referenced at `span`, provably denotes the
    /// module's ONE same-file top-level `class` declaration — so that the
    /// owner-scope type reference the evaluator lowers for it (the class's
    /// instance type) IS the instance type of the value the test compares
    /// against. The frame's lexical authority binds first (a parameter
    /// `A: typeof B` or a body-local `const A = B` shadows the owner-scope
    /// class, and the checker narrows to `B`); a namespace-owned call site
    /// binds through its block scope before the top level; a script's
    /// top-level declaration set is not enumerable from one file; and a
    /// non-class top-level value (`const A = B`, an import) is a
    /// constructor whose instance type no same-name type reference
    /// yields. A class's VALUE meaning cannot be merged away by an
    /// augmentation (a second class of the name is a duplicate
    /// identifier), so the export spelling does not matter here.
    fn closed_instanceof_constructor(&self, name: &str, span: oxc_span::Span) -> bool {
        if !self.module_scope || self.namespace_owned {
            return false;
        }
        if !matches!(self.classify_occurrence(span), NameBinding::Free) {
            return false;
        }
        let declarations = self.same_file_class_declarations(name);
        let [class] = declarations.as_slice() else {
            return false;
        };
        self.class_static_side_provably_plain(class, &mut Vec::new())
    }

    /// Whether a same-file top-level class's STATIC side provably declares
    /// no `[Symbol.hasInstance]` — the one static member that changes what
    /// `instanceof` narrows to. A static method, property or accessor of
    /// that key whose type is a type predicate makes the checker narrow to
    /// the PREDICATE's target instead of the class's instance type, and the
    /// static side is inherited through the heritage chain. Any static
    /// COMPUTED key can spell it (`static [key]` over `const key =
    /// Symbol.hasInstance`), so a class declaring one is not provably
    /// plain; nor is a class whose superclass is anything but a provably
    /// plain same-file top-level class (a value binding, an import, a call
    /// expression, an unresolvable heritage cycle). A top-level class name
    /// is a duplicate identifier against any other same-name top-level
    /// value, so "exactly one same-file class declaration" is the whole
    /// binding proof at the top level.
    fn class_static_side_provably_plain(
        &self,
        class: &oxc_ast::ast::Class<'_>,
        visited: &mut Vec<oxc_span::Span>,
    ) -> bool {
        use oxc_ast::ast::ClassElement;
        if visited.contains(&class.span) {
            return false;
        }
        visited.push(class.span);
        let own_side_plain = class.body.body.iter().all(|element| match element {
            ClassElement::MethodDefinition(method) => !(method.r#static && method.computed),
            ClassElement::PropertyDefinition(property) => !(property.r#static && property.computed),
            ClassElement::AccessorProperty(accessor) => !(accessor.r#static && accessor.computed),
            ClassElement::StaticBlock(_) | ClassElement::TSIndexSignature(_) => true,
        });
        if !own_side_plain {
            return false;
        }
        match class.super_class.as_ref().map(unwrap_parenthesized) {
            None => true,
            Some(Expression::Identifier(base)) => {
                let bases = self.same_file_class_declarations(base.name.as_str());
                match bases.as_slice() {
                    [base_class] => self.class_static_side_provably_plain(base_class, visited),
                    _ => false,
                }
            }
            Some(_) => false,
        }
    }

    /// Every same-file top-level `class` DECLARATION with `name`, in
    /// source order, across the direct, `export class`, and `export
    /// default class` spellings.
    fn same_file_class_declarations(&self, name: &str) -> Vec<&oxc_ast::ast::Class<'_>> {
        self.program
            .body
            .iter()
            .filter_map(|statement| match statement {
                Statement::ClassDeclaration(class) => Some(&**class),
                Statement::ExportNamedDeclaration(export) => match &export.declaration {
                    Some(oxc_ast::ast::Declaration::ClassDeclaration(class)) => Some(&**class),
                    _ => None,
                },
                Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                    oxc_ast::ast::ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                        Some(&**class)
                    }
                    _ => None,
                },
                _ => None,
            })
            .filter(|class| class.id.as_ref().map(|id| id.name.as_str()) == Some(name))
            .collect()
    }

    /// Every same-file top-level function DECLARATION with `name`, in
    /// source order — the direct spelling, the `export function` spelling,
    /// and the `export default function` spelling all count, because the
    /// group SIZE is a semantic fact (an overload group's signature
    /// selection) that must not depend on export syntax.
    fn same_file_function_declarations(&self, name: &str) -> Vec<&oxc_ast::ast::Function<'_>> {
        self.program
            .body
            .iter()
            .filter_map(|statement| match statement {
                Statement::FunctionDeclaration(function) => Some(&**function),
                Statement::ExportNamedDeclaration(export) => match &export.declaration {
                    Some(oxc_ast::ast::Declaration::FunctionDeclaration(function)) => {
                        Some(&**function)
                    }
                    _ => None,
                },
                Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                    oxc_ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                        Some(&**function)
                    }
                    _ => None,
                },
                _ => None,
            })
            .filter(|function| function.id.as_ref().map(|id| id.name.as_str()) == Some(name))
            .collect()
    }

    /// Whether ANY top-level export spelling exposes the module-local
    /// binding `name` on the module's export surface: `export function
    /// name`, `export default function name`, a local specifier `export {
    /// name }` / `export { name as other }` (never a re-export from
    /// another module, whose specifiers name that module's bindings),
    /// `export default name`, or `export = name`. An exposed binding is
    /// AUGMENTABLE — a `declare module "…"` block in any file merges
    /// further declarations into the export symbol, and the checker
    /// resolves even a same-file reference to an `export`-modified
    /// declaration through that merged symbol — so its declaration
    /// closure is not provable from this file's text.
    fn top_level_name_is_exported(&self, name: &str) -> bool {
        let names_binding = |expression: &Expression<'_>| matches!(unwrap_parenthesized(expression), Expression::Identifier(id) if id.name.as_str() == name);
        self.program.body.iter().any(|statement| match statement {
            Statement::ExportNamedDeclaration(export) => {
                let declares = match &export.declaration {
                    Some(oxc_ast::ast::Declaration::FunctionDeclaration(function)) => {
                        function.id.as_ref().map(|id| id.name.as_str()) == Some(name)
                    }
                    _ => false,
                };
                declares
                    || (export.source.is_none()
                        && export
                            .specifiers
                            .iter()
                            .any(|specifier| specifier.local.name().as_str() == name))
            }
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                oxc_ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                    function.id.as_ref().map(|id| id.name.as_str()) == Some(name)
                }
                declaration => declaration.as_expression().is_some_and(names_binding),
            },
            Statement::TSExportAssignment(export) => names_binding(&export.expression),
            _ => false,
        })
    }

    /// THE callee-closure gate shared by the predicate channel and the
    /// control-call certification: the ONE same-file function declaration
    /// of `name` when the callee's CHECKER-VISIBLE declaration set is
    /// provably that single declaration from this file's text alone —
    /// `None` whenever the closure cannot be proven, which establishes no
    /// fact (the caller emits the typed `GuardNarrowing` gap instead).
    ///
    /// The proof needs all four of:
    /// - the file is provably MODULE-scoped ([`Self::module_scope`]): a
    ///   script's top-level function is a global symbol merging with
    ///   every other script's and every `declare global` block's
    ///   same-name declarations, a set no single snapshot enumerates;
    /// - EXACTLY ONE same-file declaration: an overload group's
    ///   signature selection is overload/applicability resolution this
    ///   half does not perform, and the first declaration's annotation
    ///   can be the WRONG one;
    /// - the binding is NOT exported by any spelling
    ///   ([`Self::top_level_name_is_exported`]): an exported binding is
    ///   augmentable through `declare module`, so further signatures can
    ///   merge into it from any file;
    /// - the call site is NOT namespace-owned ([`Self::namespace_owned`]):
    ///   inside a `namespace` / `module` block a bare name binds through
    ///   the block scope before the top level, and the block-local
    ///   declaration set (every declaration kind, plus merged sibling
    ///   blocks and augmentations of an exported namespace) is one this
    ///   gate does not enumerate — a top-level match there names the
    ///   WRONG declaration.
    ///
    /// A module-local, unexported binding called from a top-level scope is
    /// closed: augmentations merge only into a module's exports, and a
    /// module's locals shadow every global — so the file's own text is the
    /// whole declaration set.
    fn closed_callee_declaration(&self, name: &str) -> Option<&oxc_ast::ast::Function<'_>> {
        if !self.module_scope || self.namespace_owned {
            return None;
        }
        let group = self.same_file_function_declarations(name);
        let [function] = group.as_slice() else {
            return None;
        };
        if self.top_level_name_is_exported(name) {
            return None;
        }
        Some(function)
    }

    /// Classify one STATEMENT-POSITION call — see
    /// [`StatementCallEffect`].
    ///
    /// Only a bare-identifier callee resolving FREE to a PROVABLY CLOSED
    /// same-file declaration can be proven at all. A member callee, an
    /// import, an exported binding, a script global, an overload group,
    /// a call on a call result: each has a checker-visible signature set
    /// this file cannot enumerate, so it could assert or could diverge.
    fn statement_call_effect(
        &self,
        call: &oxc_ast::ast::CallExpression<'_>,
    ) -> StatementCallEffect {
        let Expression::Identifier(callee) = unwrap_parenthesized(&call.callee) else {
            return StatementCallEffect::Unprovable;
        };
        let name = callee.name.as_str();
        if let Some(effect) = self.callee_binding_effect(callee.span) {
            return effect;
        }
        if !matches!(self.classify_occurrence(callee.span), NameBinding::Free) {
            return StatementCallEffect::Unprovable;
        }
        let Some(function) = self.closed_callee_declaration(name) else {
            return StatementCallEffect::Unprovable;
        };
        let annotation = function.return_type.as_deref();
        // An assertion signature narrows every read that follows. The
        // modeled arm already answered when it could apply the narrow;
        // reaching here means it could not.
        if annotation.is_some_and(|annotation| {
            matches!(
                &annotation.type_annotation,
                TSType::TSTypePredicate(predicate) if predicate.asserts
            )
        }) {
            return StatementCallEffect::Unprovable;
        }
        if annotation.is_some_and(|annotation| {
            matches!(annotation.type_annotation, TSType::TSNeverKeyword(_))
        }) {
            return StatementCallEffect::NeverReturns;
        }
        if self.closed_callee_provably_returns(function) {
            StatementCallEffect::Inert
        } else {
            StatementCallEffect::Unprovable
        }
    }

    /// The checker's effects signature of a statement call whose callee
    /// the closed-declaration rule could not settle
    /// (`getEffectsSignature` over `getTypeOfDottedName`): a callee that
    /// is not a dotted name, or a dotted name rooted at a parameter or
    /// local WITHOUT an authored annotation, has no explicit type, so the
    /// call neither asserts nor ends the path. A dotted name rooted at an
    /// annotated parameter or local reads its declared member type, and one
    /// rooted at a module binding, a global or a nested function
    /// declaration its value — both settled by the evaluator. A `this` or
    /// `super` root, a free closed declaration (already decided) and an
    /// unmodelled binding stay unprovable.
    fn effect_callee(&mut self, call: &oxc_ast::ast::CallExpression<'_>) -> EffectCallee {
        let mut path: Vec<Arc<str>> = Vec::new();
        let mut cursor = unwrap_parenthesized(&call.callee);
        let root = loop {
            match cursor {
                Expression::StaticMemberExpression(member) => {
                    path.push(Arc::from(member.property.name.as_str()));
                    cursor = unwrap_parenthesized(&member.object);
                }
                Expression::Identifier(root) => break root,
                Expression::ThisExpression(_) | Expression::Super(_) => {
                    return EffectCallee::Unprovable;
                }
                Expression::PrivateFieldExpression(_) => return EffectCallee::Unprovable,
                _ => return EffectCallee::Inert,
            }
        };
        path.reverse();
        let name = root.name.as_str();
        let declared = |this: &mut Self, ordinal: Option<u32>| {
            this.narrow_root(name, root.span, ordinal)
                .map(|root| {
                    EffectCallee::Settle(SliceEffectCallee::Declared(SliceNarrowSubject {
                        root,
                        path: Arc::from(path.clone().into_boxed_slice()),
                    }))
                })
                .unwrap_or(EffectCallee::Unprovable)
        };
        match self.classify_occurrence(root.span) {
            NameBinding::Param(ordinal) => {
                if self.annotated_params.contains(&ordinal) {
                    declared(self, Some(ordinal))
                } else {
                    EffectCallee::Inert
                }
            }
            NameBinding::Local(_) | NameBinding::Captured => {
                let annotated = match self.binding_at(root.span) {
                    Some(FlowBindingRef::Local(binding)) => {
                        let fact = self
                            .skeleton
                            .binding(self.bindings.canonical_local(binding));
                        if fact.kind == SkeletonBindingKind::Param {
                            return EffectCallee::Unprovable;
                        }
                        fact.annotation_span.is_some() && !fact.destructured
                    }
                    _ => return EffectCallee::Unprovable,
                };
                if annotated {
                    declared(self, None)
                } else {
                    EffectCallee::Inert
                }
            }
            NameBinding::Free if self.closed_callee_declaration(name).is_some() => {
                EffectCallee::Unprovable
            }
            NameBinding::Free | NameBinding::NestedFunction => {
                EffectCallee::Settle(SliceEffectCallee::Value(Box::new(self.lower_expr(
                    &call.callee,
                    ExprMode::BindingInit {
                        preserve_literal: true,
                    },
                ))))
            }
            NameBinding::Unmodeled => EffectCallee::Unprovable,
        }
    }

    /// The statement a call whose effects [`Self::effect_callee`] reads
    /// lowers to: a throw point, then the callee the evaluator settles.
    fn effect_callee_statement(
        &mut self,
        call: &oxc_ast::ast::CallExpression<'_>,
    ) -> SliceStatement {
        match self.effect_callee(call) {
            EffectCallee::Inert => {
                self.decided_above_call_spans.push(call.span.into());
                SliceStatement::ThrowPoint
            }
            EffectCallee::Settle(callee) => SliceStatement::Block(SliceRegion {
                statements: Arc::from(
                    vec![
                        SliceStatement::ThrowPoint,
                        SliceStatement::CallEffect {
                            callee,
                            call: call.span.into(),
                        },
                    ]
                    .into_boxed_slice(),
                ),
                can_fall_through: NormalCompletion::minted(
                    true,
                    CompletionConstruction::SynthesizedRegion,
                ),
            }),
            EffectCallee::Unprovable => {
                self.control_test_gap = true;
                SliceStatement::ThrowPoint
            }
        }
    }

    /// The control-flow effect of a statement call whose bare callee names
    /// a function-local or captured binding, when its declarations decide
    /// it; `None` for every other callee.
    ///
    /// The checker reads a call's control-flow EFFECTS — an assertion, or
    /// a `never` return that ends the path — only through a callee whose
    /// declaration carries an explicit type (`getExplicitTypeOfSymbol`),
    /// and only from that type's lone call signature (or the resolved one
    /// when an overload asserts or diverges). A variable or parameter
    /// qualifies only through its annotation (or a `for…of` head, which
    /// never has an initializer), and a function declaration through the
    /// signatures it declares, whose return is never inferred `never` nor
    /// an assertion. So an unannotated `const` / `let` / `var` with an
    /// initializer, an unannotated parameter, one annotated `any` or with a
    /// function type, and a nested function declaration narrow nothing and
    /// never end the path unless a declared signature returns `asserts …`
    /// or `never` — whatever the function they call writes or returns: a
    /// closure that assigns a narrowed binding leaves the narrowing in
    /// place after the call. A lone declared signature returning `never`
    /// ends the path; any other annotation is unprovable here.
    fn callee_binding_effect(&self, span: oxc_span::Span) -> Option<StatementCallEffect> {
        use verter_semantic::analysis::flow::FlowBindingOccurrence;
        let (gate, local) = match self.bindings.occurrence(self.rebase(span)) {
            FlowBindingOccurrence::Resolved(FlowBindingRef::Local(local)) => {
                (&*self.frame_gate, *local)
            }
            FlowBindingOccurrence::Resolved(FlowBindingRef::Captured(identity)) => {
                self.captures.defining_local(identity)?
            }
            FlowBindingOccurrence::Free
            | FlowBindingOccurrence::UnmodeledLocal
            | FlowBindingOccurrence::Missing => return None,
        };
        let declarations = gate.bindings.runtime_declarations(local);
        if declarations.is_empty() {
            return None;
        }
        let mut annotated: Vec<FrameSpan> = Vec::new();
        for declaration in declarations {
            let fact = gate.skeleton.binding(*declaration);
            match fact.kind {
                SkeletonBindingKind::Const
                | SkeletonBindingKind::Let
                | SkeletonBindingKind::Var
                    if !fact.destructured =>
                {
                    if fact.annotation_span.is_some() {
                        annotated.push(fact.span);
                    } else if fact.initializer.is_none() {
                        return None;
                    }
                }
                SkeletonBindingKind::Param if !fact.destructured => annotated.push(fact.span),
                SkeletonBindingKind::NestedFunction => annotated.push(fact.span),
                _ => return None,
            }
        }
        if annotated.is_empty() {
            return Some(StatementCallEffect::Inert);
        }
        let entry = self.index.get(gate.bindings.function())?;
        let mut finder = DeclaredCallEffects {
            within: entry.entry().span,
            anchor: gate.anchor,
            names: annotated,
            effects: Vec::new(),
        };
        finder.visit_program(self.program);
        if finder.effects.len() != finder.names.len() {
            return None;
        }
        let count = |effect: DeclaredCallEffect| {
            finder
                .effects
                .iter()
                .filter(|found| **found == effect)
                .count()
        };
        Some(
            match (
                count(DeclaredCallEffect::Asserts) + count(DeclaredCallEffect::Undecided),
                count(DeclaredCallEffect::Never),
                declarations.len(),
            ) {
                (0, 0, _) => StatementCallEffect::Inert,
                (0, 1, 1) => StatementCallEffect::NeverReturns,
                _ => StatementCallEffect::Unprovable,
            },
        )
    }

    /// Whether a closed same-file callee provably COMPLETES — the proof a
    /// statement-position call needs before the path may continue past
    /// it.
    ///
    /// An authored annotation whose syntax cannot denote `never` settles
    /// it. Otherwise the inferred return decides, and a function infers
    /// `never` exactly when it never returns: a body whose statement list
    /// is EMPTY completes at once, and a body carrying a `return` in its
    /// own frame can complete. Anything else — a bodiless signature, a
    /// body whose every path might diverge — is unproven.
    fn closed_callee_provably_returns(&self, function: &oxc_ast::ast::Function<'_>) -> bool {
        if function
            .return_type
            .as_deref()
            .is_some_and(|annotation| annotation_provably_not_never(&annotation.type_annotation))
        {
            return true;
        }
        let Some(body) = function.body.as_deref() else {
            return false;
        };
        if body.statements.is_empty() {
            return true;
        }
        let mut finder = OwnFrameReturnFinder::default();
        for statement in &body.statements {
            finder.visit_statement(statement);
        }
        finder.found
    }

    /// Read a SAME-FILE function declaration's return-type predicate:
    /// `x is T` (`asserts` false) or `asserts x is T` / a targetless
    /// `asserts x` (`asserts` true), consumed at the call site `site`.
    /// Returns the ordinal of the parameter the predicate talks about and
    /// the target type lowered through the frame gate (a BODY position —
    /// the frame's own type declarations are in scope there, exactly like
    /// a declarator annotation); the target is `None` for the targetless
    /// assertion spelling. `None` for the whole read for any other
    /// signature spelling.
    ///
    /// The channel serves EXACTLY ONE PROVABLY CLOSED declaration
    /// ([`Self::closed_callee_declaration`]). An overload group (two or
    /// more same-name declarations) is refused outright: which signature
    /// applies is overload/applicability resolution, which this half does
    /// not perform, and the first declaration's predicate target can be
    /// the WRONG one — narrowing on it would publish a checker-divergent
    /// type. A script global or an exported binding is refused for the
    /// same reason one level up: its checker-visible signature set may
    /// hold overloads this file never shows. A refused callee establishes
    /// no fact.
    fn same_file_predicate(
        &self,
        name: &str,
        asserts: bool,
        site: oxc_span::Span,
    ) -> Option<(usize, Option<GatedType>)> {
        let (function, predicate, annotation_span) = self.closed_predicate_annotation(name)?;
        if predicate.asserts != asserts {
            return None;
        }
        let target = match predicate.type_annotation.as_ref() {
            Some(target) => {
                let lowered = lower_ts_type(&target.type_annotation, self.source);
                // The target is authored in the CALLEE's declaration scope
                // and consumed in the CALLER's frame, so it must be closed
                // over names BOTH resolve identically — the module scope.
                // A target naming a binding of the CALLEE's own declaration
                // is instantiated by the CALL — `T` of `isSame<T>(x: T): x
                // is T` binds to the argument's type; `typeof y` names the
                // callee's own parameter — an inference this half does not
                // perform. A target naming a binding of the CALLER's frame
                // (its own or an enclosing `<T>`, a body-local `type T`, a
                // local `y` under a `typeof y` root) would REBIND at the
                // call site: the caller's binder environment and lexical
                // authority answer before the module's `type T = number`.
                // Either way the channel refuses it: no fact, and the call
                // takes the typed guard-narrowing gap at its consumer.
                if predicate_target_names_callee_binding(function, &lowered)
                    || self.predicate_target_names_caller_binding(&lowered, site)
                {
                    return None;
                }
                Some(self.gate(lowered, annotation_span, &[]))
            }
            None => None,
        };
        // A non-`asserts` predicate without a target type is not a
        // predicate spelling at all.
        if !asserts && target.is_none() {
            return None;
        }
        let oxc_ast::ast::TSTypePredicateName::Identifier(parameter) = &predicate.parameter_name
        else {
            return None;
        };
        let ordinal = function.params.items.iter().position(|param| {
            matches!(&param.pattern, BindingPattern::BindingIdentifier(id)
                if id.name.as_str() == parameter.name.as_str())
        })?;
        Some((ordinal, target))
    }

    /// The type-predicate return annotation of the PROVABLY CLOSED
    /// same-file callee `name` ([`Self::closed_callee_declaration`]): the
    /// declaration, its predicate, and the annotation's span. `None` for
    /// a refused callee or a non-predicate return.
    fn closed_predicate_annotation(
        &self,
        name: &str,
    ) -> Option<(
        &oxc_ast::ast::Function<'_>,
        &oxc_ast::ast::TSTypePredicate<'_>,
        oxc_span::Span,
    )> {
        let function = self.closed_callee_declaration(name)?;
        let annotation = function.return_type.as_ref()?;
        let TSType::TSTypePredicate(predicate) = &annotation.type_annotation else {
            return None;
        };
        Some((function, predicate, annotation.span))
    }

    /// Whether the provably closed same-file callee `name` declares an
    /// `asserts x is T` whose target the predicate channel REFUSES — as
    /// call-instantiated ([`predicate_target_names_callee_binding`]) or
    /// as rebound at the call site `site`
    /// ([`Self::predicate_target_names_caller_binding`]). The checker
    /// narrows the rest of the region through that assertion and this
    /// half cannot apply it, so the statement degrades through the typed
    /// guard-narrowing gap instead of lowering as a bare throw point that
    /// leaves the subject silently unnarrowed.
    fn assertion_target_is_refused(&self, name: &str, site: oxc_span::Span) -> bool {
        let Some((function, predicate, _)) = self.closed_predicate_annotation(name) else {
            return false;
        };
        predicate.asserts
            && predicate.type_annotation.as_ref().is_some_and(|target| {
                let lowered = lower_ts_type(&target.type_annotation, self.source);
                predicate_target_names_callee_binding(function, &lowered)
                    || self.predicate_target_names_caller_binding(&lowered, site)
            })
    }

    /// Whether a same-file predicate's TARGET, consumed at the call site
    /// `site` of THIS frame, references a name the frame's own environment
    /// binds — so that lowering it here would rebind it away from the
    /// callee's declaration scope. A value root (`typeof y`) is rebound by
    /// any frame binding of `y` (a parameter, a local, a capture); a type
    /// name is rebound by a type parameter of this frame or any enclosing
    /// frame (the composed binder environment interns those ahead of every
    /// owner-scope alias), or by a frame-owned declaration in either
    /// type-space meaning. A value-only local of a type name's spelling
    /// binds nothing in type space and is transparent here.
    fn predicate_target_names_caller_binding(
        &self,
        target: &TypeExpr,
        site: oxc_span::Span,
    ) -> bool {
        let names = verter_type_expr::referenced_names(target);
        if names
            .value_roots
            .iter()
            .any(|root| self.frame_gate.value_name_is_bound(root, self.rebase(site)))
        {
            return true;
        }
        names.type_names.iter().any(|occurrence| {
            let head = occurrence.head.as_str();
            self.type_param_names
                .iter()
                .any(|binder| binder.as_ref() == head)
                || self.captures.binder_is_visible(head)
                || self.name_is_frame_bound(head, site, NameMeaning::Type, &[])
                || self.name_is_frame_bound(head, site, NameMeaning::Namespace, &[])
        })
    }

    /// Whether a sequence's LAST operand — its value provider — lowers
    /// through a structural arm this half owns: a NARROWABLE REFERENCE
    /// (lowered as the read, so the frame's substitutions stay visible),
    /// a call routed to the structural call rails by the ONE shared
    /// predicate (`sequence_value_takes_call_rail`) the classifier's own
    /// sequence verdict is decided through, or an `await` (the
    /// `Awaited` arm owns it) — so this half can never delegate a form
    /// the classifier still calls an unmodeled-call position.
    fn sequence_value_lowers_structurally(&self, last: &Expression<'_>) -> bool {
        self.narrow_subject_of(last).is_some()
            || sequence_value_takes_call_rail(last)
            || sequence_value_takes_await_arm(last)
    }

    /// The narrowable reference an expression NAMES: a static member
    /// chain rooted at an identifier the frame's lexical authority
    /// resolves to a simple parameter or a modelable same-frame local.
    /// Anything else — a call result, a computed member, a captured or
    /// free root — is not positionally substitutable, so no narrow can
    /// land on it.
    ///
    /// Every step reads through [`unwrap_reference_transparent`], so the
    /// wrappers the checker treats as transparent to reference identity —
    /// parentheses and the postfix non-null assertion — name the SAME
    /// subject as the bare spelling, at the root and at every member step:
    /// `typeof x! === "string"` narrows `x`, `u!.kind === "a"` selects on
    /// `u.kind`, and `typeof a.b!` narrows `a.b` (all measured). Reading
    /// through parentheses ALONE left those spellings reference-less, so a
    /// guard over one was classified unrecognized and the whole result
    /// degraded behind a typed gap instead of narrowing.
    ///
    /// `satisfies` and the `as` / angle-bracket type assertion are
    /// deliberately NOT transparent here: neither is a matching reference
    /// for narrowing, so peeling one would narrow where the checker does
    /// not — a SUBSET of the checker's type, which drops a real
    /// contributor and is worse than the superset a missing narrow
    /// produces.
    fn narrow_subject_of(&self, expression: &Expression<'_>) -> Option<SliceNarrowSubject> {
        let mut segments: Vec<Arc<str>> = Vec::new();
        let mut current = reference_candidate(expression);
        let identifier = loop {
            match current {
                Expression::StaticMemberExpression(member) => {
                    segments.push(Arc::from(member.property.name.as_str()));
                    current = reference_candidate(&member.object);
                }
                // An element access whose key is a string or numeric
                // literal names its property exactly as a dotted access
                // does (the checker's `getAccessedPropertyName`).
                Expression::ComputedMemberExpression(member) => {
                    segments.push(literal_member_key(&member.expression)?);
                    current = unwrap_reference_transparent(&member.object);
                }
                Expression::Identifier(identifier) => break identifier,
                _ => return None,
            }
        };
        segments.reverse();
        let path: Arc<[Arc<str>]> = Arc::from(segments.into_boxed_slice());
        let name = identifier.name.as_str();
        match self.classify_occurrence(identifier.span) {
            NameBinding::Param(ordinal) => Some(SliceNarrowSubject {
                root: self.narrow_root(name, identifier.span, Some(ordinal))?,
                path,
            }),
            NameBinding::Local(_) | NameBinding::Captured => Some(SliceNarrowSubject {
                root: self.narrow_root(name, identifier.span, None)?,
                path,
            }),
            NameBinding::Free | NameBinding::NestedFunction | NameBinding::Unmodeled => None,
        }
    }

    /// Lower one expression statement's VALUE-NEUTRAL effects into
    /// content: a whole-binding `=` write to a parameter or modelable
    /// local (whose right-hand side the slice value-selected) becomes a
    /// [`SliceStatement::Assignment`] the evaluator APPLIES, and a
    /// same-file assertion call becomes a [`SliceStatement::Assertion`]
    /// whose narrowing persists for the rest of the region.
    ///
    /// Every other expression statement lowers to nothing, exactly as
    /// before: its value is never consumed and its evaluation effects
    /// ride the slice's typed effect obligations — a compound-operator
    /// write, a member-path write, and a write whose value the slice did
    /// not select all keep the typed unapplied-write degradation rather
    /// than acquiring a second, divergent verdict here.
    ///
    /// The fallthrough is still SCANNED
    /// ([`Self::scan_unmodeled_statement_effects`]): a call nested where
    /// the two modeled forms never look — a discarded sequence operand, a
    /// `void` operand, a template interpolation, an unselected
    /// right-hand side — executes at the statement and can carry an
    /// `asserts` narrowing of a frame-owned binding, and a class subtree
    /// hides writes from the skeleton entirely. The scan gaps only
    /// frame-reaching effects: the statement's own value is discarded,
    /// and its skeleton-visible writes already ride the unapplied-write
    /// ledger.
    fn lower_effect_statement(&mut self, expression: &Expression<'_>) -> Option<SliceStatement> {
        match unwrap_parenthesized(expression) {
            // A discarded value holding a write this lowering applies — a
            // whole-binding `=` write (bare or under `void`), a compound
            // write or update, an EVOLVING-array operation, or a
            // conditional, logical, sequence, object or array literal
            // holding one — applies its writes in evaluation order
            // ([`Self::lower_discarded_effects`]).
            discarded if self.discarded_value_holds_write(discarded) => {
                let mut statements = Vec::new();
                self.lower_discarded_effects(
                    expression,
                    DiscardedContext::Statement,
                    &mut statements,
                );
                match statements.len() {
                    0 => None,
                    1 => statements.pop(),
                    _ => Some(SliceStatement::Block(SliceRegion {
                        statements: Arc::from(statements.into_boxed_slice()),
                        can_fall_through: NormalCompletion::minted(
                            true,
                            CompletionConstruction::SynthesizedRegion,
                        ),
                    })),
                }
            }
            // Every operand of a comma sequence is entered like a statement
            // of its own, a `void` operand's included; any other `void`
            // operand only RUNS, and its calls are never entered into
            // control flow (the leaf scanner's `void` rule).
            Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::Void => {
                match unwrap_parenthesized(&unary.argument) {
                    Expression::SequenceExpression(sequence) => {
                        Some(self.lower_entered_sequence_statement(sequence))
                    }
                    _ => self.scan_unmodeled_statement_effects(expression),
                }
            }
            // A compound write to a parameter or modelable local (`x += v;`,
            // `i++;`) retypes it to the base type of what it held; the
            // operand still runs first.
            Expression::UpdateExpression(update) => {
                let oxc_ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) =
                    &update.argument
                else {
                    if let Some(write) = update
                        .argument
                        .as_member_expression()
                        .and_then(|member| self.modeled_member_compound(member))
                    {
                        return Some(write);
                    }
                    return self.scan_unmodeled_statement_effects(expression);
                };
                match self.write_target_root(identifier) {
                    Some(root) => Some(SliceStatement::CompoundAssignment {
                        target: SliceNarrowSubject {
                            root,
                            path: Arc::from(Vec::new().into_boxed_slice()),
                        },
                        span: self.rebase(update.span),
                        definition: None,
                    }),
                    None => self.scan_unmodeled_statement_effects(expression),
                }
            }
            Expression::AssignmentExpression(assignment)
                if assignment.operator.is_arithmetic() || assignment.operator.is_bitwise() =>
            {
                let oxc_ast::ast::AssignmentTarget::AssignmentTargetIdentifier(identifier) =
                    &assignment.left
                else {
                    let Some(write) = assignment
                        .left
                        .as_member_expression()
                        .and_then(|member| self.modeled_member_compound(member))
                    else {
                        return self.scan_unmodeled_statement_effects(expression);
                    };
                    // The operand runs before the write.
                    return match self.scan_unmodeled_statement_effects(&assignment.right) {
                        Some(throw_point) => Some(SliceStatement::Block(SliceRegion {
                            statements: Arc::from(vec![throw_point, write].into_boxed_slice()),
                            can_fall_through: NormalCompletion::minted(
                                true,
                                CompletionConstruction::SynthesizedRegion,
                            ),
                        })),
                        None => Some(write),
                    };
                };
                let Some(root) = self.write_target_root(identifier) else {
                    return self.scan_unmodeled_statement_effects(expression);
                };
                let definition = self.selection.and_then(|selection| {
                    selection.value_site(self.rebase(assignment.right.span()))
                });
                let write = SliceStatement::CompoundAssignment {
                    target: SliceNarrowSubject {
                        root,
                        path: Arc::from(Vec::new().into_boxed_slice()),
                    },
                    span: self.rebase(identifier.span),
                    definition,
                };
                match self.scan_unmodeled_statement_effects(&assignment.right) {
                    Some(throw_point) => Some(SliceStatement::Block(SliceRegion {
                        statements: Arc::from(vec![throw_point, write].into_boxed_slice()),
                        can_fall_through: NormalCompletion::minted(
                            true,
                            CompletionConstruction::SynthesizedRegion,
                        ),
                    })),
                    None => Some(write),
                }
            }
            // Only an UNPARENTHESIZED call is entered into control flow
            // (`(assertString(x));` narrows nothing — the binder's
            // `maybeBindExpressionFlowIfCall` sees a parenthesized
            // expression); a parenthesized one takes the scan below.
            Expression::CallExpression(call)
                if matches!(expression, Expression::CallExpression(_)) =>
            {
                // A bare call is a THROW POINT regardless of what it
                // resolves to; a same-file assertion call additionally
                // narrows. The marker keeps the throw point even when the
                // assertion path below does not recognise the callee.
                let assertion = self.entered_assertion(call);
                // The statement's own call is the modeled boundary above;
                // the effects NESTED in its callee expression and
                // arguments are separate positions the slice never selects
                // — scan them without re-scanning the call itself. An
                // entered `asserts` call among them (a comma operand)
                // applies before the statement's own call.
                let nested =
                    self.collecting_entered_assertions(|this| this.scan_call_operands(call));
                let own = match assertion {
                    Some(SliceAssertion { subject, target }) => {
                        SliceStatement::Assertion { subject, target }
                    }
                    // The statement's OWN call takes the same
                    // prove-or-degrade discipline every other
                    // result-independent position takes, PLUS the
                    // reachability half no other position needs: a callee
                    // that never returns ends the path, and treating it as
                    // a plain throw point publishes the following
                    // statements' contributions the checker drops.
                    None => match self.statement_call_effect(call) {
                        StatementCallEffect::Inert => {
                            self.decided_above_call_spans.push(call.span.into());
                            SliceStatement::ThrowPoint
                        }
                        StatementCallEffect::NeverReturns => SliceStatement::Throw,
                        // A free callee this file does not declare as ONE
                        // closed function: its declared signatures decide
                        // the effect at evaluation. The call's result is
                        // discarded either way.
                        StatementCallEffect::Unprovable => {
                            match self.lower_callee_effect_statement(call) {
                                Some(effect) => {
                                    self.decided_above_call_spans.push(call.span.into());
                                    effect
                                }
                                None => self.effect_callee_statement(call),
                            }
                        }
                    },
                };
                if nested.is_empty() {
                    return Some(own);
                }
                let mut statements = nested;
                statements.push(own);
                Some(sequential_block(statements))
            }
            // Every operand of a comma sequence is entered like a
            // statement of its own.
            Expression::SequenceExpression(sequence) => {
                Some(self.lower_entered_sequence_statement(sequence))
            }
            // The scan keeps the authored parentheses: a parenthesized
            // call is not the statement's own.
            _ => self.scan_unmodeled_statement_effects(expression),
        }
    }

    /// Whether a discarded value holds a write [`Self::lower_discarded_effects`]
    /// applies: a whole-binding `=` write, bare or under `void`; a compound
    /// write or an update of a whole binding; an EVOLVING-array operation;
    /// or a conditional, logical, sequence, object or array literal
    /// holding one — and every write the free
    /// [`discarded_value_holds_write`] admits (a member, destructuring or
    /// logical assignment, a write in an operator's operand).
    fn discarded_value_holds_write(&self, expression: &Expression<'_>) -> bool {
        if discarded_value_holds_write(expression) {
            return true;
        }
        match unwrap_parenthesized(expression) {
            Expression::AssignmentExpression(assignment) => match assignment.operator {
                oxc_ast::ast::AssignmentOperator::Assign => {
                    verter_semantic::analysis::flow::assignment_target_binding(&assignment.left)
                        .is_some()
                        || verter_semantic::analysis::flow::evolving_array_element_write_root(
                            assignment,
                        )
                        .is_some_and(|(_, root)| self.evolving_binding_at(root.span).is_some())
                }
                operator if operator.is_arithmetic() || operator.is_bitwise() => {
                    verter_semantic::analysis::flow::assignment_target_binding(&assignment.left)
                        .is_some()
                }
                _ => false,
            },
            Expression::UpdateExpression(update) => {
                verter_semantic::analysis::flow::simple_assignment_target_binding(&update.argument)
                    .is_some()
            }
            Expression::CallExpression(call) => self.is_evolving_operation_call(call),
            Expression::ChainExpression(chain) => match &chain.expression {
                oxc_ast::ast::ChainElement::CallExpression(call) => {
                    self.is_evolving_operation_call(call)
                }
                _ => false,
            },
            void @ Expression::UnaryExpression(unary)
                if unary.operator == oxc_ast::ast::UnaryOperator::Void =>
            {
                void_write_assignment(void).is_some()
            }
            // An operator's operands run before it: `x++ > 0`, `!(x += 1)`.
            Expression::UnaryExpression(unary) => self.discarded_value_holds_write(&unary.argument),
            Expression::BinaryExpression(binary) => {
                self.discarded_value_holds_write(&binary.left)
                    || self.discarded_value_holds_write(&binary.right)
            }
            Expression::ConditionalExpression(conditional) => {
                self.discarded_value_holds_write(&conditional.consequent)
                    || self.discarded_value_holds_write(&conditional.alternate)
            }
            Expression::LogicalExpression(logical) => {
                self.discarded_value_holds_write(&logical.left)
                    || self.discarded_value_holds_write(&logical.right)
            }
            Expression::SequenceExpression(sequence) => sequence
                .expressions
                .iter()
                .any(|operand| self.discarded_value_holds_write(operand)),
            Expression::ObjectExpression(object) => object.properties.iter().any(|property| {
                matches!(
                    property,
                    oxc_ast::ast::ObjectPropertyKind::ObjectProperty(property)
                        if property.kind == oxc_ast::ast::PropertyKind::Init
                            && !property.method
                            && self.discarded_value_holds_write(&property.value)
                )
            }),
            Expression::ArrayExpression(array) => array.elements.iter().any(|element| {
                !matches!(
                    element,
                    oxc_ast::ast::ArrayExpressionElement::SpreadElement(_)
                ) && element
                    .as_expression()
                    .is_some_and(|value| self.discarded_value_holds_write(value))
            }),
            _ => false,
        }
    }

    /// Whether a call is an operation on an EVOLVING array (`a.push(v)`,
    /// `a.unshift(v)`).
    fn is_evolving_operation_call(&self, call: &oxc_ast::ast::CallExpression<'_>) -> bool {
        verter_semantic::analysis::flow::evolving_array_mutation_root(call)
            .is_some_and(|root| self.evolving_binding_at(root.span).is_some())
    }

    /// The `x++` / `x--` updates a control test evaluates on every path
    /// through it (`while (n-- > 0)`), lowered ahead of the test's branch
    /// in source order: the checker's flow assigns each where the test
    /// evaluates, before the condition splits. An update in a conditionally
    /// evaluated operand (`c && n--`, a conditional's arm) is not on every
    /// path, and one the frame cannot model stays on the unapplied-write
    /// ledger.
    fn lower_test_updates(&mut self, test: &Expression<'_>, out: &mut Vec<SliceStatement>) {
        match test {
            Expression::UpdateExpression(update) => {
                out.extend(self.compound_update_statement(update));
            }
            Expression::BinaryExpression(binary) => {
                self.lower_test_updates(&binary.left, out);
                self.lower_test_updates(&binary.right, out);
            }
            Expression::UnaryExpression(unary) => self.lower_test_updates(&unary.argument, out),
            Expression::ParenthesizedExpression(paren) => {
                self.lower_test_updates(&paren.expression, out);
            }
            _ => {}
        }
    }

    /// The [`SliceStatement::CompoundAssignment`] an `x++` / `x--` of a
    /// parameter or modelable local applies (through parentheses and `!`;
    /// `(x as T)++` writes nothing).
    fn compound_update_statement(
        &mut self,
        update: &oxc_ast::ast::UpdateExpression<'_>,
    ) -> Option<SliceStatement> {
        let identifier =
            verter_semantic::analysis::flow::simple_assignment_target_binding(&update.argument)?;
        let root = self.write_target_root(identifier)?;
        Some(SliceStatement::CompoundAssignment {
            target: SliceNarrowSubject {
                root,
                path: Arc::from(Vec::new().into_boxed_slice()),
            },
            span: self.rebase(update.span),
            definition: None,
        })
    }

    /// Lower a discarded compound write (`x += v`, `x++`) into `out`: the
    /// operand runs first, then the target takes the
    /// [`SliceStatement::CompoundAssignment`] retype. A target this frame
    /// does not model takes the effect scan of `context`.
    fn lower_discarded_compound_write(
        &mut self,
        expression: &Expression<'_>,
        context: DiscardedContext,
        out: &mut Vec<SliceStatement>,
    ) {
        match unwrap_parenthesized(expression) {
            Expression::UpdateExpression(update) => match self.compound_update_statement(update) {
                Some(statement) => out.push(statement),
                None => self.scan_discarded_effects(expression, context, out),
            },
            Expression::AssignmentExpression(assignment) => {
                let target =
                    verter_semantic::analysis::flow::assignment_target_binding(&assignment.left)
                        .and_then(|identifier| {
                            Some((self.write_target_root(identifier)?, identifier.span))
                        });
                let Some((root, target_span)) = target else {
                    self.scan_discarded_effects(expression, context, out);
                    return;
                };
                let definition = self.selection.and_then(|selection| {
                    selection.value_site(self.rebase(assignment.right.span()))
                });
                self.scan_discarded_effects(&assignment.right, context, out);
                out.push(SliceStatement::CompoundAssignment {
                    target: SliceNarrowSubject {
                        root,
                        path: Arc::from(Vec::new().into_boxed_slice()),
                    },
                    span: self.rebase(target_span),
                    definition,
                });
            }
            other => self.scan_discarded_effects(other, context, out),
        }
    }

    /// A statement-position comma sequence: each operand lowers as an
    /// expression statement of its own, in order, so an unparenthesized
    /// call among them is entered into control flow (an `asserts` call
    /// narrows what follows it; a never-returning one ends the path). The
    /// operands lower as one block.
    fn lower_entered_sequence_statement(
        &mut self,
        sequence: &oxc_ast::ast::SequenceExpression<'_>,
    ) -> SliceStatement {
        let mut statements = Vec::with_capacity(sequence.expressions.len());
        for operand in &sequence.expressions {
            let Some(statement) = self.lower_effect_statement(operand) else {
                continue;
            };
            let ends = statement_ends_path(&statement);
            statements.push(statement);
            if ends {
                break;
            }
        }
        sequential_block(statements)
    }

    /// Run `scan` while collecting the `asserts` predicates of the entered
    /// calls it finds, which the caller applies after the scanned
    /// position.
    fn collecting_entered_assertions(
        &mut self,
        scan: impl FnOnce(&mut Self),
    ) -> Vec<SliceStatement> {
        let previous = self.entered_assertion_sink.replace(Vec::new());
        scan(self);
        std::mem::replace(&mut self.entered_assertion_sink, previous).unwrap_or_default()
    }

    /// The `asserts` predicate an ENTERED call applies — a free callee
    /// naming a closed same-file assertion declaration, over an argument
    /// this half narrows. A closed same-file assertion whose target the
    /// predicate channel refuses (`asserts x is T` on `assertSame<T>`, or
    /// over a `T` this frame rebinds) narrows in the checker but cannot be
    /// applied here: it flags the typed gap.
    fn entered_assertion(
        &mut self,
        call: &oxc_ast::ast::CallExpression<'_>,
    ) -> Option<SliceAssertion> {
        let free_callee = match unwrap_parenthesized(&call.callee) {
            Expression::Identifier(callee)
                if matches!(self.classify_occurrence(callee.span), NameBinding::Free) =>
            {
                Some(callee.name.as_str())
            }
            _ => None,
        };
        let assertion = free_callee.and_then(|name| {
            let (ordinal, target) = self.same_file_predicate(name, true, call.span)?;
            let argument = call
                .arguments
                .get(ordinal)
                .and_then(|argument| argument.as_expression())?;
            let subject = self.narrow_subject_of(argument)?;
            Some(SliceAssertion { subject, target })
        });
        if assertion.is_none()
            && free_callee.is_some_and(|name| self.assertion_target_is_refused(name, call.span))
        {
            self.control_test_gap = true;
        }
        assertion
    }

    /// Scan the effects nested in one modeled call's callee and arguments
    /// (`foo((assertString(x), 0));` narrows `x` in the checker), without
    /// re-scanning the call itself.
    fn scan_call_operands(&mut self, call: &oxc_ast::ast::CallExpression<'_>) {
        let mut scanner = LeafCallScanner::default();
        scanner.visit_expression(&call.callee);
        for argument in &call.arguments {
            if let Some(argument) = argument.as_expression() {
                scanner.visit_expression(argument);
            }
        }
        if self.drain_scanned_same_frame_effects(
            scanner,
            CertificationMode::ValueFree,
            WritePolicy::SkeletonHiddenOnly,
        ) {
            self.control_test_gap = true;
        }
    }

    /// The modeled whole-binding-write statement form, when the
    /// assignment's target is a parameter or modelable local AND the slice
    /// value-selected the right-hand side.
    fn modeled_assignment_statement(
        &mut self,
        assignment: &oxc_ast::ast::AssignmentExpression<'_>,
    ) -> Option<SliceStatement> {
        if let Some(member) = assignment.left.as_member_expression() {
            return self.modeled_member_assignment(member, assignment);
        }
        if matches!(
            assignment.left,
            oxc_ast::ast::AssignmentTarget::ObjectAssignmentTarget(_)
                | oxc_ast::ast::AssignmentTarget::ArrayAssignmentTarget(_)
        ) {
            let definition = self
                .selection?
                .value_site(self.rebase(assignment.right.span()))?;
            let pattern = self.lower_assignment_pattern(&assignment.left)?;
            let value = self.lower_expr(
                &assignment.right,
                ExprMode::BindingInit {
                    preserve_literal: true,
                },
            );
            return Some(SliceStatement::DestructureAssign {
                pattern,
                value,
                definition,
            });
        }
        let (target, definition, span) =
            self.modeled_assignment_parts(assignment, assignment.right.span())?;
        let value = self.lowered_assignment_rhs(assignment);
        Some(SliceStatement::Assignment {
            definition,
            target: SliceNarrowSubject {
                root: target,
                path: Arc::from(Vec::new().into_boxed_slice()),
            },
            // The span identity matches the slice's typed write
            // effect, which the skeleton records at the TARGET
            // IDENTIFIER — never the whole assignment expression.
            span,
            value: Box::new(value),
            freshness: expression_freshness(&assignment.right),
        })
    }

    /// The modeled member-path `=` write (`o.y = v`, `a["k"] = v`): the
    /// target a narrowable reference rooted at a parameter or modelable
    /// local, the right-hand side value-selected by the slice. `None`
    /// keeps the fail-closed scan — a write no demanded read observes
    /// selects no value, and an unmodeled target narrows nothing here.
    fn modeled_member_assignment(
        &mut self,
        member: &oxc_ast::ast::MemberExpression<'_>,
        assignment: &oxc_ast::ast::AssignmentExpression<'_>,
    ) -> Option<SliceStatement> {
        let (target, key) = self.member_write_subject(member)?;
        self.selection?
            .value_site(self.rebase(assignment.right.span()))?;
        let value = self.lower_expr(
            &assignment.right,
            ExprMode::BindingInit {
                preserve_literal: true,
            },
        );
        Some(SliceStatement::MemberWrite {
            target,
            key,
            span: self.rebase(member.span()),
            write: SliceMemberWrite::Assign {
                value: Box::new(value),
                freshness: expression_freshness(&assignment.right),
            },
        })
    }

    /// The narrowable reference a member write target spells: a static /
    /// literal-keyed path under a parameter or modelable local.
    fn member_write_subject(
        &mut self,
        member: &oxc_ast::ast::MemberExpression<'_>,
    ) -> Option<(SliceNarrowSubject, Option<SliceWriteKey>)> {
        // A computed key reading a frame binding: the object's reference,
        // extended at evaluation by the segment the key spells.
        if let oxc_ast::ast::MemberExpression::ComputedMemberExpression(computed) = member {
            if literal_member_key(&computed.expression).is_none() {
                let key = self.element_key(&computed.expression)?;
                let object = self.narrow_subject_of(&computed.object)?;
                if !self.member_root_is_frame_binding(&object) {
                    return None;
                }
                let value = self.lower_expr(
                    &computed.expression,
                    ExprMode::BindingInit {
                        preserve_literal: true,
                    },
                );
                return Some((
                    object,
                    Some(SliceWriteKey {
                        value: Box::new(value),
                        key,
                    }),
                ));
            }
        }
        let (root, path) = member_target_chain(member)?;
        let name = root.name.as_str();
        let root = match self.classify_occurrence(root.span) {
            NameBinding::Param(ordinal) => self.narrow_root(name, root.span, Some(ordinal))?,
            // A captured root's member reference narrows inside this
            // frame's own flow exactly as a local one does.
            NameBinding::Local(_) | NameBinding::Captured => {
                self.narrow_root(name, root.span, None)?
            }
            NameBinding::Free | NameBinding::NestedFunction | NameBinding::Unmodeled => {
                return None
            }
        };
        Some((
            SliceNarrowSubject {
                root,
                path: Arc::from(path.into_boxed_slice()),
            },
            None,
        ))
    }

    /// Whether a narrowable reference is rooted at a parameter or a
    /// modelable local of this frame (captured ones included) — the roots
    /// a member write narrows.
    fn member_root_is_frame_binding(&self, subject: &SliceNarrowSubject) -> bool {
        matches!(
            subject.root,
            SliceNarrowRoot::Param { .. } | SliceNarrowRoot::Local { .. }
        )
    }

    /// The reference identity of an element-access key reading a frame
    /// binding ([`SliceElementKey`]). `None` for any other key.
    fn element_key(&self, key: &Expression<'_>) -> Option<SliceElementKey> {
        let Expression::Identifier(key) = unwrap_parenthesized(key) else {
            return None;
        };
        let Some(FlowBindingRef::Local(binding)) = self.binding_at(key.span) else {
            return None;
        };
        let canonical = self.bindings.canonical_local(binding);
        let kind = self.skeleton.binding(canonical).kind;
        if !matches!(
            kind,
            SkeletonBindingKind::Const
                | SkeletonBindingKind::Let
                | SkeletonBindingKind::Var
                | SkeletonBindingKind::Param
        ) {
            return None;
        }
        let assigned =
            self.binding_is_written(binding) || self.nested_free_writes.contains(&canonical);
        Some(SliceElementKey {
            constant: kind == SkeletonBindingKind::Const && !assigned,
            identity: (!assigned)
                .then(|| Arc::from(format!("\u{0}{}:{}", self.anchor, canonical.index()))),
        })
    }

    /// The modeled compound member-path write (`o.n += v`, `o.n++`).
    fn modeled_member_compound(
        &mut self,
        member: &oxc_ast::ast::MemberExpression<'_>,
    ) -> Option<SliceStatement> {
        let (target, key) = self.member_write_subject(member)?;
        Some(SliceStatement::MemberWrite {
            target,
            key,
            span: self.rebase(member.span()),
            write: SliceMemberWrite::Compound,
        })
    }

    /// The shared modeling half of a whole-binding `=` write: the narrow
    /// root of an identifier target this frame models, the selected value
    /// site, and the TARGET-IDENTIFIER span the write-effect ledger
    /// matches. `None` = the write keeps its unmodeled disposition (leaf
    /// lowering in value position, the fail-closed scan at statement
    /// position) and the typed unapplied-write degradation.
    ///
    /// `definition_span` selects the value site: the RHS's own span at
    /// statement position (the planner's write machinery selects the
    /// write's value site), the WHOLE assignment expression's span in
    /// value position (the planner dispositions an assignment as one
    /// `Leaf` site — its RHS has no separate site there).
    fn modeled_assignment_parts(
        &mut self,
        assignment: &oxc_ast::ast::AssignmentExpression<'_>,
        definition_span: oxc_span::Span,
    ) -> Option<(
        SliceNarrowRoot,
        verter_semantic::analysis::flow::SkeletonExprSiteId,
        FrameSpan,
    )> {
        let identifier =
            verter_semantic::analysis::flow::assignment_target_binding(&assignment.left)?;
        let root = self.write_target_root(identifier)?;
        let definition = self.selection?.value_site(self.rebase(definition_span))?;
        Some((root, definition, self.rebase(identifier.span)))
    }

    /// The narrow root of a whole-binding write target this frame models:
    /// a parameter or a modelable local (captured ones included).
    fn write_target_root(
        &mut self,
        identifier: &oxc_ast::ast::IdentifierReference<'_>,
    ) -> Option<SliceNarrowRoot> {
        let name = identifier.name.as_str();
        match self.classify_occurrence(identifier.span) {
            NameBinding::Param(ordinal) => self.narrow_root(name, identifier.span, Some(ordinal)),
            NameBinding::Local(_) | NameBinding::Captured => {
                self.narrow_root(name, identifier.span, None)
            }
            NameBinding::Free | NameBinding::NestedFunction | NameBinding::Unmodeled => None,
        }
    }

    /// Lower a modeled write's right-hand side — shared by the statement
    /// and expression twins, so the two positions can never diverge on
    /// what the written value is.
    fn lowered_assignment_rhs(
        &mut self,
        assignment: &oxc_ast::ast::AssignmentExpression<'_>,
    ) -> SliceExpr {
        // A class expression assigned to a binding is named after it
        // (`C = class {}` is the checker's `C`).
        if let (Some(identifier), Expression::ClassExpression(class)) = (
            verter_semantic::analysis::flow::assignment_target_binding(&assignment.left),
            &assignment.right,
        ) {
            return self.lower_class_expression(class, Some(identifier.name.as_str()));
        }
        self.lower_expr(
            &assignment.right,
            ExprMode::BindingInit {
                // Preserve the RHS until the evaluator can reduce it
                // against the target's authored declared type. When the
                // target has no declared authority the evaluator widens
                // exactly the FRESH positions, directed by the
                // `freshness` mirror the callers carry.
                preserve_literal: true,
            },
        )
    }

    /// The modeled whole-binding-write VALUE form (an assignment in
    /// expression position). `site_span` is the span of the WHOLE value
    /// expression the planner tracked (a paren-wrapped assignment keys
    /// its site at the wrapper's span; the assignment's own span is the
    /// fallback). Same conditions as the statement twin; when they do not
    /// hold the caller keeps the leaf lowering and the typed
    /// unapplied-write degradation.
    fn modeled_assignment_expression(
        &mut self,
        assignment: &oxc_ast::ast::AssignmentExpression<'_>,
        site_span: oxc_span::Span,
    ) -> Option<SliceExpr> {
        let (root, definition, span) = self
            .modeled_assignment_parts(assignment, site_span)
            .or_else(|| self.modeled_assignment_parts(assignment, assignment.span()))
            // An assignment operand of a logical expression has no site of
            // its own: it belongs to the logical expression's value site.
            .or_else(|| {
                let sites = self.logical_value_sites.clone();
                sites
                    .iter()
                    .rev()
                    .find_map(|span| self.modeled_assignment_parts(assignment, *span))
            })?;
        // `(a = [])` starts a new EVOLVING array.
        if evolving_reset_assignment(assignment) {
            if let Some(identifier) =
                verter_semantic::analysis::flow::assignment_target_binding(&assignment.left)
            {
                if let Some(binding) = self.evolving_binding_at(identifier.span) {
                    let value = Box::new(self.lowered_assignment_rhs(assignment));
                    return Some(SliceExpr::EvolvingArray(Box::new(SliceEvolvingOperation {
                        binding,
                        kind: SliceEvolvingOperationKind::Reset {
                            definition,
                            span,
                            value,
                        },
                        span,
                    })));
                }
            }
        }
        let value = self.lowered_assignment_rhs(assignment);
        Some(SliceExpr::Assignment {
            definition,
            target: SliceNarrowSubject {
                root,
                path: Arc::from(Vec::new().into_boxed_slice()),
            },
            span,
            value: Box::new(value),
            freshness: expression_freshness(&assignment.right),
            widen: false,
        })
    }

    /// The modeled whole-binding write a `void (x = v)` discards, lowered
    /// as the [`SliceExpr::Assignment`] the evaluator applies in
    /// evaluation order. The write's value site is its right-hand side's,
    /// exactly as at statement position — the `void` site's value is not
    /// the written one.
    fn modeled_void_write(&mut self, expression: &Expression<'_>) -> Option<SliceExpr> {
        let assignment = void_write_assignment(expression)?;
        self.modeled_assignment_expression(assignment, assignment.right.span())
    }

    /// A logical expression in VALUE position ([`SliceExpr::Logical`]):
    /// the left operand, the narrowing its edges establish, and the right
    /// operand lowered under the edge that runs it — a closure created
    /// there captures the guarded reading.
    fn lower_logical_value(
        &mut self,
        logical: &oxc_ast::ast::LogicalExpression<'_>,
        mode: ExprMode,
    ) -> SliceExpr {
        // A left whose truthiness is decided by which of its OWN paths ran
        // (a logical, a conditional) holding a write: the edges out of it
        // carry only the writes of the paths reaching them, which its joined
        // value followed by its guard cannot express.
        if discarded_value_holds_write(&logical.left)
            && matches!(
                unwrap_parenthesized(&logical.left),
                Expression::LogicalExpression(_) | Expression::ConditionalExpression(_)
            )
        {
            self.control_test_gap = true;
        }
        self.logical_value_sites.push(logical.span);
        let left = self.lower_expr(&logical.left, mode);
        let operator = match logical.operator {
            LogicalOperator::And => SliceLogical::And,
            LogicalOperator::Or => SliceLogical::Or,
            LogicalOperator::Coalesce => SliceLogical::Coalesce,
        };
        let right_reachable = match (&logical.left, operator) {
            (Expression::BooleanLiteral(literal), SliceLogical::And) => Some(literal.value),
            (Expression::BooleanLiteral(literal), SliceLogical::Or) => Some(!literal.value),
            _ => None,
        };
        let guard = match operator {
            SliceLogical::Coalesce => self.nullish_guard(&logical.left),
            SliceLogical::And | SliceLogical::Or => self.lower_guard(&logical.left),
        };
        if self.record_control_position_calls(&logical.left) {
            self.control_test_gap = true;
        }
        let active_guard_base = self.active_guard_bindings.len();
        let guard_bindings = self.guard_bindings(&guard, logical.left.span());
        self.active_guard_bindings
            .extend(guard_bindings.iter().copied());
        let right = if right_reachable == Some(false) {
            // No edge reaches the right operand: nothing in it runs.
            self.mark_unreachable_right_operand(logical);
            SliceExpr::Elided
        } else {
            self.lower_expr(&logical.right, mode)
        };
        self.active_guard_bindings.truncate(active_guard_base);
        self.logical_value_sites.pop();
        SliceExpr::Logical {
            operator,
            left: Box::new(left),
            right: Box::new(right),
            guard,
            right_reachable,
            fresh_operands: (
                expr_is_bare_literal(&logical.left),
                expr_is_bare_literal(&logical.right),
            ),
            widen: false,
        }
    }

    /// The effects of a discarded logical expression holding a write,
    /// along the checker's flow graph: the left operand runs, then the
    /// right one runs only on the edge the operator selects — the left's
    /// TRUE edge for `&&`, its FALSE edge for `||`, its NULLISH edge for
    /// `??` — each edge carrying the left's narrowing, and the edges join
    /// past the expression.
    fn lower_logical_effects(
        &mut self,
        logical: &oxc_ast::ast::LogicalExpression<'_>,
        context: DiscardedContext,
        out: &mut Vec<SliceStatement>,
    ) {
        let right = |this: &mut Self, out: &mut Vec<SliceStatement>| {
            this.lower_discarded_effects(&logical.right, context, out);
        };
        let nothing = |_: &mut Self, _: &mut Vec<SliceStatement>| {};
        self.mark_unreachable_right_operand(logical);
        match logical.operator {
            LogicalOperator::And => {
                self.lower_condition(&logical.left, &right, &nothing, context, out);
            }
            LogicalOperator::Or => {
                self.lower_condition(&logical.left, &nothing, &right, context, out);
            }
            LogicalOperator::Coalesce => {
                if self.discarded_value_holds_write(&logical.left) {
                    self.lower_discarded_effects(&logical.left, context, out);
                }
                let guard = self.nullish_guard(&logical.left);
                self.push_branch(Some(&logical.left), guard, &right, &nothing, out);
            }
        }
    }

    /// A bare keyword left operand reaches the right operand on one edge
    /// only (`false && w`, `true || w`): the right operand's writes never
    /// run, and no unapplied-write effect stands for them.
    fn mark_unreachable_right_operand(&mut self, logical: &oxc_ast::ast::LogicalExpression<'_>) {
        let Expression::BooleanLiteral(literal) = unwrap_parenthesized(&logical.left) else {
            return;
        };
        let unreachable = match logical.operator {
            LogicalOperator::And => !literal.value,
            LogicalOperator::Or => literal.value,
            LogicalOperator::Coalesce => false,
        };
        if !unreachable {
            return;
        }
        let span = self.rebase(logical.right.span());
        let dead: Vec<FrameSpan> = self
            .skeleton
            .writes
            .iter()
            .filter(|write| span.contains(write.span))
            .map(|write| write.span)
            .collect();
        self.inert_write_spans.extend(dead);
    }

    /// An `if` statement whose test holds a write (`if (c && (x = "s"))`),
    /// lowered on the checker's flow graph: the test threads as a
    /// condition ([`Self::lower_condition`]) whose true and false edges
    /// break to two labels, so the consequent's entry joins every true
    /// edge and the alternate's every false edge — each edge carrying the
    /// writes and narrowings of the operands it ran through — and a
    /// third label joins the two arms' ends:
    ///
    /// ```text
    /// end: { false: { true: { <condition> } <consequent> break end; } <alternate> }
    /// ```
    ///
    /// The labels are not identifiers, so no authored `break` names them.
    fn lower_if_with_test_writes(
        &mut self,
        if_stmt: &oxc_ast::ast::IfStatement<'_>,
    ) -> LoweredRegion {
        let at = if_stmt.span.start;
        let true_label: Arc<str> = Arc::from(format!("#if-true@{at}"));
        let false_label: Arc<str> = Arc::from(format!("#if-false@{at}"));
        let end_label: Arc<str> = Arc::from(format!("#if-end@{at}"));
        let region = |statements: Vec<SliceStatement>, falls: bool| SliceRegion {
            statements: Arc::from(statements.into_boxed_slice()),
            can_fall_through: NormalCompletion::minted(
                falls,
                CompletionConstruction::SynthesizedRegion,
            ),
        };
        let break_to = |label: &Arc<str>| {
            let label = Arc::clone(label);
            move |_: &mut Self, out: &mut Vec<SliceStatement>| {
                out.push(SliceStatement::Break {
                    target: Some(Arc::clone(&label)),
                });
            }
        };
        let on_true = break_to(&true_label);
        let on_false = break_to(&false_label);
        let outer_collect = self.condition_guard_bindings.replace(Vec::new());
        let mut condition = Vec::new();
        self.lower_condition(
            &if_stmt.test,
            &on_true,
            &on_false,
            DiscardedContext::Statement,
            &mut condition,
        );
        let guard_bindings = std::mem::replace(&mut self.condition_guard_bindings, outer_collect)
            .unwrap_or_default();
        if let Some(outer) = self.condition_guard_bindings.as_mut() {
            outer.extend(guard_bindings.iter().copied());
        }
        let mut prefix = Vec::new();
        if std::mem::take(&mut self.control_test_gap) {
            prefix.push(SliceStatement::Gap(
                crate::semantic_query::FlowGap::GuardNarrowing,
            ));
        }
        let active_guard_base = self.active_guard_bindings.len();
        self.active_guard_bindings
            .extend(guard_bindings.iter().copied());
        let consequent = self.lower_arm(&if_stmt.consequent);
        let alternate = if_stmt
            .alternate
            .as_ref()
            .map(|alternate| self.lower_arm(alternate));
        self.active_guard_bindings.truncate(active_guard_base);
        let consequent_falls = consequent
            .region
            .can_fall_through
            .reaches_end(CompletionDischarge::RegionComposition);
        let alternate_falls = alternate.as_ref().is_none_or(|alternate| {
            alternate
                .region
                .can_fall_through
                .reaches_end(CompletionDischarge::RegionComposition)
        });
        let mut may_break = consequent.may_break;
        let mut hit_unsupported = consequent.hit_unsupported;
        let true_block = SliceStatement::Labeled {
            label: true_label,
            body: Box::new(region(condition, false)),
        };
        let mut false_body = vec![true_block];
        false_body.extend(consequent.region.statements.iter().cloned());
        if consequent_falls {
            false_body.push(SliceStatement::Break {
                target: Some(Arc::clone(&end_label)),
            });
        }
        let mut end_body = vec![SliceStatement::Labeled {
            label: false_label,
            body: Box::new(region(false_body, false)),
        }];
        if let Some(alternate) = alternate {
            may_break.extend(alternate.may_break);
            hit_unsupported |= alternate.hit_unsupported;
            end_body.extend(alternate.region.statements.iter().cloned());
        }
        prefix.push(SliceStatement::Labeled {
            label: end_label,
            body: Box::new(region(end_body, alternate_falls)),
        });
        LoweredRegion {
            region: region(prefix, consequent_falls || alternate_falls),
            hit_unsupported,
            may_break,
        }
    }

    /// Lower `test` as a CONDITION whose true edge runs `on_true` and whose
    /// false edge runs `on_false`, exactly as the binder's
    /// `bindCondition` threads a condition: a `&&` / `||` test threads its
    /// right operand onto the left's selected edge (so a write in an
    /// operand runs only on the edges through it), a bare `true` /
    /// `false` keyword has one edge, and any other test runs its own
    /// effects and branches on its narrowing.
    fn lower_condition(
        &mut self,
        test: &Expression<'_>,
        on_true: &dyn Fn(&mut Self, &mut Vec<SliceStatement>),
        on_false: &dyn Fn(&mut Self, &mut Vec<SliceStatement>),
        context: DiscardedContext,
        out: &mut Vec<SliceStatement>,
    ) {
        match unwrap_parenthesized(test) {
            Expression::LogicalExpression(logical)
                if logical.operator != LogicalOperator::Coalesce =>
            {
                let and = logical.operator == LogicalOperator::And;
                // A bare keyword left reaches the right operand on one edge
                // only; the other operand's writes never run.
                self.mark_unreachable_right_operand(logical);
                let right = |this: &mut Self, out: &mut Vec<SliceStatement>| {
                    this.lower_condition(&logical.right, on_true, on_false, context, out);
                };
                if and {
                    self.lower_condition(&logical.left, &right, on_false, context, out);
                } else {
                    self.lower_condition(&logical.left, on_true, &right, context, out);
                }
            }
            Expression::BooleanLiteral(literal) => {
                if literal.value {
                    on_true(self, out);
                } else {
                    on_false(self, out);
                }
            }
            // `!t` is `t` with its edges exchanged (`bindCondition` over
            // a prefix `!`).
            Expression::UnaryExpression(unary)
                if unary.operator == oxc_ast::ast::UnaryOperator::LogicalNot
                    && self.discarded_value_holds_write(&unary.argument) =>
            {
                self.lower_condition(&unary.argument, on_false, on_true, context, out);
            }
            other => {
                if self.discarded_value_holds_write(other) {
                    self.lower_discarded_effects(other, context, out);
                }
                let guard = self.lower_guard(other);
                self.push_branch(Some(other), guard, on_true, on_false, out);
            }
        }
    }

    /// The effects of a discarded logical assignment to a binding
    /// (`x &&= v`, `x ||= v`, `x ??= v`): the write runs only on the
    /// edge the operator selects over the target's own value.
    fn lower_logical_assignment_effects(
        &mut self,
        assignment: &oxc_ast::ast::AssignmentExpression<'_>,
        expression: &Expression<'_>,
        context: DiscardedContext,
        out: &mut Vec<SliceStatement>,
    ) {
        let oxc_ast::ast::AssignmentTarget::AssignmentTargetIdentifier(identifier) =
            &assignment.left
        else {
            self.scan_discarded_effects(expression, context, out);
            return;
        };
        let name = identifier.name.as_str();
        let subject = match self.classify_occurrence(identifier.span) {
            NameBinding::Param(ordinal) => self.narrow_root(name, identifier.span, Some(ordinal)),
            NameBinding::Local(_) => self.narrow_root(name, identifier.span, None),
            _ => None,
        }
        .map(|root| SliceNarrowSubject {
            root,
            path: Arc::from(Vec::new().into_boxed_slice()),
        })
        .filter(|subject| !self.subject_root_carries_an_unmentioned_narrowing(subject));
        let guard = match subject {
            Some(subject) => match assignment.operator {
                oxc_ast::ast::AssignmentOperator::LogicalNullish => nullish_guard_of(subject),
                _ => SliceGuard::Truthy {
                    subject,
                    negated: false,
                },
            },
            None => {
                self.control_test_gap = true;
                SliceGuard::None
            }
        };
        let write = |this: &mut Self, out: &mut Vec<SliceStatement>| match this
            .modeled_assignment_statement(assignment)
        {
            Some(statement) => out.push(statement),
            None => this.lower_unmodeled_write_effects(assignment, expression, context, out),
        };
        let nothing = |_: &mut Self, _: &mut Vec<SliceStatement>| {};
        if assignment.operator == oxc_ast::ast::AssignmentOperator::LogicalOr {
            self.push_branch(None, guard, &nothing, &write, out);
        } else {
            self.push_branch(None, guard, &write, &nothing, out);
        }
    }

    /// Push one conditional edge pair over `test` — the `If` whose arms
    /// run the effects `on_true` and `on_false` produce, under the test's
    /// two readings — exactly as a conditional expression's arms lower:
    /// the test's control-position calls certify or gap, a call in it is a
    /// throw point, and a closure created in an arm captures the guarded
    /// reading.
    fn push_branch(
        &mut self,
        test: Option<&Expression<'_>>,
        guard: SliceGuard,
        on_true: &dyn Fn(&mut Self, &mut Vec<SliceStatement>),
        on_false: &dyn Fn(&mut Self, &mut Vec<SliceStatement>),
        out: &mut Vec<SliceStatement>,
    ) {
        if let Some(test) = test {
            if self.record_control_position_calls(test) {
                self.control_test_gap = true;
            }
            if verter_semantic::analysis::flow::expression_contains_call(test) {
                out.push(SliceStatement::ThrowPoint);
            }
        }
        let active_guard_base = self.active_guard_bindings.len();
        let guard_bindings = self.guard_bindings(&guard, oxc_span::Span::default());
        if let Some(collected) = self.condition_guard_bindings.as_mut() {
            collected.extend(guard_bindings.iter().copied());
        }
        self.active_guard_bindings
            .extend(guard_bindings.iter().copied());
        let mut consequent = Vec::new();
        on_true(self, &mut consequent);
        let mut alternate = Vec::new();
        on_false(self, &mut alternate);
        self.active_guard_bindings.truncate(active_guard_base);
        let region = |statements: Vec<SliceStatement>| {
            Box::new(SliceRegion {
                statements: Arc::from(statements.into_boxed_slice()),
                can_fall_through: NormalCompletion::minted(
                    true,
                    CompletionConstruction::SynthesizedRegion,
                ),
            })
        };
        out.push(SliceStatement::If {
            guard,
            consequent: region(consequent),
            alternate: Some(region(alternate)),
        });
    }

    /// The narrowing a `??` left operand's NULLISH edge establishes (its
    /// negated reading is the non-nullish edge): the reference is `null`
    /// or `undefined` there. A left that names no reference narrows
    /// nothing; one whose reference this half cannot express takes the
    /// typed gap.
    fn nullish_guard(&mut self, left: &Expression<'_>) -> SliceGuard {
        let reference = unwrap_reference_transparent(left);
        match self.narrow_subject_of(reference) {
            Some(subject) if !self.subject_root_carries_an_unmentioned_narrowing(&subject) => {
                nullish_guard_of(subject)
            }
            Some(_) => {
                self.control_test_gap = true;
                SliceGuard::None
            }
            None => match self.narrow_destination_of(reference) {
                NarrowDestination::Absent => SliceGuard::None,
                NarrowDestination::Represented | NarrowDestination::Unrepresented => {
                    self.control_test_gap = true;
                    SliceGuard::None
                }
            },
        }
    }

    /// The effects of a binding write this frame does not apply — its
    /// target unmodelled, or its value selected by no demanded read (the
    /// write-effect ledger decides whether the write itself degrades): its
    /// right-hand side still runs first, so a write the right-hand side
    /// holds (`d = (x = "s")`, `d &&= (x = "s") === "s"`) applies in
    /// order. Every other shape takes the effect scan of `context`.
    fn lower_unmodeled_write_effects(
        &mut self,
        assignment: &oxc_ast::ast::AssignmentExpression<'_>,
        expression: &Expression<'_>,
        context: DiscardedContext,
        out: &mut Vec<SliceStatement>,
    ) {
        if matches!(
            assignment.left,
            oxc_ast::ast::AssignmentTarget::AssignmentTargetIdentifier(_)
        ) && discarded_value_holds_write(&assignment.right)
        {
            self.lower_discarded_effects(&assignment.right, context, out);
        } else {
            self.scan_discarded_effects(expression, context, out);
        }
    }

    /// Lower the EFFECTS of a value nothing consumes — an expression
    /// statement, or a declarator initializer the demand did not select —
    /// into statements the evaluator applies in evaluation order, when the
    /// value holds a write ([`Self::discarded_value_holds_write`]): a
    /// whole-binding `=` write (bare or under `void`) applies as the
    /// statement write does, `a = []` starts a new EVOLVING array, a
    /// compound write or update retypes its target, an EVOLVING-array
    /// operation evolves its array, a conditional's arms — and a logical
    /// expression's conditionally evaluated right operand — apply under
    /// the test's narrowing exactly as an `if` statement's arms do, and a
    /// sequence, object literal or array literal applies its operands,
    /// member values and elements in source order. Every other position —
    /// and a write this frame cannot model — takes the effect scan of
    /// `context`, exactly as the whole value did before.
    fn lower_discarded_effects(
        &mut self,
        expression: &Expression<'_>,
        context: DiscardedContext,
        out: &mut Vec<SliceStatement>,
    ) {
        if !self.discarded_value_holds_write(expression) {
            self.scan_discarded_effects(expression, context, out);
            return;
        }
        match self.lower_evolving_operation(
            unwrap_parenthesized(expression),
            ExprMode::BindingInit {
                preserve_literal: true,
            },
        ) {
            EvolvingLowering::Operation(operation) => {
                out.push(SliceStatement::EvolvingArray(operation));
                return;
            }
            // An operation on an array the demand did not select only runs
            // its operands; a `push` / `unshift` is still a throw point.
            EvolvingLowering::Unselected => {
                if verter_semantic::analysis::flow::expression_contains_call(expression) {
                    out.push(SliceStatement::ThrowPoint);
                }
                return;
            }
            EvolvingLowering::NotEvolving => {}
        }
        match unwrap_parenthesized(expression) {
            Expression::AssignmentExpression(assignment) if assignment.operator.is_logical() => {
                self.lower_logical_assignment_effects(assignment, expression, context, out);
            }
            // A compound write to a whole binding retypes it
            // ([`Self::lower_discarded_compound_write`]).
            Expression::AssignmentExpression(assignment)
                if assignment.operator != oxc_ast::ast::AssignmentOperator::Assign
                    && verter_semantic::analysis::flow::assignment_target_binding(
                        &assignment.left,
                    )
                    .is_some() =>
            {
                self.lower_discarded_compound_write(expression, context, out);
            }
            Expression::UpdateExpression(_) => {
                self.lower_discarded_compound_write(expression, context, out);
            }
            Expression::AssignmentExpression(assignment) => {
                let modeled = if assignment.operator == oxc_ast::ast::AssignmentOperator::Assign {
                    self.evolving_reset_statement(assignment)
                        .or_else(|| self.modeled_assignment_statement(assignment))
                } else {
                    self.modeled_assignment_statement(assignment)
                };
                match modeled {
                    Some(statement) => out.push(statement),
                    None => {
                        self.lower_unmodeled_write_effects(assignment, expression, context, out)
                    }
                }
            }
            Expression::LogicalExpression(logical) => {
                self.lower_logical_effects(logical, context, out);
            }
            // An operator's operands run in source order before it.
            Expression::UnaryExpression(unary)
                if unary.operator != oxc_ast::ast::UnaryOperator::Void =>
            {
                self.lower_discarded_effects(&unary.argument, context, out);
            }
            Expression::BinaryExpression(binary) => {
                self.lower_discarded_effects(&binary.left, context, out);
                self.lower_discarded_effects(&binary.right, context, out);
            }
            // The one `void` form that holds a write: `void (x = v)`.
            // Any other `void` operand is scanned whole, under the leaf
            // scanner's `void` rule.
            void @ Expression::UnaryExpression(_) => {
                let modeled = void_write_assignment(void)
                    .and_then(|assignment| self.modeled_assignment_statement(assignment));
                match modeled {
                    Some(statement) => out.push(statement),
                    None => self.scan_discarded_effects(expression, context, out),
                }
            }
            Expression::ConditionalExpression(conditional) => {
                let branch = self.lower_discarded_branch(
                    Some(&conditional.test),
                    Some(&conditional.consequent),
                    Some(&conditional.alternate),
                    context,
                    out,
                );
                out.push(branch);
            }
            Expression::SequenceExpression(sequence) => {
                for operand in &sequence.expressions {
                    // An unparenthesized call operand is entered into
                    // control flow wherever the sequence sits, exactly as a
                    // statement's own call is.
                    let context = if matches!(operand, Expression::CallExpression(_)) {
                        DiscardedContext::Statement
                    } else {
                        context
                    };
                    self.lower_discarded_effects(operand, context, out);
                }
            }
            Expression::ObjectExpression(object) => {
                for property in &object.properties {
                    match property {
                        oxc_ast::ast::ObjectPropertyKind::ObjectProperty(property) => {
                            if property.computed {
                                if let Some(key) = property.key.as_expression() {
                                    self.scan_discarded_effects(key, context, out);
                                }
                            }
                            // A method or accessor value is a function:
                            // creating it runs nothing.
                            if property.kind == oxc_ast::ast::PropertyKind::Init && !property.method
                            {
                                self.lower_discarded_effects(&property.value, context, out);
                            }
                        }
                        oxc_ast::ast::ObjectPropertyKind::SpreadProperty(spread) => {
                            self.scan_discarded_effects(&spread.argument, context, out);
                        }
                    }
                }
            }
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    match element {
                        oxc_ast::ast::ArrayExpressionElement::SpreadElement(spread) => {
                            self.scan_discarded_effects(&spread.argument, context, out);
                        }
                        oxc_ast::ast::ArrayExpressionElement::Elision(_) => {}
                        other => {
                            if let Some(value) = other.as_expression() {
                                self.lower_discarded_effects(value, context, out);
                            }
                        }
                    }
                }
            }
            other => self.scan_discarded_effects(other, context, out),
        }
    }

    /// The conditionally evaluated positions of a discarded value — a
    /// conditional's arms, a logical expression's right operand — lowered
    /// as the `if` the checker's flow branches into: the test's calls and
    /// updates run first, its guard narrows each arm, each arm applies its
    /// discarded writes, and the paths join after it. A missing arm is the
    /// path that skips the position; no test is a branch no guard narrows.
    fn lower_discarded_branch(
        &mut self,
        test: Option<&Expression<'_>>,
        consequent: Option<&Expression<'_>>,
        alternate: Option<&Expression<'_>>,
        context: DiscardedContext,
        out: &mut Vec<SliceStatement>,
    ) -> SliceStatement {
        let mut guard = SliceGuard::None;
        let mut guard_bindings = Vec::new();
        if let Some(test) = test {
            guard = self.lower_guard(test);
            if self.record_control_position_calls(test) {
                self.control_test_gap = true;
            }
            if verter_semantic::analysis::flow::expression_contains_call(test) {
                out.push(SliceStatement::ThrowPoint);
            }
            self.lower_test_updates(test, out);
            guard_bindings = self.guard_bindings(&guard, test.span());
        }
        // The arms are GUARDED exactly as an `if` statement's are: a
        // closure created inside one captures the guarded reading.
        let active_guard_base = self.active_guard_bindings.len();
        self.active_guard_bindings
            .extend(guard_bindings.iter().copied());
        let lower_arm = |this: &mut Self, arm: Option<&Expression<'_>>| {
            let mut statements = Vec::new();
            if let Some(arm) = arm {
                this.lower_discarded_effects(arm, context, &mut statements);
            }
            Box::new(SliceRegion {
                statements: Arc::from(statements.into_boxed_slice()),
                can_fall_through: NormalCompletion::minted(
                    true,
                    CompletionConstruction::SynthesizedRegion,
                ),
            })
        };
        let consequent = lower_arm(self, consequent);
        let alternate = lower_arm(self, alternate);
        self.active_guard_bindings.truncate(active_guard_base);
        SliceStatement::If {
            guard,
            consequent,
            alternate: Some(alternate),
        }
    }

    /// The effect scan a discarded position of `context` takes: an
    /// expression statement's ([`Self::scan_unmodeled_statement_effects`],
    /// which also marks its throw point) or an unselected initializer's
    /// ([`Self::scan_unmodeled_position_effects`]).
    fn scan_discarded_effects(
        &mut self,
        expression: &Expression<'_>,
        context: DiscardedContext,
        out: &mut Vec<SliceStatement>,
    ) {
        match context {
            DiscardedContext::Statement => {
                out.extend(self.scan_unmodeled_statement_effects(expression));
            }
            // An entered `asserts` call narrows once the position has run.
            DiscardedContext::Initializer => {
                let entered = self.collecting_entered_assertions(|this| {
                    this.scan_unmodeled_position_effects(expression)
                });
                out.extend(entered);
            }
        }
    }

    /// Scan one UNMODELED expression statement's effects: the statement's
    /// value is discarded, so a call certifies unless it could carry an
    /// `asserts` narrowing of a frame-owned binding
    /// ([`CertificationMode::ValueFree`]), and only SKELETON-HIDDEN writes
    /// (a class subtree) gap — a visible write already rides the typed
    /// unapplied-write ledger. The throw-point marker is unchanged: a call
    /// nested anywhere in the statement executes — and can throw —
    /// whether or not its value is consumed.
    fn scan_unmodeled_statement_effects(
        &mut self,
        expression: &Expression<'_>,
    ) -> Option<SliceStatement> {
        let mut scanner = LeafCallScanner::default();
        if let Expression::CallExpression(call) = expression {
            scanner.statement_calls.insert(call.span);
        }
        scanner.visit_expression(expression);
        // An entered `asserts` call (a comma operand anywhere in the
        // statement) narrows once the statement has run: its value is
        // discarded, so nothing the statement lowers reads past it.
        let entered = self.collecting_entered_assertions(|this| {
            if this.drain_scanned_same_frame_effects(
                scanner,
                CertificationMode::ValueFree,
                WritePolicy::SkeletonHiddenOnly,
            ) {
                this.control_test_gap = true;
            }
        });
        let throw_point = verter_semantic::analysis::flow::expression_contains_call(expression)
            .then_some(SliceStatement::ThrowPoint);
        if entered.is_empty() {
            return throw_point;
        }
        let mut statements: Vec<SliceStatement> = throw_point.into_iter().collect();
        statements.extend(entered);
        Some(sequential_block(statements))
    }

    /// The call carrier of one call expression that is not an immediately
    /// invoked function — its callee resolved through the frame's one
    /// lexical binding authority, then the file-level callee rails. Its
    /// frame-lowered arguments are attached by [`Self::with_call_arguments`].
    fn lower_call_expression(
        &mut self,
        expr: &Expression<'_>,
        call: &oxc_ast::ast::CallExpression<'_>,
        mode: ExprMode,
    ) -> SliceExpr {
        if let Expression::Identifier(callee) = &call.callee {
            let name = callee.name.as_str();
            // ONE lexical binding authority (the frame's
            // skeleton), then the file-level callee rails.
            match self.classify_occurrence(callee.span) {
                // A local function declaration — this frame's or an
                // enclosing one's — is called as the value it declares.
                NameBinding::NestedFunction | NameBinding::Unmodeled => {
                    return match self.lower_local_function_declaration(callee.span) {
                        Some(function) => SliceExpr::Call(
                            SliceCall::Nested(Box::new(function)),
                            call_site(call),
                            SliceCallArguments::none(),
                        ),
                        None if matches!(
                            self.classify_occurrence(callee.span),
                            NameBinding::NestedFunction
                        ) =>
                        {
                            SliceExpr::Call(
                                SliceCall::LocalFunctionShadow,
                                call_site(call),
                                SliceCallArguments::none(),
                            )
                        }
                        None => SliceExpr::UnmodeledBinding,
                    };
                }
                // A parameter or local SHADOWS the file-level
                // declaration: the call goes through the binding's
                // signature, never a flow obligation edge.
                NameBinding::Param(ordinal) => {
                    return SliceExpr::Call(
                        SliceCall::OnBinding {
                            binding: match self.binding_at(callee.span) {
                                Some(binding) => binding,
                                None => return SliceExpr::UnmodeledBinding,
                            },
                            param: Some(ordinal),
                            name: Arc::from(name),
                            captured: false,
                        },
                        call_site(call),
                        SliceCallArguments::none(),
                    )
                }
                NameBinding::Local(param) => {
                    return SliceExpr::Call(
                        SliceCall::OnBinding {
                            binding: match self.binding_at(callee.span) {
                                Some(binding) => binding,
                                None => return SliceExpr::UnmodeledBinding,
                            },
                            param,
                            name: Arc::from(name),
                            captured: false,
                        },
                        call_site(call),
                        SliceCallArguments::none(),
                    )
                }
                NameBinding::Captured => {
                    return SliceExpr::Call(
                        SliceCall::OnBinding {
                            binding: match self.binding_at(callee.span) {
                                Some(binding) => binding,
                                None => return SliceExpr::UnmodeledBinding,
                            },
                            param: None,
                            name: Arc::from(name),
                            captured: true,
                        },
                        call_site(call),
                        SliceCallArguments::none(),
                    )
                }
                NameBinding::Free => {}
            }
            // A bare-identifier call to the function itself — a
            // direct same-slot recursion hold.
            if Some(name) == self.self_name {
                return SliceExpr::Call(
                    SliceCall::DirectSelf,
                    call_site(call),
                    SliceCallArguments::none(),
                );
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
                    SliceCallArguments::none(),
                );
            }
        }
        // A `this.m()` callee: the member of the frame's receiver —
        // an object literal's own method is a direct call of it.
        if let (Some(this), Expression::StaticMemberExpression(member)) =
            (self.this.clone(), unwrap_parenthesized(&call.callee))
        {
            if let (SliceThis::Value { .. } | SliceThis::Static { .. }, Some([name])) =
                (&this, this_member_path(member).as_deref())
            {
                return match self.object_this_member(name) {
                    Some(ObjectThisMember::Method(target)) => SliceExpr::Call(
                        SliceCall::Direct(target),
                        call_site(call),
                        SliceCallArguments::none(),
                    ),
                    // A static the class inherits (or declares without a
                    // body) is called off its constructor.
                    _ if matches!(this, SliceThis::Static { .. }) => SliceExpr::Call(
                        SliceCall::Member {
                            receiver: Box::new(SliceExpr::This(this.clone())),
                            member: Arc::from([Arc::clone(name)]),
                        },
                        call_site(call),
                        SliceCallArguments::none(),
                    ),
                    _ => SliceExpr::Gap(crate::semantic_query::FlowGap::UnmodeledExpression),
                };
            }
            if let Some(path) = this_member_path(member) {
                return SliceExpr::Call(
                    SliceCall::Member {
                        receiver: Box::new(SliceExpr::This(this)),
                        member: Arc::from(path.into_boxed_slice()),
                    },
                    call_site(call),
                    SliceCallArguments::none(),
                );
            }
        }
        // A member call on a constructed value or an object
        // literal: the object is a flow value, never a leaf answer.
        if let Expression::StaticMemberExpression(member) = unwrap_parenthesized(&call.callee) {
            if value_rooted_member_object(&member.object) {
                self.open_value_rooted_reads += 1;
                let object = self.lower_expr(&member.object, mode);
                self.open_value_rooted_reads -= 1;
                return SliceExpr::Call(
                    SliceCall::OnValue {
                        object: Box::new(object),
                        member: Arc::from(member.property.name.as_str()),
                    },
                    call_site(call),
                    SliceCallArguments::none(),
                );
            }
        }
        // A `super.m()` callee root: the base member resolves
        // through the heritage surface. A heritage this half
        // cannot lower keeps the rail below.
        if let Some(member) = super_callee_static_path(&call.callee) {
            if let Some(carrier) = self.lower_super_call_on_heritage(&member, call, mode) {
                return carrier;
            }
        }
        // The SAME root-identifier gate the leaf path takes: a
        // non-identifier callee rooted at a frame binding
        // (`localObj.m()`) resolves in owner scope exactly like a
        // bare read would, so it is gated here too.
        match self.leaf_type(expr, mode) {
            LeafLowering::Unmodeled => SliceExpr::UnreducedCallValue,
            // The callee could not be represented at all (an
            // `obj[k]()` computed-member callee, say): the leaf
            // answered a bare `any`. This IS a call with no
            // structural arm, so it takes the same fail-closed
            // verdict the classifier gives every other one —
            // publishing the `any` was a fabricated value at a
            // call position, warm and clean.
            LeafLowering::Free(ty) if is_any(&ty) => SliceExpr::UnreducedCallValue,
            LeafLowering::Free(ty) => SliceExpr::Call(
                SliceCall::Symbolic(ty.clone(), self.frame_root_for_type(&ty, expr)),
                call_site(call),
                SliceCallArguments::none(),
            ),
            LeafLowering::FrameShadowed { ty, shadowed } => SliceExpr::FrameShadowed {
                inner: Box::new(SliceExpr::Call(
                    SliceCall::Symbolic(ty.clone(), self.frame_root_for_type(&ty, expr)),
                    call_site(call),
                    SliceCallArguments::none(),
                )),
                shadowed,
            },
        }
    }

    /// Attach `arguments`' frame-lowered calls to the call carrier
    /// `lowered` (through a frame-shadow wrapper). A position that lowered
    /// to anything but a call carrier has no call sink to read them.
    fn with_call_arguments(
        &mut self,
        lowered: SliceExpr,
        arguments: &[oxc_ast::ast::Argument<'_>],
        mode: ExprMode,
    ) -> SliceExpr {
        match lowered {
            SliceExpr::Call(call, site, _) => {
                SliceExpr::Call(call, site, self.lower_call_arguments(arguments, mode))
            }
            SliceExpr::FrameShadowed { inner, shadowed } => SliceExpr::FrameShadowed {
                inner: Box::new(self.with_call_arguments(*inner, arguments, mode)),
                shadowed,
            },
            lowered => lowered,
        }
    }

    /// The [`SliceCallArguments`] of one call's `arguments`: each argument
    /// that is itself a call (through parentheses), or a static member
    /// read (`o.a`), lowered through this frame's own carriers, so the
    /// bindings it reads are the frame's. An immediately invoked function,
    /// a spread element and every other argument stay with the call sink's
    /// own reading — a bare binding read already reads the frame.
    fn lower_call_arguments(
        &mut self,
        arguments: &[oxc_ast::ast::Argument<'_>],
        mode: ExprMode,
    ) -> SliceCallArguments {
        let in_frame = |argument: &oxc_ast::ast::Argument<'_>| {
            argument
                .as_expression()
                .map(unwrap_parenthesized)
                .is_some_and(|argument| match argument {
                    Expression::CallExpression(call) => !matches!(
                        unwrap_parenthesized(&call.callee),
                        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)
                    ),
                    Expression::StaticMemberExpression(_) => true,
                    _ => false,
                })
        };
        if !arguments.iter().any(in_frame) {
            return SliceCallArguments::none();
        }
        let lowered: Vec<Option<SliceExpr>> = arguments
            .iter()
            .map(|argument| {
                in_frame(argument).then(|| {
                    let expression = unwrap_parenthesized(
                        argument
                            .as_expression()
                            .expect("an in-frame argument is an expression"),
                    );
                    self.lower_expr(expression, mode)
                })
            })
            .collect();
        SliceCallArguments(Arc::from(lowered.into_boxed_slice()))
    }

    /// Lower one expression. Parameter and in-scope local identifiers
    /// become dedicated carriers, a nested function value becomes its own
    /// frame, and a bare-identifier call to the function itself becomes
    /// the recursion hold — three frame-local carriers this half mints
    /// and the demand planner has no descent into.
    ///
    /// Every OTHER form takes the disposition the shared classifier
    /// `verter_semantic::analysis::flow::value_descent` assigns it, which
    /// is the same verdict the skeleton's `open_site` descends on: a
    /// plain string-keyed object literal lowers STRUCTURALLY (each member
    /// value is a flow expression, gated by the demand selection), a
    /// conditional joins its branch values, a type carrier and a leaf
    /// take the shared shallow-pass per-expression lowering for the
    /// position.
    fn lower_expr(&mut self, expr: &Expression<'_>, mode: ExprMode) -> SliceExpr {
        match self.lower_evolving_operation(expr, mode) {
            EvolvingLowering::Operation(operation) => {
                return SliceExpr::EvolvingArray(Box::new(operation))
            }
            // The operation's value without its binding: a `push` /
            // `unshift` is the new length; an element write is its
            // assigned value, which the demand did not select.
            EvolvingLowering::Unselected => {
                return match expr {
                    Expression::AssignmentExpression(_) => SliceExpr::Elided,
                    _ => {
                        SliceExpr::Type(GatedLeaf(TypeExpr::Primitive(PrimitiveName::Number), None))
                    }
                }
            }
            EvolvingLowering::NotEvolving => {}
        }
        match expr {
            Expression::Identifier(identifier) => self.lower_identifier_read(identifier, mode),
            Expression::ChainExpression(chain) => {
                // A MEMBER-valued chain (`maybeObj?.b`) is a typed optional
                // member read over a non-call base: it publishes
                // `T | undefined` through the shared path walk, not the
                // still-`any` optional-call rail below.
                if !verter_semantic::analysis::flow::chain_is_call_valued(&chain.expression) {
                    if let Some(member) = self.lower_optional_member_chain(chain, mode) {
                        return member;
                    }
                }
                match pure_optional_chain_root_identifier(&chain.expression) {
                    Some(root) if self.optional_chain_root_has_prior_flow_change(root) => {
                        SliceExpr::Gap(crate::semantic_query::FlowGap::UnmodeledExpression)
                    }
                    Some(root) => {
                        // The chain's value derives from the ROOT's
                        // `any`-ness (the evaluator admits it only while
                        // the reaching root is still `any`), never from
                        // resolving the chain's terminal call: decided
                        // above. A non-`any` root degrades at evaluation,
                        // which blocks the seal regardless.
                        self.record_optional_any_chain_calls(expr, chain);
                        SliceExpr::OptionalAnyChain {
                            root: Box::new(self.lower_identifier_read(root, mode)),
                        }
                    }
                    None => self.lower_leaf(expr, mode),
                }
            }
            Expression::ThisExpression(_) if self.this.is_some() => {
                SliceExpr::This(self.this.clone().expect("guarded"))
            }
            // A member read off an object literal's `this` lowers from the
            // member the literal declares.
            Expression::StaticMemberExpression(member)
                if matches!(
                    self.this,
                    Some(SliceThis::Value { .. } | SliceThis::Static { .. })
                ) && this_member_path(member).is_some() =>
            {
                let path = this_member_path(member).expect("guarded");
                self.lower_object_this_read(&path, member.span, mode)
            }
            // A member read off the receiver (`this.v`, `this.a.b`) projects
            // through the same member-path walk an optional chain takes.
            Expression::StaticMemberExpression(member)
                if self.this.is_some() && this_member_path(member).is_some() =>
            {
                let path = this_member_path(member).expect("guarded");
                SliceExpr::OptionalMember {
                    root: Box::new(SliceExpr::This(self.this.clone().expect("guarded"))),
                    links: path.into_iter().map(|name| (name, false)).collect(),
                }
            }
            // An element read at an integer literal position off a
            // parameter or local reference (`a[0]`, `o.xs[1]`) reads that
            // key of the reference's type through the member-path walk a
            // static member read takes.
            Expression::ComputedMemberExpression(member)
                if element_read_path(member).is_some_and(|(root, _)| {
                    matches!(
                        self.classify_occurrence(root.span),
                        NameBinding::Param(_) | NameBinding::Local(_) | NameBinding::Captured
                    )
                }) =>
            {
                let (root, links) = element_read_path(member).expect("guarded");
                SliceExpr::OptionalMember {
                    root: Box::new(self.lower_identifier_read(root, mode)),
                    links: links.into_iter().map(|name| (name, false)).collect(),
                }
            }
            Expression::FunctionExpression(func) => {
                self.lower_nested_function(&FunctionNode::Function(func))
            }
            Expression::ArrowFunctionExpression(arrow) => {
                self.lower_nested_function(&FunctionNode::Arrow(arrow))
            }
            Expression::ClassExpression(class) => self.lower_class_expression(class, None),
            // An `await x`: the operand lowers through its own arm (a
            // call operand rides the one call carrier, a binding read the
            // binding carriers) and the evaluator unwraps the resolved
            // value through the lib `Awaited` surface. Taken BEFORE the
            // classifier dispatch, exactly like the call arms — the
            // classifier's [`ValueDescent::Awaited`] verdict descends both
            // halves onto the operand, so neither half can fail the await
            // closed as an unmodeled-call position.
            Expression::AwaitExpression(awaited) => SliceExpr::Awaited {
                operand: Box::new(self.lower_expr(&awaited.argument, mode)),
            },
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
                        self.lower_function_value(&FunctionNode::Function(func), Some(call))
                    }
                    Expression::ArrowFunctionExpression(arrow) => {
                        self.lower_function_value(&FunctionNode::Arrow(arrow), Some(call))
                    }
                    _ => unreachable!("the guard admits function values only"),
                };
                SliceExpr::Call(
                    SliceCall::Nested(Box::new(function)),
                    call_site(call),
                    SliceCallArguments::none(),
                )
            }
            // A `new` expression: the constructor is a flow value of this
            // frame, and the construction resolves at the evaluator's one
            // call sink. Its arguments are parse facts the sink re-reads
            // from the retained snapshot, exactly like a call's, except
            // the ones that are calls themselves.
            Expression::NewExpression(new) => {
                let constructor = self.lower_expr(&new.callee, mode);
                SliceExpr::Call(
                    SliceCall::Construct(Box::new(constructor)),
                    construct_site(new),
                    self.lower_call_arguments(&new.arguments, mode),
                )
            }
            // A tagged template calls its tag: the tag is a flow value of
            // this frame, and the call resolves at the same sink, with the
            // template strings as its first argument.
            Expression::TaggedTemplateExpression(tagged) => SliceExpr::Call(
                SliceCall::TaggedTemplate(Box::new(self.lower_expr(&tagged.tag, mode))),
                tagged_template_site(tagged),
                SliceCallArguments::none(),
            ),
            Expression::CallExpression(call) => {
                self.record_call_arguments(call);
                let lowered = self.lower_call_expression(expr, call, mode);
                self.with_call_arguments(lowered, &call.arguments, mode)
            }
            // A member read off a constructed value or an object literal
            // (`new C().p`, `({ a: 1 }).a`, `({ o: { x: 1 } }).o.x`).
            Expression::StaticMemberExpression(member)
                if value_rooted_member_object(&member.object) =>
            {
                self.open_value_rooted_reads += 1;
                let object = self.lower_expr(&member.object, mode);
                self.open_value_rooted_reads -= 1;
                SliceExpr::MemberOf {
                    object: Box::new(object),
                    member: Arc::from(member.property.name.as_str()),
                }
            }
            // An element access whose every key is a literal (`a["k"]`,
            // `t[1]`, `o.p["q"]`) is the member reference its names spell:
            // rooted at a frame binding, it reads exactly as the dotted
            // spelling does — through the frame's substitution and every
            // narrowing standing at the path.
            Expression::ComputedMemberExpression(_) if literal_member_chain(expr).is_some() => {
                match self.lower_literal_member_read(expr) {
                    Some(read) => read,
                    None => self.lower_leaf(expr, mode),
                }
            }
            // A sequence whose LAST operand is its structural value
            // provider: a NARROWABLE REFERENCE (`(touch(), u)`) or a CALL
            // routed to the structural call rails (`(0, f())` is `f()`'s
            // value). The sequence's VALUE is that operand — the earlier
            // operands' evaluation effects ride the slice's typed effect
            // obligations regardless. Lowering it as the operand keeps
            // the frame's substitutions (the narrowing overlay above
            // all) visible at the read, and routes the call through the
            // same rails its bare spelling takes; folding the whole
            // sequence through the leaf lowering answered a fabricated
            // `any` for it, clean and warm. Any other sequence keeps the
            // leaf lowering.
            //
            // A DISCARDED operand's call still RUNS, and an assertion
            // call among them narrows the read that follows
            // (`(assertString(x), x)` is `string`). Such a call is
            // certified decided-above only when it provably establishes
            // no narrowing; every other one flags the enclosing
            // statement's guard-narrowing gap. The routed call answers
            // its own question through the rails — a callee they cannot
            // represent keeps the positional fail-closed marker, so the
            // sequence context invents no arm.
            Expression::SequenceExpression(sequence)
                if sequence
                    .expressions
                    .last()
                    .is_some_and(|last| self.sequence_value_lowers_structurally(last)) =>
            {
                let last = sequence
                    .expressions
                    .last()
                    .expect("the guard proved a last operand");
                // Every unparenthesized call operand is entered into
                // control flow: an `asserts` call among the discarded
                // operands narrows ahead of the value, and one that IS the
                // value operand narrows once it has evaluated (`const y =
                // (0, assertString(x))` narrows `x`).
                let mut before = Vec::new();
                for discarded in &sequence.expressions[..sequence.expressions.len() - 1] {
                    let entered = self.collecting_entered_assertions(|this| {
                        if this.record_discarded_operand_calls(discarded) {
                            this.control_test_gap = true;
                        }
                    });
                    before.extend(entered);
                }
                let after = match last {
                    Expression::CallExpression(call) => {
                        let assertion = self.entered_assertion(call);
                        // An entered call this half cannot apply keeps the
                        // typed gap.
                        if assertion.is_none() && self.call_may_assert_a_frame_binding(call) {
                            self.control_test_gap = true;
                        }
                        assertion
                    }
                    _ => None,
                };
                let value = self.lower_expr(last, mode);
                if before.is_empty() && after.is_none() {
                    value
                } else {
                    SliceExpr::Sequence {
                        before: Arc::from(before.into_boxed_slice()),
                        value: Box::new(value),
                        after,
                    }
                }
            }
            // ── THE shared value-structural descent ──────────────────
            //
            // Every remaining form takes the disposition the ONE shared
            // classifier assigns it (`verter_semantic::analysis::flow::
            // value_descent`), which is the SAME verdict the skeleton's
            // `open_site` descends on. Neither half carries a wildcard
            // over `Expression`: the exhaustive match lives in the
            // classifier, so a new variant does not compile until it is
            // dispositioned there — and both halves inherit that
            // disposition in the same change.
            //
            // The arms above (identifier / nested function value / call)
            // are `Leaf` to the classifier: they have no
            // value-contributing sub-expression the demand plan must
            // reach, only a frame-local carrier this half mints.
            other => {
                // A whole-binding `=` write in VALUE position — the
                // expression twin of [`SliceStatement::Assignment`] — is
                // applied by the evaluator IN EVALUATION ORDER (a read
                // before it keeps the pre-write reaching definition, a
                // read after it and a DEFERRED closure read observe the
                // write). Paren wrappers are unwrapped HERE rather than
                // through the shared `Transparent` descent so the
                // planner-tracked site span (the OUTERMOST wrapper's) is
                // still in hand at the probe. An unmodeled target shape
                // falls through to the leaf lowering below and keeps the
                // typed unapplied-write degradation, exactly as before.
                let mut unwrapped = other;
                while let Expression::ParenthesizedExpression(paren) = unwrapped {
                    unwrapped = &paren.expression;
                }
                // `void (x = v)` in value position: the write applies in
                // evaluation order exactly as the bare write does, and the
                // expression answers `undefined`.
                if let Some(write) = self.modeled_void_write(unwrapped) {
                    return SliceExpr::Void {
                        operand: Box::new(write),
                        value: Box::new(SliceExpr::Type(GatedLeaf(
                            widening_nullish_type(unwrapped),
                            None,
                        ))),
                    };
                }
                if let Some(operation) = self.lower_operator_form(unwrapped, mode) {
                    return operation;
                }
                if let Expression::AssignmentExpression(assignment) = unwrapped {
                    if matches!(
                        assignment.operator,
                        oxc_ast::ast::AssignmentOperator::Assign
                    ) {
                        let site_span = other.span();
                        if let Some(modeled) =
                            self.modeled_assignment_expression(assignment, site_span)
                        {
                            return modeled;
                        }
                    }
                }
                // A static member read off a CALL's value (`f(c).a.b`): the
                // call rides the one call sink and each member is read off
                // its value through the shared path walk, as an optional
                // member chain reads its links.
                if let Some((call, links)) = call_rooted_member_path(unwrapped) {
                    return SliceExpr::OptionalMember {
                        root: Box::new(self.lower_expr(call, mode)),
                        links: Arc::from(
                            links
                                .into_iter()
                                .map(|name| (name, false))
                                .collect::<Vec<_>>()
                                .into_boxed_slice(),
                        ),
                    };
                }
                match value_descent(other) {
                    ValueDescent::Transparent(inner) => self.lower_expr(inner, mode),
                    // Unreachable in practice — the await arm above takes
                    // `Expression::AwaitExpression` before the classifier
                    // dispatch — but the classifier's verdict is the
                    // authority: an await lowers its OPERAND through its
                    // own arm and the evaluator unwraps through `Awaited`.
                    ValueDescent::Awaited(awaited) => SliceExpr::Awaited {
                        operand: Box::new(self.lower_expr(&awaited.argument, mode)),
                    },
                    // A TYPE carrier decides the published type (`x as
                    // const` pins what a bare literal would widen). That is
                    // a statement about the MEMBER POLICY, not a reason to
                    // abandon the structural lowering: folding the carrier
                    // into one leaf answer takes every sibling with it, and
                    // a leaf answer over a CALL-sourced spread embeds the
                    // callee's unreduced `ReturnType<…>` carrier, which the
                    // fabricated-value gate refuses. `{ ...base(), n: 1 } as
                    // const` failed its whole return closed for a value the
                    // checker calls `{ readonly label: string; readonly n: 1
                    // }`.
                    //
                    // So a carrier over an OBJECT LITERAL lowers the literal
                    // structurally under the carrier's own member policy,
                    // and every other carrier keeps the whole-carrier leaf
                    // lowering (its type is genuinely the carrier's, not its
                    // operand's).
                    //
                    // An ARRAY literal lowers structurally under the same
                    // policy: its const tuple under `as const`.
                    //
                    // `satisfies` over an object or array literal keeps
                    // the literal's own lowering and carries its target,
                    // which contextually types the literal at evaluation.
                    ValueDescent::TypeCarrier(inner)
                        if matches!(
                            value_descent(unwrap_parenthesized(inner)),
                            ValueDescent::Object(_) | ValueDescent::Array(_)
                        ) && satisfies_target(other).is_some() =>
                    {
                        let target = satisfies_target(other).expect("checked above");
                        SliceExpr::Satisfies {
                            // The operand keeps its fresh member and
                            // element views whatever the position: the
                            // target decides which of them survive.
                            operand: Box::new(self.lower_expr(
                                inner,
                                ExprMode::BindingInit {
                                    preserve_literal: true,
                                },
                            )),
                            target: self.gate(
                                lower_ts_type(&target.type_annotation, self.source),
                                target.span,
                                &[],
                            ),
                        }
                    }
                    ValueDescent::TypeCarrier(inner) => {
                        match member_literal_policy(other, self.source) {
                            Some(policy) => match value_descent(inner) {
                                ValueDescent::Object(object) => self
                                    .lower_object_literal_with_policy(object, other, mode, policy),
                                ValueDescent::Array(array) => {
                                    self.lower_array_literal(array, policy)
                                }
                                _ => self.lower_leaf(other, mode),
                            },
                            None => self.lower_leaf(other, mode),
                        }
                    }
                    ValueDescent::Object(object) => self.lower_object_literal_with_policy(
                        object,
                        other,
                        mode,
                        ObjectMemberPolicy::Widen,
                    ),
                    ValueDescent::Array(array) => {
                        self.lower_array_literal(array, ObjectMemberPolicy::Widen)
                    }
                    // A CONDITIONAL's value is the union of its branch
                    // values, and each branch is lowered as a flow
                    // expression — so a call in a branch rides
                    // `SliceExpr::Call` to the evaluator's one call sink,
                    // exactly as the `if` / `return` twin's does. Folding
                    // the whole ternary through the leaf lowering instead
                    // published the callee's UNREDUCED return carrier: its
                    // own binders intact, its overload group unconsulted,
                    // warm.
                    ValueDescent::Branches(conditional) => {
                        let guard = self.lower_guard(&conditional.test);
                        // The ternary's TEST is a control position exactly as
                        // the `if` twin's: only its provably result-independent
                        // calls are decided above; an unprovable one flags the
                        // enclosing statement's guard-narrowing gap. An
                        // entered `asserts` call in it narrows once the test
                        // has run, ahead of both arms.
                        let test_assertions = self.collecting_entered_assertions(|this| {
                            if this.record_control_position_calls(&conditional.test) {
                                this.control_test_gap = true;
                            }
                        });
                        // The ternary's arms are GUARDED exactly as the `if`
                        // statement's are: a closure created inside one
                        // reads a capture's guarded narrowing only when the
                        // capture is extended into it. The two control
                        // spellings must reach the closure-capture rail with
                        // the same active guard set, or the same source
                        // degrades under `if` and seals clean under `?:`.
                        let active_guard_base = self.active_guard_bindings.len();
                        let guard_bindings = self.guard_bindings(&guard, conditional.test.span());
                        self.active_guard_bindings
                            .extend(guard_bindings.iter().copied());
                        let consequent = self.lower_expr(&conditional.consequent, mode);
                        let alternate = self.lower_expr(&conditional.alternate, mode);
                        self.active_guard_bindings.truncate(active_guard_base);
                        let union = SliceExpr::Union {
                            arms: Arc::from(vec![consequent, alternate].into_boxed_slice()),
                            guard,
                        };
                        if test_assertions.is_empty() {
                            union
                        } else {
                            SliceExpr::Sequence {
                                before: Arc::from(test_assertions.into_boxed_slice()),
                                value: Box::new(union),
                                after: None,
                            }
                        }
                    }
                    // A CALL POSITION with no structural arm (`f?.()`,
                    // `(0, f?.())`, `z = f()`). The
                    // fail-closed verdict is the CLASSIFIER's, taken on the
                    // expression FORM — not on whether the shallow pass
                    // happened to mint a `ReturnType<callee>` carrier the
                    // leaf gate could recognise. For every form here it does
                    // not: it answers a bare `any`, which reaches
                    // `SliceExpr::Any` BEFORE the carrier gate and publishes
                    // warm and clean. That is the hole this arm closes.
                    ValueDescent::UnmodeledCall => SliceExpr::UnreducedCallValue,
                    // A leaf-answered form takes the shared shallow-pass
                    // leaf lowering THROUGH `lower_leaf`, whose gate refuses
                    // a leaf answer that embeds an unreduced call-return
                    // carrier AND refuses a bare `any` answer at a call
                    // position. A form here therefore either contains no
                    // call in value position, or fails closed. It does NOT
                    // follow that every leaf form is modeled: several answer
                    // the shallow pass's fallback `any` for reasons that have
                    // nothing to do with calls (`JSXElement`, `Super`) — see
                    // `lower_leaf`. A VALUE-position `await x` never reaches
                    // here (the `Awaited` arm takes it first); one FOLDED
                    // into a leaf-composed answer composes an untyped
                    // position and the leaf gate refuses it.
                    ValueDescent::Reference
                    | ValueDescent::Logical
                    | ValueDescent::Sequence
                    | ValueDescent::Leaf => self.lower_leaf(other, mode),
                }
            }
        }
    }

    /// The EVOLVING-array binding an identifier occurrence names, when it
    /// names one: a local of this frame
    /// ([`SliceStatement::Binding::evolving_array`]) or a captured binding
    /// of an enclosing frame
    /// ([`verter_semantic::analysis::function_program::FlowBindingIdentity::evolving_array`]).
    fn evolving_binding_at(&self, span: oxc_span::Span) -> Option<FlowBindingRef> {
        use verter_semantic::analysis::flow::FlowBindingOccurrence;
        match self.bindings.occurrence(self.rebase(span)) {
            FlowBindingOccurrence::Resolved(binding) if self.is_evolving_binding(binding) => {
                Some(binding.clone())
            }
            _ => None,
        }
    }

    /// Whether a resolved binding is an EVOLVING array.
    fn is_evolving_binding(&self, binding: &FlowBindingRef) -> bool {
        match binding {
            FlowBindingRef::Local(local) => self.skeleton.binding(*local).evolving_array,
            FlowBindingRef::Captured(identity) => identity.evolving_array,
        }
    }

    /// Whether the demand selected an EVOLVING-array binding's value. A
    /// captured one is this frame's input: its operations always lower.
    fn evolving_binding_selected(&self, binding: &FlowBindingRef) -> bool {
        match binding {
            FlowBindingRef::Local(local) => self.binding_is_selected(*local),
            FlowBindingRef::Captured(_) => true,
        }
    }

    /// Lower `expression` as an EVOLVING-array operation, when it is one
    /// ([`SliceEvolvingOperation`]): a `push` / `unshift` call on an
    /// evolving local (optional forms included), or an element `=` write
    /// into one. A call is modelled HERE either way — it feeds no callee
    /// return to any value and an array's `push` / `unshift` narrows
    /// nothing — so its span is decided above; an operation on a binding
    /// the demand did not select only runs its operands' effects.
    fn lower_evolving_operation(
        &mut self,
        expression: &Expression<'_>,
        mode: ExprMode,
    ) -> EvolvingLowering {
        let call = match expression {
            Expression::CallExpression(call) => Some(&**call),
            Expression::ChainExpression(chain) => match &chain.expression {
                oxc_ast::ast::ChainElement::CallExpression(call) => Some(&**call),
                _ => None,
            },
            _ => None,
        };
        if let Some(call) = call {
            let Some(binding) = verter_semantic::analysis::flow::evolving_array_mutation_root(call)
                .and_then(|root| self.evolving_binding_at(root.span))
            else {
                return EvolvingLowering::NotEvolving;
            };
            self.decided_above_call_spans.push(call.span.into());
            let arguments = call.arguments.iter().map(|argument| match argument {
                oxc_ast::ast::Argument::SpreadElement(spread) => (&spread.argument, true),
                other => (other.to_expression(), false),
            });
            if !self.evolving_binding_selected(&binding) {
                for (argument, _) in arguments {
                    self.scan_unmodeled_position_effects(argument);
                }
                return EvolvingLowering::Unselected;
            }
            let arguments: Vec<SliceMutationArgument> = arguments
                .map(|(argument, spread)| SliceMutationArgument {
                    value: self.lower_expr(argument, mode),
                    spread,
                    freshness: expression_freshness(argument),
                })
                .collect();
            return EvolvingLowering::Operation(SliceEvolvingOperation {
                binding,
                kind: SliceEvolvingOperationKind::Append(Arc::from(arguments.into_boxed_slice())),
                span: self.rebase(call.span),
            });
        }
        let Expression::AssignmentExpression(assignment) = expression else {
            return EvolvingLowering::NotEvolving;
        };
        let Some((member, binding)) =
            verter_semantic::analysis::flow::evolving_array_element_write_root(assignment)
                .and_then(|(member, root)| Some((member, self.evolving_binding_at(root.span)?)))
        else {
            return EvolvingLowering::NotEvolving;
        };
        if !self.evolving_binding_selected(&binding) {
            self.scan_unmodeled_position_effects(&member.expression);
            self.scan_unmodeled_position_effects(&assignment.right);
            return EvolvingLowering::Unselected;
        }
        EvolvingLowering::Operation(SliceEvolvingOperation {
            binding,
            kind: SliceEvolvingOperationKind::ElementWrite {
                index: Box::new(self.lower_expr(&member.expression, mode)),
                value: Box::new(self.lower_expr(&assignment.right, mode)),
                freshness: expression_freshness(&assignment.right),
            },
            span: self.rebase(member.span),
        })
    }

    /// The statement-position `a = []` that starts a new EVOLVING array
    /// ([`SliceEvolvingOperationKind::Reset`]) — the unparenthesized empty
    /// literal only, written to a selected evolving local.
    fn evolving_reset_statement(
        &mut self,
        assignment: &oxc_ast::ast::AssignmentExpression<'_>,
    ) -> Option<SliceStatement> {
        if !evolving_reset_assignment(assignment) {
            return None;
        }
        let identifier =
            verter_semantic::analysis::flow::assignment_target_binding(&assignment.left)?;
        let binding = self.evolving_binding_at(identifier.span)?;
        let (_, definition, span) =
            self.modeled_assignment_parts(assignment, assignment.right.span())?;
        let value = Box::new(self.lowered_assignment_rhs(assignment));
        Some(SliceStatement::EvolvingArray(SliceEvolvingOperation {
            binding,
            kind: SliceEvolvingOperationKind::Reset {
                definition,
                span,
                value,
            },
            span,
        }))
    }

    /// The frame-rooted member read a literal-keyed element access spells
    /// (`a["k"]` is `a.k`, `t[1]` is `t["1"]`): the same `typeof` path
    /// carrier a dotted read lowers to, behind the same frame gate. `None`
    /// when the chain's root is not a frame binding — the shared leaf
    /// lowering then answers the whole form.
    fn lower_literal_member_read(&mut self, expr: &Expression<'_>) -> Option<SliceExpr> {
        let (root, path) = literal_member_chain(expr)?;
        let mut full = Vec::with_capacity(path.len() + 1);
        full.push(root.name.to_string());
        full.extend(path.iter().map(|segment| segment.to_string()));
        let ty = TypeExpr::TypeOf(verter_type_expr::ValueRef {
            path: full,
            type_args: Vec::new(),
        });
        let shadowed = self.answer_names_frame_bound(&ty, expr.span(), &[]);
        if shadowed.is_empty() {
            return None;
        }
        let frame_root = self.frame_root_for_type(&ty, expr);
        Some(SliceExpr::FrameShadowed {
            inner: Box::new(SliceExpr::Type(GatedLeaf(ty, frame_root))),
            shadowed: Arc::from(shadowed.into_boxed_slice()),
        })
    }

    /// An operator form whose value the checker computes from its
    /// operands' TYPES — arithmetic, bitwise and `+` operators, unary `-`
    /// / `+` / `~`, an element access with an evaluated key, an update
    /// expression — lowered over flow operands. Taken only where the shared
    /// leaf lowering cannot answer the form (its operands read this
    /// frame); a literal-only form keeps the leaf's literal answer (`-1`
    /// is `-1`). An update expression always lowers here when its target
    /// is a modelable binding, because its write applies in evaluation
    /// order.
    fn lower_operator_form(&mut self, expr: &Expression<'_>, mode: ExprMode) -> Option<SliceExpr> {
        use oxc_ast::ast::{BinaryOperator, UnaryOperator};
        let operand_mode = ExprMode::BindingInit {
            preserve_literal: true,
        };
        if let Expression::UpdateExpression(update) = expr {
            let oxc_ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) =
                &update.argument
            else {
                return None;
            };
            let root = self.write_target_root(identifier)?;
            return Some(SliceExpr::Update {
                target: SliceNarrowSubject {
                    root,
                    path: Arc::from(Vec::new().into_boxed_slice()),
                },
                span: self.rebase(update.span),
            });
        }
        let operator_form = match expr {
            Expression::BinaryExpression(binary) => matches!(
                binary.operator,
                BinaryOperator::Addition
                    | BinaryOperator::Subtraction
                    | BinaryOperator::Multiplication
                    | BinaryOperator::Division
                    | BinaryOperator::Remainder
                    | BinaryOperator::Exponential
                    | BinaryOperator::ShiftLeft
                    | BinaryOperator::ShiftRight
                    | BinaryOperator::ShiftRightZeroFill
                    | BinaryOperator::BitwiseOR
                    | BinaryOperator::BitwiseXOR
                    | BinaryOperator::BitwiseAnd
            ),
            Expression::UnaryExpression(unary) => matches!(
                unary.operator,
                UnaryOperator::UnaryNegation
                    | UnaryOperator::UnaryPlus
                    | UnaryOperator::BitwiseNot
                    | UnaryOperator::LogicalNot
            ),
            Expression::ComputedMemberExpression(_) | Expression::LogicalExpression(_) => true,
            // A non-null assertion over a call is a call position with no
            // structural arm (`value_is_unmodeled_call`).
            Expression::TSNonNullExpression(_) => {
                !verter_semantic::analysis::flow::value_is_unmodeled_call(expr)
            }
            _ => false,
        };
        // A logical expression and a `!` lower structurally whatever the
        // leaf answers: a write in an operand applies on its edge only, and
        // the result keeps its operands' literal freshness.
        let structural = matches!(expr, Expression::LogicalExpression(_))
            || matches!(expr, Expression::UnaryExpression(unary)
                if unary.operator == UnaryOperator::LogicalNot);
        if !operator_form
            || (!structural && !matches!(self.leaf_type(expr, mode), LeafLowering::Unmodeled))
        {
            return None;
        }
        Some(match expr {
            Expression::BinaryExpression(binary) => SliceExpr::Arithmetic {
                operator: if binary.operator == BinaryOperator::Addition {
                    SliceArithmetic::Add
                } else {
                    SliceArithmetic::Numeric
                },
                operands: Arc::from(
                    vec![
                        self.lower_expr(&binary.left, operand_mode),
                        self.lower_expr(&binary.right, operand_mode),
                    ]
                    .into_boxed_slice(),
                ),
            },
            Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::LogicalNot => {
                SliceExpr::Not {
                    operand: Box::new(self.lower_expr(&unary.argument, operand_mode)),
                    widen: false,
                }
            }
            Expression::UnaryExpression(unary) => SliceExpr::Arithmetic {
                operator: if unary.operator == UnaryOperator::UnaryPlus {
                    SliceArithmetic::Plus
                } else {
                    SliceArithmetic::Negate
                },
                operands: Arc::from(
                    vec![self.lower_expr(&unary.argument, operand_mode)].into_boxed_slice(),
                ),
            },
            Expression::ComputedMemberExpression(member) => {
                // A key read from a frame binding makes the access the
                // reference its identity spells ([`SliceElementKey`]).
                let key = self.element_key(&member.expression);
                let reference = key
                    .as_ref()
                    .and_then(|_| self.narrow_subject_of(&member.object));
                let key = key.filter(|_| reference.is_some());
                SliceExpr::ElementAccess {
                    object: Box::new(self.lower_expr(&member.object, operand_mode)),
                    index: Box::new(self.lower_expr(&member.expression, operand_mode)),
                    reference,
                    key,
                }
            }
            Expression::LogicalExpression(logical) => {
                self.lower_logical_value(logical, operand_mode)
            }
            Expression::TSNonNullExpression(non_null) => SliceExpr::NonNull {
                operand: Box::new(self.lower_expr(&non_null.expression, mode)),
            },
            _ => return None,
        })
    }

    fn lower_identifier_read(
        &mut self,
        identifier: &oxc_ast::ast::IdentifierReference<'_>,
        _mode: ExprMode,
    ) -> SliceExpr {
        let name = identifier.name.as_str();
        match self.classify_occurrence(identifier.span) {
            NameBinding::Param(ordinal) => match self
                .params
                .get(ordinal as usize)
                .and_then(|param| param.binding)
            {
                Some(binding) => SliceExpr::Param { ordinal, binding },
                None => SliceExpr::UnmodeledBinding,
            },
            NameBinding::Local(param) => SliceExpr::Local {
                binding: match self.binding_at(identifier.span) {
                    Some(binding) => binding,
                    None => return SliceExpr::UnmodeledBinding,
                },
                name: Arc::from(name),
                param,
                captured: false,
            },
            NameBinding::Captured => SliceExpr::Local {
                binding: match self.binding_at(identifier.span) {
                    Some(binding) => binding,
                    None => return SliceExpr::UnmodeledBinding,
                },
                name: Arc::from(name),
                param: None,
                captured: true,
            },
            // A local function declaration is the function value it
            // declares.
            NameBinding::NestedFunction | NameBinding::Unmodeled => self
                .lower_local_function_declaration(identifier.span)
                .unwrap_or(SliceExpr::UnmodeledBinding),
            // A free `undefined` is not a declaration the owner scope can
            // answer: its value IS the `undefined` type (the shared shallow
            // pass reads it the same way).
            NameBinding::Free if name == "undefined" => SliceExpr::Type(GatedLeaf(
                TypeExpr::Primitive(PrimitiveName::Undefined),
                None,
            )),
            // A static member reading its own class, or an object literal's
            // method reading the variable that holds the literal, reads the
            // value its receiver is: the deferred type of the value being
            // declared, whose surface this member is part of.
            NameBinding::Free
                if matches!(
                    &self.this,
                    Some(SliceThis::Static { class: value, .. } | SliceThis::Value { value, .. })
                        if value.as_ref() == name
                ) =>
            {
                SliceExpr::This(self.this.clone().expect("guarded"))
            }
            NameBinding::Free => {
                match self.namespace_scoped_leaf(TypeExpr::TypeOf(verter_type_expr::ValueRef {
                    path: vec![name.to_owned()],
                    type_args: Vec::new(),
                })) {
                    LeafLowering::Free(ty) => SliceExpr::Type(GatedLeaf(ty, None)),
                    _ => SliceExpr::UnmodeledBinding,
                }
            }
        }
    }

    /// Lower an array literal to [`SliceExpr::Array`] under an object
    /// literal's member policy. Every element is its own evaluated
    /// position — the planner opened each one (a spread's argument for a
    /// spread) as a child site of the array — so an element outside the
    /// demand selection rides the typed `Elided` carrier, exactly as an
    /// object member value does. A fresh element literal widens (the
    /// element slot is mutable) unless the policy keeps literals or a
    /// const assertion pins that element; under `as const` a nested
    /// object or array literal is in the const context too.
    fn lower_array_literal(
        &mut self,
        array: &oxc_ast::ast::ArrayExpression<'_>,
        policy: ObjectMemberPolicy,
    ) -> SliceExpr {
        let const_asserted = policy == ObjectMemberPolicy::ConstAssert;
        let mode = ExprMode::BindingInit {
            preserve_literal: true,
        };
        let mut elements = Vec::with_capacity(array.elements.len());
        for element in &array.elements {
            let (expression, spread) = match element {
                oxc_ast::ast::ArrayExpressionElement::SpreadElement(spread) => {
                    (&spread.argument, true)
                }
                oxc_ast::ast::ArrayExpressionElement::Elision(_) => {
                    elements.push(SliceArrayElement::Elision);
                    continue;
                }
                other => match other.as_expression() {
                    Some(expression) => (expression, false),
                    None => continue,
                },
            };
            let value = if !self.value_span_selected(expression.span()) {
                // The elided element still RUNS at the literal's
                // evaluation: its effects take the fail-closed scan.
                self.scan_unmodeled_position_effects(expression);
                SliceExpr::Elided
            } else if const_asserted {
                self.lower_in_const_context(expression, mode)
            } else {
                self.lower_expr(expression, mode)
            };
            if spread {
                elements.push(SliceArrayElement::Spread { source: value });
                continue;
            }
            let pinned = !policy.widens_member_literals()
                || verter_semantic::analysis::type_eval_build::expr_is_const_asserted(
                    expression,
                    self.source,
                );
            let (value, pre_widening) = if pinned {
                (value, None)
            } else {
                let widened = widen_mutable_slot_literals(value.clone());
                let pre_widening = (widened != value).then(|| Box::new(value));
                (widened, pre_widening)
            };
            elements.push(SliceArrayElement::Value {
                value,
                freshness: expression_freshness(expression),
                pre_widening,
            });
        }
        SliceExpr::Array {
            elements: Arc::from(elements.into_boxed_slice()),
            const_asserted,
        }
    }

    /// Whether an initializer is a bare `null` or a FREE `undefined`,
    /// through parentheses — the checker's `isNullOrUndefined`, which
    /// resolves the name, so a local `undefined` does not qualify.
    fn is_null_or_undefined_keyword(&self, expression: &Expression<'_>) -> bool {
        match unwrap_parenthesized(expression) {
            Expression::NullLiteral(_) => true,
            Expression::Identifier(identifier) => {
                identifier.name.as_str() == "undefined"
                    && matches!(self.classify_occurrence(identifier.span), NameBinding::Free)
            }
            _ => false,
        }
    }

    /// Lower a value sitting DIRECTLY in a const context — a member value
    /// or element of a const-asserted literal, through parentheses: a
    /// nested object or array literal inherits the const assertion
    /// (TypeScript's `isConstContext`), and every other form lowers as it
    /// would anywhere else.
    fn lower_in_const_context(&mut self, expression: &Expression<'_>, mode: ExprMode) -> SliceExpr {
        match value_descent(unwrap_parenthesized(expression)) {
            ValueDescent::Object(object) => self.lower_object_literal_with_policy(
                object,
                expression,
                mode,
                ObjectMemberPolicy::ConstAssert,
            ),
            ValueDescent::Array(array) => {
                self.lower_array_literal(array, ObjectMemberPolicy::ConstAssert)
            }
            _ => self.lower_expr(expression, mode),
        }
    }

    /// Lower a MEMBER-valued optional chain to the typed
    /// [`SliceExpr::OptionalMember`] carrier. `None` for every shape the
    /// carrier cannot retain (see [`optional_member_chain_parts`]) — the
    /// caller then keeps the rails the chain already had.
    fn lower_optional_member_chain(
        &mut self,
        chain: &oxc_ast::ast::ChainExpression<'_>,
        mode: ExprMode,
    ) -> Option<SliceExpr> {
        let SplitOptionalMemberChain { root, links } =
            optional_member_chain_parts(&chain.expression)?;
        // The same write-effect rail the optional-`any`-chain root takes:
        // a root whose reaching definition an in-frame guard or write
        // changes is not honestly re-readable here.
        if self.optional_chain_root_has_prior_flow_change(root) {
            return Some(SliceExpr::Gap(
                crate::semantic_query::FlowGap::UnmodeledExpression,
            ));
        }
        let root_expr = self.lower_identifier_read(root, mode);
        Some(SliceExpr::OptionalMember {
            root: Box::new(root_expr),
            links: Arc::from(links.into_boxed_slice()),
        })
    }

    /// Lower a `super.m(…)` call onto the heritage surface:
    /// [`SliceCall::OnHeritage`], carrying the enclosing class's `extends`
    /// expression as a gated value type plus the authored member path.
    ///
    /// `None` when there is no heritage context (not a direct class
    /// member, or a heritage-less class), the heritage expression is one
    /// the shared shallow pass cannot model (a call, a mixin composition),
    /// or the heritage is GENERIC (`extends Base<Args>`) — the carrier
    /// projects the base's PROTOTYPE side unbound, which would publish
    /// the base's free type parameters instead of `Args`-instantiated
    /// members; failing closed here is honest, an eager fabrication is
    /// not. In every `None` case the call then keeps the fail-closed
    /// rail. The heritage answer is gated exactly like any leaf that
    /// names names: a frame that shadows the heritage expression's root
    /// wraps the call in the root-identifier gate's carrier, and the
    /// evaluator fails it closed when the owner scope would answer the
    /// shadowed name.
    fn lower_super_call_on_heritage(
        &mut self,
        member: &[Arc<str>],
        call: &oxc_ast::ast::CallExpression<'_>,
        mode: ExprMode,
    ) -> Option<SliceExpr> {
        let heritage_access = self.enclosing_heritage?;
        if heritage_access.super_type_arguments.is_some() {
            return None;
        }
        let super_class = heritage_access.super_class;
        // The heritage expression's gated value type, through the same
        // leaf lowering + frame gate any authored heritage read takes.
        let (ty, shadowed) = match self.leaf_type(super_class, mode) {
            // A heritage the shared shallow pass cannot model (a call, a
            // computed composition) has no honest base to walk.
            LeafLowering::Unmodeled => return None,
            LeafLowering::Free(ty) => {
                let shadowed = self.answer_names_frame_bound(&ty, super_class.span(), &[]);
                (ty, Arc::from(shadowed.into_boxed_slice()))
            }
            LeafLowering::FrameShadowed { ty, shadowed } => (ty, shadowed),
        };
        let call_expr = SliceExpr::Call(
            SliceCall::OnHeritage {
                heritage: GatedType {
                    ty,
                    shadowed: Arc::clone(&shadowed),
                },
                member: Arc::from(member.to_vec().into_boxed_slice()),
                static_side: heritage_access.static_side,
            },
            call_site(call),
            SliceCallArguments::none(),
        );
        Some(if shadowed.is_empty() {
            call_expr
        } else {
            SliceExpr::FrameShadowed {
                inner: Box::new(call_expr),
                shadowed,
            }
        })
    }

    fn optional_chain_root_has_prior_flow_change(
        &self,
        root: &oxc_ast::ast::IdentifierReference<'_>,
    ) -> bool {
        let read = self.rebase(root.span);
        let Some(FlowBindingRef::Local(binding)) = self.binding_at(root.span) else {
            return false;
        };
        let binding = self.bindings.canonical_local(binding);
        self.active_guard_bindings.contains(&binding)
            || self.skeleton.writes.iter().any(|write| {
                write.span < read && write.path.is_empty()
                    && matches!(write.binding, Some(FlowBindingRef::Local(local)) if self.bindings.canonical_local(local) == binding)
            })
    }

    /// Lower one object literal STRUCTURALLY under `policy`: each entry's
    /// contributing expression is a flow expression, gated by the demand
    /// selection. A literal this half cannot model structurally (a
    /// private-name key, a non-function method value) falls back to the
    /// whole-literal leaf lowering of `whole`.
    ///
    /// Entry dispositions come from the ONE shared classifier
    /// (`object_entry_descent`), the same one the skeleton's
    /// `open_object_site` opens child sites from: a SPREAD is a value
    /// provider on both sides, so its source lowers here exactly as a
    /// member value does and rides whatever arm its own form takes.
    ///
    /// The fallback is the LAST resort and deliberately narrow, because
    /// it is not local: it folds every sibling — spreads included — into
    /// one shallow-pass leaf answer, and a leaf answer over a
    /// CALL-sourced spread embeds the callee's unreduced `ReturnType<…>`
    /// carrier, which the leaf's fabricated-value gate refuses. One
    /// unmodellable ENTRY therefore used to fail the whole RETURN closed.
    /// A key whose property name is not its authored text is no longer
    /// such an entry: it lowers as its own value position
    /// ([`SliceObjectKey::Computed`]) and the evaluator names the key from
    /// that value, or fails the literal closed if it cannot — the same
    /// verdict, without the siblings.
    fn lower_object_literal_with_policy(
        &mut self,
        object: &oxc_ast::ast::ObjectExpression<'_>,
        whole: &Expression<'_>,
        mode: ExprMode,
        policy: ObjectMemberPolicy,
    ) -> SliceExpr {
        let mut entries = Vec::with_capacity(object.properties.len());
        let mut structural = true;
        for property in &object.properties {
            let (key, value_expression, kind, p) = match object_entry_descent(property) {
                ObjectEntryDescent::Spread { source } => {
                    // The spread SOURCE is an ordinary selected value
                    // position: an unselected one rides the same typed
                    // `Elided` carrier a member value does, so a
                    // planner/content selection mismatch stays visible
                    // rather than silently contributing nothing.
                    let source = if self.value_span_selected(source.span()) {
                        self.lower_expr(source, mode)
                    } else {
                        // The elided spread source still RUNS: scan its
                        // effects with the unmodeled-position discipline.
                        self.scan_unmodeled_position_effects(source);
                        SliceExpr::Elided
                    };
                    entries.push(SliceObjectEntry::Spread {
                        source: Box::new(source),
                    });
                    continue;
                }
                ObjectEntryDescent::Property {
                    key,
                    value,
                    kind,
                    property,
                } => (key, value, kind, property),
            };
            let key = match key {
                ObjectEntryKey::Static(name) => SliceObjectKey::Static(Arc::from(name)),
                // The key expression is its OWN evaluated position — the
                // planner already tracks it as a child site for exactly
                // that reason — so it lowers through the same
                // `lower_expr` every value position takes. A numeric
                // literal key lands here too: `{ 1: x }`'s authored text
                // is not its property name, and the canonical name is its
                // NUMBER's, which only the value knows.
                ObjectEntryKey::Computed(expression) => SliceObjectKey::Computed {
                    value: Box::new(self.lower_expr(expression, mode)),
                },
                // A private name is a key form neither half models, and
                // unlike a computed key it has no value to resolve.
                ObjectEntryKey::Unmodeled => {
                    structural = false;
                    break;
                }
            };
            let method_kind = match kind {
                ObjectEntryKind::Init => None,
                ObjectEntryKind::Method => Some(verter_type_expr::ObjectMethodKind::Method),
                ObjectEntryKind::Get => Some(verter_type_expr::ObjectMethodKind::Get),
                ObjectEntryKind::Set => Some(verter_type_expr::ObjectMethodKind::Set),
            };
            let spans = verter_type_expr::MemberSpans {
                declaration: Some(p.span.into()),
                name: Some(p.key.span().into()),
                type_annotation: None,
            };
            // A member value OUTSIDE the demand selection never
            // lowers — the elided sibling rides the typed carrier
            // (present in the member LIST so missing-member
            // detection stays static, content-free forever). The value
            // still RUNS at the object literal's evaluation, so its
            // effects take the same fail-closed scan every elided
            // position gets.
            if !self.value_span_selected(value_expression.span()) {
                self.scan_unmodeled_position_effects(value_expression);
                entries.push(SliceObjectEntry::Member(Box::new(SliceObjectMember {
                    key,
                    value: SliceExpr::Elided,
                    assignment_value: None,
                    unwidened: None,
                    method_kind,
                    readonly: policy.readonly(),
                    spans,
                    accessor_annotated: false,
                })));
                continue;
            }
            // A method / accessor member with a body is a nested
            // function value (its return evaluates inline through
            // the same flow machinery); a method without a body
            // keeps the whole-literal leaf lowering.
            if method_kind.is_some() {
                let accessor_annotated = match (method_kind, value_expression) {
                    (
                        Some(verter_type_expr::ObjectMethodKind::Get),
                        Expression::FunctionExpression(func),
                    ) => func.return_type.is_some(),
                    (
                        Some(verter_type_expr::ObjectMethodKind::Set),
                        Expression::FunctionExpression(func),
                    ) => func
                        .params
                        .items
                        .first()
                        .is_some_and(|param| param.type_annotation.is_some()),
                    _ => false,
                };
                let value = match value_expression {
                    Expression::FunctionExpression(func) => {
                        // A method or accessor of the literal runs against
                        // the object the literal builds.
                        self.member_this = Some(Some(SliceThis::Receiver));
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
                entries.push(SliceObjectEntry::Member(Box::new(SliceObjectMember {
                    key,
                    value,
                    assignment_value: None,
                    unwidened: None,
                    method_kind,
                    // A method / accessor member is never `readonly`,
                    // under `as const` or otherwise: the modifier applies
                    // to data properties.
                    readonly: false,
                    spans,
                    accessor_annotated,
                })));
                continue;
            }
            // `strictNullChecks` off: a bare `null` / `undefined` /
            // `void` member is the widening nullable type, which the
            // literal's widening turns into `any` under every member
            // policy (`{ a: null } as const` is `{ readonly a: any }`).
            // The value still RUNS: a `void (x = v)` applies its write,
            // and a call inside `void f()` takes the same scan an elided
            // position does.
            if !self.nullability.is_strict() && expr_is_widening_nullish(value_expression) {
                let value = match self.modeled_void_write(value_expression) {
                    Some(write) => SliceExpr::Void {
                        operand: Box::new(write),
                        value: Box::new(SliceExpr::SemanticAny),
                    },
                    None => {
                        self.scan_unmodeled_position_effects(value_expression);
                        SliceExpr::SemanticAny
                    }
                };
                entries.push(SliceObjectEntry::Member(Box::new(SliceObjectMember {
                    key,
                    value,
                    assignment_value: None,
                    unwidened: Some(SliceExpr::Type(GatedLeaf(
                        widening_nullish_type(value_expression),
                        None,
                    ))),
                    method_kind,
                    readonly: policy.readonly(),
                    spans,
                    accessor_annotated: false,
                })));
                continue;
            }
            // An object-literal member's fresh literal ALWAYS
            // widens to its primitive (the member slot is
            // mutable), in every enclosing position — tsc's
            // object-literal property widening rule. A per-member
            // `as const` (`{ tag: "x" as const }`) pins that one
            // member's literal, and an ENCLOSING `as const` pins
            // every member's — which is what `policy` carries.
            let widen_member = policy.widens_member_literals()
                && !verter_semantic::analysis::type_eval_build::expr_is_const_asserted(
                    value_expression,
                    self.source,
                );
            // A class expression a static key holds is named after the key
            // (`{ K: class {} }` is the checker's `K`); every other value of
            // an `as const` literal lowers in the const context.
            let value = match &key {
                SliceObjectKey::Static(name)
                    if matches!(value_expression, Expression::ClassExpression(_)) =>
                {
                    let name = Arc::clone(name);
                    self.lower_assigned_value(value_expression, &name, mode)
                }
                // A const-asserted member keeps its literal in every
                // position, a mutable declaration's initializer included.
                _ if policy == ObjectMemberPolicy::ConstAssert => self.lower_in_const_context(
                    value_expression,
                    ExprMode::BindingInit {
                        preserve_literal: true,
                    },
                ),
                _ => self.lower_expr(value_expression, mode),
            };
            let assignment_value = value.clone();
            let value = if widen_member {
                widen_mutable_slot_literals(value)
            } else {
                value
            };
            let assignment_value = (assignment_value != value).then_some(assignment_value);
            entries.push(SliceObjectEntry::Member(Box::new(SliceObjectMember {
                key,
                value,
                assignment_value,
                unwidened: None,
                method_kind,
                readonly: policy.readonly(),
                spans,
                accessor_annotated: false,
            })));
        }
        if structural {
            SliceExpr::Object {
                entries: Arc::from(entries.into_boxed_slice()),
                offset: object.span.start,
            }
        } else {
            self.lower_leaf(whole, mode)
        }
    }

    /// Lower a NESTED function node (a function / arrow expression or an
    /// object-literal method) into an owned nested function value: the
    /// nested body lowers under its OWN [`FunctionBodySkeleton`] — the
    /// same lexical authority the root frame uses, built over the nested
    /// body alone — plus the CAPTURE SCOPE of every enclosing frame,
    /// resolved at this function value's own position. The descriptor retains
    /// exact captured identities and lexical signature facts. Its body lowers
    /// only when evaluated, through the child's own indexed graph and demand.
    /// Lower every argument of `call` in this frame as a whole value
    /// ([`SliceContent::call_arguments`]). The lowering leaves no trace
    /// beside the recorded values: a side channel it reached is undone
    /// and the call keeps its indexed arguments.
    fn record_call_arguments(&mut self, call: &oxc_ast::ast::CallExpression<'_>) {
        if call
            .arguments
            .iter()
            .any(|argument| argument.as_expression().is_none())
        {
            return;
        }
        let budget_failure = self.budget_failure;
        let decided_above = self.decided_above_call_spans.len();
        let control_test_gap = self.control_test_gap;
        self.whole_value_nesting += 1;
        let arguments: Vec<SliceExpr> = call
            .arguments
            .iter()
            .filter_map(|argument| argument.as_expression())
            .map(|argument| self.lower_expr(argument, ExprMode::Return))
            .collect();
        self.whole_value_nesting -= 1;
        let side_channel = self.budget_failure != budget_failure
            || self.decided_above_call_spans.len() != decided_above
            || self.control_test_gap != control_test_gap;
        self.budget_failure = budget_failure;
        self.decided_above_call_spans.truncate(decided_above);
        self.control_test_gap = control_test_gap;
        if !side_channel {
            self.call_arguments
                .insert(call.span.into(), Arc::from(arguments.into_boxed_slice()));
        }
    }

    fn lower_nested_function(&mut self, node: &FunctionNode<'_>) -> SliceExpr {
        self.lower_function_value(node, None)
    }

    /// The member `name` of the object literal the frame's `this` is, as
    /// the literal declares it; `None` when the literal does not declare
    /// it (or a later spread may replace it).
    fn object_this_member(&self, name: &str) -> Option<ObjectThisMember<'a>> {
        let (contributor, declarator) = match &self.this {
            Some(SliceThis::Value {
                contributor,
                declarator,
                ..
            }) => (contributor, declarator),
            Some(SliceThis::Static {
                contributor: Some(contributor),
                ..
            }) => return self.static_this_member(*contributor, name),
            _ => return None,
        };
        let program: &'a Program<'a> = self.program;
        let declaration = match program.body.get(*contributor as usize)? {
            Statement::VariableDeclaration(declaration) => declaration,
            Statement::ExportNamedDeclaration(export) => match export.declaration.as_ref()? {
                oxc_ast::ast::Declaration::VariableDeclaration(declaration) => declaration,
                _ => return None,
            },
            _ => return None,
        };
        let Expression::ObjectExpression(object) = declaration
            .declarations
            .get(*declarator as usize)?
            .init
            .as_ref()?
        else {
            return None;
        };
        let mut found = None;
        for (ordinal, property) in object.properties.iter().enumerate() {
            let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(property) = property else {
                // A spread after the member may replace it.
                found = None;
                continue;
            };
            if property.computed
                || verter_semantic::analysis::flow::static_property_key_text(&property.key)
                    != Some(name)
            {
                continue;
            }
            let key = || verter_semantic::analysis::function_program::FunctionProgramKey {
                declaration: self.bindings.function().declaration.clone(),
                part: verter_type_expr::facts::FunctionPartIdentity::Member {
                    member_path: Arc::from([u32::try_from(ordinal).unwrap_or(u32::MAX)]),
                },
                overload_ordinal: 0,
            };
            found = match (&property.value, property.kind, property.method) {
                (Expression::FunctionExpression(_), oxc_ast::ast::PropertyKind::Get, _) => {
                    Some(ObjectThisMember::Getter(key()))
                }
                (Expression::FunctionExpression(_), oxc_ast::ast::PropertyKind::Init, true) => {
                    Some(ObjectThisMember::Method(key()))
                }
                (value, oxc_ast::ast::PropertyKind::Init, false) => {
                    Some(ObjectThisMember::Property(value))
                }
                // A setter reads nothing.
                _ => found,
            };
        }
        found
    }

    /// The static member `name` the class of statement `contributor`
    /// declares: a property is its annotation, else its initializer; a
    /// method or getter its own served position.
    fn static_this_member(&self, contributor: u32, name: &str) -> Option<ObjectThisMember<'a>> {
        let program: &'a Program<'a> = self.program;
        let class = match program.body.get(contributor as usize)? {
            Statement::ClassDeclaration(class) => class,
            Statement::ExportNamedDeclaration(export) => match export.declaration.as_ref()? {
                oxc_ast::ast::Declaration::ClassDeclaration(class) => class,
                _ => return None,
            },
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                oxc_ast::ast::ExportDefaultDeclarationKind::ClassDeclaration(class) => class,
                _ => return None,
            },
            _ => return None,
        };
        let key =
            |ordinal: usize| verter_semantic::analysis::function_program::FunctionProgramKey {
                declaration: self.bindings.function().declaration.clone(),
                part: verter_type_expr::facts::FunctionPartIdentity::Member {
                    member_path: Arc::from([u32::try_from(ordinal).unwrap_or(u32::MAX)]),
                },
                overload_ordinal: 0,
            };
        let mut found = None;
        for (ordinal, element) in class.body.body.iter().enumerate() {
            match element {
                oxc_ast::ast::ClassElement::PropertyDefinition(property)
                    if property.r#static
                        && !property.computed
                        && verter_semantic::analysis::flow::static_property_key_text(
                            &property.key,
                        ) == Some(name) =>
                {
                    found = match (&property.type_annotation, &property.value) {
                        (None, Some(value)) => Some(ObjectThisMember::Property(value)),
                        // An annotated or uninitialized static reads its
                        // declaration, which this read does not lower.
                        _ => None,
                    };
                }
                oxc_ast::ast::ClassElement::MethodDefinition(method)
                    if method.r#static
                        && !method.computed
                        && method.value.body.is_some()
                        && verter_semantic::analysis::flow::static_property_key_text(
                            &method.key,
                        ) == Some(name) =>
                {
                    found = match method.kind {
                        oxc_ast::ast::MethodDefinitionKind::Method => {
                            Some(ObjectThisMember::Method(key(ordinal)))
                        }
                        oxc_ast::ast::MethodDefinitionKind::Get => {
                            Some(ObjectThisMember::Getter(key(ordinal)))
                        }
                        _ => found,
                    };
                }
                _ => {}
            }
        }
        found
    }

    /// A member read off an object literal's (or a class's static) `this`:
    /// a property is the value its declaration initializes it with, widened;
    /// a getter is its served return.
    fn lower_object_this_read(
        &mut self,
        path: &[Arc<str>],
        span: oxc_span::Span,
        _mode: ExprMode,
    ) -> SliceExpr {
        let gap = SliceExpr::Gap(crate::semantic_query::FlowGap::UnmodeledExpression);
        let Some((first, rest)) = path.split_first() else {
            return gap;
        };
        let root = match self.object_this_member(first) {
            // The declared literal type widens where the read is used (a
            // readonly literal is the checker's widening literal type).
            Some(ObjectThisMember::Property(value)) => self.lower_leaf(
                value,
                ExprMode::BindingInit {
                    preserve_literal: false,
                },
            ),
            Some(ObjectThisMember::Getter(target)) => SliceExpr::Call(
                SliceCall::Direct(target),
                SliceCallSite::new(0, false, false, span.into()),
                SliceCallArguments::none(),
            ),
            // Any other member of a static `this` — an annotated or
            // inherited static, a method read as a value — reads off the
            // class's constructor, where the class declares that member.
            _ if matches!(self.this, Some(SliceThis::Static { .. })) => {
                return SliceExpr::OptionalMember {
                    root: Box::new(SliceExpr::This(self.this.clone().expect("guarded"))),
                    links: path.iter().map(|name| (Arc::clone(name), false)).collect(),
                };
            }
            _ => return gap,
        };
        if rest.is_empty() {
            root
        } else {
            SliceExpr::OptionalMember {
                root: Box::new(root),
                links: rest.iter().map(|name| (Arc::clone(name), false)).collect(),
            }
        }
    }

    /// The local function DECLARATION the name at `span` binds — declared in
    /// this frame or an enclosing one — as the function value it is, read
    /// where the name is referenced. `None` when the name binds anything
    /// else.
    ///
    /// A declaration is hoisted: its value exists from its frame's entry,
    /// so it is read wherever the name is, before the declaration too. And
    /// it is never a control-flow container the checker extends a capture's
    /// narrowing into (`getControlFlowContainer` stops at a function
    /// declaration): every capture reads its declared type.
    fn lower_local_function_declaration(&mut self, span: oxc_span::Span) -> Option<SliceExpr> {
        let (function, gate, own_frame) = self.local_function_declaration(span)?;
        Some(self.lower_declared_function_value(function, &gate, own_frame))
    }

    /// The local function DECLARATION the name at `span` binds, with the
    /// frame that declares it and whether that frame is this one.
    fn local_function_declaration(
        &self,
        span: oxc_span::Span,
    ) -> Option<(&'a oxc_ast::ast::Function<'a>, Arc<DefiningFrameGate>, bool)> {
        use verter_semantic::analysis::flow::FlowBindingOccurrence;
        let (gate, local) = match self.bindings.occurrence(self.rebase(span)) {
            // The declaration the occurrence names exactly — never its
            // runtime alias (a function expression's own name and a body
            // declaration of that name share one runtime variable).
            FlowBindingOccurrence::Resolved(FlowBindingRef::Local(binding)) => {
                (Arc::clone(&self.frame_gate), *binding)
            }
            FlowBindingOccurrence::Resolved(FlowBindingRef::Captured(identity)) => {
                let mut current = self.captures.enclosing.as_deref();
                loop {
                    let frame = current?;
                    if frame.gate.bindings.function() == &identity.defining_function {
                        break (
                            Arc::clone(&frame.gate),
                            frame.gate.bindings.local(identity)?,
                        );
                    }
                    current = frame.gate.outer.enclosing.as_deref();
                }
            }
            _ => return None,
        };
        if gate.skeleton.binding(local).kind != SkeletonBindingKind::NestedFunction {
            return None;
        }
        let own_frame = Arc::ptr_eq(&gate, &self.frame_gate);
        // The declaration node, found by its name: the walk descends only
        // into nodes that contain it.
        struct Finder<'a> {
            name: verter_span::Span,
            found: Option<&'a oxc_ast::ast::Function<'a>>,
        }
        impl<'a> Visit<'a> for Finder<'a> {
            fn visit_statement(&mut self, statement: &Statement<'a>) {
                let span = statement.span();
                if self.found.is_none()
                    && span.start <= self.name.start
                    && span.end >= self.name.end
                {
                    walk::walk_statement(self, statement);
                }
            }
            fn visit_expression(&mut self, expression: &Expression<'a>) {
                let span = expression.span();
                if self.found.is_none()
                    && span.start <= self.name.start
                    && span.end >= self.name.end
                {
                    walk::walk_expression(self, expression);
                }
            }
            fn visit_function(
                &mut self,
                function: &oxc_ast::ast::Function<'a>,
                flags: oxc_syntax::scope::ScopeFlags,
            ) {
                if self.found.is_some() {
                    return;
                }
                if function.r#type == oxc_ast::ast::FunctionType::FunctionDeclaration
                    && function.id.as_ref().is_some_and(|id| {
                        id.span.start == self.name.start && id.span.end == self.name.end
                    })
                {
                    self.found = Some(self.alloc(function));
                    return;
                }
                walk::walk_function(self, function, flags);
            }
        }
        // The occurrence's own declaration, else the last function
        // declaration of its runtime variable: a body declaration of a
        // function expression's own name is the value the name reads.
        let candidates = std::iter::once(local).chain(
            gate.bindings
                .runtime_declarations(local)
                .iter()
                .rev()
                .copied(),
        );
        for candidate in candidates {
            let fact = gate.skeleton.binding(candidate);
            if fact.kind != SkeletonBindingKind::NestedFunction {
                continue;
            }
            let mut finder = Finder {
                name: fact.span.to_absolute(gate.anchor),
                found: None,
            };
            finder.visit_program(self.program);
            if let Some(function) = finder.found {
                return Some((function, gate, own_frame));
            }
        }
        None
    }

    /// Whether `argument` of a `return` is a bare call of this frame's own
    /// function declaration (`return rec(n - 1)` inside `function rec`):
    /// the checker's `checkAndAggregateReturnExpressionTypes` lets such a
    /// return contribute nothing (parentheses, and an `await` in an async
    /// function, peeled).
    fn returns_bare_self_call(&self, argument: &Expression<'_>) -> bool {
        let mut expression = unwrap_parenthesized(argument);
        if let Expression::AwaitExpression(awaited) = expression {
            if self.frame_is_async {
                expression = unwrap_parenthesized(&awaited.argument);
            }
        }
        let Expression::CallExpression(call) = expression else {
            return false;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return false;
        };
        let Some((function, gate, _)) = self.local_function_declaration(callee.span) else {
            return false;
        };
        self.index
            .nested_at(gate.bindings.function(), function.span.into())
            .is_some_and(|entry| &entry.entry().key == self.bindings.function())
    }

    /// The function value of the local function DECLARATION `function`,
    /// declared in the frame `gate` (this frame when `own_frame`). See
    /// [`Self::lower_local_function_declaration`].
    fn lower_declared_function_value(
        &mut self,
        function: &oxc_ast::ast::Function<'_>,
        gate: &Arc<DefiningFrameGate>,
        own_frame: bool,
    ) -> SliceExpr {
        let node = FunctionNode::Function(function);
        let Some(entry) = self
            .index
            .nested_at(gate.bindings.function(), node_span(&node).into())
        else {
            return SliceExpr::UnmodeledBinding;
        };
        let entry = entry.entry();
        let captures = CaptureScope {
            enclosing: Some(Arc::new(CapturedFrame {
                gate: Arc::clone(gate),
                region: gate.skeleton.innermost_region_containing(FrameSpan::rebase(
                    gate.anchor,
                    function.span.into(),
                )),
            })),
        };
        // Every capture reads its declared type where the value is read: the
        // evaluator supplies it for a declaring-frame parameter and whole
        // `let` / `var` ([`Self::capture_reads_declared_type`]), whatever
        // the frame assigns. A destructured one, a binding under an active
        // guard whose declared authority is not read, and any mutable binding
        // of an ENCLOSING frame takes the typed gap. A captured EVOLVING
        // array reads its declared type (`any[]`) as every capture a
        // function does not extend does.
        let mut gap = None;
        let mut declared_evolving_captures = Vec::new();
        let mut checked = rustc_hash::FxHashSet::default();
        for read in entry.captured_reads.iter() {
            let identity = &read.binding;
            if !checked.insert(identity) {
                continue;
            }
            let Some(binding) = gate.bindings.local(identity) else {
                continue;
            };
            let binding = gate.bindings.canonical_local(binding);
            let fact = gate.skeleton.binding(binding);
            if fact.evolving_array {
                declared_evolving_captures.push(identity.clone());
                continue;
            }
            let retyped = if own_frame {
                match fact.kind {
                    SkeletonBindingKind::Let
                    | SkeletonBindingKind::Var
                    | SkeletonBindingKind::Param => !self.capture_reads_declared_type(binding),
                    _ => false,
                }
            } else {
                matches!(
                    fact.kind,
                    SkeletonBindingKind::Let | SkeletonBindingKind::Var
                ) || (fact.kind == SkeletonBindingKind::Param && fact.destructured)
            };
            if retyped
                || (own_frame
                    && self.active_guard_bindings.contains(&binding)
                    && !self.capture_reads_declared_type(binding))
            {
                gap = Some(crate::semantic_query::FlowGap::ClosureCapture);
                break;
            }
        }
        SliceExpr::NestedFunctionValue {
            function: entry.key.clone(),
            context: Arc::new(NestedFlowContext {
                captures,
                // A function declaration's `this` is its own.
                this: None,
            }),
            has_declared_return: function.return_type.is_some(),
            gap,
            declared_evolving_captures: Arc::from(declared_evolving_captures.into_boxed_slice()),
            extended_captures: Arc::from([]),
        }
    }

    /// Lower a nested function value, `invocation` naming the call that
    /// invokes it where it is created (an IIFE).
    ///
    /// A capture reads the narrowing reaching the function's creation when
    /// the checker extends the capture's control-flow container to the
    /// enclosing one: a `const`, or a parameter / `let` past its last
    /// assignment (never a `var`, never inside a class property
    /// initializer). An immediately invoked function — async and generator
    /// ones included — is no control-flow container of its own, so every
    /// capture reads the narrowing reaching the call, whatever is assigned
    /// later or by another nested function, unless the call's own arguments
    /// write it.
    fn lower_function_value(
        &mut self,
        node: &FunctionNode<'_>,
        invocation: Option<&oxc_ast::ast::CallExpression<'_>>,
    ) -> SliceExpr {
        let member_this = self.member_this.take();
        let Some(entry) = self
            .index
            .nested_at(self.bindings.function(), node_span(node).into())
        else {
            return SliceExpr::UnmodeledBinding;
        };
        let entry = entry.entry();
        let captures = self.capture_scope_for(node_span(node));
        let invoked_arguments =
            invocation.map(|call| oxc_span::Span::new(call.callee.span().end, call.span.end));
        let mut extended_captures: Vec<SkeletonBindingId> = Vec::new();
        if self.class_property_initializers == 0 {
            for read in entry.captured_reads.iter() {
                let Some(binding) = self.bindings.local(&read.binding) else {
                    continue;
                };
                if extended_captures.contains(&binding) {
                    continue;
                }
                // A `var` is never a mutable local the checker extends
                // (`isMutableLocalVariableDeclaration` is `let`-only), and a
                // `let` declared after the creation is assigned after it.
                // An invoked function's captures read the call's flow, so
                // only a write among the call's arguments (which run before
                // the body) moves them.
                let fact = self.skeleton.binding(binding);
                let past_last_assignment = || {
                    !self.nested_free_writes.contains(&binding)
                        && self.is_past_last_assignment(binding, node_span(node))
                };
                let declared_before = || fact.span < self.rebase(node_span(node));
                let eligible = match (fact.kind, invoked_arguments) {
                    (SkeletonBindingKind::Const, _) => true,
                    (SkeletonBindingKind::Param, Some(arguments)) => {
                        !self.binding_has_write_within(binding, arguments)
                    }
                    (
                        SkeletonBindingKind::Let
                        | SkeletonBindingKind::Var
                        | SkeletonBindingKind::CatchParam,
                        Some(arguments),
                    ) => declared_before() && !self.binding_has_write_within(binding, arguments),
                    (SkeletonBindingKind::Param, None) => past_last_assignment(),
                    (SkeletonBindingKind::Let | SkeletonBindingKind::CatchParam, None) => {
                        declared_before() && past_last_assignment()
                    }
                    _ => false,
                };
                if eligible {
                    extended_captures.push(binding);
                }
            }
        }
        let mut gap = None;
        let mut declared_evolving_captures = Vec::new();
        let mut checked = rustc_hash::FxHashSet::default();
        for read in entry.captured_reads.iter() {
            let identity = &read.binding;
            if !checked.insert(identity) {
                continue;
            }
            let Some(binding) = self.bindings.local(identity) else {
                continue;
            };
            // A captured EVOLVING array reads its declared type unless it is
            // a `let` past its last assignment, whose flow the function
            // continues from where it is created — its operations evolve
            // the array it holds there (tsc 7.0.2: `let a = []; a.push(1);
            // return () => a` is `() => number[]`, `const` and `var` alike
            // `() => any[]`).
            let fact = self.skeleton.binding(binding);
            if fact.evolving_array {
                if fact.kind != SkeletonBindingKind::Let
                    || self.nested_free_writes.contains(&binding)
                    || !self.is_past_last_assignment(binding, node_span(node))
                {
                    declared_evolving_captures.push(identity.clone());
                }
                continue;
            }
            // A guard's narrowing reaches an extended capture's body. Any
            // other capture reads its DECLARED type there, whatever the
            // enclosing body assigns before or after the creation or
            // another closure assigns: the evaluator supplies it for a
            // parameter and an annotated `let` / `var` (their declared
            // authority) and for an unannotated `let` / `var` (its
            // initializer's widened type). Every other capture that is
            // mutable where the function is created — a destructured
            // element, a `catch` parameter — takes the typed gap.
            if extended_captures.contains(&binding) || self.capture_reads_declared_type(binding) {
                continue;
            }
            if self.active_guard_bindings.contains(&binding)
                || (matches!(
                    fact.kind,
                    SkeletonBindingKind::Let | SkeletonBindingKind::CatchParam
                ) && (self.nested_free_writes.contains(&binding)
                    || self.binding_has_write_after(binding, node_span(node))))
                || (fact.kind == SkeletonBindingKind::Var
                    && self.binding_has_write_before(binding, node_span(node)))
            {
                gap = Some(crate::semantic_query::FlowGap::ClosureCapture);
                break;
            }
        }
        SliceExpr::NestedFunctionValue {
            function: entry.key.clone(),
            context: Arc::new(NestedFlowContext {
                captures,
                // An arrow has no `this` of its own: it reads its creating
                // frame's (a class expression's instance initializer reads
                // the class's own receiver). A class expression's member
                // function reads the receiver the class binds.
                this: match member_this {
                    Some(this) => this,
                    None => match node {
                        FunctionNode::Arrow(_) => self.this.clone(),
                        FunctionNode::Function(_) => None,
                    },
                },
            }),
            has_declared_return: node.return_type().is_some(),
            gap,
            declared_evolving_captures: Arc::from(declared_evolving_captures.into_boxed_slice()),
            extended_captures: Arc::from(extended_captures.into_boxed_slice()),
        }
    }

    /// The checker's `isPastLastAssignment` for a `let` binding at a
    /// function created at `creation_span`: every whole-binding write of the
    /// frame counts up to the end of the outermost statement enclosing it
    /// that starts after the declaration (`extendAssignmentPosition` — an
    /// assignment in a loop reaches the loop's end), and the function must
    /// start after all of them.
    fn is_past_last_assignment(
        &self,
        binding: SkeletonBindingId,
        creation_span: oxc_span::Span,
    ) -> bool {
        let declaration = self.skeleton.binding(binding).span.to_absolute(self.anchor);
        let runtime = self.bindings.canonical_local(binding);
        self.skeleton.writes.iter().all(|write| {
            if !write.path.is_empty()
                || !matches!(write.binding, Some(FlowBindingRef::Local(local))
                    if self.bindings.canonical_local(local) == runtime)
            {
                return true;
            }
            let written = write.span.to_absolute(self.anchor);
            let end = self
                .assignment_extent_statements
                .iter()
                .filter(|statement| {
                    statement.start > declaration.start
                        && statement.start <= written.start
                        && statement.end >= written.end
                })
                .map(|statement| statement.end)
                .fold(written.end, u32::max);
            end < creation_span.start
        })
    }

    /// Lower a leaf expression through the shared shallow-pass entry,
    /// wrapping the result. A semantically complete `any` surfaces as
    /// [`SliceExpr::SemanticAny`], while an unmodelled fallback surfaces as
    /// [`SliceExpr::Gap`]. A modelled answer naming a frame binding rides the
    /// [`SliceExpr::FrameShadowed`] carrier.
    fn lower_leaf(&mut self, expr: &Expression<'_>, mode: ExprMode) -> SliceExpr {
        match self.leaf_type(expr, mode) {
            LeafLowering::Unmodeled => {
                SliceExpr::Gap(crate::semantic_query::FlowGap::UnmodeledExpression)
            }
            // THE fabricated-value gate, in ONE arm over BOTH shapes the
            // shallow pass produces for a call it cannot model.
            //
            // Shape one — the unreduced `ReturnType<callee>` carrier:
            // honest for a declaration initializer that is re-resolved
            // later, a FOREIGN binder for a consumer that publishes the
            // answer (nothing instantiated the callee's own clause and
            // nothing consulted its overload group).
            //
            // Shape two — a fabricated `any`, at the root (`return
            // new Box()`) or NESTED inside an otherwise-modelled answer
            // (`["s", new Box()]` is `Array<string | any>`). The nested
            // case carries no carrier and is not itself `any`, so both
            // halves of the old gate passed it warm and clean. It is
            // decided on the FORM — does this expression's value compose
            // over a call with no structural arm — conjoined with "the
            // answer embeds `any`", so a form whose answer the pass DOES
            // model (`f() === 1` is `boolean`) is never refused.
            LeafLowering::Free(ty) | LeafLowering::FrameShadowed { ty, .. }
                if leaf_answer_is_fabricated_at_a_call_position(&ty, expr) =>
            {
                SliceExpr::UnreducedCallValue
            }
            // A semantically complete `any` is WARM-ADMISSIBLE — so it is
            // exactly the arm that cannot skip the leaf call scanner: the
            // leaf still RUNS (a class static block inside it executes at
            // class evaluation), and an `asserts` callee there narrows
            // what follows in the checker. The calls take the same
            // per-callee certification the modelled arms apply; an
            // unprovable one flags the enclosing statement's typed gap.
            LeafLowering::Free(ty) if is_any(&ty) => {
                self.record_decided_above_calls(expr);
                SliceExpr::SemanticAny
            }
            LeafLowering::Free(ty) => {
                self.record_decided_above_calls(expr);
                SliceExpr::Type(GatedLeaf(ty.clone(), self.frame_root_for_type(&ty, expr)))
            }
            LeafLowering::FrameShadowed { ty, shadowed } => {
                self.record_decided_above_calls(expr);
                SliceExpr::FrameShadowed {
                    inner: Box::new(SliceExpr::Type(GatedLeaf(
                        ty.clone(),
                        self.frame_root_for_type(&ty, expr),
                    ))),
                    shadowed,
                }
            }
        }
    }

    /// Record the authored call / construct spans of one DECIDED-ABOVE
    /// position — see [`SliceContent::decided_above_call_spans`]. Called
    /// only by positions whose produced type PROVABLY does not derive
    /// from any call inside the expression: the leaf arms that passed
    /// the fabricated-value gate. (The optional-`any`-chain carrier takes
    /// [`Self::record_optional_any_chain_calls`] — its terminal call is
    /// decided on the chain's own account, not this one's.)
    ///
    /// "The produced type does not derive from the calls" is NOT "the
    /// calls cannot narrow": the shallow pass folds control-flow-bearing
    /// forms into one leaf answer WITHOUT visiting them (`[isString(x) ?
    /// x : false]`, `(assertString(x) as void)`, `((0, assertString(x))
    /// as unknown)`), so a call inside a leaf can still narrow a read of
    /// this frame — through a nested test's branch narrowing or through
    /// an `asserts` callee in ANY same-frame position — and a class
    /// static block inside a leaf RUNS at class evaluation. Every
    /// same-frame call / construct of the lowered expression therefore
    /// takes the SAME per-callee certification the control-position and
    /// discarded-operand arms apply ([`Self::certify_result_independent_calls`]):
    /// a nested control position (a ternary test, a `&&` / `||` left
    /// operand, a statement test inside an immediately-evaluated static
    /// block) certifies under [`ResultIndependentPosition::ControlTest`],
    /// every other same-frame position under [`ResultIndependentPosition::
    /// DiscardedOperand`] (a value position's predicate narrows nothing;
    /// only an `asserts` callee narrows what follows). A call that
    /// provably establishes no narrowing is decided above; every other
    /// one flags the enclosing statement's typed `GuardNarrowing` gap.
    ///
    /// A nested function / class body is its OWN frame — its names never
    /// resolve against this frame's skeleton and its calls run at ITS
    /// evaluation, never in this statement — so its calls keep the
    /// blanket decided-above treatment they always had. The exceptions
    /// evaluate in the ENCLOSING frame and take the same-frame
    /// discipline: a class's decorators, `super_class` heritage
    /// expression, computed member keys, member decorators, STATIC BLOCKS,
    /// and static property / accessor initializers (enclosing-frame-
    /// immediate at class evaluation — unlike methods, which run when
    /// called, and instance initializers, which run at construction).
    fn record_decided_above_calls(&mut self, expr: &Expression<'_>) {
        let mut scanner = LeafCallScanner::default();
        scanner.visit_expression(expr);
        self.drain_leaf_call_scanner(scanner);
    }

    /// Scan one UNMODELED position whose expressions still EXECUTE at the
    /// enclosing statement — an elided (unselected) or destructured
    /// declarator's initializer, an enum member initializer: the demand
    /// slice never selected the position, so no call obligation reaches it
    /// and nothing lowers it, yet an assertion call narrows and a
    /// whole-binding write retypes what follows in the checker. The ONE
    /// shared scanner answers for it, under
    /// [`CertificationMode::ValueFree`]: the position's own value feeds no
    /// read, so a call certifies unless it could carry an `asserts`
    /// narrowing of a FRAME-OWNED binding (an exported callee's merged
    /// signature set can hide one — [`Lowerer::closed_callee_declaration`]
    /// decides), and any frame-owned whole-binding write is unprovable.
    /// Anything unprovable flags the enclosing statement's typed
    /// `GuardNarrowing` gap (through [`Lowerer::control_test_gap`], which
    /// the statement loop drains AHEAD of the statement). A position with
    /// no frame-reaching effect — a pure literal, a call with no
    /// frame-owned subject, a nested-frame-only initializer — stays
    /// silent.
    fn scan_unmodeled_position_effects(&mut self, expr: &Expression<'_>) {
        let mut scanner = LeafCallScanner::default();
        scanner.visit_expression(expr);
        self.decided_above_call_spans.append(&mut scanner.decided);
        if self.drain_scanned_same_frame_effects(
            scanner,
            CertificationMode::ValueFree,
            WritePolicy::All,
        ) {
            self.control_test_gap = true;
        }
    }

    /// Scan the body statements of one executable namespace / module
    /// declaration: the block RUNS at the declaration statement, in this
    /// frame, while no content lowers for it — the same fail-closed
    /// discipline every unmodeled position takes. The skeleton indexes the
    /// block's statements (only nested function / class subtrees are its
    /// own frames), so visible writes keep their ledger verdict and only
    /// class-hidden ones gap. A nested `namespace A.B` chain executes with
    /// its outermost block; an ambient inner declaration runs nothing.
    fn scan_module_declaration_effects(&mut self, module: &oxc_ast::ast::TSModuleDeclaration<'_>) {
        let mut scanner = LeafCallScanner::default();
        let mut body = module.body.as_ref();
        while let Some(current) = body {
            match current {
                oxc_ast::ast::TSModuleDeclarationBody::TSModuleDeclaration(nested) => {
                    if nested.declare
                        || !matches!(
                            nested.id,
                            oxc_ast::ast::TSModuleDeclarationName::Identifier(_)
                        )
                    {
                        return;
                    }
                    body = nested.body.as_ref();
                }
                oxc_ast::ast::TSModuleDeclarationBody::TSModuleBlock(block) => {
                    for statement in &block.body {
                        scanner.visit_statement(statement);
                    }
                    body = None;
                }
            }
        }
        self.decided_above_call_spans.append(&mut scanner.decided);
        if self.drain_scanned_same_frame_effects(
            scanner,
            CertificationMode::ValueFree,
            WritePolicy::SkeletonHiddenOnly,
        ) {
            self.control_test_gap = true;
        }
    }

    /// Record the call / construct spans of one admitted OPTIONAL-`any`
    /// CHAIN. The chain's TERMINAL call is decided above on the chain's
    /// own account: the evaluator admits the [`SliceExpr::OptionalAnyChain`]
    /// carrier only while the reaching root is still `any` (a non-`any`
    /// root degrades at evaluation), and a call on an `any`-typed callee
    /// resolves no signature — it cannot carry an `asserts` predicate, so
    /// it narrows nothing. Every OTHER call the route carries — inside
    /// the terminal call's ARGUMENTS or a computed member key
    /// (`a?.b(assertString(x))` narrows `x` in the checker whether or not
    /// the receiver is `any`) — is an ordinary same-frame position and
    /// takes the per-callee certification of [`Self::record_decided_above_calls`].
    fn record_optional_any_chain_calls(
        &mut self,
        whole: &Expression<'_>,
        chain: &oxc_ast::ast::ChainExpression<'_>,
    ) {
        let mut scanner = LeafCallScanner::default();
        if let oxc_ast::ast::ChainElement::CallExpression(call) = &chain.expression {
            // The route admits at most one call, the terminal element;
            // the callee route itself holds none (only computed keys,
            // which the scanner reaches as ordinary expressions).
            self.decided_above_call_spans.push(call.span.into());
            scanner.visit_expression(&call.callee);
            for argument in &call.arguments {
                if let Some(expression) = argument.as_expression() {
                    scanner.visit_expression(expression);
                }
            }
        } else {
            scanner.visit_expression(whole);
        }
        self.drain_leaf_call_scanner(scanner);
    }

    /// Discharge one walked [`LeafCallScanner`] whose nested-frame
    /// (`decided`) calls ARE this run's decided-above positions — the leaf
    /// path, where a folded leaf's nested-frame calls keep the blanket
    /// certification they always had. Any unprovable same-frame effect
    /// flags the enclosing statement's typed gap.
    fn drain_leaf_call_scanner(&mut self, mut scanner: LeafCallScanner<'_>) {
        self.decided_above_call_spans.append(&mut scanner.decided);
        if self.drain_scanned_same_frame_effects(
            scanner,
            CertificationMode::Strict,
            WritePolicy::All,
        ) {
            self.control_test_gap = true;
        }
    }

    /// The effects one entered item applies once it has run, pushing the
    /// spans of the calls that apply them: a call's own `asserts`
    /// narrowing, and for a conditional the checker's join of its arms —
    /// an `if` over the test whose arms apply each arm's effects, so every
    /// reference reads the union of its per-arm narrowed types past it.
    /// Arms that apply the same effects are that effect list alone.
    fn entered_item_effects(
        &mut self,
        item: &ArmEntered<'_>,
        spans: &mut Vec<verter_span::Span>,
    ) -> Vec<SliceStatement> {
        match item {
            ArmEntered::Call(call) => {
                let assertion = self.entered_assertion(call);
                if assertion.is_some() {
                    spans.push(call.span.into());
                }
                assertion.map(assertion_statement).into_iter().collect()
            }
            ArmEntered::Join {
                test,
                consequent,
                alternate,
            } => {
                let mut arms = [Vec::new(), Vec::new()];
                for (arm, items) in arms.iter_mut().zip([consequent, alternate]) {
                    for item in items {
                        arm.extend(self.entered_item_effects(item, spans));
                    }
                }
                let [consequent, alternate] = arms;
                if consequent == alternate {
                    return consequent;
                }
                let guard = self.lower_guard(test);
                vec![SliceStatement::If {
                    guard,
                    consequent: entered_effect_region(consequent),
                    alternate: Some(entered_effect_region(alternate)),
                }]
            }
        }
    }

    /// Discharge the same-frame channels of one walked
    /// [`LeafCallScanner`]: the `control` / `discarded` channels certify
    /// per-callee under `mode`, and a collected same-frame WRITE admitted
    /// by `write_policy` whose target this frame OWNS is unprovable — the
    /// write runs at the enclosing statement but never enters the slice's
    /// effect ledger (the flow skeleton skips the class subtree), so
    /// neither this half nor the evaluator can model the retype the
    /// checker applies: a degraded success, never a silently certified
    /// superset. A FREE target writes no binding this frame tracks and
    /// stays silent. Returns whether anything was unprovable; the caller
    /// decides where the typed gap lands. The scanner's `decided` channel
    /// is NOT consumed here — whether a nested-frame call is recorded
    /// decided-above is the caller's position's rule, not this
    /// discharge's.
    fn drain_scanned_same_frame_effects(
        &mut self,
        scanner: LeafCallScanner<'_>,
        mode: CertificationMode,
        write_policy: WritePolicy,
    ) -> bool {
        self.decided_above_call_spans.extend(scanner.unentered);
        let mut applied: Vec<verter_span::Span> = Vec::new();
        if self.entered_assertion_sink.is_some() {
            for item in &scanner.entered {
                let effects = self.entered_item_effects(item, &mut applied);
                if let Some(sink) = self.entered_assertion_sink.as_mut() {
                    sink.extend(effects);
                }
            }
        }
        // A modeled `asserts` call whose narrowing joins away past its
        // conditional narrows nothing that follows: decided.
        for call in &scanner.joined_away {
            if self.entered_assertion(call).is_some() {
                applied.push(call.span.into());
                self.decided_above_call_spans.push(call.span.into());
            }
        }
        let not_applied = |call: &ControlCall| match call {
            ControlCall::Call { span, .. } => !applied.contains(span),
            _ => true,
        };
        let control: Vec<ControlCall> = scanner.control.into_iter().filter(not_applied).collect();
        let discarded: Vec<ControlCall> =
            scanner.discarded.into_iter().filter(not_applied).collect();
        let control_unprovable = self.certify_result_independent_calls(
            control,
            ResultIndependentPosition::ControlTest,
            mode,
        );
        let discarded_unprovable = self.certify_result_independent_calls(
            discarded,
            ResultIndependentPosition::DiscardedOperand,
            mode,
        );
        let unmodelled_write = scanner
            .writes
            .into_iter()
            .any(|(_name, span, skeleton_hidden)| {
                use verter_semantic::analysis::flow::FlowBindingOccurrence;
                // This scanner collects whole-binding targets only. A proven
                // static-block local does not write a tracked function subject.
                (matches!(write_policy, WritePolicy::All) || skeleton_hidden)
                    && !matches!(
                        self.bindings.occurrence(self.rebase(span)),
                        FlowBindingOccurrence::Free | FlowBindingOccurrence::UnmodeledLocal
                    )
            });
        control_unprovable || discarded_unprovable || unmodelled_write
    }

    /// Record the RESULT-INDEPENDENT call / construct spans of one
    /// CONTROL-POSITION test (an `if` / ternary test). The demanded
    /// value never consumes a test's VALUE, but a test call's RESULT can
    /// still decide the narrowing the arms evaluate under — a
    /// type-predicate callee — so a control call is decided above ONLY
    /// when the callee provably establishes no narrowing:
    /// - a `new` construct (a construct signature cannot be a type
    ///   predicate, and the checker derives no narrowing from one);
    /// - a tagged template (the checker derives no narrowing from one,
    ///   whatever its tag's signature);
    /// - a bare-identifier callee resolving FREE to a PROVABLY CLOSED
    ///   same-file declaration ([`Self::closed_callee_declaration`]: a
    ///   module-scoped file, a call site outside every namespace block,
    ///   exactly one declaration, not exported by any
    ///   spelling) whose authored return annotation exists and is NOT a
    ///   type predicate (an inferred boolean return can be an inferred
    ///   predicate, so an unannotated declaration never qualifies; an
    ///   overload group's signature selection is beyond this half; a
    ///   script global's or an exported binding's checker-visible
    ///   signature set may hold a predicate overload this file never
    ///   shows).
    ///
    /// A call that minted a [`SliceGuard::TypePredicate`] fact takes
    /// REAL evaluator evidence at guard application instead. Every OTHER
    /// control call is one this half can neither certify nor evidence —
    /// the callee could be a predicate whose narrowing the checker
    /// applies and this substrate does not — so the test returns `true`
    /// and the caller emits the typed `GuardNarrowing` gap: a degraded
    /// success, `ReturnOnly`, never a silently certified superset.
    fn record_control_position_calls(&mut self, test: &Expression<'_>) -> bool {
        self.record_result_independent_calls(test, ResultIndependentPosition::ControlTest)
    }

    /// Record the RESULT-INDEPENDENT call / construct spans of one
    /// DISCARDED sequence operand. The sequence's value never consumes
    /// the operand, but the checker binds a call in that position into
    /// the control flow (the left-hand side of a comma expression is a
    /// potential assertion): an `asserts` callee narrows every read that
    /// follows it. A discarded call is decided above ONLY when the
    /// callee provably establishes no narrowing:
    /// - a `new` construct (a construct signature is never an assertion);
    /// - a tagged template (the checker binds no assertion for one);
    /// - a bare-identifier callee resolving FREE to a PROVABLY CLOSED
    ///   same-file declaration ([`Self::closed_callee_declaration`])
    ///   whose return annotation is absent or is not an `asserts`
    ///   predicate — assertion signatures are never inferred, so an
    ///   unannotated closed declaration cannot assert, and a plain `x is
    ///   T` predicate narrows nothing when its result is discarded.
    ///
    /// Every other discarded call — an imported, exported, script-global
    /// or member callee, a closed `asserts` callee — is one this half can
    /// neither certify nor evidence, so the operand returns `true` and
    /// the caller flags the enclosing statement's typed `GuardNarrowing`
    /// gap: a degraded success, never a silently certified superset.
    fn record_discarded_operand_calls(&mut self, operand: &Expression<'_>) -> bool {
        self.record_result_independent_calls(operand, ResultIndependentPosition::DiscardedOperand)
    }

    /// The shared scanner behind [`Self::record_control_position_calls`]
    /// and [`Self::record_discarded_operand_calls`]: the ONE
    /// [`LeafCallScanner`] walks the expression (the same channel split,
    /// the same class phase split, and the same whole-binding WRITE
    /// collection the leaf path applies — a write hiding in a discarded
    /// operand's class static block runs at the enclosing statement
    /// exactly as a leaf's does), then the channels discharge through the
    /// position's rule: the whole expression takes `position` for its
    /// top-level calls (a control test seeds the control nesting; a
    /// discarded operand starts discarded), and nested control positions
    /// split exactly as the leaf path's. A nested frame's calls are not
    /// this frame's: the scanner's blanket `decided` channel is DROPPED
    /// here, never recorded — a call that runs only when the nested value
    /// is called has nothing to certify on this frame's account. Returns
    /// whether any effect was unprovable.
    fn record_result_independent_calls(
        &mut self,
        expr: &Expression<'_>,
        position: ResultIndependentPosition,
    ) -> bool {
        let mut scanner = LeafCallScanner::default();
        match position {
            ResultIndependentPosition::ControlTest => scanner.control_nesting = 1,
            // A comma operator's left operand: its own call is entered.
            ResultIndependentPosition::DiscardedOperand => {
                if let Expression::CallExpression(call) = expr {
                    scanner.comma_operand_calls.insert(call.span);
                }
            }
        }
        scanner.visit_expression(expr);
        self.drain_scanned_same_frame_effects(
            scanner,
            CertificationMode::Strict,
            WritePolicy::SkeletonHiddenOnly,
        )
    }

    /// Certify one collected set of result-independent calls: each is
    /// either pushed decided-above or reported unprovable (`true`). A call
    /// whose [`SliceGuard::TypePredicate`] fact this lowering minted is
    /// evidence-backed at guard application instead — neither certified
    /// nor gapped.
    ///
    /// `mode` decides how hard an unprovable-by-closure call is chased.
    /// [`CertificationMode::Strict`] is the rule for every position whose
    /// evaluation the demand can observe: any call that is not certified
    /// under `position`'s rule is unprovable. [`CertificationMode::ValueFree`]
    /// is the rule for a position whose VALUE nothing consumes (an elided
    /// declaration position): the call's own result feeds no read, so the
    /// one effect that can still change what the frame later evaluates is
    /// an `asserts` narrowing of a FRAME-OWNED binding — a call with no
    /// frame-owned reference among its assertion-subject roots cannot
    /// carry one and certifies outright, and one that could still
    /// certifies exactly when its callee is a provably closed same-file
    /// declaration whose return provably is not an `asserts` predicate
    /// (the [`ResultIndependentPosition::DiscardedOperand`] rule).
    fn certify_result_independent_calls(
        &mut self,
        calls: Vec<ControlCall>,
        position: ResultIndependentPosition,
        mode: CertificationMode,
    ) -> bool {
        let mut unprovable = false;
        for call in calls {
            let span = match call {
                ControlCall::Construct(span) | ControlCall::TaggedTemplate(span) => span,
                ControlCall::Call {
                    span,
                    callee,
                    assertion_subject_roots,
                } => {
                    if self.predicate_guard_call_spans.contains(&span) {
                        // Evidence-backed at guard application: neither
                        // certified here nor gapped.
                        continue;
                    }
                    let closed_non_narrowing = |position: ResultIndependentPosition| {
                        callee.as_ref().is_some_and(|(name, callee_span)| {
                            // A discarded call narrows only as an
                            // assertion, which a frame binding's declared
                            // type decides.
                            if matches!(position, ResultIndependentPosition::DiscardedOperand)
                                && matches!(
                                    self.callee_binding_effect(*callee_span),
                                    Some(StatementCallEffect::Inert)
                                )
                            {
                                return true;
                            }
                            matches!(self.classify_occurrence(*callee_span), NameBinding::Free)
                                && self
                                    .closed_callee_declaration(name)
                                    .is_some_and(|function| {
                                        position.certifies_closed_return(
                                            function.return_type.as_deref(),
                                        )
                                    })
                        })
                    };
                    let certified = match mode {
                        CertificationMode::Strict => {
                            self.non_narrowing_call_spans.contains(&span)
                                || closed_non_narrowing(position)
                        }
                        CertificationMode::ValueFree => {
                            let frame_subject =
                                assertion_subject_roots.iter().any(|(_name, span)| {
                                    !matches!(self.classify_occurrence(*span), NameBinding::Free)
                                });
                            !frame_subject
                                || closed_non_narrowing(ResultIndependentPosition::DiscardedOperand)
                        }
                    };
                    if !certified {
                        unprovable = true;
                        continue;
                    }
                    span
                }
            };
            self.decided_above_call_spans.push(span);
        }
        unprovable
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
        self.frame_gate
            .answer_names_frame_bound(ty, self.rebase(span), binders)
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
    /// Every answer produced through the shared shallow-pass leaf path is
    /// minted here and carries the root-identifier gate's verdict. Dedicated
    /// frame carriers (including bare identifier reads) are lowered by their
    /// own typed arms rather than through this leaf path.
    /// Resolve a leaf's value root while its exact lexical position is available.
    fn frame_root_for_type(&self, ty: &TypeExpr, expr: &Expression<'_>) -> Option<FlowBindingRef> {
        let value = match ty {
            TypeExpr::TypeOf(value) => value,
            TypeExpr::Ref { type_arguments, .. } if type_arguments.len() == 1 => {
                let TypeExpr::TypeOf(value) = &type_arguments[0] else {
                    return None;
                };
                value
            }
            _ => return None,
        };
        let root = chain_root_identifier(expr)?;
        (value.path.first()?.as_str() == root.name.as_str())
            .then(|| self.binding_at(root.span))
            .flatten()
    }

    fn leaf_type(&mut self, expr: &Expression<'_>, mode: ExprMode) -> LeafLowering {
        // A JSX element / fragment's value is the configured `JSX`
        // namespace's `Element` type — a TYPE-space reference the shared
        // lowering resolves exactly as an authored `JSX.Element`
        // annotation would (the global `JSX` namespace here; the gate's
        // `Namespace`-meaning probe below covers a frame-local shadow of
        // it). The element's own structure — attributes, children — never
        // contributes to the value, so this is a whole-form leaf answer,
        // and the shared shallow pass has no arm for the form itself.
        if matches!(expr, Expression::JSXElement(_) | Expression::JSXFragment(_)) {
            let ty = TypeExpr::Ref {
                name: Arc::from("JSX.Element"),
                type_arguments: Arc::from(Vec::new().into_boxed_slice()),
            };
            let shadowed = self.answer_names_frame_bound(&ty, expr.span(), &[]);
            if shadowed.is_empty() {
                return LeafLowering::Free(ty);
            }
            return LeafLowering::FrameShadowed {
                ty,
                shadowed: Arc::from(shadowed.into_boxed_slice()),
            };
        }
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
        let nested_nullish = if self.nullability.is_strict() {
            NestedNullishLiterals::Keep
        } else {
            NestedNullishLiterals::WidenToAny
        };
        let inference = infer_declaration_expression_type_with_nested_nullish(
            expr,
            self.source,
            policy,
            nested_nullish,
        );
        let (ty, completeness) = inference
            .map(|inference| (inference.ty, inference.completeness))
            .unwrap_or_else(|reason| {
                if self.budget_failure.is_none() {
                    self.budget_failure = Some(reason);
                }
                (
                    TypeExpr::Primitive(PrimitiveName::Any),
                    ExpressionInferenceCompleteness::Complete,
                )
            });
        if completeness == ExpressionInferenceCompleteness::Unmodeled {
            return LeafLowering::Unmodeled;
        }
        if is_any(&ty) {
            return LeafLowering::Free(ty);
        }
        // A leaf expression is a BODY position: it sits IN this frame's
        // region chain, so no clause is NEARER than the frame's own
        // lexical authority. This frame's clause answers at its own step,
        // behind the skeleton.
        let shadowed = self.answer_names_frame_bound(&ty, expr.span(), &[]);
        if shadowed.is_empty() {
            self.namespace_scoped_leaf(ty)
        } else {
            LeafLowering::FrameShadowed {
                ty,
                shadowed: Arc::from(shadowed.into_boxed_slice()),
            }
        }
    }

    /// A free leaf answer of a namespace-owned function: a value name an
    /// enclosing namespace block declares is that block's member, not the
    /// file's top-level name — the innermost block first. A read of an
    /// EXPORTED member is its qualified declaration (`N.k`); a member the
    /// block does not export has no declaration of its own the lane can
    /// address, and an answer that embeds a block member in a composite
    /// is not rewritten, so both fail closed.
    fn namespace_scoped_leaf(&self, ty: TypeExpr) -> LeafLowering {
        if self.namespace_scopes.is_empty() {
            return LeafLowering::Free(ty);
        }
        let names = verter_type_expr::referenced_names(&ty);
        let block_member = |root: &str| {
            self.namespace_scopes
                .iter()
                .rev()
                .find_map(|scope| scope.names.get(root).map(|exported| (scope, *exported)))
        };
        if !names
            .value_roots
            .iter()
            .any(|root| block_member(root).is_some())
        {
            return LeafLowering::Free(ty);
        }
        let TypeExpr::TypeOf(value) = &ty else {
            return LeafLowering::Unmodeled;
        };
        let Some((scope, true)) = value.path.first().and_then(|root| block_member(root)) else {
            return LeafLowering::Unmodeled;
        };
        if !value.type_args.is_empty() {
            return LeafLowering::Unmodeled;
        }
        let mut path: Vec<String> = scope.qualified.split('.').map(str::to_string).collect();
        path.extend(value.path.iter().cloned());
        LeafLowering::Free(TypeExpr::TypeOf(verter_type_expr::ValueRef {
            path,
            type_args: Vec::new(),
        }))
    }
}

/// Whether a parameter's declared type provably cannot be a discriminated
/// union — the only shape whose destructured elements are correlated with
/// one another.
fn param_type_forbids_correlation(ty: &TypeExpr) -> bool {
    match ty {
        TypeExpr::Object(_) | TypeExpr::Primitive(_) | TypeExpr::Literal(_) => true,
        TypeExpr::Intersection(arms) => arms.iter().all(param_type_forbids_correlation),
        _ => false,
    }
}

/// How a statement call's effects signature is settled
/// ([`Lowerer::effect_callee`]).
enum EffectCallee {
    /// The callee has no explicit type: the call neither asserts nor ends
    /// the path.
    Inert,
    /// The evaluator reads the callee's signatures.
    Settle(SliceEffectCallee),
    /// Neither half can settle it: the typed guard-narrowing gap.
    Unprovable,
}

/// What ONE STATEMENT-POSITION call establishes.
///
/// A bare call statement feeds no value, but two of its effects reach the
/// demand and neither is visible at the call site: an `asserts` callee
/// narrows every read that follows, and a callee that never returns ENDS
/// the path — the statements after it are unreachable and the checker
/// drops their contributions. So the callee must be PROVEN, exactly as a
/// control test's is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatementCallEffect {
    /// The callee provably establishes no narrowing AND provably
    /// returns: the call is decided above and the path continues.
    Inert,
    /// A closed same-file callee whose authored return is `never`: the
    /// statement ends the path exactly as an authored `throw` does.
    NeverReturns,
    /// The callee could narrow what follows or could end the path, and
    /// this half can prove neither.
    Unprovable,
}

/// Whether a return-type annotation's SYNTAX cannot denote `never`.
///
/// A name is never provable here — `type Fail = never` makes `Foo` a
/// `never` return with no syntactic tell — and neither is a conditional,
/// indexed-access, mapped, intersection or `keyof` form, whose result
/// this half does not compute. A union is provable through any single
/// member that is: a union with a non-`never` member is not `never`.
fn annotation_provably_not_never(ty: &TSType<'_>) -> bool {
    match ty {
        TSType::TSAnyKeyword(_)
        | TSType::TSBigIntKeyword(_)
        | TSType::TSBooleanKeyword(_)
        | TSType::TSNullKeyword(_)
        | TSType::TSNumberKeyword(_)
        | TSType::TSObjectKeyword(_)
        | TSType::TSStringKeyword(_)
        | TSType::TSSymbolKeyword(_)
        | TSType::TSThisType(_)
        | TSType::TSUndefinedKeyword(_)
        | TSType::TSUnknownKeyword(_)
        | TSType::TSVoidKeyword(_)
        | TSType::TSLiteralType(_)
        | TSType::TSTemplateLiteralType(_)
        | TSType::TSTupleType(_)
        | TSType::TSArrayType(_)
        | TSType::TSTypeLiteral(_)
        | TSType::TSFunctionType(_)
        | TSType::TSConstructorType(_) => true,
        TSType::TSParenthesizedType(inner) => annotation_provably_not_never(&inner.type_annotation),
        TSType::TSUnionType(union) => union.types.iter().any(annotation_provably_not_never),
        _ => false,
    }
}

/// The checker's `extendAssignmentPosition` over one frame-relative write:
/// the outermost variable, expression, `if`, loop, `with`, `switch`, `try`
/// or class-declaration statement holding the write that begins after the
/// binding's declaration.
struct AssignmentExtent {
    anchor: u32,
    write: FrameSpan,
    declaration: FrameSpan,
    found: Option<FrameSpan>,
}

impl<'a> Visit<'a> for AssignmentExtent {
    fn visit_statement(&mut self, it: &Statement<'a>) {
        if self.found.is_some() {
            return;
        }
        let span = FrameSpan::rebase(self.anchor, it.span().into());
        if !span.contains(self.write) {
            return;
        }
        let extends = matches!(
            it,
            Statement::VariableDeclaration(_)
                | Statement::ExpressionStatement(_)
                | Statement::IfStatement(_)
                | Statement::DoWhileStatement(_)
                | Statement::WhileStatement(_)
                | Statement::ForStatement(_)
                | Statement::ForInStatement(_)
                | Statement::ForOfStatement(_)
                | Statement::WithStatement(_)
                | Statement::SwitchStatement(_)
                | Statement::TryStatement(_)
                | Statement::ClassDeclaration(_)
        );
        if extends && span > self.declaration && !span.contains(self.declaration) {
            self.found = Some(span);
            return;
        }
        walk::walk_statement(self, it);
    }
}

/// A top-level callee whose declaration set this file closes.
#[derive(Debug, Clone, Copy)]
struct ClosedCalleeKind {
    /// Whether the declaration carries an explicit type — a function, or
    /// an annotated variable.
    explicit: bool,
}

/// What one declaration's explicit type says about the control-flow effect
/// of a statement call through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeclaredCallEffect {
    /// No declared signature asserts or returns `never`: an unannotated
    /// parameter, an `any` annotation, a function type or a function
    /// declaration returning anything else.
    Inert,
    /// A declared signature returns `asserts x` / `asserts x is T`.
    Asserts,
    /// A declared signature returns `never`.
    Never,
    /// An annotation whose call signatures this syntax does not show.
    Undecided,
}

/// The effect a declared return annotation gives a call statement.
fn declared_return_effect(
    annotation: Option<&oxc_ast::ast::TSTypeAnnotation<'_>>,
) -> DeclaredCallEffect {
    match annotation.map(|annotation| &annotation.type_annotation) {
        Some(TSType::TSTypePredicate(predicate)) if predicate.asserts => {
            DeclaredCallEffect::Asserts
        }
        Some(TSType::TSNeverKeyword(_)) => DeclaredCallEffect::Never,
        _ => DeclaredCallEffect::Inert,
    }
}

/// The effect a variable or parameter annotation gives a call statement
/// through it: its type's call signatures.
fn declared_type_effect(
    annotation: Option<&oxc_ast::ast::TSTypeAnnotation<'_>>,
) -> DeclaredCallEffect {
    match annotation.map(|annotation| &annotation.type_annotation) {
        None | Some(TSType::TSAnyKeyword(_)) => DeclaredCallEffect::Inert,
        Some(TSType::TSFunctionType(function)) => {
            declared_return_effect(Some(&function.return_type))
        }
        Some(_) => DeclaredCallEffect::Undecided,
    }
}

/// The declared call effects of the variables, parameters and function
/// declarations named by `names` (binding-identifier spans in the frame
/// anchored at `anchor`), found inside the defining function's source
/// range `within`.
struct DeclaredCallEffects {
    within: verter_span::Span,
    anchor: u32,
    names: Vec<FrameSpan>,
    effects: Vec<DeclaredCallEffect>,
}

impl DeclaredCallEffects {
    fn names(&self, span: oxc_span::Span) -> bool {
        self.names
            .contains(&FrameSpan::rebase(self.anchor, span.into()))
    }
}

impl<'a> Visit<'a> for DeclaredCallEffects {
    fn visit_statement(&mut self, it: &Statement<'a>) {
        let span = it.span();
        if span.end < self.within.start || span.start > self.within.end {
            return;
        }
        walk::walk_statement(self, it);
    }
    fn visit_function(
        &mut self,
        it: &oxc_ast::ast::Function<'a>,
        flags: oxc_syntax::scope::ScopeFlags,
    ) {
        if it.id.as_ref().is_some_and(|id| self.names(id.span)) {
            self.effects
                .push(declared_return_effect(it.return_type.as_deref()));
        }
        walk::walk_function(self, it, flags);
    }
    fn visit_variable_declarator(&mut self, it: &oxc_ast::ast::VariableDeclarator<'a>) {
        if matches!(&it.id, BindingPattern::BindingIdentifier(id) if self.names(id.span)) {
            self.effects
                .push(declared_type_effect(it.type_annotation.as_deref()));
        }
        walk::walk_variable_declarator(self, it);
    }
    fn visit_formal_parameter(&mut self, it: &oxc_ast::ast::FormalParameter<'a>) {
        if matches!(&it.pattern, BindingPattern::BindingIdentifier(id) if self.names(id.span)) {
            self.effects
                .push(declared_type_effect(it.type_annotation.as_deref()));
        }
        walk::walk_formal_parameter(self, it);
    }
}

/// Whether a statement list contains a `return` in ITS OWN frame — a
/// nested function or class body is a different frame and its returns
/// say nothing about this one.
#[derive(Default)]
struct OwnFrameReturnFinder {
    nested_frame_nesting: u32,
    found: bool,
}

impl<'a> Visit<'a> for OwnFrameReturnFinder {
    fn visit_return_statement(&mut self, it: &oxc_ast::ast::ReturnStatement<'a>) {
        if self.nested_frame_nesting == 0 {
            self.found = true;
        }
        walk::walk_return_statement(self, it);
    }
    fn visit_function(
        &mut self,
        it: &oxc_ast::ast::Function<'a>,
        flags: oxc_syntax::scope::ScopeFlags,
    ) {
        self.nested_frame_nesting += 1;
        walk::walk_function(self, it, flags);
        self.nested_frame_nesting -= 1;
    }
    fn visit_arrow_function_expression(&mut self, it: &oxc_ast::ast::ArrowFunctionExpression<'a>) {
        self.nested_frame_nesting += 1;
        walk::walk_arrow_function_expression(self, it);
        self.nested_frame_nesting -= 1;
    }
    fn visit_class(&mut self, it: &oxc_ast::ast::Class<'a>) {
        self.nested_frame_nesting += 1;
        walk::walk_class(self, it);
        self.nested_frame_nesting -= 1;
    }
}

/// Whether a statement contains a `yield` of ITS OWN frame. A generator's
/// yield type joins every yield it evaluates, so a loop whose body yields
/// contributes to the answer exactly as one whose body returns does and
/// can never be skipped as transparent.
/// Whether unreachable `statements` hold a `return` or a `yield` of their
/// own frame — contributions the checker aggregates whether or not a path
/// reaches them.
fn unreachable_statements_contribute(statements: &[Statement<'_>]) -> bool {
    let mut returns = OwnFrameReturnFinder::default();
    let mut yields = OwnFrameYieldFinder::default();
    for statement in statements {
        returns.visit_statement(statement);
        yields.visit_statement(statement);
    }
    returns.found || yields.found
}

fn statement_yields_in_own_frame(statement: &Statement<'_>) -> bool {
    let mut finder = OwnFrameYieldFinder::default();
    finder.visit_statement(statement);
    finder.found
}

/// Whether a body holds a `yield` of its own frame the yield join does not
/// model: one nested inside another expression, a statement-position
/// `yield*` delegation, or a yield inside a statement-position yield's
/// own argument.
fn body_has_unmodeled_yield(statements: &[Statement<'_>]) -> bool {
    let mut finder = OwnFrameYieldFinder {
        statement_yields_modeled: true,
        ..OwnFrameYieldFinder::default()
    };
    for statement in statements {
        finder.visit_statement(statement);
    }
    finder.found
}

/// The `yield` twin of [`OwnFrameReturnFinder`]. With
/// `statement_yields_modeled` set it skips the yield of a statement-position
/// `yield x` / `yield;` (its argument is still searched) and finds only
/// the other yields.
#[derive(Default)]
struct OwnFrameYieldFinder {
    nested_frame_nesting: u32,
    statement_yields_modeled: bool,
    found: bool,
}

impl<'a> Visit<'a> for OwnFrameYieldFinder {
    fn visit_expression_statement(&mut self, it: &oxc_ast::ast::ExpressionStatement<'a>) {
        if self.statement_yields_modeled {
            if let Expression::YieldExpression(yield_expr) = unwrap_parenthesized(&it.expression) {
                if !yield_expr.delegate {
                    if let Some(argument) = &yield_expr.argument {
                        self.visit_expression(argument);
                    }
                    return;
                }
            }
        }
        walk::walk_expression_statement(self, it);
    }
    fn visit_yield_expression(&mut self, it: &oxc_ast::ast::YieldExpression<'a>) {
        if self.nested_frame_nesting == 0 {
            self.found = true;
        }
        walk::walk_yield_expression(self, it);
    }
    fn visit_function(
        &mut self,
        it: &oxc_ast::ast::Function<'a>,
        flags: oxc_syntax::scope::ScopeFlags,
    ) {
        self.nested_frame_nesting += 1;
        walk::walk_function(self, it, flags);
        self.nested_frame_nesting -= 1;
    }
    fn visit_arrow_function_expression(&mut self, it: &oxc_ast::ast::ArrowFunctionExpression<'a>) {
        self.nested_frame_nesting += 1;
        walk::walk_arrow_function_expression(self, it);
        self.nested_frame_nesting -= 1;
    }
    fn visit_class(&mut self, it: &oxc_ast::ast::Class<'a>) {
        self.nested_frame_nesting += 1;
        walk::walk_class(self, it);
        self.nested_frame_nesting -= 1;
    }
}

/// A position whose calls never feed the demanded value, with the rule
/// under which a PROVABLY CLOSED same-file callee's authored return
/// annotation certifies a call there as establishing no narrowing.
#[derive(Clone, Copy)]
enum ResultIndependentPosition {
    /// An `if` / ternary test — including one folded inside a
    /// leaf-lowered expression: a predicate callee CONTROLS the arms'
    /// narrowing, and an inferred boolean return can be an inferred
    /// predicate, so only an authored NON-predicate annotation certifies.
    ControlTest,
    /// A sequence's discarded operand, or any other position whose VALUE
    /// the demanded answer provably does not consume (a call folded into
    /// a leaf-lowered expression, a statement inside an immediately
    /// evaluated class static block): only an `asserts` callee narrows
    /// what follows, and assertion signatures are never inferred, so an
    /// absent annotation or any non-`asserts` annotation certifies.
    DiscardedOperand,
}

impl ResultIndependentPosition {
    fn certifies_closed_return(
        self,
        annotation: Option<&oxc_ast::ast::TSTypeAnnotation<'_>>,
    ) -> bool {
        match self {
            Self::ControlTest => annotation.is_some_and(|annotation| {
                !matches!(annotation.type_annotation, TSType::TSTypePredicate(_))
            }),
            Self::DiscardedOperand => !annotation.is_some_and(|annotation| {
                matches!(
                    &annotation.type_annotation,
                    TSType::TSTypePredicate(predicate) if predicate.asserts
                )
            }),
        }
    }
}

/// How hard the per-callee certification chases a call it cannot prove
/// closed — see [`Lowerer::certify_result_independent_calls`].
#[derive(Clone, Copy)]
enum CertificationMode {
    /// The position's evaluation is observable by the demand (a control
    /// test, a discarded operand of a lowered expression, a leaf, an
    /// immediately evaluated class position): an unprovable call gaps.
    Strict,
    /// The position's VALUE is never consumed (an elided declaration
    /// initializer): only an effect that could narrow a FRAME-OWNED
    /// binding — an `asserts` call with a frame-owned subject — is
    /// unprovable; every other call's result feeds no read and certifies.
    ValueFree,
}

/// Which collected whole-binding writes a scan flags.
#[derive(Clone, Copy)]
enum WritePolicy {
    /// Every frame-owned write: the scanned position is one the slice's
    /// effect ledger cannot reach (a folded leaf answer, a class subtree,
    /// an elided declaration initializer), so no other rail covers it.
    All,
    /// Only SKELETON-HIDDEN writes (under a class subtree): the position
    /// itself is a statement the skeleton indexes, so its visible writes
    /// already ride the typed unapplied-write ledger — including its
    /// demand-selection discipline, which a scan cannot reproduce (a
    /// write to a binding nothing reads degrades nothing).
    SkeletonHiddenOnly,
}

/// One call / construct collected for the result-independent discipline:
/// its authored span plus, for a call, the bare-identifier callee (name +
/// span) when the callee spells one and the ROOT identifiers an `asserts`
/// effect could narrow (every argument that is a reference, plus a member
/// callee's receiver root — `asserts this`).
enum ControlCall {
    Construct(verter_span::Span),
    /// A tagged template: the checker binds no call flow node for one, so
    /// its tag narrows nothing even when it is a type predicate or an
    /// `asserts` signature.
    TaggedTemplate(verter_span::Span),
    Call {
        span: verter_span::Span,
        callee: Option<(String, oxc_span::Span)>,
        assertion_subject_roots: Vec<(String, oxc_span::Span)>,
    },
}

/// The root identifier a reference expression is rooted at, through
/// parenthesized / TS wrappers and static member chains — the only
/// expression shape an `asserts` narrowing can land on. Any other base (a
/// call result, a literal) is not a reference and narrows nothing.
fn expression_root_identifier(expression: &Expression<'_>) -> Option<(String, oxc_span::Span)> {
    match unwrap_parenthesized(expression) {
        Expression::Identifier(identifier) => {
            Some((identifier.name.as_str().to_owned(), identifier.span))
        }
        Expression::StaticMemberExpression(member) => expression_root_identifier(&member.object),
        Expression::TSAsExpression(inner) => expression_root_identifier(&inner.expression),
        Expression::TSSatisfiesExpression(inner) => expression_root_identifier(&inner.expression),
        Expression::TSNonNullExpression(inner) => expression_root_identifier(&inner.expression),
        Expression::TSTypeAssertion(inner) => expression_root_identifier(&inner.expression),
        _ => None,
    }
}

/// The binding a whole assignment writes: an identifier target, also
/// through parentheses and non-null assertions (`x! = v` assigns `x`, as
/// the checker's `getAssignmentTargetKind` walks through both). A type
/// assertion is not walked through (`(x as T) = v` assigns nothing), and
/// a member or destructuring target writes no single binding.
fn assigned_identifier<'b, 'a>(
    target: &'b oxc_ast::ast::AssignmentTarget<'a>,
) -> Option<&'b oxc_ast::ast::IdentifierReference<'a>> {
    match target {
        oxc_ast::ast::AssignmentTarget::AssignmentTargetIdentifier(identifier) => Some(identifier),
        oxc_ast::ast::AssignmentTarget::TSNonNullExpression(non_null) => {
            let mut inner = &non_null.expression;
            loop {
                inner = match inner {
                    Expression::ParenthesizedExpression(paren) => &paren.expression,
                    Expression::TSNonNullExpression(non_null) => &non_null.expression,
                    Expression::Identifier(identifier) => return Some(identifier),
                    _ => return None,
                };
            }
        }
        _ => None,
    }
}

impl ControlCall {
    fn of_call(call: &oxc_ast::ast::CallExpression<'_>) -> Self {
        let callee_expression = unwrap_parenthesized(&call.callee);
        let callee = match callee_expression {
            Expression::Identifier(identifier) => {
                Some((identifier.name.as_str().to_owned(), identifier.span))
            }
            _ => None,
        };
        let mut assertion_subject_roots: Vec<(String, oxc_span::Span)> = Vec::new();
        if let Expression::StaticMemberExpression(member) = callee_expression {
            if let Some(root) = expression_root_identifier(&member.object) {
                assertion_subject_roots.push(root);
            }
        }
        for argument in &call.arguments {
            if let Some(root) = argument
                .as_expression()
                .and_then(expression_root_identifier)
            {
                assertion_subject_roots.push(root);
            }
        }
        ControlCall::Call {
            span: call.span.into(),
            callee,
            assertion_subject_roots,
        }
    }
}

/// The call collector behind [`Lowerer::record_decided_above_calls`]:
/// every call / construct of a leaf-lowered expression lands in exactly
/// one of three channels, and the DEFAULT is fail-closed — there is no
/// blanket same-frame certification. A call inside a same-frame CONTROL
/// position (a ternary test, a `&&` / `||` left operand, a statement test
/// inside an immediately-evaluated class static block) is `control` (its
/// result can decide the narrowing a branch evaluates under, so it takes
/// the [`ResultIndependentPosition::ControlTest`] rule) — but an operand
/// of a comparison or arithmetic operator inside one is not: the checker
/// reads a call's predicate only on the test's narrowing spine, never from
/// such an operand. A same-frame call the checker enters into control
/// flow — an expression statement's own call or a comma operator's
/// operand (the binder's `maybeBindExpressionFlowIfCall`) — is
/// `discarded` (its value is unused, so only an `asserts` callee narrows
/// what follows — the [`ResultIndependentPosition::DiscardedOperand`]
/// rule). Both channels
/// certify per-callee through
/// [`Lowerer::certify_result_independent_calls`]; an unprovable call
/// flags the enclosing statement's typed `GuardNarrowing` gap.
///
/// A nested function / class body is its OWN frame — its names never
/// resolve against this frame's skeleton and its calls run at ITS
/// evaluation (a method when called, an instance property / accessor
/// initializer at construction), never in this statement — so its calls
/// keep the blanket `decided` treatment they always had. The
/// class-evaluation-time positions are NOT deferred: a class's
/// decorators, `super_class` heritage expression, computed member keys,
/// member decorators, STATIC BLOCKS, and static property / accessor
/// initializers evaluate in the ENCLOSING frame, so they are visited
/// outside the nested-frame guard and their control positions split like
/// any other same-frame position.
///
/// The same positions can also WRITE a frame binding: `class C { static {
/// x = "s"; } }` retypes `x` in the checker for every read that follows,
/// but the flow skeleton skips the whole class subtree, so the write
/// never enters the slice's effect ledger and the evaluator's
/// unapplied-write gate never sees it. The scanner therefore collects
/// every same-frame WHOLE-BINDING write target (a plain `=` assignment, a
/// compound-operator write, an update — identifier and destructuring
/// targets; a member write is a projection under its root and never
/// retypes the binding itself), and the drain resolves each against this
/// frame's lexical authority: a target the frame OWNS flags the enclosing
/// statement's typed `GuardNarrowing` gap, a FREE target stays silent.
///
/// Every other same-frame call — an argument, an initializer, an operand
/// whose value something consumes, a `void` operand — is never entered
/// into control flow, so it narrows nothing and is `unentered`: decided
/// above with no certification (TypeScript 7.0.2: `g(assertString(x))`,
/// `const y = assertString(x)` and `void assertString(x)` each keep `x`
/// unnarrowed, while `assertString(x);`, `(assertString(x), 0)` and
/// `const y = (0, assertString(x))` narrow it). A skeleton-visible write inside a `void` operand is not collected
/// either: the operand's value cannot depend on it, and a read after it
/// rides the slice's unapplied-write ledger like any statement write's.
/// A call the checker enters into control flow, or the join of a
/// conditional (a ternary, or a `&&` / `||` whose right operand is the one
/// arm) with an entered call in an arm: the flow after it is the union of
/// the arms' ends, each under its reading of `test`.
enum ArmEntered<'a> {
    Call(&'a oxc_ast::ast::CallExpression<'a>),
    Join {
        test: &'a Expression<'a>,
        consequent: Vec<ArmEntered<'a>>,
        alternate: Vec<ArmEntered<'a>>,
    },
}

impl<'a> ArmEntered<'a> {
    /// Every call this item holds, in source order.
    fn flatten_into(self, out: &mut Vec<&'a oxc_ast::ast::CallExpression<'a>>) {
        match self {
            Self::Call(call) => out.push(call),
            Self::Join {
                consequent,
                alternate,
                ..
            } => {
                for item in consequent.into_iter().chain(alternate) {
                    item.flatten_into(out);
                }
            }
        }
    }
}

#[derive(Default)]
struct LeafCallScanner<'a> {
    decided: Vec<verter_span::Span>,
    /// Same-frame calls / constructs the checker never enters into
    /// control flow — see the `void` rule above.
    unentered: Vec<verter_span::Span>,
    /// `void` operand nesting.
    void_nesting: usize,
    /// The spans of calls that are a comma operator's operand.
    comma_operand_calls: FxHashSet<oxc_span::Span>,
    /// The spans of calls that are a comma operator's DISCARDED (not last)
    /// operand: a test never reads their predicate, even in a control
    /// position.
    comma_discarded_calls: FxHashSet<oxc_span::Span>,
    /// The same-frame calls the checker enters into control flow (an
    /// expression statement's own call, a comma operand) outside any
    /// conditional arm, whose `asserts` predicate persists past the
    /// scanned position, in source order — with the joins of the
    /// conditionals whose every arm entered calls.
    entered: Vec<ArmEntered<'a>>,
    /// Conditional-arm nesting: a ternary arm or a `&&` / `||` / `??`
    /// right operand, whose effects join the other path's.
    conditional_nesting: usize,
    /// Entered calls (and joins) inside a conditional arm, not yet joined.
    conditional_entered: Vec<ArmEntered<'a>>,
    /// Entered calls of an operand no path runs, or of a `??` right
    /// operand, whose path joins one that skipped it: an `asserts`
    /// narrowing they make does not persist past it.
    joined_away: Vec<&'a oxc_ast::ast::CallExpression<'a>>,
    /// The spans of calls that are an expression statement's own
    /// expression (the scanned position's own, or one inside a class
    /// static block).
    statement_calls: FxHashSet<oxc_span::Span>,
    discarded: Vec<ControlCall>,
    control: Vec<ControlCall>,
    /// Same-frame whole-binding write targets: `(name, identifier span,
    /// skeleton-hidden)`. A write is SKELETON-HIDDEN when it sits under a
    /// class subtree, whose writes the flow skeleton never records: the
    /// slice's effect ledger cannot see it. A skeleton-VISIBLE write instead rides
    /// the typed unapplied-write ledger, which applies the demand-selection
    /// discipline (a write to a binding nothing reads degrades nothing).
    writes: Vec<(&'a str, oxc_span::Span, bool)>,
    control_nesting: usize,
    nested_frame_nesting: usize,
    /// Class-subtree nesting: the flow skeleton never records a write
    /// inside a class, so a write collected under one is invisible to the
    /// slice's effect ledger. Unlike `nested_frame_nesting` this is NOT dropped
    /// for the class-evaluation-time positions (a static block runs here,
    /// but the skeleton still never saw it).
    class_nesting: usize,
    /// Class-member container nesting: the checker binds a member's
    /// computed key and a property or accessor initializer inside the
    /// member's own control-flow container, so a write there, static or
    /// not, never retypes a binding the enclosing flow reads.
    member_container_nesting: usize,
}

impl<'a> LeafCallScanner<'a> {
    /// Visit one same-frame CONTROL-position expression: every call it
    /// holds (transitively, until a nested frame) takes the
    /// [`ResultIndependentPosition::ControlTest`] rule.
    fn visit_control_expression(&mut self, expr: &Expression<'a>) {
        self.control_nesting += 1;
        self.visit_expression(expr);
        self.control_nesting -= 1;
    }

    /// Visit an operand no path runs: an entered call there narrows
    /// nothing that follows.
    fn visit_unreached(&mut self, expr: &Expression<'a>) {
        let start = self.conditional_entered.len();
        self.conditional_nesting += 1;
        self.visit_expression(expr);
        self.conditional_nesting -= 1;
        self.join_away_from(start);
    }

    /// Record the join of a conditional's arms, when an arm entered a call.
    fn push_join(
        &mut self,
        test: &'a Expression<'a>,
        consequent: Vec<ArmEntered<'a>>,
        alternate: Vec<ArmEntered<'a>>,
    ) {
        if consequent.is_empty() && alternate.is_empty() {
            return;
        }
        let join = ArmEntered::Join {
            test,
            consequent,
            alternate,
        };
        if self.conditional_nesting == 0 {
            self.entered.push(join);
        } else {
            self.conditional_entered.push(join);
        }
    }

    /// The arm items from `start` join a path that entered none: their
    /// narrowing does not persist past the conditional.
    fn join_away_from(&mut self, start: usize) {
        let joined: Vec<_> = self.conditional_entered.drain(start..).collect();
        for item in joined {
            item.flatten_into(&mut self.joined_away);
        }
    }

    /// Visit a class member's key. A computed key evaluates at class
    /// definition, but inside the member's own control-flow container: its
    /// calls keep the class-definition discipline, its writes never retype
    /// a binding the enclosing flow reads.
    fn visit_member_key(&mut self, key: &oxc_ast::ast::PropertyKey<'a>) {
        self.member_container_nesting += 1;
        self.visit_property_key(key);
        self.member_container_nesting -= 1;
    }

    /// Record one same-frame whole-binding write target, marking whether
    /// the flow skeleton can see it (it never records a write inside a
    /// class subtree, so a write under `class_nesting` is invisible to the
    /// slice's effect ledger).
    fn push_write(&mut self, name: &'a str, span: oxc_span::Span) {
        if self.member_container_nesting > 0 {
            return;
        }
        if self.void_nesting > 0 && self.class_nesting == 0 {
            return;
        }
        self.writes.push((name, span, self.class_nesting > 0));
    }

    /// Collect the WHOLE-BINDING write targets of one assignment target:
    /// an identifier target writes its binding, a destructuring pattern
    /// writes every element it binds, and a TS wrapper forwards to its
    /// inner expression. A member write is a projection under its root —
    /// it never retypes the binding itself — so it collects nothing.
    fn collect_write_targets(&mut self, target: &oxc_ast::ast::AssignmentTarget<'a>) {
        use oxc_ast::ast::AssignmentTarget;
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                self.push_write(identifier.name.as_str(), identifier.span);
            }
            AssignmentTarget::TSAsExpression(as_expression) => {
                self.collect_expression_write_target(&as_expression.expression, true);
            }
            AssignmentTarget::TSSatisfiesExpression(satisfies) => {
                self.collect_expression_write_target(&satisfies.expression, true);
            }
            AssignmentTarget::TSNonNullExpression(non_null) => {
                self.collect_expression_write_target(&non_null.expression, false);
            }
            AssignmentTarget::TSTypeAssertion(assertion) => {
                self.collect_expression_write_target(&assertion.expression, true);
            }
            AssignmentTarget::ArrayAssignmentTarget(array) => {
                for element in array.elements.iter().flatten() {
                    self.collect_maybe_default_write_target(element);
                }
                if let Some(rest) = array.rest.as_ref() {
                    self.collect_write_targets(&rest.target);
                }
            }
            AssignmentTarget::ObjectAssignmentTarget(object) => {
                use oxc_ast::ast::AssignmentTargetProperty;
                for property in &object.properties {
                    match property {
                        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(
                            identifier,
                        ) => {
                            self.push_write(
                                identifier.binding.name.as_str(),
                                identifier.binding.span,
                            );
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(property) => {
                            self.collect_maybe_default_write_target(&property.binding);
                        }
                    }
                }
                if let Some(rest) = object.rest.as_ref() {
                    self.collect_write_targets(&rest.target);
                }
            }
            AssignmentTarget::StaticMemberExpression(_)
            | AssignmentTarget::ComputedMemberExpression(_)
            | AssignmentTarget::PrivateFieldExpression(_) => {}
        }
    }

    fn collect_maybe_default_write_target(
        &mut self,
        target: &oxc_ast::ast::AssignmentTargetMaybeDefault<'a>,
    ) {
        match target {
            oxc_ast::ast::AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(
                with_default,
            ) => {
                self.collect_write_targets(&with_default.binding);
            }
            _ => self.collect_write_targets(target.to_assignment_target()),
        }
    }

    /// A whole-binding write the target names through TS carriers; an
    /// identifier under a type assertion (`asserted`, or one reached on the
    /// way) is a reference, not an assignment target
    /// ([`verter_semantic::analysis::flow::WrappedAssignmentTarget`]).
    fn collect_expression_write_target(&mut self, expression: &Expression<'a>, asserted: bool) {
        if let verter_semantic::analysis::flow::WrappedAssignmentTarget::Binding(identifier) =
            verter_semantic::analysis::flow::wrapped_assignment_target(expression, asserted)
        {
            self.push_write(identifier.name.as_str(), identifier.span);
        }
    }

    /// The WRITE targets of a `for … in` / `for … of` loop head: an
    /// assignment-target left side (`for (x of xs)`) writes that binding
    /// once per iteration — the same whole-binding vocabulary
    /// [`Self::collect_write_targets`] applies. A DECLARATION left side
    /// (`for (const y of xs)`) binds a fresh binding: nothing this frame
    /// owns is written, and the walk never mistakes the declarator for a
    /// write on its own.
    fn collect_for_left_writes(&mut self, left: &oxc_ast::ast::ForStatementLeft<'a>) {
        if let Some(target) = left.as_assignment_target() {
            self.collect_write_targets(target);
        }
    }
}

impl<'a> Visit<'a> for LeafCallScanner<'a> {
    fn visit_assignment_expression(&mut self, it: &oxc_ast::ast::AssignmentExpression<'a>) {
        // A same-frame assignment RUNS at the enclosing statement: a
        // whole-binding write to a frame-owned target retypes the binding
        // in the checker while the slice's effect ledger never sees it
        // (the skeleton skips the class subtree). Collect it for the
        // drain's fail-closed gap; the walk still descends for nested
        // calls and writes in computed keys and the right-hand side.
        if self.nested_frame_nesting == 0 {
            self.collect_write_targets(&it.left);
        }
        walk::walk_assignment_expression(self, it);
    }
    fn visit_update_expression(&mut self, it: &oxc_ast::ast::UpdateExpression<'a>) {
        if self.nested_frame_nesting == 0 {
            // The checker assigns through non-null `!` and parentheses and
            // never through a type assertion: `x!++` retypes `x` as `x++`
            // does, `(x as any)++` leaves it as it was.
            if let Some(identifier) =
                verter_semantic::analysis::flow::simple_assignment_target_binding(&it.argument)
            {
                self.push_write(identifier.name.as_str(), identifier.span);
            }
        }
        walk::walk_update_expression(self, it);
    }
    fn visit_conditional_expression(&mut self, it: &oxc_ast::ast::ConditionalExpression<'a>) {
        if self.nested_frame_nesting > 0 {
            walk::walk_conditional_expression(self, it);
            return;
        }
        // The ternary's TEST is a control position exactly as the
        // statement-level `if` twin's: a predicate call there narrows the
        // branch reads even when the whole conditional folds into a leaf.
        self.visit_control_expression(&it.test);
        // A literal test takes one arm for certain (its entered calls
        // narrow what follows) and never the other (whose calls never run).
        if let Some(taken) = literal_boolean_value(&it.test) {
            let (always, never) = if taken {
                (&it.consequent, &it.alternate)
            } else {
                (&it.alternate, &it.consequent)
            };
            self.visit_expression(always);
            self.visit_unreached(never);
            return;
        }
        let start = self.conditional_entered.len();
        self.conditional_nesting += 1;
        self.visit_expression(&it.consequent);
        let middle = self.conditional_entered.len();
        self.visit_expression(&it.alternate);
        self.conditional_nesting -= 1;
        // The flow past the conditional joins its arms' ends.
        let alternate: Vec<_> = self.conditional_entered.drain(middle..).collect();
        let consequent: Vec<_> = self.conditional_entered.drain(start..).collect();
        let test = self.alloc(&it.test);
        self.push_join(test, consequent, alternate);
    }
    fn visit_logical_expression(&mut self, it: &oxc_ast::ast::LogicalExpression<'a>) {
        if self.nested_frame_nesting > 0 {
            walk::walk_logical_expression(self, it);
            return;
        }
        // The left operand decides whether the right evaluates and under
        // which narrowing (`isString(x) && x` reads `x` as `string` in
        // the checker) — a control position, conservatively for `??` too.
        self.visit_control_expression(&it.left);
        // A literal boolean left operand decides whether the right one
        // runs: for certain (`true && …`, `false || …`) or never (`false
        // && …`, `true || …`, `true ?? …`).
        if let Some(value) = literal_boolean_value(&it.left) {
            let runs = match it.operator {
                oxc_ast::ast::LogicalOperator::And => value,
                oxc_ast::ast::LogicalOperator::Or => !value,
                oxc_ast::ast::LogicalOperator::Coalesce => false,
            };
            if runs {
                self.visit_expression(&it.right);
            } else {
                self.visit_unreached(&it.right);
            }
            return;
        }
        let start = self.conditional_entered.len();
        self.conditional_nesting += 1;
        self.visit_expression(&it.right);
        self.conditional_nesting -= 1;
        let right: Vec<_> = self.conditional_entered.drain(start..).collect();
        let left = self.alloc(&it.left);
        match it.operator {
            // The right operand runs on the left's truthy edge for `&&`,
            // its falsy edge for `||`; the other edge skips it.
            oxc_ast::ast::LogicalOperator::And => self.push_join(left, right, Vec::new()),
            oxc_ast::ast::LogicalOperator::Or => self.push_join(left, Vec::new(), right),
            // `??` has no guard reading here: the path that skips the
            // right operand joins it unnarrowed.
            oxc_ast::ast::LogicalOperator::Coalesce => {
                for item in right {
                    item.flatten_into(&mut self.joined_away);
                }
            }
        }
    }
    // Statement TESTS are control positions. Statements are reachable
    // inside a leaf-lowered expression only through an immediately
    // evaluated class static block; everywhere else a nested-frame guard
    // has already routed the whole subtree to the blanket channel.
    fn visit_if_statement(&mut self, it: &oxc_ast::ast::IfStatement<'a>) {
        if self.nested_frame_nesting > 0 {
            walk::walk_if_statement(self, it);
            return;
        }
        self.visit_control_expression(&it.test);
        self.visit_statement(&it.consequent);
        if let Some(alternate) = &it.alternate {
            self.visit_statement(alternate);
        }
    }
    fn visit_while_statement(&mut self, it: &oxc_ast::ast::WhileStatement<'a>) {
        if self.nested_frame_nesting > 0 {
            walk::walk_while_statement(self, it);
            return;
        }
        self.visit_control_expression(&it.test);
        self.visit_statement(&it.body);
    }
    fn visit_do_while_statement(&mut self, it: &oxc_ast::ast::DoWhileStatement<'a>) {
        if self.nested_frame_nesting > 0 {
            walk::walk_do_while_statement(self, it);
            return;
        }
        self.visit_statement(&it.body);
        self.visit_control_expression(&it.test);
    }
    fn visit_for_statement(&mut self, it: &oxc_ast::ast::ForStatement<'a>) {
        if self.nested_frame_nesting > 0 {
            walk::walk_for_statement(self, it);
            return;
        }
        if let Some(init) = &it.init {
            self.visit_for_statement_init(init);
        }
        if let Some(test) = &it.test {
            self.visit_control_expression(test);
        }
        if let Some(update) = &it.update {
            self.visit_expression(update);
        }
        self.visit_statement(&it.body);
    }
    fn visit_for_in_statement(&mut self, it: &oxc_ast::ast::ForInStatement<'a>) {
        if self.nested_frame_nesting > 0 {
            walk::walk_for_in_statement(self, it);
            return;
        }
        self.collect_for_left_writes(&it.left);
        walk::walk_for_in_statement(self, it);
    }
    fn visit_for_of_statement(&mut self, it: &oxc_ast::ast::ForOfStatement<'a>) {
        if self.nested_frame_nesting > 0 {
            walk::walk_for_of_statement(self, it);
            return;
        }
        self.collect_for_left_writes(&it.left);
        walk::walk_for_of_statement(self, it);
    }
    // A `switch` in a scanned position (a statement inside an immediately
    // evaluated class static block): the discriminant's value never feeds
    // the demanded answer (discarded), while each case TEST is a control
    // position exactly as an `if` test is — a call there can control the
    // clause's narrowing.
    fn visit_switch_statement(&mut self, it: &oxc_ast::ast::SwitchStatement<'a>) {
        if self.nested_frame_nesting > 0 {
            walk::walk_switch_statement(self, it);
            return;
        }
        self.visit_expression(&it.discriminant);
        for case in &it.cases {
            if let Some(test) = case.test.as_ref() {
                self.visit_control_expression(test);
            }
            for statement in &case.consequent {
                self.visit_statement(statement);
            }
        }
    }
    fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'a>) {
        if self.nested_frame_nesting == 0
            && (self.comma_operand_calls.contains(&call.span)
                || self.statement_calls.contains(&call.span))
        {
            let call = ArmEntered::Call(self.alloc(call));
            if self.conditional_nesting == 0 {
                self.entered.push(call);
            } else {
                self.conditional_entered.push(call);
            }
        }
        if self.nested_frame_nesting > 0 {
            self.decided.push(call.span.into());
        } else if self.comma_discarded_calls.contains(&call.span) {
            self.discarded.push(ControlCall::of_call(call));
        } else if self.control_nesting > 0 {
            self.control.push(ControlCall::of_call(call));
        } else if self.comma_operand_calls.contains(&call.span)
            || self.statement_calls.contains(&call.span)
        {
            self.discarded.push(ControlCall::of_call(call));
        } else {
            self.unentered.push(call.span.into());
        }
        walk::walk_call_expression(self, call);
    }
    fn visit_new_expression(&mut self, new: &oxc_ast::ast::NewExpression<'a>) {
        if self.nested_frame_nesting > 0 {
            self.decided.push(new.span.into());
        } else if self.control_nesting > 0 {
            self.control.push(ControlCall::Construct(new.span.into()));
        } else if self.void_nesting > 0 {
            self.unentered.push(new.span.into());
        } else {
            self.discarded.push(ControlCall::Construct(new.span.into()));
        }
        walk::walk_new_expression(self, new);
    }
    fn visit_tagged_template_expression(
        &mut self,
        tagged: &oxc_ast::ast::TaggedTemplateExpression<'a>,
    ) {
        if self.nested_frame_nesting > 0 {
            self.decided.push(tagged.span.into());
        } else if self.control_nesting > 0 {
            self.control
                .push(ControlCall::TaggedTemplate(tagged.span.into()));
        } else if self.void_nesting > 0 {
            self.unentered.push(tagged.span.into());
        } else {
            self.discarded
                .push(ControlCall::TaggedTemplate(tagged.span.into()));
        }
        walk::walk_tagged_template_expression(self, tagged);
    }
    fn visit_expression_statement(&mut self, it: &oxc_ast::ast::ExpressionStatement<'a>) {
        if let Expression::CallExpression(call) = &it.expression {
            self.statement_calls.insert(call.span);
        }
        walk::walk_expression_statement(self, it);
    }
    /// A comparison against a boolean literal keeps its other side on the
    /// narrowing spine; every other operator's operands leave it. That
    /// includes an operand of `in` / `instanceof`, a VALUE position even
    /// inside a control test: the checker narrows by those operators only
    /// the reference an operand names, so a call there is never a predicate
    /// condition, and only an `asserts` callee could narrow — the
    /// discarded-operand rule.
    fn visit_binary_expression(&mut self, it: &oxc_ast::ast::BinaryExpression<'a>) {
        let boolean_comparison = matches!(
            it.operator,
            oxc_ast::ast::BinaryOperator::Equality
                | oxc_ast::ast::BinaryOperator::Inequality
                | oxc_ast::ast::BinaryOperator::StrictEquality
                | oxc_ast::ast::BinaryOperator::StrictInequality
        ) && (literal_boolean_value(&it.left).is_some()
            || literal_boolean_value(&it.right).is_some());
        if self.control_nesting == 0 || boolean_comparison {
            walk::walk_binary_expression(self, it);
            return;
        }
        let control = std::mem::replace(&mut self.control_nesting, 0);
        walk::walk_binary_expression(self, it);
        self.control_nesting = control;
    }
    fn visit_unary_expression(&mut self, it: &oxc_ast::ast::UnaryExpression<'a>) {
        let void = it.operator == UnaryOperator::Void;
        self.void_nesting += usize::from(void);
        walk::walk_unary_expression(self, it);
        self.void_nesting -= usize::from(void);
    }
    fn visit_sequence_expression(&mut self, it: &oxc_ast::ast::SequenceExpression<'a>) {
        // The checker enters a call that is a comma operator's operand —
        // either side — into control flow.
        for (index, operand) in it.expressions.iter().enumerate() {
            if let Expression::CallExpression(call) = operand {
                self.comma_operand_calls.insert(call.span);
                if index + 1 < it.expressions.len() {
                    self.comma_discarded_calls.insert(call.span);
                }
            }
        }
        walk::walk_sequence_expression(self, it);
    }
    // Nested function / arrow / class bodies are their own frames.
    fn visit_function(
        &mut self,
        it: &oxc_ast::ast::Function<'a>,
        flags: oxc_syntax::scope::ScopeFlags,
    ) {
        self.nested_frame_nesting += 1;
        walk::walk_function(self, it, flags);
        self.nested_frame_nesting -= 1;
    }
    fn visit_arrow_function_expression(&mut self, it: &oxc_ast::ast::ArrowFunctionExpression<'a>) {
        self.nested_frame_nesting += 1;
        walk::walk_arrow_function_expression(self, it);
        self.nested_frame_nesting -= 1;
    }
    fn visit_static_block(&mut self, it: &oxc_ast::ast::StaticBlock<'a>) {
        // A static block runs at CLASS EVALUATION — one frame out from
        // the body guard `visit_class` applied, and never deferred the
        // way a method body or property initializer is. Drop exactly the
        // class's own guard level (a genuinely enclosing nested frame's
        // guard stays: a class inside a nested function evaluates with
        // THAT function, not with this statement).
        self.nested_frame_nesting = self.nested_frame_nesting.saturating_sub(1);
        walk::walk_static_block(self, it);
        self.nested_frame_nesting += 1;
    }
    fn visit_method_definition(&mut self, it: &oxc_ast::ast::MethodDefinition<'a>) {
        // Member decorators and a computed key evaluate at CLASS
        // DEFINITION — enclosing-frame immediate, exactly as the class's
        // own decorators do. The method's VALUE is a function whose body
        // runs when CALLED: `visit_function` re-arms the nested-frame
        // guard for exactly the deferred body. The key belongs to the
        // member's own control-flow container.
        self.nested_frame_nesting = self.nested_frame_nesting.saturating_sub(1);
        self.visit_decorators(&it.decorators);
        self.visit_member_key(&it.key);
        let flags = match it.kind {
            oxc_ast::ast::MethodDefinitionKind::Get => {
                oxc_syntax::scope::ScopeFlags::Function | oxc_syntax::scope::ScopeFlags::GetAccessor
            }
            oxc_ast::ast::MethodDefinitionKind::Set => {
                oxc_syntax::scope::ScopeFlags::Function | oxc_syntax::scope::ScopeFlags::SetAccessor
            }
            oxc_ast::ast::MethodDefinitionKind::Constructor => {
                oxc_syntax::scope::ScopeFlags::Function | oxc_syntax::scope::ScopeFlags::Constructor
            }
            oxc_ast::ast::MethodDefinitionKind::Method => oxc_syntax::scope::ScopeFlags::Function,
        };
        self.visit_function(&it.value, flags);
        self.nested_frame_nesting += 1;
    }
    fn visit_property_definition(&mut self, it: &oxc_ast::ast::PropertyDefinition<'a>) {
        // Member decorators and a computed key evaluate at CLASS
        // DEFINITION, and a STATIC initializer runs at class evaluation —
        // the same enclosing-frame discipline as a static block. An
        // INSTANCE property initializer is deferred to construction and
        // keeps the body guard.
        self.nested_frame_nesting = self.nested_frame_nesting.saturating_sub(1);
        self.visit_decorators(&it.decorators);
        self.visit_member_key(&it.key);
        self.nested_frame_nesting += 1;
        if let Some(type_annotation) = &it.type_annotation {
            self.visit_ts_type_annotation(type_annotation);
        }
        if let Some(value) = &it.value {
            if it.r#static {
                self.nested_frame_nesting = self.nested_frame_nesting.saturating_sub(1);
                self.member_container_nesting += 1;
                self.visit_expression(value);
                self.member_container_nesting -= 1;
                self.nested_frame_nesting += 1;
            } else {
                self.visit_expression(value);
            }
        }
    }
    fn visit_accessor_property(&mut self, it: &oxc_ast::ast::AccessorProperty<'a>) {
        // The same phase split as a property definition: decorators and a
        // computed key evaluate at class definition; a STATIC
        // auto-accessor initializer runs at class evaluation; an INSTANCE
        // one is deferred to construction and keeps the body guard.
        self.nested_frame_nesting = self.nested_frame_nesting.saturating_sub(1);
        self.visit_decorators(&it.decorators);
        self.visit_member_key(&it.key);
        self.nested_frame_nesting += 1;
        if let Some(type_annotation) = &it.type_annotation {
            self.visit_ts_type_annotation(type_annotation);
        }
        if let Some(value) = &it.value {
            if it.r#static {
                self.nested_frame_nesting = self.nested_frame_nesting.saturating_sub(1);
                self.member_container_nesting += 1;
                self.visit_expression(value);
                self.member_container_nesting -= 1;
                self.nested_frame_nesting += 1;
            } else {
                self.visit_expression(value);
            }
        }
    }
    fn visit_class(&mut self, it: &oxc_ast::ast::Class<'a>) {
        // Decorators and the `super_class` heritage expression evaluate in
        // the ENCLOSING frame — before the class body exists — so they are
        // visited OUTSIDE the nested-frame guard and their calls take the
        // same-frame discipline. Only the class BODY is guarded — and
        // inside it the member visitors drop the guard again for every
        // class-evaluation-time position (computed keys, member
        // decorators, static blocks, static property / accessor
        // initializers). (`walk_class` visits children in exactly this
        // order; the name binding, type parameters, and implements clause
        // carry no runtime expressions.)
        //
        // The skeleton records no write of the class subtree, so every
        // write under this visit — heritage included — is skeleton-hidden.
        self.class_nesting += 1;
        self.visit_decorators(&it.decorators);
        if let Some(super_class) = &it.super_class {
            self.visit_expression(super_class);
        }
        self.nested_frame_nesting += 1;
        if let Some(id) = &it.id {
            self.visit_binding_identifier(id);
        }
        if let Some(type_parameters) = &it.type_parameters {
            self.visit_ts_type_parameter_declaration(type_parameters);
        }
        if let Some(super_type_arguments) = &it.super_type_arguments {
            self.visit_ts_type_parameter_instantiation(super_type_arguments);
        }
        self.visit_ts_class_implements_list(&it.implements);
        self.visit_class_body(&it.body);
        self.nested_frame_nesting -= 1;
        self.class_nesting -= 1;
    }
}

/// The root-identifier gate's verdict on one leaf lowering.
enum LeafLowering {
    Unmodeled,
    /// Every name the answer depends on is genuinely FREE in this frame:
    /// the owner-scope answer is the right one.
    Free(TypeExpr),
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

/// Negate a lowered guard, De Morgan-complete: leaf `negated` flags
/// flip and `And` / `Or` swap, so the evaluator never meets a third
/// composition rule.
fn negate_guard(guard: SliceGuard) -> SliceGuard {
    match guard {
        SliceGuard::None => SliceGuard::None,
        SliceGuard::Typeof {
            subject,
            kind,
            negated,
        } => SliceGuard::Typeof {
            subject,
            kind,
            negated: !negated,
        },
        SliceGuard::Truthy { subject, negated } => SliceGuard::Truthy {
            subject,
            negated: !negated,
        },
        SliceGuard::EqLiteral {
            subject,
            literal,
            negated,
            loose,
        } => SliceGuard::EqLiteral {
            subject,
            literal,
            negated: !negated,
            loose,
        },
        SliceGuard::EqReference {
            subject,
            value,
            negated,
            loose,
        } => SliceGuard::EqReference {
            subject,
            value,
            negated: !negated,
            loose,
        },
        SliceGuard::Instanceof {
            subject,
            ctor,
            negated,
        } => SliceGuard::Instanceof {
            subject,
            ctor,
            negated: !negated,
        },
        SliceGuard::In {
            key,
            subject,
            negated,
        } => SliceGuard::In {
            key,
            subject,
            negated: !negated,
        },
        SliceGuard::TypePredicate {
            subject,
            target,
            negated,
            call,
        } => SliceGuard::TypePredicate {
            subject,
            target,
            negated: !negated,
            call,
        },
        SliceGuard::CallPredicate {
            callee,
            site,
            arguments,
            receiver,
            negated,
        } => SliceGuard::CallPredicate {
            callee,
            site,
            arguments,
            receiver,
            negated: !negated,
        },
        SliceGuard::EqValue {
            left,
            right,
            loose,
            negated,
        } => SliceGuard::EqValue {
            left,
            right,
            loose,
            negated: !negated,
        },
        SliceGuard::CalleePredicate {
            callee,
            arguments,
            negated,
            call,
        } => SliceGuard::CalleePredicate {
            callee,
            arguments,
            negated: !negated,
            call,
        },
        SliceGuard::And(parts) => SliceGuard::Or(Arc::from(
            parts
                .iter()
                .map(|part| negate_guard(part.clone()))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )),
        SliceGuard::Or(parts) => SliceGuard::And(Arc::from(
            parts
                .iter()
                .map(|part| negate_guard(part.clone()))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )),
        SliceGuard::Both(parts) => SliceGuard::Both(Arc::from(
            parts
                .iter()
                .map(|part| negate_guard(part.clone()))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )),
    }
}

/// Conjoin two guards while preserving an unmodelled conjunct as an
/// explicit [`SliceGuard::None`] alternative. The positive edge may apply
/// every modelled conjunct, but the false edge is a disjunction of their
/// negations: an unmodelled conjunct could be the false one, so no modelled
/// negation is guaranteed there. Keeping `None` in the tree lets the shared
/// evaluator derive both readings from this one authority.
fn and_guard(left: SliceGuard, right: SliceGuard) -> SliceGuard {
    let mut parts: Vec<SliceGuard> = Vec::new();
    for guard in [left, right] {
        match guard {
            SliceGuard::And(nested) => parts.extend(nested.iter().cloned()),
            other => parts.push(other),
        }
    }
    if parts.iter().all(|part| matches!(part, SliceGuard::None)) {
        SliceGuard::None
    } else {
        SliceGuard::And(Arc::from(parts.into_boxed_slice()))
    }
}

/// Disjoin two guards while preserving an unmodelled disjunct explicitly.
/// The positive edge then establishes nothing (one alternative is
/// unnarrowed), while the false edge still applies every modelled disjunct's
/// negation because reaching it proves all disjuncts false.
fn or_guard(left: SliceGuard, right: SliceGuard) -> SliceGuard {
    let mut parts: Vec<SliceGuard> = Vec::new();
    for guard in [left, right] {
        match guard {
            SliceGuard::Or(nested) => parts.extend(nested.iter().cloned()),
            other => parts.push(other),
        }
    }
    if parts.iter().all(|part| matches!(part, SliceGuard::None)) {
        SliceGuard::None
    } else {
        SliceGuard::Or(Arc::from(parts.into_boxed_slice()))
    }
}

/// The literal operand of an equality guard, if the expression IS one.
/// Whether an equality operand is a value [`SliceGuard::EqValue`] reads:
/// a reference (an identifier, `this`, or a static member chain over
/// one) or a primitive literal. Any other operand evaluates in its own
/// right, and the guard never re-evaluates it.
fn is_equality_value_operand(expression: &Expression<'_>) -> bool {
    match unwrap_reference_transparent(expression) {
        Expression::Identifier(_)
        | Expression::ThisExpression(_)
        | Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_) => true,
        Expression::StaticMemberExpression(member) => is_equality_value_operand(&member.object),
        Expression::UnaryExpression(unary) => {
            unary.operator == UnaryOperator::UnaryNegation
                && matches!(
                    unwrap_parenthesized(&unary.argument),
                    Expression::NumericLiteral(_) | Expression::BigIntLiteral(_)
                )
        }
        _ => false,
    }
}

fn guard_literal_of(expression: &Expression<'_>, source: &str) -> Option<SliceGuardLiteral> {
    match unwrap_parenthesized(expression) {
        Expression::StringLiteral(literal) => {
            Some(SliceGuardLiteral::String(Arc::from(literal.value.as_str())))
        }
        Expression::NumericLiteral(literal) => Some(SliceGuardLiteral::Number(Arc::from(
            &source[literal.span.start as usize..literal.span.end as usize],
        ))),
        Expression::BooleanLiteral(literal) => Some(SliceGuardLiteral::Boolean(literal.value)),
        Expression::NullLiteral(_) => Some(SliceGuardLiteral::Null),
        Expression::Identifier(identifier) if identifier.name.as_str() == "undefined" => {
            Some(SliceGuardLiteral::Undefined)
        }
        Expression::UnaryExpression(unary)
            if unary.operator == UnaryOperator::UnaryNegation
                && matches!(
                    unwrap_parenthesized(&unary.argument),
                    Expression::NumericLiteral(_)
                ) =>
        {
            Some(SliceGuardLiteral::Number(Arc::from(
                &source[unary.span.start as usize..unary.span.end as usize],
            )))
        }
        _ => None,
    }
}

/// Whether a leaf ANSWER contains a value the shared shallow pass
/// FABRICATED for a call it has no model for.
///
/// Two independent readings, both of the same fact:
///
/// - the answer EMBEDS the pass's own unreduced `ReturnType<callee>`
///   carrier — decided off the answer alone, because the carrier is a
///   shape only this pass mints;
/// - the answer EMBEDS `any` AND the expression's value composes over a
///   call with no structural arm — decided off the answer AND the FORM,
///   because an `any` is indistinguishable from an authored one and the
///   form is what says whether it was authored.
///
/// The conjunction is what keeps the second reading from over-refusing: a
/// form that contains a call but whose answer the pass models
/// (`f() === 1` is `boolean`, `f() as T` is `T`) embeds no `any` and
/// passes.
fn leaf_answer_is_fabricated_at_a_call_position(ty: &TypeExpr, expr: &Expression<'_>) -> bool {
    if embeds_call_return_carrier(ty) {
        return true;
    }
    verter_type_expr::referenced_names(ty).embeds_any
        && verter_semantic::analysis::flow::value_composes_unmodeled_call(expr)
}
