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
//! callee) as [`SliceCall::Symbolic`].

use std::sync::Arc;

use oxc_ast::ast::{
    BindingPattern, Expression, FormalParameters, LogicalOperator, Program, Statement, TSType,
    UnaryOperator, VariableDeclarationKind,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;
use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::flow::flow_ir::{FlowExprRole, FlowSliceIR};
use verter_semantic::analysis::flow::{
    build_function_body_skeleton, object_entry_descent, value_descent, FrameSpan,
    FunctionBodySkeleton, FunctionBodySource, NameMeaning, ObjectEntryDescent, ObjectEntryKey,
    ObjectEntryKind, SkeletonBindingId, SkeletonBindingKind, SkeletonPathSegment,
    SkeletonWriteTarget, ValueDescent,
};
use verter_semantic::analysis::function_program::{
    for_each_call_expression, inventory_statement_list, resolve_function_node,
    FunctionControlRegion, FunctionNode, FunctionProgramEntry,
};
use verter_semantic::analysis::type_eval_build::{
    embeds_call_return_carrier, infer_declaration_expression_type,
    infer_declaration_expression_type_with_completeness, ExpressionInferenceCompleteness,
    TopLevelLiteralPolicy,
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
    /// same-file single-declaration callee with an authored non-predicate
    /// return annotation. A predicate call in a test CONTROLS narrowing
    /// and is never recorded here: it takes real evaluator evidence at
    /// guard application, or its obligations stay unclaimed. Each span is
    /// a call occurrence whose type position this run decided without the
    /// call — the containment evidence the discharge-report producer
    /// accepts for a call obligation the evaluator's call sink never
    /// reached. Absolute spans: the report rebases them onto the frame
    /// anchor when pairing against the skeleton footprint.
    pub decided_above_call_spans: Vec<verter_span::Span>,
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
    pub can_fall_through: bool,
}

/// One statement of the slice content.
#[derive(Debug, Clone, PartialEq)]
pub enum SliceStatement {
    Gap(crate::semantic_query::FlowGap),
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
    /// parameter or a modelable same-frame local, whose right-hand side
    /// the demand slice value-selected. This is the ONE statement form
    /// through which a write re-enters evaluation: the evaluator applies
    /// it (retyping the binding in source order), so the write-effect
    /// ledger no longer has to degrade on sight of it.
    ///
    /// Every other write shape stays out of the content tree exactly as
    /// before and keeps the typed unapplied-write degradation: a
    /// projection-path write (`x.a = v`) never retypes the binding, and a
    /// write in expression position (`return (x = 1, x)`) has no
    /// evaluation-order guarantee against the reads around it.
    Assignment {
        /// The write target (a binding root; the path is always empty for
        /// this variant — a member-path write never lowers).
        target: SliceNarrowSubject,
        /// The write expression's span, in this frame's coordinates — the
        /// identity the evaluator's write-effect ledger matches against,
        /// so a lowered write and a degraded write are the same fact seen
        /// by the two halves, never two independent verdicts.
        span: FrameSpan,
        /// The lowered right-hand side.
        value: Box<SliceExpr>,
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
        /// `None` for an anonymous (switch) break, the label's name for a
        /// named one.
        target: Option<Arc<str>>,
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
    /// A return-free loop with no selected downstream transfer: fall-through
    /// transparent because no captured guard, call, write, or escaping `var`
    /// can change a later selected read. (A return-free LABELED statement is
    /// transparent too, but its body still lowers — as a labeled region, so
    /// its inner rails and its break exits keep deciding.)
    TransparentLoop,
    /// An unsupported construct (return-bearing loop, `with`, a
    /// `break`/`continue` jump no enclosing modelled construct absorbs, a
    /// module-level statement). The whole function is unsupported: the
    /// region is produced up to this marker and the evaluator degrades
    /// the whole result.
    Unsupported(SliceUnsupported),
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
    /// the statement after the switch).
    pub breaks: bool,
    /// The case test as a guard literal, when it is one (`case "a":`).
    /// `None` for the default clause and for a non-literal test — the
    /// dispatch edge then establishes no discriminant narrow.
    pub test: Option<SliceGuardLiteral>,
}

/// One `catch` clause: its parameter binding (a plain identifier only — a
/// destructured catch parameter binds nothing here) and its body region.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceCatchClause {
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SliceNarrowRoot {
    /// A simple formal parameter, by ordinal in source order.
    Param(u32),
    /// A modelable same-frame local (`const` / `let` / `var`), by name.
    Local(Arc<str>),
}

/// A narrowable reference: a binding root plus a static member path under
/// it (`u.v` carries `[v]`; the empty path is the binding itself).
///
/// Which position a fact narrows is the guard variant's call, not this
/// type's: a `typeof u.v === "string"` narrows the type AT the path,
/// while `u.kind === "a"` narrows the ROOT (the discriminant selects
/// which of the root's union arms survives).
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
}

/// The narrowing facts ONE conditional test establishes, lowered once and
/// shared by the ternary's branch join and the `if` statement's arms —
/// the single authority over test-expression forms, so the two control
/// spellings of one guard can never disagree about what it narrows.
///
/// This is a structural description only: it carries no evaluated types
/// (the content half has no resolver), so a consumer can never inherit a
/// narrow the evaluator did not itself compute. A test form this
/// vocabulary cannot express lowers to [`SliceGuard::None`] — the arms
/// evaluate unnarrowed, exactly as they did before guards existed.
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
    /// is a DISCRIMINANT — the narrow selects the root's union arms by
    /// the member's type.
    EqLiteral {
        /// The compared reference.
        subject: SliceNarrowSubject,
        /// The literal operand.
        literal: SliceGuardLiteral,
        /// Whether the comparison is negated.
        negated: bool,
    },
    /// `subject instanceof Ctor`, the constructor named by a bare
    /// identifier (resolved evaluator-side, in owner scope).
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
    /// `predicate(subject)` — a same-file, single-declaration function
    /// whose declared return is `x is T`. The target type lowers through
    /// the frame gate exactly like a declarator annotation; a cross-file
    /// callee, an overloaded group, or a callee without the predicate
    /// spelling lowers to [`SliceGuard::None`].
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
    /// A conjunction: every fact applies at once.
    And(Arc<[SliceGuard]>),
    /// A disjunction: the positive reading unions each disjunct's
    /// positive narrow; the negated reading applies every negation.
    Or(Arc<[SliceGuard]>),
}

