//! The slice IR: the owned, arena-free statements, expressions, guards and calls a flow
//! slice lowers to, and the content-free lexical context of nested functions. Produced by
//! the semantic source lowering and consumed by flow evaluation.

use crate::flow::completion::NormalCompletion;
use crate::flow::flow_ir::{FlowExprRole, FlowSliceIR};
use crate::flow::{
    binding::FlowBindingRef,
    frame_span::FrameSpan,
    skeleton::{FunctionBodySkeleton, NameMeaning, SkeletonBindingId, SkeletonBindingKind},
};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use verter_type_expr::TypeExpr;

/// The demand selection one content lowering serves: the value-selected
/// expression spans and the value-selected slot declaration spans of ONE
/// lowered flow slice. Derived from the content-free `FlowSliceIR` — the
/// plan is the sole authority for what lowers; this carrier only
/// transports the selection into the lease-only run.
#[derive(Debug, Clone)]
pub struct FlowSliceSelection {
    /// VALUE-selected expression addresses, in the frame's own coordinates.
    /// A conflicting duplicate span stays selected but has no unique site;
    /// content must never attach one site's definition to another site.
    value_sites:
        rustc_hash::FxHashMap<FrameSpan, Option<crate::flow::skeleton::SkeletonExprSiteId>>,
    /// Binding-identifier spans of the slice's VALUE-selected slots —
    /// DECLARATION-precise identity, so a shadowed same-named sibling
    /// declarator the plan kept out never lowers (name identity would
    /// re-conflate what the plan's lexical resolution separated).
    value_slot_spans: FxHashSet<FrameSpan>,
    #[cfg(any(test, feature = "test-support"))]
    pub assignment_lookup_work: Option<Arc<std::sync::atomic::AtomicUsize>>,
}

impl FlowSliceSelection {
    /// The selection of one lowered slice.
    pub fn from_slice_ir(ir: &FlowSliceIR) -> Self {
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
            #[cfg(any(test, feature = "test-support"))]
            assignment_lookup_work: None,
            value_slot_spans: ir
                .slots
                .iter()
                .filter(|slot| slot.value_selected)
                .map(|slot| slot.span)
                .collect(),
        }
    }

    pub fn value_span(&self, span: FrameSpan) -> bool {
        self.value_sites.contains_key(&span)
    }

    pub fn value_site(&self, span: FrameSpan) -> Option<crate::flow::skeleton::SkeletonExprSiteId> {
        // This also performs the pre-existing selection filter. Only an
        // admitted value position requests definition identity; no unrelated
        // write inventory is inspected after that filter.
        let site = self.value_sites.get(&span).copied().flatten()?;
        #[cfg(any(test, feature = "test-support"))]
        if let Some(work) = &self.assignment_lookup_work {
            work.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Some(site)
    }

    pub fn value_slot_span(&self, span: FrameSpan) -> bool {
        self.value_slot_spans.contains(&span)
    }
}

/// The OWNED content of one demanded slice: lowered parameters, the root
/// body region with slice-gated expression content, and the region's
/// reachability result.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceContent {
    pub bindings: Arc<crate::flow::binding::FlowBindingMap>,
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
    pub call_arguments: Arc<FxHashMap<verter_span::Span, Arc<[SliceCallArgument]>>>,
}

/// One argument of an authored call, lowered in the frame as a whole value
/// ([`SliceContent::call_arguments`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SliceCallArgument {
    /// The argument's value.
    pub value: SliceExpr,
    /// An object or array literal argument lowered in a const context, as
    /// `as const` lowers it: the value the checker checks the literal as
    /// when its contextual type is a `const` type parameter
    /// (`isConstContext`). `None` for any other argument.
    pub const_context: Option<SliceExpr>,
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
    /// Whether the parameter is a plain identifier with neither an
    /// annotation nor a default: the parameter a contextual signature
    /// types (`getTypeAtPosition` of the contextual signature), `any`
    /// without one.
    pub contextually_typed: bool,
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

/// One statement of the slice content.
#[derive(Debug, Clone, PartialEq)]
pub enum SliceStatement {
    Gap(crate::flow::policy::FlowGap),
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
        definition: crate::flow::skeleton::SkeletonExprSiteId,
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
        /// The authored assertion call's span (absolute): the evaluator
        /// records its call evidence against exactly this call.
        call: verter_span::Span,
    },
    /// A statement call whose callee is a dotted name this half cannot
    /// settle alone (`o.m();`, `obj.run();`): the checker's
    /// `getEffectsSignature` reads the callee's explicit type, and an
    /// `asserts` signature narrows what follows while a `never` return
    /// ends the path. The evaluator reads the callee's call signatures
    /// (`SignaturesOfType`): none asserting and none returning `never`
    /// leaves the path as it is, and a `never` effects signature ends it;
    /// an asserting one takes the typed guard-narrowing gap.
    CallEffect {
        /// The callee's type source.
        callee: SliceEffectCallee,
        /// The call expression — its absolute span is the call obligation
        /// a settled callee discharges, and its arguments resolve an
        /// overloaded or generic effects signature.
        site: SliceCallSite,
    },
    /// A statement call whose bare callee names a value this file does not
    /// declare as ONE closed function (an import, a declared constant, an
    /// overload group): the checker reads the call's effect from the
    /// callee's declared signatures alone. None asserting ⇒ no effect; one
    /// non-generic `asserts x is T` / `asserts x` ⇒ its argument narrows
    /// for the rest of the region; a set holding a `never`
    /// return takes the effect of the signature the call resolves; any other
    /// signature set takes the typed guard-narrowing gap.
    CalleeEffect {
        /// `typeof callee`, resolved in owner scope.
        callee: GatedType,
        /// Each argument's narrowable reference, positionally.
        arguments: Arc<[Option<SliceNarrowSubject>]>,
        /// The call expression, whose arguments resolve an overloaded or
        /// generic effects signature.
        site: SliceCallSite,
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
        definition: Option<crate::flow::skeleton::SkeletonExprSiteId>,
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
        definition: crate::flow::skeleton::SkeletonExprSiteId,
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
        binding: crate::flow::skeleton::SkeletonBindingId,
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
        /// (`autoArrayType`, [`verter_session_query::flow::skeleton::SkeletonBinding::evolving_array`]):
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
    pub binding: crate::flow::binding::FlowBindingRef,
    pub reads: Arc<[crate::flow::binding::FlowBindingRef]>,
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

/// A pattern is as deep as the destructuring it lowers, and the derived
/// drop would release it a native level per nested pattern: the nested
/// patterns this element solely owns are taken off and released from this
/// loop, each leaving an empty pattern behind.
impl Drop for SlicePatternElement {
    fn drop(&mut self) {
        let empty = || SlicePattern::Array {
            elements: Arc::from([]),
            rest: None,
        };
        let mut released = vec![std::mem::replace(&mut self.pattern, empty())];
        while let Some(mut pattern) = released.pop() {
            match &mut pattern {
                SlicePattern::Object { properties, .. } => {
                    if let Some(properties) = Arc::get_mut(properties) {
                        for (_, element) in properties.iter_mut() {
                            released.push(std::mem::replace(&mut element.pattern, empty()));
                        }
                    }
                }
                SlicePattern::Array { elements, .. } => {
                    if let Some(elements) = Arc::get_mut(elements) {
                        for element in elements.iter_mut().flatten() {
                            released.push(std::mem::replace(&mut element.pattern, empty()));
                        }
                    }
                }
                SlicePattern::Binding { .. } | SlicePattern::Target { .. } => {}
            }
        }
    }
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
    /// `case <literal>:` against a represented discriminant. Its dispatch
    /// edge narrows the discriminant to the literal.
    Literal(SliceGuardLiteral),
    /// A clause whose dispatch relation IS a guard: `case "number":` under
    /// `switch (typeof x)` is `typeof x === "number"`
    /// (`narrowTypeBySwitchOnTypeOf`), and `case c:` under `switch (true)`
    /// is the condition `c` (`narrowTypeBySwitchOnTrue`). Its dispatch edge
    /// applies the guard; the default clause's edge applies every clause's
    /// guard negated.
    Guard(Box<SliceGuard>),
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
        binding: crate::flow::skeleton::SkeletonBindingId,
    },
    /// A modelable same-frame local (`const` / `let` / `var`), by name.
    Local {
        name: Arc<str>,
        binding: crate::flow::binding::FlowBindingRef,
    },
}