fn collect_guard_subjects(guard: &SliceGuard, visitor: &mut impl FnMut(&SliceNarrowSubject)) {
    match guard {
        SliceGuard::None => {}
        SliceGuard::Typeof { subject, .. }
        | SliceGuard::Truthy { subject, .. }
        | SliceGuard::EqLiteral { subject, .. }
        | SliceGuard::Instanceof { subject, .. }
        | SliceGuard::TypePredicate { subject, .. }
        | SliceGuard::In { subject, .. } => visitor(subject),
        SliceGuard::And(parts) | SliceGuard::Or(parts) => {
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
        /// The parameter's ordinal in source order (rest last).
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
    /// An object-literal return evaluated STRUCTURALLY: every entry's
    /// contributing expression is a flow expression (parameter / local
    /// references substitute). Plain string-keyed properties, method /
    /// accessor members, and SPREADS lower this way; a computed key still
    /// keeps the whole-literal leaf lowering.
    Object {
        /// The entries in source order — construction order is meaning
        /// (a later entry overrides what an earlier one provisioned).
        entries: Arc<[SliceObjectEntry]>,
    },
    /// A nested function VALUE (a function / arrow expression or an
    /// object-literal method in any expression position): its parameters
    /// and OWNED body region, lowered inline — the evaluator answers its
    /// body-derived return through the same flow evaluation, never a body
    /// scan and never a leaf fallback.
    NestedFunctionValue {
        gap: Option<crate::semantic_query::FlowGap>,
        /// The nested function's formal parameters (rest last).
        params: Arc<[SliceParam]>,
        /// The nested function's own type parameters (the signature's own
        /// binders — carried so the composed signature keeps `<T>`).
        type_parameters: Arc<[SliceTypeParam]>,
        /// Authored declared authorities for mutable bindings captured from
        /// enclosing frames. These exact declaration facts type reads across
        /// the closure boundary without changing source-ordered initialization.
        mutable_capture_authorities: Arc<[SliceCaptureAuthority]>,
        /// The DECLARED return annotation, when authored. A declared
        /// return always wins over the body-derived join (the checker
        /// checks the body AGAINST the annotation; the signature's return
        /// IS the annotation), so the evaluator answers the annotation
        /// and never evaluates the body for the signature's return.
        declared_return: Option<GatedType>,
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
    SemanticAny,
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
}

/// The source of one mutable closure capture's authored declaration authority.
#[derive(Debug, Clone, PartialEq)]
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
    let fixed = call
        .arguments
        .iter()
        .take_while(|argument| !matches!(argument, oxc_ast::ast::Argument::SpreadElement(_)))
        .count();
    SliceCallSite::new(
        fixed as u32,
        fixed != call.arguments.len(),
        call.type_arguments.is_some(),
        call.span.into(),
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
        source: SliceExpr,
    },
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
        /// Its evaluated LITERAL names the property.
        value: Box<SliceExpr>,
        /// The AUTHORED key, through the shared property-key lowering.
        ///
        /// The value channel cannot carry every nameable key: a `unique
        /// symbol` key names exactly one nominal property, and the
        /// evaluator flattens its value to the bare `symbol` primitive,
        /// losing the identity the name IS. The authored channel keeps
        /// it (`typeof ob12Key`), and is the same carrier the
        /// whole-literal leaf answer used to produce — so a symbol key
        /// names its property exactly as before, without the literal
        /// having to abandon its structural lowering to get there.
        authored: verter_type_expr::TypeAuthoredPropertyKey,
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
    /// Under `satisfies T`: members keep their literals and are NOT
    /// `readonly`.
    ///
    /// `satisfies` keeps the operand's SOURCE type, and whether a fresh
    /// member literal survives in that source type depends on whether the
    /// TARGET contextually types it — `{ mode: "dark" } satisfies { mode:
    /// "dark" | "light" }` is `{ mode: "dark" }`, while `{ n: 1 }
    /// satisfies object` is `{ n: number }` (both pinned against tsgo
    /// `7.0.0-dev.20260526.1`). This substrate performs no contextual
    /// typing, so it takes the PRESERVING side of that split uniformly,
    /// which is the shallow pass's own long-standing choice; the
    /// target-driven half is the separate deferred contextual-widening
    /// contract.
    Preserve,
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
/// Only the two carriers that PRESERVE the operand's own member set
/// qualify. `x as const` pins every member; `x satisfies T` keeps the
/// operand's source type (see [`ObjectMemberPolicy`]). A non-const `as T`
/// / `<T>x` REPLACES the type with `T`, a `!` non-null assertion and a
/// `<T>`-instantiation say nothing about members — every one of those
/// keeps the whole-carrier leaf lowering, where the carrier's own answer
/// is the honest one.
fn member_literal_policy(expression: &Expression<'_>, source: &str) -> Option<ObjectMemberPolicy> {
    match expression {
        Expression::ParenthesizedExpression(paren) => {
            member_literal_policy(&paren.expression, source)
        }
        Expression::TSSatisfiesExpression(_) => Some(ObjectMemberPolicy::Preserve),
        // The SHARED const-assertion authority decides, so `as const`
        // and a non-const `as T` are never told apart twice.
        Expression::TSAsExpression(_) => {
            verter_semantic::analysis::type_eval_build::expr_is_const_asserted(expression, source)
                .then_some(ObjectMemberPolicy::ConstAssert)
        }
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
    /// The authored method / accessor kind (`None` for a plain property).
    pub method_kind: Option<verter_type_expr::ObjectMethodKind>,
    /// Whether the member is `readonly` — true exactly under an enclosing
    /// `as const`, which is the only object-literal form that mints one.
    pub readonly: bool,
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
    let params = match lower_params(
        node.params(),
        source,
        &SignatureScope::Root,
        skeleton,
        anchor,
    ) {
        Ok(params) => params,
        Err(reason) => {
            return Some(SliceContent {
                can_fall_through: false,
                params: Arc::from(Vec::new().into_boxed_slice()),
                type_parameters: Arc::from(Vec::new().into_boxed_slice()),
                enclosing_type_parameters: Arc::from(Vec::new().into_boxed_slice()),
                body: SliceRegion {
                    statements: Arc::from(Vec::new().into_boxed_slice()),
                    can_fall_through: false,
                },
                budget_failure: Some(reason),
                inert_write_spans: FxHashSet::default(),
                decided_above_call_spans: Vec::new(),
            });
        }
    };
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
        program,
        budget_failure: None,
        inert_write_spans: FxHashSet::default(),
        decided_above_call_spans: Vec::new(),
        predicate_guard_call_spans: FxHashSet::default(),
        control_test_gap: false,
        unsafe_invoked_closure_effects: FxHashSet::default(),
        nested_free_writes: FxHashSet::default(),
        active_guard_bindings: Vec::new(),
        active_guard_names: Vec::new(),
        break_targets: Vec::new(),
        break_target_followed_by_return: Vec::new(),
        current_statement_followed_by_return: false,
    };
    lowerer.unsafe_invoked_closure_effects =
        lowerer.index_unsafe_invoked_closure_effects(&body.statements);
    lowerer.nested_free_writes = lowerer.build_nested_free_writes(&body.statements);
    let region = if node.is_expression_body() {
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
                can_fall_through: false,
            }
        } else {
            let widening_literal = expr_is_bare_literal(&expression.expression);
            let argument = if lowerer.value_span_selected(expression.expression.span()) {
                lowerer.lower_expr(&expression.expression, ExprMode::Return)
            } else {
                SliceExpr::Elided
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
                widening_literal,
            });
            SliceRegion {
                statements: Arc::from(statements.into_boxed_slice()),
                can_fall_through: false,
            }
        }
    } else {
        lowerer.lower_region(&body.statements).region
    };
    let budget_failure = lowerer.budget_failure;
    let inert_write_spans = lowerer.inert_write_spans;
    let decided_above_call_spans = lowerer.decided_above_call_spans;
    Some(SliceContent {
        can_fall_through: region.can_fall_through,
        params: Arc::from(params.into_boxed_slice()),
        type_parameters: Arc::from(type_parameters.into_boxed_slice()),
        enclosing_type_parameters: Arc::from(enclosing_type_parameters.into_boxed_slice()),
        body: region,
        budget_failure,
        inert_write_spans,
        decided_above_call_spans,
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

/// Whether entering this statement guarantees that the current function
/// reaches an authored return before normal completion. This is deliberately
/// stricter than the control inventory's `has_return`: a conditional return
/// does not prevent a preceding labelled break from reaching function end.
fn statement_guarantees_current_function_return(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::ReturnStatement(_) => true,
        Statement::BlockStatement(block) => block
            .body
            .iter()
            .any(statement_guarantees_current_function_return),
        Statement::IfStatement(branch) => {
            statement_guarantees_current_function_return(&branch.consequent)
                && branch
                    .alternate
                    .as_ref()
                    .is_some_and(statement_guarantees_current_function_return)
        }
        _ => false,
    }
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
                    let (binding, has_default) = match &property.value {
                        BindingPattern::BindingIdentifier(id) => {
                            (Arc::from(id.name.as_str()), false)
                        }
                        BindingPattern::AssignmentPattern(assignment) => {
                            match &assignment.left {
                                BindingPattern::BindingIdentifier(id) => {
                                    (Arc::from(id.name.as_str()), true)
                                }
                                // A default over an ALIASED / nested
                                // pattern is not modelled.
                                _ => return None,
                            }
                        }
                        _ => return None,
                    };
                    Some(SliceDestructuredElement {
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

/// The region lowering result: the region plus whether any nested lowering
/// hit an unsupported construct (the marker is in the tree; the flag
/// propagates so the root region stops at the same point).
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
    /// A modelable enclosing parameter / local. Mutable captures use authored
    /// declared authority across the closure boundary; stable captures retain
    /// their reaching value.
    Local { mutable: bool },
    /// An enclosing binding the content half cannot model.
    Unmodeled,
}

/// The names a nested function value captures from its ENCLOSING frames,
/// resolved once at the position the function value itself occupies.
/// Empty for the root frame.
#[derive(Default)]
struct CaptureScope {
    names: FxHashMap<Arc<str>, CapturedBinding>,
    mutable_declared: FxHashMap<Arc<str>, SliceCaptureAuthority>,
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

struct DeclaratorAnnotationFinder<'s> {
    source: &'s str,
    target: oxc_span::Span,
    found: Option<TypeExpr>,
}

impl<'a> Visit<'a> for DeclaratorAnnotationFinder<'_> {
    fn visit_variable_declarator(&mut self, declarator: &oxc_ast::ast::VariableDeclarator<'a>) {
        if self.found.is_none()
            && matches!(&declarator.id, BindingPattern::BindingIdentifier(id) if id.span == self.target)
        {
            self.found = declarator
                .type_annotation
                .as_ref()
                .map(|annotation| lower_ts_type(&annotation.type_annotation, self.source));
            return;
        }
        walk::walk_variable_declarator(self, declarator);
    }
}

impl CaptureScope {
    fn lookup(&self, name: &str) -> NameBinding {
        match self.names.get(name) {
            // A captured binding is read BY NAME from the evaluator's
            // seeded snapshot: no parameter ordinal applies (ordinals
            // index the NESTED frame's own signature).
            Some(CapturedBinding::Local { .. }) => NameBinding::Captured,
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
    /// The whole retained parse snapshot this frame's function lives in.
    /// The guard lowering reads SAME-FILE predicate / assertion
    /// signatures from it (`isStr(u)` / `assertStr(u);` carry their
    /// narrowing fact in the callee's declared return, not at the use
    /// site); a cross-file callee is beyond this channel and lowers to
    /// [`SliceGuard::None`].
    program: &'a Program<'a>,
    /// The first budget edge a SELECTED leaf's expression lowering hit.
    budget_failure: Option<verter_type_expr::facts::InferenceUnavailableReason>,
    /// Write effects proven unreachable by a literal control edge.
    inert_write_spans: FxHashSet<FrameSpan>,
    /// Call / construct spans of decided-above positions — see
    /// [`SliceContent::decided_above_call_spans`].
    decided_above_call_spans: Vec<verter_span::Span>,
    /// The authored call spans whose [`SliceGuard::TypePredicate`] fact
    /// this lowering MINTED: evidence-backed at guard application, so the
    /// control-position recorder neither certifies them decided-above nor
    /// gaps them.
    predicate_guard_call_spans: FxHashSet<verter_span::Span>,
    /// A control-position test lowered inside the CURRENT statement
    /// carried a call this half can neither certify result-independent
    /// nor back with guard evidence: the statement loop drains this into
    /// a [`SliceStatement::Gap`] (`GuardNarrowing`) AHEAD of the
    /// statement — a typed degradation, never a silent certification.
    control_test_gap: bool,
    unsafe_invoked_closure_effects: FxHashSet<FrameSpan>,
    nested_free_writes: FxHashSet<SkeletonBindingId>,
    active_guard_bindings: Vec<SkeletonBindingId>,
    active_guard_names: Vec<Arc<str>>,
    /// The stack of breakable constructs whose bodies are currently being
    /// lowered (innermost last): `None` for a `switch`, `Some(label)` for
    /// a labeled statement. A `break` resolves against this stack — an
    /// unlabeled one targets the innermost `None` entry (labels do not
    /// accept unlabeled breaks), a labeled one the innermost matching
    /// name. Loop bodies never lower, so a loop is never an entry.
    break_targets: Vec<Option<Arc<str>>>,
    /// For each break target, whether the target statement has a guaranteed
    /// current-function return later in its enclosing statement list. A
    /// pending break contributes implicit `undefined` only when its
    /// destination can reach the function end rather than that return.
    break_target_followed_by_return: Vec<bool>,
    /// The suffix fact for the statement currently being lowered; captured
    /// when that statement introduces a break target.
    current_statement_followed_by_return: bool,
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

    /// Whether one resolved binding is part of the demanded value slice.
    /// Nested function values lower without a selection and therefore treat
    /// every binding as selected within their own frame.
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
                            && self
                                .skeleton
                                .bindings_of_name_in_scope(read.name, site.region)
                                .contains(&binding)
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
                    self.skeleton
                        .bindings_of_name_in_scope(read.name, site.region)
                        .iter()
                        .any(|binding| self.binding_is_read_after_loop(*binding, loop_span))
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

        self.skeleton.writes.iter().any(|write| {
            if !loop_span.contains(write.span) {
                return false;
            }
            if self.span_is_in_literal_dead_branch(statement, write.span) {
                self.inert_write_spans.insert(write.span);
                return false;
            }
            let SkeletonWriteTarget::Named(name) = write.target else {
                return false;
            };
            self.skeleton
                .bindings_of_name_in_scope(name, write.region)
                .iter()
                .any(|binding| {
                    self.binding_is_read_after_loop_at_path(*binding, &write.path, loop_span)
                })
        })
    }

    /// Whether a loop test carries a modelled guard over a downstream-selected
    /// slot. A bare arithmetic/truthy expression that establishes no narrowing
    /// fact is inert here; calls and writes are classified independently.
    fn control_test_narrows_downstream_slot(
        &mut self,
        test: &Expression<'_>,
        loop_span: FrameSpan,
    ) -> bool {
        !matches!(self.lower_guard(test), SliceGuard::None)
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
            let call_region = self
                .skeleton
                .innermost_region_containing(self.rebase(call.span));
            let mut inspect = |node: FunctionNode<'_>| {
                if self.nested_function_transfers_downstream_slot(&node, call_region, loop_span) {
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
                let call_region = self.skeleton.innermost_region_containing(call_span);
                if self.nested_function_transfers_downstream_slot(&node, call_region, call_span) {
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

    fn build_nested_free_writes(
        &self,
        statements: &[Statement<'_>],
    ) -> FxHashSet<SkeletonBindingId> {
        struct Collector<'a> {
            nested: Vec<(oxc_span::Span, FunctionBodySkeleton)>,
            marker: std::marker::PhantomData<&'a ()>,
        }

        impl<'a> Visit<'a> for Collector<'a> {
            fn visit_statement(&mut self, statement: &Statement<'a>) {
                if let Statement::FunctionDeclaration(function) = statement {
                    if let Some(source) = FunctionBodySource::from_function_expression(function) {
                        self.nested
                            .push((function.span, build_function_body_skeleton(&source)));
                    }
                }
                walk::walk_statement(self, statement);
            }

            fn visit_expression(&mut self, expression: &Expression<'a>) {
                match expression {
                    Expression::FunctionExpression(function) => {
                        if let Some(source) = FunctionBodySource::from_function_expression(function)
                        {
                            self.nested
                                .push((function.span, build_function_body_skeleton(&source)));
                        }
                    }
                    Expression::ArrowFunctionExpression(arrow) => {
                        let source = FunctionBodySource::from_arrow(arrow);
                        self.nested
                            .push((arrow.span, build_function_body_skeleton(&source)));
                    }
                    _ => {}
                }
                walk::walk_expression(self, expression);
            }
        }

        let mut collector = Collector {
            nested: Vec::new(),
            marker: std::marker::PhantomData,
        };
        for statement in statements {
            collector.visit_statement(statement);
        }

        let mut writes = FxHashSet::default();
        for (creation_span, nested) in collector.nested {
            let outer_region = self
                .skeleton
                .innermost_region_containing(self.rebase(creation_span));
            for write in nested.writes.iter() {
                let SkeletonWriteTarget::Named(name) = write.target else {
                    continue;
                };
                if !nested
                    .bindings_of_name_in_scope(name, write.region)
                    .is_empty()
                {
                    continue;
                }
                let Some(outer_name) = self.skeleton.name_id(nested.name(name)) else {
                    continue;
                };
                for binding in self
                    .skeleton
                    .bindings_of_name_in_scope(outer_name, outer_region)
                {
                    if self.skeleton.binding(binding).kind == SkeletonBindingKind::Let {
                        writes.insert(binding);
                    }
                }
            }
        }
        writes
    }

    fn nested_free_read_bindings(
        &self,
        nested: &FunctionBodySkeleton,
        creation_span: oxc_span::Span,
    ) -> FxHashSet<SkeletonBindingId> {
        let outer_region = self
            .skeleton
            .innermost_region_containing(self.rebase(creation_span));
        let mut reads = FxHashSet::default();
        for site in nested.expr_sites.iter() {
            for read in site.reads.iter() {
                if !nested
                    .bindings_of_name_in_scope(read.name, site.region)
                    .is_empty()
                {
                    continue;
                }
                let Some(outer_name) = self.skeleton.name_id(nested.name(read.name)) else {
                    continue;
                };
                reads.extend(
                    self.skeleton
                        .bindings_of_name_in_scope(outer_name, outer_region)
                        .iter()
                        .copied(),
                );
            }
        }
        reads
    }

    fn nested_has_free_write(
        &self,
        nested: &FunctionBodySkeleton,
        binding: SkeletonBindingId,
        creation_span: oxc_span::Span,
    ) -> bool {
        let outer_region = self
            .skeleton
            .innermost_region_containing(self.rebase(creation_span));
        nested.writes.iter().any(|write| {
            let SkeletonWriteTarget::Named(name) = write.target else {
                return false;
            };
            nested
                .bindings_of_name_in_scope(name, write.region)
                .is_empty()
                && self
                    .skeleton
                    .name_id(nested.name(name))
                    .is_some_and(|outer_name| {
                        self.skeleton
                            .bindings_of_name_in_scope(outer_name, outer_region)
                            .contains(&binding)
                    })
        })
    }

    fn binding_has_write_after(
        &self,
        binding: SkeletonBindingId,
        creation_span: oxc_span::Span,
    ) -> bool {
        let creation = self.rebase(creation_span);
        self.skeleton.writes.iter().any(|write| {
            write.span > creation
                && matches!(write.target, SkeletonWriteTarget::Named(_))
                && match write.target {
                    SkeletonWriteTarget::Named(name) => self
                        .skeleton
                        .bindings_of_name_in_scope(name, write.region)
                        .contains(&binding),
                    SkeletonWriteTarget::Opaque => false,
                }
        })
    }

    fn guard_bindings(&self, guard: &SliceGuard, at: oxc_span::Span) -> Vec<SkeletonBindingId> {
        let region = self.skeleton.innermost_region_containing(self.rebase(at));
        let mut bindings = Vec::new();
        let mut add_subject = |subject: &SliceNarrowSubject| {
            let name = match &subject.root {
                SliceNarrowRoot::Local(name) => Some(name.as_ref()),
                SliceNarrowRoot::Param(ordinal) => self
                    .params
                    .get(*ordinal as usize)
                    .and_then(|param| param.name.as_deref()),
            };
            let Some(name) = name.and_then(|name| self.skeleton.name_id(name)) else {
                return;
            };
            for binding in self.skeleton.bindings_of_name_in_scope(name, region) {
                if !bindings.contains(&binding) {
                    bindings.push(binding);
                }
            }
        };
        collect_guard_subjects(guard, &mut add_subject);
        bindings
    }

    fn predicate_subject_name(&self, test: &Expression<'_>) -> Option<Arc<str>> {
        let Expression::CallExpression(call) = unwrap_parenthesized(test) else {
            return None;
        };
        let Expression::Identifier(callee) = unwrap_parenthesized(&call.callee) else {
            return None;
        };
        let (ordinal, _) = self.same_file_predicate(callee.name.as_str(), false)?;
        let argument = call
            .arguments
            .get(ordinal)
            .and_then(|argument| argument.as_expression())?;
        chain_root_identifier(argument).map(|identifier| Arc::from(identifier.name.as_str()))
    }

    fn nested_function_transfers_downstream_slot(
        &self,
        node: &FunctionNode<'_>,
        outer_region: verter_semantic::analysis::flow::SkeletonRegionId,
        loop_span: FrameSpan,
    ) -> bool {
        let nested_source = match node {
            FunctionNode::Function(function) => {
                FunctionBodySource::from_function_expression(function)
            }
            FunctionNode::Arrow(arrow) => Some(FunctionBodySource::from_arrow(arrow)),
        };
        let Some(nested_source) = nested_source else {
            return false;
        };
        let nested = build_function_body_skeleton(&nested_source);
        let free_name_targets_downstream = |nested_name, nested_region| {
            if !nested
                .bindings_of_name_in_scope(nested_name, nested_region)
                .is_empty()
            {
                return false;
            }
            let Some(outer_name) = self.skeleton.name_id(nested.name(nested_name)) else {
                return false;
            };
            self.skeleton
                .bindings_of_name_in_scope(outer_name, outer_region)
                .iter()
                .any(|binding| self.binding_is_read_after_loop(*binding, loop_span))
        };
        if nested.writes.iter().any(|write| {
            let SkeletonWriteTarget::Named(nested_name) = write.target else {
                return false;
            };
            free_name_targets_downstream(nested_name, write.region)
        }) {
            return true;
        }

        nested.expr_sites.iter().enumerate().any(|(index, site)| {
            let control_or_call = !site.calls.is_empty()
                || nested.regions.iter().any(|region| {
                    region
                        .control_input
                        .is_some_and(|site| site.index() == index)
                });
            control_or_call
                && site
                    .reads
                    .iter()
                    .any(|read| free_name_targets_downstream(read.name, site.region))
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

    fn param_ordinal(&self, name: &str) -> Option<u32> {
        self.params
            .iter()
            .position(|param| param.name.as_deref() == Some(name))
            .map(|ordinal| ordinal as u32)
    }

    /// Whether `name` is a modelled destructured object-pattern element of
    /// one of this frame's parameters. This is the ONE classification both
    /// halves read: the content side lowers a read of it as an ordinary
    /// local, and the evaluator seeds the binding lazily on first read —
    /// from the SAME [`SliceParam`] metadata, so the two can never
    /// disagree about which destructured names are modelled.
    fn is_destructured_element(&self, name: &str) -> bool {
        self.params.iter().any(|param| {
            param
                .destructured
                .iter()
                .any(|element| element.name.as_ref() == name)
        })
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
                    // carrier (`lower_params` records no name for it) —
                    // but a modelled object-pattern ELEMENT binds its
                    // annotation member, which the evaluator seeds lazily
                    // on first read.
                    match (
                        self.skeleton.binding(*id).destructured,
                        self.param_ordinal(name),
                    ) {
                        (false, Some(ordinal)) => param = Some(ordinal),
                        (true, _) if self.is_destructured_element(name) => {
                            modelable_local = true;
                        }
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

    /// Recover one mutable capture's authored annotation by exact binding
    /// identity. The skeleton supplies the declaration-precise span; the
    /// retained AST supplies the type syntax. No name-based or source-text
    /// reconstruction participates.
    fn mutable_declared_authority(
        &self,
        name: &str,
        binding: &verter_semantic::analysis::flow::SkeletonBinding,
    ) -> Option<SliceCaptureAuthority> {
        if binding.kind == SkeletonBindingKind::Param {
            let (param, key, has_default) = if binding.destructured {
                self.params.iter().find_map(|param| {
                    param
                        .destructured
                        .iter()
                        .find(|element| element.name.as_ref() == name)
                        .map(|element| (param, Some(Arc::clone(&element.key)), element.has_default))
                })?
            } else {
                (
                    self.params
                        .iter()
                        .find(|param| param.name.as_deref() == Some(name))?,
                    None,
                    false,
                )
            };
            return Some(SliceCaptureAuthority {
                name: Arc::from(name),
                declared: param.ty.clone(),
                source: SliceCaptureAuthoritySource::Parameter { key, has_default },
            });
        }
        let kind = match binding.kind {
            SkeletonBindingKind::Let if !binding.destructured => SliceBindingKind::Let,
            SkeletonBindingKind::Var if !binding.destructured => SliceBindingKind::Var,
            _ => return None,
        };
        let absolute = binding.span.to_absolute(self.anchor);
        let target = oxc_span::Span::new(absolute.start, absolute.end);
        let mut finder = DeclaratorAnnotationFinder {
            source: self.source,
            target,
            found: None,
        };
        finder.visit_program(self.program);
        finder.found.map(|ty| SliceCaptureAuthority {
            name: Arc::from(name),
            declared: self.gate(ty, target, &[]),
            source: SliceCaptureAuthoritySource::Local(kind),
        })
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
        let mut mutable_declared = self.captures.mutable_declared.clone();
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
                NameBinding::Param(_) => CapturedBinding::Local { mutable: true },
                NameBinding::Local(_) => CapturedBinding::Local {
                    mutable: resolved.iter().any(|id| {
                        matches!(
                            self.skeleton.binding(*id).kind,
                            SkeletonBindingKind::Param
                                | SkeletonBindingKind::Let
                                | SkeletonBindingKind::Var
                        )
                    }),
                },
                NameBinding::Captured => self
                    .captures
                    .names
                    .get(text)
                    .copied()
                    .unwrap_or(CapturedBinding::Unmodeled),
                NameBinding::Free => continue,
                NameBinding::NestedFunction | NameBinding::Unmodeled => CapturedBinding::Unmodeled,
            };
            if let Some(declared) = resolved
                .iter()
                .find_map(|id| self.mutable_declared_authority(text, self.skeleton.binding(*id)))
            {
                mutable_declared.insert(Arc::from(text), declared);
            } else {
                // A nearer binding without a mutable annotation shadows any
                // inherited authority of the same name.
                mutable_declared.remove(text);
            }
            names.insert(Arc::from(text), captured);
        }
        CaptureScope {
            names,
            mutable_declared,
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
        let enclosing_followed_by_return = self.current_statement_followed_by_return;
        let mut out: Vec<SliceStatement> = Vec::new();
        let mut can_fall_through = true;
        let mut hit_unsupported = false;
        let mut may_break: Vec<SliceBreakTarget> = Vec::new();
        for (index, statement) in statements.iter().enumerate() {
            if !can_fall_through {
                break;
            }
            self.current_statement_followed_by_return = enclosing_followed_by_return
                || statements[index + 1..]
                    .iter()
                    .any(statement_guarantees_current_function_return);
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
                    // A block absorbs no `break` — an exit targeting an
                    // enclosing switch / labeled statement passes through.
                    may_break.extend(child.may_break);
                    out.push(SliceStatement::Block(child.region));
                }
                Statement::IfStatement(if_stmt) => {
                    // The evaluator never consumes the test's VALUE, so no
                    // test content lowers — but its narrowing facts do,
                    // through the ONE guard authority both control
                    // spellings share.
                    let guard = self.lower_guard(&if_stmt.test);
                    // A call in the TEST is decided above ONLY when its
                    // result provably cannot control the arms' narrowing;
                    // a predicate call takes evaluator evidence at guard
                    // application, and an unprovable callee degrades the
                    // demand through the typed guard-narrowing gap below.
                    let unprovable_control_call = self.record_control_position_calls(&if_stmt.test);
                    let active_guard_base = self.active_guard_bindings.len();
                    let active_guard_name_base = self.active_guard_names.len();
                    let guard_bindings = self.guard_bindings(&guard, if_stmt.test.span());
                    let guard_name = self.predicate_subject_name(&if_stmt.test);
                    let nested_predicate_gap = guard_name.as_ref().is_some_and(|name| {
                        self.active_guard_names.contains(name)
                            && matches!(
                                self.resolve_name(name, if_stmt.test.span()),
                                NameBinding::Captured
                            )
                    });
                    self.active_guard_bindings
                        .extend(guard_bindings.iter().copied());
                    self.active_guard_names.extend(guard_name.iter().cloned());
                    let consequent = self.lower_arm(&if_stmt.consequent);
                    self.active_guard_bindings.truncate(active_guard_base);
                    self.active_guard_names.truncate(active_guard_name_base);
                    let alternate = if_stmt.alternate.as_ref().map(|alternate| {
                        self.active_guard_bindings
                            .extend(guard_bindings.iter().copied());
                        self.active_guard_names.extend(guard_name.iter().cloned());
                        let lowered = self.lower_arm(alternate);
                        self.active_guard_bindings.truncate(active_guard_base);
                        self.active_guard_names.truncate(active_guard_name_base);
                        lowered
                    });
                    can_fall_through = consequent.region.can_fall_through
                        || alternate
                            .as_ref()
                            .map(|region| region.region.can_fall_through)
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
                    if nested_predicate_gap || unprovable_control_call {
                        out.push(SliceStatement::Gap(
                            crate::semantic_query::FlowGap::GuardNarrowing,
                        ));
                    }
                    if verter_semantic::analysis::flow::expression_contains_call(&if_stmt.test) {
                        out.push(SliceStatement::ThrowPoint);
                    }
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
                // effect obligations, not this content tree — EXCEPT the
                // two value-neutral forms whose effect IS the point: a
                // whole-binding `=` write the evaluator can apply in
                // source order, and a same-file assertion call whose
                // narrowing persists.
                Statement::ExpressionStatement(expression) => {
                    if let Some(statement) = self.lower_effect_statement(&expression.expression) {
                        out.push(statement);
                    }
                }
                // A `throw` terminates the region path without contributing
                // a return arm; the marker carries the throw POINT to the
                // evaluator (a `catch` is entered from it too).
                Statement::ThrowStatement(_) => {
                    out.push(SliceStatement::Throw);
                    can_fall_through = false;
                }
                Statement::DoWhileStatement(_)
                | Statement::ForInStatement(_)
                | Statement::ForOfStatement(_)
                | Statement::ForStatement(_)
                | Statement::WhileStatement(_) => {
                    // A return-free loop is fall-through TRANSPARENT only
                    // while it binds nothing that outlives it and carries no
                    // unmodelled transfer for a downstream-selected slot. A
                    // `var` declaration escapes the loop; a selected guard,
                    // call/assertion, or write depends on iteration flow.
                    // Either shape takes the existing typed loop refusal.
                    if self.control_has_return(statement)
                        || declares_var(statement)
                        || self.loop_has_selected_transfer(statement)
                    {
                        out.push(SliceStatement::Unsupported(SliceUnsupported::Loop));
                        hit_unsupported = true;
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
                    let child = self.lower_arm(&labeled.body);
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
                    can_fall_through = child.region.can_fall_through || absorbed;
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
                    let has_default = switch.cases.iter().any(|case| case.test.is_none());
                    let discriminant = self.narrow_subject_of(&switch.discriminant);
                    self.break_targets.push(None);
                    self.break_target_followed_by_return.push(false);
                    let mut cases = Vec::with_capacity(switch.cases.len());
                    for case in &switch.cases {
                        let test = case
                            .test
                            .as_ref()
                            .and_then(|test| guard_literal_of(test, self.source));
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
                            breaks,
                            test,
                        });
                    }
                    self.break_targets.pop();
                    self.break_target_followed_by_return.pop();
                    // Past the switch is reachable when no `default`
                    // exists (a non-matching discriminant skips every
                    // case), when the LAST clause falls off the end of the
                    // switch, or when any clause exits via `break`.
                    can_fall_through = !has_default
                        || cases
                            .last()
                            .is_some_and(|case| case.region.can_fall_through)
                        || cases.iter().any(|case| case.breaks);
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
                        Box::new(SliceCatchClause {
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
                    let finally_blocks_exits = finally
                        .as_ref()
                        .is_some_and(|(region, _)| !region.can_fall_through);
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
                    let pending_break_contributes_undefined = finally_blocks_exits
                        && clause_may_break.iter().any(|target| match target {
                            SliceBreakTarget::Named(name) => {
                                target_followed_by_return(name) == Some(false)
                            }
                            SliceBreakTarget::Anonymous => false,
                        });
                    let mut pending_break_following_return_targets: Vec<Arc<str>> = Vec::new();
                    if finally_blocks_exits {
                        for target in &clause_may_break {
                            let SliceBreakTarget::Named(name) = target else {
                                continue;
                            };
                            if target_followed_by_return(name) == Some(true)
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
                    let pre_finally_fall_through = block.can_fall_through
                        || catch
                            .as_ref()
                            .is_some_and(|catch| catch.region.can_fall_through);
                    can_fall_through = pre_finally_fall_through
                        && finally
                            .as_ref()
                            .is_none_or(|(region, _)| region.can_fall_through);
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
                Statement::ContinueStatement(_) => {
                    // A `continue` always targets a loop, and a loop body
                    // never lowers — so no modelled construct can absorb
                    // one.
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
                can_fall_through,
            },
            hit_unsupported,
            may_break,
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

    // ── Guards ──────────────────────────────────────────────────────

    /// THE guard lowering — the single authority over conditional-test
    /// forms, shared by the ternary's branch join and the `if`
    /// statement's arms.
    ///
    /// The output is a structural description of the narrowing facts the
    /// test establishes; it evaluates nothing (this half has no
    /// resolver). A form the [`SliceGuard`] vocabulary cannot express
    /// lowers to [`SliceGuard::None`], which applies no narrow at all —
    /// the exact behaviour the arms had before guards existed. Negation
    /// is pushed to the leaves at lowering time (De Morgan), so the
    /// evaluator only ever asks a guard for its positive or its negated
    /// reading.
    fn lower_guard(&mut self, test: &Expression<'_>) -> SliceGuard {
        match unwrap_parenthesized(test) {
            Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::LogicalNot => {
                let inner = self.lower_guard(&unary.argument);
                negate_guard(inner)
            }
            Expression::LogicalExpression(logical) => {
                let left = self.lower_guard(&logical.left);
                let right = self.lower_guard(&logical.right);
                match logical.operator {
                    LogicalOperator::And => and_guard(left, right),
                    LogicalOperator::Or => or_guard(left, right),
                    // `a ?? b` tests nullishness of `a`, but its result is
                    // the OPERAND's value, not a boolean fact over the
                    // branch arms — a narrow from it would apply to the
                    // wrong reference. No guard.
                    LogicalOperator::Coalesce => SliceGuard::None,
                }
            }
            Expression::BinaryExpression(binary) => self.lower_binary_guard(binary),
            Expression::CallExpression(call) => self.lower_predicate_guard(call),
            other => self
                .narrow_subject_of(other)
                .map(|subject| SliceGuard::Truthy {
                    subject,
                    negated: false,
                })
                .unwrap_or(SliceGuard::None),
        }
    }

    /// The binary-operator guard forms: strict (in)equality — including
    /// the `typeof x === "kind"` spelling — `instanceof`, and `in`.
    fn lower_binary_guard(&mut self, binary: &oxc_ast::ast::BinaryExpression<'_>) -> SliceGuard {
        use oxc_ast::ast::BinaryOperator;
        match binary.operator {
            BinaryOperator::StrictEquality | BinaryOperator::StrictInequality => {
                let negated = matches!(binary.operator, BinaryOperator::StrictInequality);
                // `typeof x === "string"` (either operand order).
                if let Some(guard) = self.typeof_guard(&binary.left, &binary.right, negated) {
                    return guard;
                }
                if let Some(guard) = self.typeof_guard(&binary.right, &binary.left, negated) {
                    return guard;
                }
                // `subject === literal` (either operand order).
                for (subject_side, literal_side) in
                    [(&binary.left, &binary.right), (&binary.right, &binary.left)]
                {
                    let Some(subject) = self.narrow_subject_of(subject_side) else {
                        continue;
                    };
                    let Some(literal) = guard_literal_of(literal_side, self.source) else {
                        continue;
                    };
                    return SliceGuard::EqLiteral {
                        subject,
                        literal,
                        negated,
                    };
                }
                SliceGuard::None
            }
            BinaryOperator::Instanceof => {
                let Some(subject) = self.narrow_subject_of(&binary.left) else {
                    return SliceGuard::None;
                };
                match unwrap_parenthesized(&binary.right) {
                    Expression::Identifier(ctor) => SliceGuard::Instanceof {
                        subject,
                        ctor: Arc::from(ctor.name.as_str()),
                        negated: false,
                    },
                    _ => SliceGuard::None,
                }
            }
            BinaryOperator::In => {
                let Expression::StringLiteral(key) = unwrap_parenthesized(&binary.left) else {
                    return SliceGuard::None;
                };
                let Some(subject) = self.narrow_subject_of(&binary.right) else {
                    return SliceGuard::None;
                };
                SliceGuard::In {
                    key: Arc::from(key.value.as_str()),
                    subject,
                    negated: false,
                }
            }
            _ => SliceGuard::None,
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
    /// not at the use site. Only a same-file function declaration carries
    /// its authored signature through this channel — a frame-local shadow
    /// names a different function, and a cross-file callee's annotation
    /// is beyond the retained snapshot this half reads.
    fn lower_predicate_guard(&mut self, call: &oxc_ast::ast::CallExpression<'_>) -> SliceGuard {
        let Expression::Identifier(callee) = unwrap_parenthesized(&call.callee) else {
            return SliceGuard::None;
        };
        let name = callee.name.as_str();
        if !matches!(self.resolve_name(name, callee.span), NameBinding::Free) {
            return SliceGuard::None;
        }
        let Some((ordinal, Some(target))) = self.same_file_predicate(name, false) else {
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

    /// Every same-file top-level function DECLARATION with `name`, in
    /// source order — the direct spelling and the `export function`
    /// spelling both count, because the group SIZE is a semantic fact
    /// (an overload group's signature selection) that must not depend on
    /// export syntax.
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
                _ => None,
            })
            .filter(|function| function.id.as_ref().map(|id| id.name.as_str()) == Some(name))
            .collect()
    }

    /// Read a SAME-FILE function declaration's return-type predicate:
    /// `x is T` (`asserts` false) or `asserts x is T` / a targetless
    /// `asserts x` (`asserts` true). Returns the ordinal of the parameter
    /// the predicate talks about and the target type lowered through the
    /// frame gate (a BODY position — the frame's own type declarations are
    /// in scope there, exactly like a declarator annotation); the target
    /// is `None` for the targetless assertion spelling. `None` for the
    /// whole read for any other signature spelling.
    ///
    /// The channel serves EXACTLY ONE declaration. An overload group
    /// (two or more same-name declarations) is refused outright: which
    /// signature applies is overload/applicability resolution, which
    /// this half does not perform, and the first declaration's predicate
    /// target can be the WRONG one — narrowing on it would publish a
    /// checker-divergent type. A refused group establishes no fact.
    fn same_file_predicate(&self, name: &str, asserts: bool) -> Option<(usize, Option<GatedType>)> {
        let group = self.same_file_function_declarations(name);
        let [function] = group.as_slice() else {
            return None;
        };
        let annotation = function.return_type.as_ref()?;
        let TSType::TSTypePredicate(predicate) = &annotation.type_annotation else {
            return None;
        };
        if predicate.asserts != asserts {
            return None;
        }
        let target = predicate.type_annotation.as_ref().map(|target| {
            self.gate(
                lower_ts_type(&target.type_annotation, self.source),
                annotation.span,
                &[],
            )
        });
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

    /// The narrowable reference an expression NAMES: a static member
    /// chain rooted at an identifier the frame's lexical authority
    /// resolves to a simple parameter or a modelable same-frame local.
    /// Anything else — a call result, a computed member, a captured or
    /// free root — is not positionally substitutable, so no narrow can
    /// land on it.
    fn narrow_subject_of(&self, expression: &Expression<'_>) -> Option<SliceNarrowSubject> {
        let mut segments: Vec<Arc<str>> = Vec::new();
        let mut current = unwrap_parenthesized(expression);
        let identifier = loop {
            match current {
                Expression::StaticMemberExpression(member) => {
                    segments.push(Arc::from(member.property.name.as_str()));
                    current = unwrap_parenthesized(&member.object);
                }
                Expression::Identifier(identifier) => break identifier,
                _ => return None,
            }
        };
        segments.reverse();
        let path: Arc<[Arc<str>]> = Arc::from(segments.into_boxed_slice());
        let name = identifier.name.as_str();
        match self.resolve_name(name, identifier.span) {
            NameBinding::Param(ordinal) => Some(SliceNarrowSubject {
                root: SliceNarrowRoot::Param(ordinal),
                path,
            }),
            NameBinding::Local(_) => Some(SliceNarrowSubject {
                root: SliceNarrowRoot::Local(Arc::from(name)),
                path,
            }),
            NameBinding::Free
            | NameBinding::Captured
            | NameBinding::NestedFunction
            | NameBinding::Unmodeled => None,
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
    fn lower_effect_statement(&mut self, expression: &Expression<'_>) -> Option<SliceStatement> {
        match unwrap_parenthesized(expression) {
            Expression::AssignmentExpression(assignment)
                if matches!(
                    assignment.operator,
                    oxc_ast::ast::AssignmentOperator::Assign
                ) =>
            {
                let oxc_ast::ast::AssignmentTarget::AssignmentTargetIdentifier(identifier) =
                    &assignment.left
                else {
                    return None;
                };
                let name = identifier.name.as_str();
                let root = match self.resolve_name(name, identifier.span) {
                    NameBinding::Param(ordinal) => SliceNarrowRoot::Param(ordinal),
                    NameBinding::Local(_) => SliceNarrowRoot::Local(Arc::from(name)),
                    NameBinding::Captured => SliceNarrowRoot::Local(Arc::from(name)),
                    NameBinding::Free | NameBinding::NestedFunction | NameBinding::Unmodeled => {
                        return None
                    }
                };
                if !self.value_span_selected(assignment.right.span()) {
                    return None;
                }
                let value = self.lower_expr(
                    &assignment.right,
                    ExprMode::BindingInit {
                        // Preserve the RHS until the evaluator can reduce it
                        // against the target's authored declared type. It
                        // widens the literal itself when the target has no
                        // annotation.
                        preserve_literal: true,
                    },
                );
                Some(SliceStatement::Assignment {
                    target: SliceNarrowSubject {
                        root,
                        path: Arc::from(Vec::new().into_boxed_slice()),
                    },
                    // The span identity matches the slice's typed write
                    // effect, which the skeleton records at the TARGET
                    // IDENTIFIER — never the whole assignment expression.
                    span: self.rebase(identifier.span),
                    value: Box::new(value),
                })
            }
            Expression::CallExpression(call) => {
                // A bare call is a THROW POINT regardless of what it
                // resolves to; a same-file assertion call additionally
                // narrows. The marker keeps the throw point even when the
                // assertion path below does not recognise the callee.
                let assertion = (|| {
                    let Expression::Identifier(callee) = unwrap_parenthesized(&call.callee) else {
                        return None;
                    };
                    let name = callee.name.as_str();
                    if !matches!(self.resolve_name(name, callee.span), NameBinding::Free) {
                        return None;
                    }
                    let (ordinal, target) = self.same_file_predicate(name, true)?;
                    let argument = call
                        .arguments
                        .get(ordinal)
                        .and_then(|argument| argument.as_expression())?;
                    let subject = self.narrow_subject_of(argument)?;
                    Some(SliceStatement::Assertion { subject, target })
                })();
                Some(assertion.unwrap_or(SliceStatement::ThrowPoint))
            }
            // Every other value-neutral statement still carries its throw
            // points: a `new`, and a call nested anywhere the value
            // descent never reaches (a template literal's interpolation,
            // a sequence's operand, a conditional's arm, a member chain)
            // executes — and can throw — whether or not its value is
            // consumed. The ONE shared scanner answers for every form.
            other => verter_semantic::analysis::flow::expression_contains_call(other)
                .then_some(SliceStatement::ThrowPoint),
        }
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
        match expr {
            Expression::Identifier(identifier) => self.lower_identifier_read(identifier, mode),
            Expression::ChainExpression(chain) => {
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
                        self.record_decided_above_calls(expr);
                        SliceExpr::OptionalAnyChain {
                            root: Box::new(self.lower_identifier_read(root, mode)),
                        }
                    }
                    None => self.lower_leaf(expr, mode),
                }
            }
            Expression::FunctionExpression(func) => {
                self.lower_nested_function(&FunctionNode::Function(func))
            }
            Expression::ArrowFunctionExpression(arrow) => {
                self.lower_nested_function(&FunctionNode::Arrow(arrow))
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
                    LeafLowering::Unmodeled => SliceExpr::UnreducedCallValue,
                    // The callee could not be represented at all (an
                    // `obj[k]()` computed-member callee, say): the leaf
                    // answered a bare `any`. This IS a call with no
                    // structural arm, so it takes the same fail-closed
                    // verdict the classifier gives every other one —
                    // publishing the `any` was a fabricated value at a
                    // call position, warm and clean.
                    LeafLowering::Free(ty) if is_any(&ty) => SliceExpr::UnreducedCallValue,
                    LeafLowering::Free(ty) => {
                        SliceExpr::Call(SliceCall::Symbolic(ty), call_site(call))
                    }
                    LeafLowering::FrameShadowed { ty, shadowed } => SliceExpr::FrameShadowed {
                        inner: Box::new(SliceExpr::Call(SliceCall::Symbolic(ty), call_site(call))),
                        shadowed,
                    },
                }
            }
            // A sequence whose LAST operand is a narrowable reference
            // (`(touch(), u)`): the sequence's VALUE is that operand —
            // the earlier operands' evaluation effects ride the slice's
            // typed effect obligations regardless. Lowering it as the
            // reference keeps the frame's substitutions (the narrowing
            // overlay above all) visible at the read; folding the whole
            // sequence through the leaf lowering answered a fabricated
            // `any` for it, clean and warm. Any other sequence keeps the
            // leaf lowering.
            Expression::SequenceExpression(sequence)
                if sequence
                    .expressions
                    .last()
                    .is_some_and(|last| self.narrow_subject_of(last).is_some()) =>
            {
                let last = sequence
                    .expressions
                    .last()
                    .expect("the guard proved a last operand");
                // The DISCARDED operands' calls never feed the sequence's
                // value (it is the last operand's): decided above.
                for discarded in &sequence.expressions[..sequence.expressions.len() - 1] {
                    self.record_decided_above_calls(discarded);
                }
                self.lower_expr(last, mode)
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
            other => match value_descent(other) {
                ValueDescent::Transparent(inner) => self.lower_expr(inner, mode),
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
                ValueDescent::TypeCarrier(inner) => match member_literal_policy(other, self.source)
                {
                    Some(policy) => match value_descent(inner) {
                        ValueDescent::Object(object) => {
                            self.lower_object_literal_with_policy(object, other, mode, policy)
                        }
                        _ => self.lower_leaf(other, mode),
                    },
                    None => self.lower_leaf(other, mode),
                },
                ValueDescent::Object(object) => self.lower_object_literal_with_policy(
                    object,
                    other,
                    mode,
                    ObjectMemberPolicy::Widen,
                ),
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
                    // enclosing statement's guard-narrowing gap.
                    if self.record_control_position_calls(&conditional.test) {
                        self.control_test_gap = true;
                    }
                    let consequent = self.lower_expr(&conditional.consequent, mode);
                    let alternate = self.lower_expr(&conditional.alternate, mode);
                    SliceExpr::Union {
                        arms: Arc::from(vec![consequent, alternate].into_boxed_slice()),
                        guard,
                    }
                }
                // A CALL POSITION with no structural arm (`new f()`,
                // `` tag`…` ``, `f?.()`, `await f()`, `(0, f())`). The
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
                // nothing to do with calls (`JSXElement`, `Super`,
                // `await x` over a non-call) — see `lower_leaf`.
                ValueDescent::Leaf => self.lower_leaf(other, mode),
            },
        }
    }

    fn lower_identifier_read(
        &mut self,
        identifier: &oxc_ast::ast::IdentifierReference<'_>,
        _mode: ExprMode,
    ) -> SliceExpr {
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
            NameBinding::NestedFunction | NameBinding::Unmodeled => SliceExpr::UnmodeledBinding,
            NameBinding::Free => {
                SliceExpr::Type(GatedLeaf(TypeExpr::TypeOf(verter_type_expr::ValueRef {
                    path: vec![name.to_owned()],
                    type_args: Vec::new(),
                })))
            }
        }
    }

    fn optional_chain_root_has_prior_flow_change(
        &self,
        root: &oxc_ast::ast::IdentifierReference<'_>,
    ) -> bool {
        let read = self.rebase(root.span);
        let region = self.skeleton.innermost_region_containing(read);
        let Some(name) = self.skeleton.name_id(root.name.as_str()) else {
            return false;
        };
        let bindings = self.skeleton.bindings_of_name_in_scope(name, region);
        bindings.iter().any(|binding| {
            self.active_guard_bindings.contains(binding)
                || self.skeleton.writes.iter().any(|write| {
                    write.span < read
                        && write.path.is_empty()
                        && match write.target {
                            SkeletonWriteTarget::Named(write_name) => self
                                .skeleton
                                .bindings_of_name_in_scope(write_name, write.region)
                                .contains(binding),
                            SkeletonWriteTarget::Opaque => false,
                        }
                })
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
                        SliceExpr::Elided
                    };
                    entries.push(SliceObjectEntry::Spread { source });
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
                    authored: verter_type_expr_oxc::lower_property_key(&p.key, self.source),
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
            // detection stays static, content-free forever).
            if !self.value_span_selected(value_expression.span()) {
                entries.push(SliceObjectEntry::Member(Box::new(SliceObjectMember {
                    key,
                    value: SliceExpr::Elided,
                    assignment_value: None,
                    method_kind,
                    readonly: policy.readonly(),
                    spans,
                })));
                continue;
            }
            // A method / accessor member with a body is a nested
            // function value (its return evaluates inline through
            // the same flow machinery); a method without a body
            // keeps the whole-literal leaf lowering.
            if method_kind.is_some() {
                let value = match value_expression {
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
                entries.push(SliceObjectEntry::Member(Box::new(SliceObjectMember {
                    key,
                    value,
                    assignment_value: None,
                    method_kind,
                    // A method / accessor member is never `readonly`,
                    // under `as const` or otherwise: the modifier applies
                    // to data properties.
                    readonly: false,
                    spans,
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
            let value = self.lower_expr(value_expression, mode);
            let assignment_value = value.clone();
            let value = match (widen_member, value) {
                (true, SliceExpr::Type(leaf)) => SliceExpr::Type(
                    leaf.map_ty(verter_semantic::analysis::type_eval_build::widen_shallow_literal),
                ),
                // The member slot's widening reaches INTO a branch join:
                // a ternary member's fresh literal arm is the member's
                // fresh literal, and tsc widens it at the property
                // exactly like a direct literal member. A narrowed
                // reference arm is NOT fresh (the checker's own
                // early-return-guard shapes keep their literal unions),
                // so only leaf arms widen here.
                (true, SliceExpr::Union { arms, guard }) => SliceExpr::Union {
                    arms: Arc::from(
                        arms.iter()
                            .map(|arm| match arm {
                                SliceExpr::Type(leaf) => SliceExpr::Type(leaf.clone().map_ty(
                                    verter_semantic::analysis::type_eval_build::widen_shallow_literal,
                                )),
                                other => other.clone(),
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    ),
                    guard,
                },
                (_, value) => value,
            };
            let assignment_value = (assignment_value != value).then_some(assignment_value);
            entries.push(SliceObjectEntry::Member(Box::new(SliceObjectMember {
                key,
                value,
                assignment_value,
                method_kind,
                readonly: policy.readonly(),
                spans,
            })));
        }
        if structural {
            SliceExpr::Object {
                entries: Arc::from(entries.into_boxed_slice()),
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
        let free_reads = self.nested_free_read_bindings(&nested_skeleton, node_span(node));
        let mut gap = None;
        for binding in free_reads {
            let binding_fact = self.skeleton.binding(binding);
            let same_closure_write =
                self.nested_has_free_write(&nested_skeleton, binding, node_span(node));
            let active_guard = self.active_guard_bindings.contains(&binding);
            let closure_gap = active_guard
                || (binding_fact.kind == SkeletonBindingKind::Let
                    && (self.binding_has_write_after(binding, node_span(node))
                        || (self.nested_free_writes.contains(&binding) && !same_closure_write)));
            if closure_gap {
                gap = Some(crate::semantic_query::FlowGap::ClosureCapture);
                break;
            }
        }
        let nested_anchor = node_span(node).start;
        let params_result = lower_params(
            node.params(),
            self.source,
            &scope,
            &nested_skeleton,
            nested_anchor,
        );
        let type_parameters = lower_slice_type_params(node, self.source, &scope);
        // The DECLARED return annotation, lowered in the nested
        // signature's own scope (its clause binds inside it), gated
        // against THIS frame exactly like the parameter list.
        let declared_return = node.return_type().map(|annotation| {
            scope.gate(
                lower_ts_type(&annotation.type_annotation, self.source),
                scope.param_binders(),
            )
        });
        let params = match params_result {
            Ok(params) => params,
            Err(reason) => {
                self.budget_failure.get_or_insert(reason);
                Vec::new()
            }
        };
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
        let mut mutable_capture_authorities = captures
            .mutable_declared
            .values()
            .filter(|authority| {
                matches!(
                    captures.names.get(authority.name.as_ref()),
                    Some(CapturedBinding::Local { mutable: true })
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        mutable_capture_authorities.sort_by(|a, b| a.name.cmp(&b.name));
        let mutable_capture_authorities = Arc::from(mutable_capture_authorities.into_boxed_slice());
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
            program: self.program,
            budget_failure: None,
            inert_write_spans: FxHashSet::default(),
            decided_above_call_spans: Vec::new(),
            predicate_guard_call_spans: FxHashSet::default(),
            control_test_gap: false,
            unsafe_invoked_closure_effects: FxHashSet::default(),
            nested_free_writes: FxHashSet::default(),
            active_guard_bindings: Vec::new(),
            active_guard_names: Vec::new(),
            break_targets: Vec::new(),
            break_target_followed_by_return: Vec::new(),
            current_statement_followed_by_return: false,
        };
        if let Some(body) = node.body() {
            nested.unsafe_invoked_closure_effects =
                nested.index_unsafe_invoked_closure_effects(&body.statements);
            nested.nested_free_writes = nested.build_nested_free_writes(&body.statements);
        }
        let region = if node.is_expression_body() {
            // An expression-bodied arrow's body is one synthesized
            // expression statement; it lowers to a single `return` of the
            // expression.
            let body = node.body();
            let unsafe_body =
                body.and_then(|body| body.statements.first())
                    .is_some_and(|statement| {
                        nested.span_contains_unsafe_invoked_closure(statement.span())
                    });
            if unsafe_body {
                SliceRegion {
                    statements: Arc::from([SliceStatement::Unsupported(
                        SliceUnsupported::InvokedClosureEffect,
                    )]),
                    can_fall_through: false,
                }
            } else {
                let argument = body
                    .and_then(|body| body.statements.first())
                    .map(|statement| match statement {
                        Statement::ExpressionStatement(expression) => (
                            nested.lower_expr(&expression.expression, ExprMode::Return),
                            expr_is_bare_literal(&expression.expression),
                        ),
                        _ => (
                            SliceExpr::Gap(crate::semantic_query::FlowGap::UnmodeledExpression),
                            false,
                        ),
                    });
                // The expression body's ternary-test gap lands ahead of
                // the synthesized `return` (no statement loop drains it).
                let mut statements = Vec::with_capacity(2);
                if std::mem::take(&mut nested.control_test_gap) {
                    statements.push(SliceStatement::Gap(
                        crate::semantic_query::FlowGap::GuardNarrowing,
                    ));
                }
                statements.extend(argument.map(|(argument, widening_literal)| {
                    SliceStatement::Return {
                        argument: Some(argument),
                        widening_literal,
                    }
                }));
                SliceRegion {
                    statements: Arc::from(statements.into_boxed_slice()),
                    can_fall_through: false,
                }
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
        // Absolute spans concatenate across frames: a call a NESTED
        // body's lowering decided above is still a decided call of this
        // run.
        self.decided_above_call_spans
            .append(&mut nested.decided_above_call_spans);
        SliceExpr::NestedFunctionValue {
            gap,
            params: Arc::from(params.into_boxed_slice()),
            type_parameters: Arc::from(type_parameters.into_boxed_slice()),
            mutable_capture_authorities,
            declared_return,
            can_fall_through: region.can_fall_through,
            body: region,
        }
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
            LeafLowering::Free(ty) if is_any(&ty) => SliceExpr::SemanticAny,
            LeafLowering::Free(ty) => {
                self.record_decided_above_calls(expr);
                SliceExpr::Type(GatedLeaf(ty))
            }
            LeafLowering::FrameShadowed { ty, shadowed } => {
                self.record_decided_above_calls(expr);
                SliceExpr::FrameShadowed {
                    inner: Box::new(SliceExpr::Type(GatedLeaf(ty))),
                    shadowed,
                }
            }
        }
    }

    /// Record the authored call / construct spans of one DECIDED-ABOVE
    /// position — see [`SliceContent::decided_above_call_spans`]. Called
    /// only by positions whose produced type PROVABLY does not derive
    /// from any call inside the expression: the leaf arms that passed
    /// the fabricated-value gate, the optional-`any`-chain carrier, and
    /// a sequence's discarded operands. Control-position tests take the
    /// narrower [`Self::record_control_position_calls`] instead — a test
    /// call can CONTROL the arms' narrowing, which this blanket recorder
    /// cannot see.
    fn record_decided_above_calls(&mut self, expr: &Expression<'_>) {
        struct CallSpans<'s> {
            spans: &'s mut Vec<verter_span::Span>,
        }
        impl<'a> Visit<'a> for CallSpans<'_> {
            fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'a>) {
                self.spans.push(call.span.into());
                walk::walk_call_expression(self, call);
            }
            fn visit_new_expression(&mut self, new: &oxc_ast::ast::NewExpression<'a>) {
                self.spans.push(new.span.into());
                walk::walk_new_expression(self, new);
            }
        }
        let mut scanner = CallSpans {
            spans: &mut self.decided_above_call_spans,
        };
        scanner.visit_expression(expr);
    }

    /// Record the RESULT-INDEPENDENT call / construct spans of one
    /// CONTROL-POSITION test (an `if` / ternary test). The demanded
    /// value never consumes a test's VALUE, but a test call's RESULT can
    /// still decide the narrowing the arms evaluate under — a
    /// type-predicate callee — so a control call is decided above ONLY
    /// when the callee provably establishes no narrowing:
    /// - a `new` construct (a construct signature cannot be a type
    ///   predicate, and the checker derives no narrowing from one);
    /// - a bare-identifier callee resolving FREE to a same-file function
    ///   group with EXACTLY ONE declaration whose authored return
    ///   annotation exists and is NOT a type predicate (an inferred
    ///   boolean return can be an inferred predicate, so an unannotated
    ///   declaration never qualifies; an overload group's signature
    ///   selection is beyond this half).
    ///
    /// A call that minted a [`SliceGuard::TypePredicate`] fact takes
    /// REAL evaluator evidence at guard application instead. Every OTHER
    /// control call is one this half can neither certify nor evidence —
    /// the callee could be a predicate whose narrowing the checker
    /// applies and this substrate does not — so the test returns `true`
    /// and the caller emits the typed `GuardNarrowing` gap: a degraded
    /// success, `ReturnOnly`, never a silently certified superset.
    fn record_control_position_calls(&mut self, test: &Expression<'_>) -> bool {
        enum ControlCall {
            Construct(verter_span::Span),
            Call {
                span: verter_span::Span,
                callee: Option<(String, oxc_span::Span)>,
            },
        }
        struct ControlCalls {
            calls: Vec<ControlCall>,
        }
        impl<'a> Visit<'a> for ControlCalls {
            fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'a>) {
                let callee = match unwrap_parenthesized(&call.callee) {
                    Expression::Identifier(identifier) => {
                        Some((identifier.name.as_str().to_owned(), identifier.span))
                    }
                    _ => None,
                };
                self.calls.push(ControlCall::Call {
                    span: call.span.into(),
                    callee,
                });
                walk::walk_call_expression(self, call);
            }
            fn visit_new_expression(&mut self, new: &oxc_ast::ast::NewExpression<'a>) {
                self.calls.push(ControlCall::Construct(new.span.into()));
                walk::walk_new_expression(self, new);
            }
        }
        let mut scanner = ControlCalls { calls: Vec::new() };
        scanner.visit_expression(test);
        let mut unprovable = false;
        for call in scanner.calls {
            let span = match call {
                ControlCall::Construct(span) => span,
                ControlCall::Call { span, callee } => {
                    if self.predicate_guard_call_spans.contains(&span) {
                        // Evidence-backed at guard application: neither
                        // certified here nor gapped.
                        continue;
                    }
                    let certified = callee.as_ref().is_some_and(|(name, callee_span)| {
                        matches!(self.resolve_name(name, *callee_span), NameBinding::Free)
                            && match self.same_file_function_declarations(name).as_slice() {
                                [function] => {
                                    function.return_type.as_ref().is_some_and(|annotation| {
                                        !matches!(
                                            annotation.type_annotation,
                                            TSType::TSTypePredicate(_)
                                        )
                                    })
                                }
                                _ => false,
                            }
                    });
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
        let inference =
            infer_declaration_expression_type_with_completeness(expr, self.source, policy);
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
        } => SliceGuard::EqLiteral {
            subject,
            literal,
            negated: !negated,
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