impl PartialEq for SliceNarrowRoot {
    fn eq(&self, other: &Self) -> bool {
        use crate::flow::binding::FlowBindingRef;
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
                &crate::flow::binding::FlowBindingRef::Local(*binding),
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
    /// The authored assertion call's span (absolute).
    pub call: verter_span::Span,
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
pub struct GatedLeaf(pub TypeExpr, pub Option<FlowBindingRef>);

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
    pub fn map_ty(self, f: impl FnOnce(TypeExpr) -> TypeExpr) -> Self {
        Self(f(self.0), self.1)
    }
}

/// A statement nests without bound (a block in a block, an `if` in an
/// arm), and the derived drop glue would drop a nest a native level per
/// level. Dropping moves the statements of the regions a statement solely
/// owns onto an explicit stack first, so a nest however deep drops from this
/// loop. A region behind a shared `Arc` another owner still holds is left
/// to that owner.
impl Drop for SliceStatement {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        self.take_nested_statements(&mut pending);
        while let Some(mut statement) = pending.pop() {
            statement.take_nested_statements(&mut pending);
        }
    }
}

impl SliceStatement {
    /// Move the statements of the regions this statement solely owns onto
    /// `out`, leaving [`SliceStatement::Throw`] in their place.
    fn take_nested_statements(&mut self, out: &mut Vec<SliceStatement>) {
        fn take(region: &mut SliceRegion, out: &mut Vec<SliceStatement>) {
            if let Some(statements) = Arc::get_mut(&mut region.statements) {
                for statement in statements.iter_mut() {
                    out.push(std::mem::replace(statement, SliceStatement::Throw));
                }
            }
        }
        match self {
            SliceStatement::If {
                consequent,
                alternate,
                ..
            } => {
                take(consequent, out);
                if let Some(alternate) = alternate {
                    take(alternate, out);
                }
            }
            SliceStatement::Block(region) => take(region, out),
            SliceStatement::Loop(lowered) => {
                take(&mut lowered.init, out);
                take(&mut lowered.test_effects, out);
                take(&mut lowered.body, out);
                take(&mut lowered.update, out);
            }
            SliceStatement::Unreachable(region) => take(region, out),
            SliceStatement::Switch { cases, .. } => {
                if let Some(cases) = Arc::get_mut(cases) {
                    for case in cases.iter_mut() {
                        take(&mut case.region, out);
                    }
                }
            }
            SliceStatement::Try {
                block,
                catch,
                finally,
                ..
            } => {
                take(block, out);
                if let Some(catch) = catch {
                    take(&mut catch.region, out);
                }
                if let Some(finally) = finally {
                    take(finally, out);
                }
            }
            SliceStatement::Labeled { body, .. } => take(body, out),
            _ => {}
        }
    }
}

/// A slice expression nests without bound (an operand of an operand, a
/// member value of a member value), and the derived drop glue would drop it
/// a native level per level. Dropping moves each expression's owned
/// sub-expressions onto an explicit stack first, so a nest however deep
/// drops from this loop. A sub-expression behind a shared `Arc` another
/// owner still holds is left to that owner.
impl Drop for SliceExpr {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        self.take_sub_expressions(&mut pending);
        while let Some(mut expr) = pending.pop() {
            expr.take_sub_expressions(&mut pending);
        }
    }
}

impl SliceExpr {
    /// Move the sub-expressions this expression solely owns onto `out`,
    /// leaving [`SliceExpr::Elided`] in their place.
    fn take_sub_expressions(&mut self, out: &mut Vec<SliceExpr>) {
        fn take(expr: &mut SliceExpr, out: &mut Vec<SliceExpr>) {
            if !matches!(expr, SliceExpr::Elided) {
                out.push(std::mem::replace(expr, SliceExpr::Elided));
            }
        }
        fn take_all(exprs: &mut Arc<[SliceExpr]>, out: &mut Vec<SliceExpr>) {
            if let Some(exprs) = Arc::get_mut(exprs) {
                for expr in exprs.iter_mut() {
                    take(expr, out);
                }
            }
        }
        match self {
            SliceExpr::FrameShadowed { inner, .. } => take(inner, out),
            SliceExpr::OptionalAnyChain { root } | SliceExpr::OptionalMember { root, .. } => {
                take(root, out)
            }
            SliceExpr::Object { entries, .. } => {
                if let Some(entries) = Arc::get_mut(entries) {
                    for entry in entries.iter_mut() {
                        match entry {
                            SliceObjectEntry::Member(member) => {
                                take(&mut member.value, out);
                                if let Some(value) = member.assignment_value.as_mut() {
                                    take(value, out);
                                }
                                if let Some(value) = member.unwidened.as_mut() {
                                    take(value, out);
                                }
                            }
                            SliceObjectEntry::Spread { source, .. } => take(source, out),
                        }
                    }
                }
            }
            SliceExpr::Array { elements, .. } => {
                if let Some(elements) = Arc::get_mut(elements) {
                    for element in elements.iter_mut() {
                        match element {
                            SliceArrayElement::Value {
                                value,
                                pre_widening,
                                ..
                            } => {
                                take(value, out);
                                if let Some(value) = pre_widening.as_mut() {
                                    take(value, out);
                                }
                            }
                            SliceArrayElement::Spread { source } => take(source, out),
                            SliceArrayElement::Elision => {}
                        }
                    }
                }
            }
            SliceExpr::Sequence { value, .. }
            | SliceExpr::Awaited { operand: value }
            | SliceExpr::Satisfies { operand: value, .. }
            | SliceExpr::Not { operand: value, .. }
            | SliceExpr::NonNull { operand: value }
            | SliceExpr::MemberOf { object: value, .. }
            | SliceExpr::Assignment { value, .. } => take(value, out),
            SliceExpr::Void { operand, value } => {
                take(operand, out);
                take(value, out);
            }
            SliceExpr::ElementAccess { object, index, .. } => {
                take(object, out);
                take(index, out);
            }
            SliceExpr::Logical { left, right, .. } => {
                take(left, out);
                take(right, out);
            }
            SliceExpr::Arithmetic { operands, .. } => take_all(operands, out),
            SliceExpr::Union { arms, .. } => take_all(arms, out),
            SliceExpr::Call(call, _, arguments) => {
                match call {
                    SliceCall::Nested(value)
                    | SliceCall::OnValue { object: value, .. }
                    | SliceCall::Member {
                        receiver: value, ..
                    }
                    | SliceCall::Construct(value)
                    | SliceCall::TaggedTemplate(value)
                    | SliceCall::OptionalChain { root: value, .. } => take(value, out),
                    SliceCall::OnElement { object, index } => {
                        take(object, out);
                        take(index, out);
                    }
                    _ => {}
                }
                if let Some(arguments) = Arc::get_mut(&mut arguments.0) {
                    for argument in arguments.iter_mut().flatten() {
                        take(argument, out);
                    }
                }
            }
            _ => {}
        }
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
        binding: crate::flow::binding::FlowBindingRef,
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
    /// A template literal expression in a const context (``a${n}` as
    /// const`): the template literal type of its holes' types, each hole a
    /// value this frame evaluates in the const context.
    ConstTemplate {
        /// The raw text around the holes, one more than the holes.
        quasis: Arc<[Arc<str>]>,
        /// The holes in source order.
        holes: Arc<[SliceExpr]>,
    },
    /// A nested function VALUE (a function / arrow expression or an
    /// object-literal method in any expression position): its parameters
    /// and OWNED body region, lowered inline — the evaluator answers its
    /// body-derived return through the same flow evaluation, never a body
    /// scan and never a leaf fallback.
    NestedFunctionValue {
        function: crate::function_program::FunctionProgramKey,
        context: Arc<NestedFlowContext>,
        has_declared_return: bool,
        gap: Option<crate::flow::policy::FlowGap>,
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
        declared_evolving_captures: Arc<[crate::function_program::FlowBindingIdentity]>,
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
        /// The authored member expression's span: the identity a consuming
        /// position matches the read's freshness by.
        span: verter_span::Span,
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
    Gap(crate::flow::policy::FlowGap),
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
        definition: crate::flow::skeleton::SkeletonExprSiteId,
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
    pub outer_clauses: Arc<[crate::flow::policy::ClassExpressionClause]>,
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
    pub binding: crate::function_program::FlowBindingIdentity,
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
pub struct SliceCallArguments(pub Arc<[Option<SliceExpr>]>);

#[allow(clippy::len_without_is_empty)]
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

    /// How many argument positions this records (lowered or not).
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
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
    /// The call an optional chain holds (`g?.()`, `o.m?.()`, `o?.m()`,
    /// `o?.m?.()`, `g?.().length`): the callee is `root` read through the
    /// static member `links`, called — through its own nullish strip when
    /// the call is authored `?.()` — exactly as a member callee is, and
    /// `after` are the static member links read off the call's value.
    /// Every link carries its `?.`-authored optionality. The chain
    /// short-circuits to `undefined` on each edge a strip removes a
    /// nullish arm on, so its value is the read with `undefined` beside
    /// it exactly when a strip removed arms (the checker's optional-chain
    /// marker), and `undefined` alone when a stripped base is nullish
    /// through and through.
    OptionalChain {
        root: Box<SliceExpr>,
        links: Arc<[(Arc<str>, bool)]>,
        optional_call: bool,
        after: Arc<[(Arc<str>, bool)]>,
    },
    /// A call on a parameter or in-scope local binding of function type —
    /// the call's value is the binding's signature return (a shadowed
    /// name is never a flow obligation edge).
    OnBinding {
        binding: crate::flow::binding::FlowBindingRef,
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
    Direct(crate::function_program::FunctionProgramKey),
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
        /// The calling class's own `this`, which a base member's
        /// polymorphic `this` is bound to.
        this: Option<SliceThis>,
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
    /// A call of an element of a value whose key this frame evaluates
    /// (`t[k]()`): the object is the call's receiver, and the key's
    /// literal type names the member the call resolves over.
    OnElement {
        object: Box<SliceExpr>,
        index: Box<SliceExpr>,
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
    pub predicate: Arc<verter_type_expr::TypePredicate>,
    pub target: Option<GatedType>,
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
    pub ty: TypeExpr,
    pub shadowed: Arc<[FrameShadowedName]>,
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
    pub fn add_shadowed(&mut self, extra: impl IntoIterator<Item = FrameShadowedName>) {
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
        /// Whether the literal is in a const context, which copies the
        /// source's properties `readonly`.
        readonly: bool,
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
        definition: crate::flow::skeleton::SkeletonExprSiteId,
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
    /// Whether the member is context sensitive (the checker's
    /// `isContextSensitive` of a property assignment or method): a function
    /// value its context types, or a literal holding one. A call's first
    /// inference pass reads such a member as the non-inferring
    /// `anyFunctionType` (`SkipContextSensitive`).
    pub context_sensitive: bool,
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

/// The names THIS signature's parameter list binds, paired with their
/// binding-identifier spans, read from the frame's own
/// [`FunctionBodySkeleton`] — the SAME single lexical authority every
/// other classification in this module routes through. A DESTRUCTURED
/// element is inventoried exactly like a plain binding identifier: the
/// checker resolves `typeof a` in `f({ a }: { a: number }, b: typeof a)`
/// to the destructured element.
#[cfg(any(test, feature = "test-support"))]
pub mod capture_lookup_probe {
    use std::cell::RefCell;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    thread_local! {
        static ACTIVE: RefCell<Option<Arc<AtomicUsize>>> = const { RefCell::new(None) };
    }
    pub struct Scope(Option<Arc<AtomicUsize>>);
    impl Drop for Scope {
        fn drop(&mut self) {
            ACTIVE.with(|active| *active.borrow_mut() = self.0.take());
        }
    }
    pub fn enter(counter: Arc<AtomicUsize>) -> Scope {
        Scope(ACTIVE.with(|active| active.replace(Some(counter))))
    }
    pub fn inspect() {
        ACTIVE.with(|active| {
            if let Some(counter) = active.borrow().as_ref() {
                counter.fetch_add(1, Ordering::Relaxed);
            }
        });
    }
}

/// How the frame's lexical authority classifies one identifier.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NameBinding {
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
pub struct DefiningFrameGate {
    pub skeleton: Arc<FunctionBodySkeleton>,
    pub bindings: Arc<crate::flow::binding::FlowBindingMap>,
    pub type_parameters: Arc<[Arc<str>]>,
    /// The ENCLOSING declaration's clause, by name: a class member's
    /// class clause (`class C<T> { m() { … } }`), empty otherwise.
    pub enclosing_type_parameters: Arc<[Arc<str>]>,
    pub parameters: Arc<rustc_hash::FxHashMap<SkeletonBindingId, CaptureParameterLocator>>,
    pub parameter_names: Arc<rustc_hash::FxHashMap<Arc<str>, u32>>,
    /// The bindings a destructuring declarator binds whose whole pattern
    /// this half models ([`SlicePattern`]).
    pub modelled_patterns: Arc<FxHashSet<SkeletonBindingId>>,
    pub body_hash: [u8; 16],
    pub snapshot: crate::source::snapshot::SnapshotKey,
    pub outer: CaptureScope,
    pub anchor: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureParameterLocator {
    pub binding: SkeletonBindingId,
    pub ordinal: usize,
    pub key: Option<Arc<str>>,
    pub has_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFrame {
    pub gate: Arc<DefiningFrameGate>,
    pub region: crate::flow::skeleton::SkeletonRegionId,
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
    /// A method or accessor of an object literal (or an arrow one creates)
    /// in a project without `noImplicitThis`: the checker types the
    /// literal's `this` only under that option
    /// (`getContextualThisParameterType`), so `this` is `any`.
    Untyped,
}

/// The exact lexical chain at a nested function's authored position.
/// Shared frame handles avoid enumerating or copying visible declarations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureScope {
    pub enclosing: Option<Arc<CapturedFrame>>,
}

/// Owned content-free lexical context at the exact nested function position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedFlowContext {
    pub captures: CaptureScope,
    /// The lexical `this` an ARROW created in a class member reads; `None`
    /// for every other nested function, whose `this` is its own.
    pub this: Option<SliceThis>,
}

impl NestedFlowContext {
    /// What `this` reads inside the nested function.
    pub fn this(&self) -> Option<&SliceThis> {
        self.this.as_ref()
    }
}

/// An exact source declaration eligible to provide a selected capture's type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceCaptureAuthorityLocator {
    pub binding: crate::function_program::FlowBindingIdentity,
    pub declaration: crate::function_program::FlowBindingIdentity,
    pub source: SliceCaptureAuthoritySource,
    pub parameter_ordinal: Option<usize>,
    pub gate: Arc<DefiningFrameGate>,
}

impl SliceCaptureAuthorityLocator {
    pub fn local_declaration(&self) -> Option<SkeletonBindingId> {
        self.gate.bindings.local(&self.declaration)
    }

    pub fn matches_snapshot(&self, snapshot: &crate::source::snapshot::SnapshotKey) -> bool {
        &self.gate.snapshot == snapshot
    }
}

impl NestedFlowContext {
    pub fn matches_snapshot(&self, snapshot: &crate::source::snapshot::SnapshotKey) -> bool {
        self.captures
            .enclosing
            .as_ref()
            .is_some_and(|frame| &frame.gate.snapshot == snapshot)
    }

    pub fn mutable_authorities(
        &self,
        identity: &crate::function_program::FlowBindingIdentity,
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
                        #[cfg(any(test, feature = "test-support"))]
                        capture_lookup_probe::inspect();
                        let fact = frame.gate.skeleton.binding(declaration);
                        #[cfg(any(test, feature = "test-support"))]
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

/// A chain drops from this loop, each frame no other scope shares taken off
/// in turn: dropping it frame inside frame would take a native level per
/// enclosing function.
impl Drop for CaptureScope {
    fn drop(&mut self) {
        let mut next = self.enclosing.take();
        while let Some(frame) = next {
            next = Arc::try_unwrap(frame).ok().and_then(|frame| {
                Arc::try_unwrap(frame.gate)
                    .ok()
                    .and_then(|mut gate| gate.outer.enclosing.take())
            });
        }
    }
}

impl CaptureScope {
    pub fn gate(&self, ty: TypeExpr, binders: &[Arc<str>]) -> GatedType {
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
    pub fn defining_local(
        &self,
        identity: &crate::function_program::FlowBindingIdentity,
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

    pub fn classify_identity(
        &self,
        identity: &crate::function_program::FlowBindingIdentity,
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

    pub fn lookup(&self, name: &str) -> NameBinding {
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

    pub fn binder_is_visible(&self, name: &str) -> bool {
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
    pub fn destructured_var_is_modelled(&self, binding: SkeletonBindingId) -> bool {
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
    pub fn value_name_is_bound(&self, name: &str, span: FrameSpan) -> bool {
        let region = self.skeleton.innermost_region_containing(span);
        self.skeleton.name_id(name).is_some_and(|name| {
            !self
                .skeleton
                .bindings_of_name_in_scope(name, region)
                .is_empty()
        }) || !matches!(self.outer.lookup(name), NameBinding::Free)
    }
    pub fn name_is_bound(
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

    pub fn answer_names_frame_bound(
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

crate::transports_completion!(SliceRegion => SliceRegion);
crate::transports_completion!(SliceContent => SliceContent);
crate::transports_completion!(SliceSwitchCase => SliceSwitchCase);
