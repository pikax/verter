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
//! evaluator), a bare-identifier call resolves through ONE lexical
//! binding authority — a hoisted nested function declaration of the same
//! name is [`SliceExpr::LocalFunctionShadow`] (fail-closed), a parameter
//! or in-scope local is [`SliceExpr::CallOnBinding`], a call to the
//! function itself is [`SliceExpr::DirectSelfCall`], an index-exact
//! direct call is [`SliceExpr::DirectCall`] — and any other call rides
//! the symbolic `ReturnType<typeof …>` carrier (or `any` for an
//! unrepresentable callee) as [`SliceExpr::SymbolicCall`].

use std::sync::Arc;

use oxc_ast::ast::{
    BindingPattern, Expression, FormalParameters, ObjectPropertyKind, Program, PropertyKey,
    PropertyKind, Statement, VariableDeclarationKind,
};
use oxc_span::GetSpan;
use rustc_hash::FxHashSet;
use verter_semantic::analysis::flow::flow_ir::{FlowExprRole, FlowSliceIR};
use verter_semantic::analysis::function_program::{
    inventory_statement_list, resolve_function_node, FunctionBindingKind, FunctionControlRegion,
    FunctionNode, FunctionProgramEntry,
};
use verter_semantic::analysis::type_eval_build::infer_declaration_expression_type;
use verter_type_expr::{PrimitiveName, TypeExpr};
use verter_type_expr_oxc::lower_ts_type;

/// The demand selection one content lowering serves: the value-selected
/// expression spans and the value-selected slot names of ONE lowered
/// flow slice. Derived from the content-free `FlowSliceIR` — the plan is
/// the sole authority for what lowers; this carrier only transports the
/// selection into the lease-only run.
#[derive(Debug, Clone)]
pub(crate) struct FlowSliceSelection {
    /// Spans of the slice's VALUE-selected expression records.
    value_spans: FxHashSet<verter_span::Span>,
    /// Names of the slice's VALUE-selected slots (the planner selects
    /// every same-name binding a read can reach, so name identity is
    /// exactly as precise as the plan's own resolution).
    value_slot_names: FxHashSet<Arc<str>>,
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
            value_slot_names: ir
                .slots
                .iter()
                .filter(|slot| slot.value_selected)
                .map(|slot| Arc::clone(&slot.name))
                .collect(),
        }
    }

    fn value_span(&self, span: verter_span::Span) -> bool {
        self.value_spans.contains(&span)
    }

    fn value_slot(&self, name: &str) -> bool {
        self.value_slot_names.contains(name)
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
    /// The authored TS annotation lowered through `lower_ts_type`, or `any`
    /// when the parameter carries no annotation.
    pub ty: TypeExpr,
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

/// One expression of the slice content.
#[derive(Debug, Clone, PartialEq)]
pub enum SliceExpr {
    /// A fully lowered leaf: literals, arrays, objects (spread members ride
    /// as `ObjectMember::Spread` for later delegation to the object-spread
    /// projection), templates, `typeof` paths, `as` / `satisfies` /
    /// parenthesized results — the shared shallow-pass per-expression
    /// lowering.
    Type(TypeExpr),
    /// A parameter reference, substituted by the evaluator.
    Param {
        /// The parameter's ordinal in source order (rest last).
        ordinal: u32,
    },
    /// A local binding reference; its reaching definition is resolved by
    /// the evaluator.
    Local {
        /// The binding name.
        name: Arc<str>,
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
    /// A direct call on a nested function value (an IIFE) — the call's
    /// value is the nested function's evaluated return.
    NestedCall(Box<SliceExpr>),
    /// A call on a parameter or in-scope local binding of function type —
    /// the call's value is the binding's signature return (a shadowed
    /// name is never a flow obligation edge).
    CallOnBinding {
        /// The parameter ordinal (when the callee is a parameter).
        param: Option<u32>,
        /// The binding name.
        name: Arc<str>,
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
    DirectSelfCall,
    /// A bare-identifier call whose target the per-file function index
    /// resolves EXACTLY (a same-file served function position) — a Flow
    /// obligation edge to that target.
    DirectCall(verter_semantic::analysis::function_program::FunctionProgramKey),
    /// A call lowered to the symbolic `ReturnType<typeof …>` carrier.
    SymbolicCall(TypeExpr),
    /// An expression the leaf lowering cannot represent (its `any`
    /// fallback), including a call with an unrepresentable callee.
    Any,
    /// Content the demand slice did NOT select: never lowered, never
    /// evaluable. Observing an elided value is a planner/content mismatch
    /// and fails closed at the evaluator — it is never a fabricated
    /// `any` and never a silently widened sibling.
    Elided,
}

/// One type parameter of a function value.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceTypeParam {
    /// The parameter name.
    pub name: Arc<str>,
    /// The lowered constraint, when authored.
    pub constraint: Option<TypeExpr>,
    /// The lowered default, when authored.
    pub default: Option<TypeExpr>,
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

/// Lower one function node's own type parameter clause (name, lowered
/// constraint, lowered default) — shared by the root content and every
/// nested function value.
fn lower_slice_type_params(node: &FunctionNode<'_>, source: &str) -> Vec<SliceTypeParam> {
    node.type_parameters()
        .map(|declaration| {
            declaration
                .params
                .iter()
                .map(|param| SliceTypeParam {
                    name: Arc::from(param.name.name.as_str()),
                    constraint: param
                        .constraint
                        .as_ref()
                        .map(|constraint| lower_ts_type(constraint, source)),
                    default: param
                        .default
                        .as_ref()
                        .map(|default| lower_ts_type(default, source)),
                })
                .collect()
        })
        .unwrap_or_default()
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
) -> Option<SliceContent> {
    let (node, self_name) = resolve_function_node(program, &entry.locator)?;
    let params = lower_params(node.params(), source);
    let type_parameters = lower_slice_type_params(&node, source);
    let body = node.body()?;
    // ONE lexical binding authority, from the index's whole-function
    // binding inventory: hoisted nested function declarations shadow every
    // outer same-name callee; hoisted `var` names are in scope from the
    // function's first statement (an unbound one evaluates to `any`).
    let fn_shadows: Vec<Arc<str>> = entry
        .bindings
        .iter()
        .filter(|binding| binding.kind == FunctionBindingKind::NestedFunction)
        .map(|binding| Arc::clone(&binding.name))
        .collect();
    let hoisted_vars: Vec<Arc<str>> = entry
        .bindings
        .iter()
        .filter(|binding| binding.kind == FunctionBindingKind::Var)
        .map(|binding| Arc::clone(&binding.name))
        .collect();
    let mut lowerer = Lowerer {
        source,
        selection: Some(selection),
        params: &params,
        self_name: self_name.as_deref(),
        scopes: vec![hoisted_vars],
        fn_shadows: &fn_shadows,
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
        let argument = if lowerer.value_span_selected(expression.expression.span()) {
            lowerer.lower_expr(&expression.expression, ExprMode::ArrowBody)
        } else {
            SliceExpr::Elided
        };
        SliceRegion {
            statements: Arc::from([SliceStatement::Return {
                argument: Some(argument),
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
        body: region,
        budget_failure,
    })
}

/// Unwrap a parenthesized expression (the IIFE callee shape).
fn unwrap_parenthesized<'a>(expression: &'a Expression<'a>) -> &'a Expression<'a> {
    match expression {
        Expression::ParenthesizedExpression(paren) => unwrap_parenthesized(&paren.expression),
        inner => inner,
    }
}

/// Whether an initializer is a BARE literal expression — a fresh
/// (widening) literal source: a string / numeric / boolean literal or a
/// substitution-free template, possibly parenthesized. A const assertion
/// (`1 as const`) or any other assertion / expression shape is NOT bare —
/// its literal is pinned or derived, never widening.
fn expr_is_bare_literal(expression: &Expression<'_>) -> bool {
    match unwrap_parenthesized(expression) {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_) => true,
        Expression::TemplateLiteral(template) => template.expressions.is_empty(),
        _ => false,
    }
}

/// Lower the formal parameters: binding name, optional/rest flags, and the
/// parameter type — the authored TS annotation through `lower_ts_type`,
/// else the default initializer's inferred type, else `any`.
fn lower_params(params: &FormalParameters<'_>, source: &str) -> Vec<SliceParam> {
    let mut out = Vec::with_capacity(params.items.len() + usize::from(params.rest.is_some()));
    for param in &params.items {
        let name = match &param.pattern {
            BindingPattern::BindingIdentifier(id) => Some(Arc::from(id.name.as_str())),
            _ => None,
        };
        let ty = param
            .type_annotation
            .as_ref()
            .map(|annotation| lower_ts_type(&annotation.type_annotation, source))
            .or_else(|| {
                param.initializer.as_ref().map(|initializer| {
                    infer_declaration_expression_type(initializer, source, false)
                        .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any))
                })
            })
            .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
        // An optional (`?`) parameter is `T | undefined` inside the body; a
        // defaulted parameter always has a value.
        let ty = if param.optional && param.initializer.is_none() {
            TypeExpr::union(vec![ty, TypeExpr::Primitive(PrimitiveName::Undefined)])
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
        let ty = rest
            .type_annotation
            .as_ref()
            .map(|annotation| lower_ts_type(&annotation.type_annotation, source))
            .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
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
/// entry's literal policy (and its widening behavior).
#[derive(Clone, Copy)]
enum ExprMode {
    /// Return-argument position: a fresh top-level literal widens to its
    /// primitive.
    Return,
    /// Arrow expression-body position: a standalone literal keeps its
    /// literal type.
    ArrowBody,
    /// Binding-initializer position: `const` preserves the literal,
    /// `let` / `var` widen.
    BindingInit(SliceBindingKind),
}

/// The region lowering result: the region plus whether any nested lowering
/// hit an unsupported construct (the marker is in the tree; the flag
/// propagates so the root region stops at the same point).
struct LoweredRegion {
    region: SliceRegion,
    hit_unsupported: bool,
}

/// The statement/expression lowering state: the demand selection (root
/// frame only — nested function values lower ungated, their bodies are
/// beyond slice granularity), the shared leaf-lowering entry, the
/// function's parameters (for [`SliceExpr::Param`] ordinals), its
/// bare-identifier self name (for [`SliceExpr::DirectSelfCall`]), the
/// hoisted nested function declaration names (for
/// [`SliceExpr::LocalFunctionShadow`]), and the local-binding scope stack
/// (for [`SliceExpr::Local`]). One scope frame per region; a region
/// PRE-DECLARES every lexical name its own statements bind (a forward
/// reference is in scope but unbound at evaluation — the TDZ-honest `any`)
/// and a binding's reaching definition still enters AFTER its initializer
/// is lowered.
struct Lowerer<'a> {
    source: &'a str,
    /// The demand selection gating content lowering (`None` inside a
    /// nested function value — its whole body is one selected value).
    selection: Option<&'a FlowSliceSelection>,
    params: &'a [SliceParam],
    self_name: Option<&'a str>,
    scopes: Vec<Vec<Arc<str>>>,
    /// The function's hoisted nested function declaration names (from the
    /// index's whole-function binding inventory at the root; from the SAME
    /// single inventory walk over a nested function value's own body).
    fn_shadows: &'a [Arc<str>],
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
    /// Whether a root content position is value-selected by the demand
    /// slice. Ungated (nested function value) frames select everything.
    fn value_span_selected(&self, span: oxc_span::Span) -> bool {
        self.selection
            .is_none_or(|selection| selection.value_span(span.into()))
    }

    /// Whether a binding slot is value-selected by the demand slice.
    fn slot_selected(&self, name: &str) -> bool {
        self.selection
            .is_none_or(|selection| selection.value_slot(name))
    }

    fn param_ordinal(&self, name: &str) -> Option<u32> {
        self.params
            .iter()
            .position(|param| param.name.as_deref() == Some(name))
            .map(|ordinal| ordinal as u32)
    }

    fn is_local_in_scope(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .rev()
            .any(|frame| frame.iter().any(|candidate| candidate.as_ref() == name))
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
        // Pre-declare every lexical name THIS region's own statements bind
        // (one level deep — a nested block's bindings stay block-local): a
        // forward `const` / `let` / `var` reference resolves to the local
        // binding (unbound at evaluation — `any`), never to an outer
        // same-name callee. Pre-declaration is selection-INDEPENDENT:
        // classification never varies with the demand.
        let mut frame: Vec<Arc<str>> = Vec::new();
        for statement in statements {
            let Statement::VariableDeclaration(decl) = statement else {
                continue;
            };
            for declarator in &decl.declarations {
                if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                    frame.push(Arc::from(id.name.as_str()));
                }
            }
        }
        self.scopes.push(frame);
        let mut out: Vec<SliceStatement> = Vec::new();
        let mut can_fall_through = true;
        let mut hit_unsupported = false;
        for statement in statements {
            if !can_fall_through {
                break;
            }
            match statement {
                Statement::ReturnStatement(ret) => {
                    let argument = ret.argument.as_ref().map(|arg| {
                        if self.value_span_selected(arg.span()) {
                            self.lower_expr(arg, ExprMode::Return)
                        } else {
                            SliceExpr::Elided
                        }
                    });
                    out.push(SliceStatement::Return { argument });
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
                        // resolution, no budget charge). Its name is
                        // already pre-declared, so classification of later
                        // reads/calls is unchanged.
                        if !self.slot_selected(id.name.as_str()) {
                            continue;
                        }
                        let init = declarator
                            .init
                            .as_ref()
                            .map(|expr| self.lower_expr(expr, ExprMode::BindingInit(kind)));
                        // A WIDENING literal binding: an unannotated
                        // `const` initialized from a bare literal with no
                        // const assertion. `let` / `var` initializers
                        // already widened at `BindingInit` lowering, and
                        // an annotation or `as const` pins the literal.
                        let widening_literal = kind == SliceBindingKind::Const
                            && declarator.type_annotation.is_none()
                            && declarator.init.as_ref().is_some_and(expr_is_bare_literal);
                        let name: Arc<str> = Arc::from(id.name.as_str());
                        out.push(SliceStatement::Binding {
                            name: Arc::clone(&name),
                            kind,
                            init,
                            widening_literal,
                        });
                        if let Some(frame) = self.scopes.last_mut() {
                            frame.push(name);
                        }
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
                    if self.control_has_return(statement) {
                        out.push(SliceStatement::Unsupported(SliceUnsupported::Loop));
                        hit_unsupported = true;
                        can_fall_through = false;
                    } else {
                        out.push(SliceStatement::TransparentLoop);
                    }
                }
                Statement::LabeledStatement(_) => {
                    if self.control_has_return(statement) {
                        out.push(SliceStatement::Unsupported(SliceUnsupported::Labeled));
                        hit_unsupported = true;
                        can_fall_through = false;
                    } else {
                        out.push(SliceStatement::TransparentLoop);
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
        self.scopes.pop();
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
                if let Some(ordinal) = self.param_ordinal(name) {
                    return SliceExpr::Param { ordinal };
                }
                if self.is_local_in_scope(name) {
                    return SliceExpr::Local {
                        name: Arc::from(name),
                    };
                }
                self.lower_leaf(expr, mode)
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
                    // Member-value literal widening follows the enclosing
                    // position's policy (an arrow body / binding initializer
                    // widens a fresh member literal; a block return
                    // preserves it); an `as const` member always keeps its
                    // literal.
                    let widen_member =
                        matches!(mode, ExprMode::ArrowBody | ExprMode::BindingInit(_))
                            && !verter_semantic::analysis::type_eval_build::expr_is_const_asserted(
                                &p.value,
                                self.source,
                            );
                    let value = self.lower_expr(&p.value, mode);
                    let value = match (widen_member, value) {
                        (true, SliceExpr::Type(ty)) => SliceExpr::Type(
                            verter_semantic::analysis::type_eval_build::widen_shallow_literal(ty),
                        ),
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
                SliceExpr::NestedCall(Box::new(function))
            }
            Expression::CallExpression(call) => {
                if let Expression::Identifier(callee) = &call.callee {
                    let name = callee.name.as_str();
                    // ONE lexical binding authority, in precedence order.
                    // 1. A hoisted nested function declaration of this name
                    //    shadows every outer callee — its own return is
                    //    beyond the direct-call inventory (fail closed).
                    if self
                        .fn_shadows
                        .iter()
                        .any(|candidate| candidate.as_ref() == name)
                    {
                        return SliceExpr::LocalFunctionShadow;
                    }
                    // 2. A parameter SHADOWS the declaration: the call goes
                    //    through the binding's signature, never a flow
                    //    obligation edge.
                    if let Some(ordinal) = self.param_ordinal(name) {
                        return SliceExpr::CallOnBinding {
                            param: Some(ordinal),
                            name: Arc::from(name),
                        };
                    }
                    // 3. An in-scope local (pre-declared or already bound)
                    //    shadows the declaration the same way.
                    if self.is_local_in_scope(name) {
                        return SliceExpr::CallOnBinding {
                            param: None,
                            name: Arc::from(name),
                        };
                    }
                    // 4. A bare-identifier call to the function itself — a
                    //    direct same-slot recursion hold.
                    if Some(name) == self.self_name {
                        return SliceExpr::DirectSelfCall;
                    }
                    // 5. A bare-identifier callee the function index
                    //    resolves EXACTLY (same-file served function
                    //    position, the trailing implementation of its
                    //    overload group) is a Flow obligation edge — the
                    //    fixed point's mutual recursion discharges through
                    //    it.
                    if let Some(direct) = self
                        .direct_calls
                        .iter()
                        .find(|direct| direct.span == call.span.into())
                    {
                        return SliceExpr::DirectCall(direct.target.clone());
                    }
                }
                let ty = self.leaf_type(expr, mode);
                if is_any(&ty) {
                    SliceExpr::Any
                } else {
                    SliceExpr::SymbolicCall(ty)
                }
            }
            _ => self.lower_leaf(expr, mode),
        }
    }

    /// Lower a NESTED function node (a function / arrow expression or an
    /// object-literal method) into an owned nested function value: the
    /// nested body's statements lower under the nested function's OWN
    /// parameter scope — outer parameters and locals are NOT in scope
    /// (closure capture stays the leaf fallback). Nested bodies are one
    /// selected value: content inside them is never selection-gated.
    fn lower_nested_function(&mut self, node: &FunctionNode<'_>) -> SliceExpr {
        let params = lower_params(node.params(), self.source);
        let type_parameters = lower_slice_type_params(node, self.source);
        let nested_scopes: Vec<Arc<str>> = params
            .iter()
            .filter_map(|param| param.name.clone())
            .collect();
        let self_name = match node {
            FunctionNode::Function(func) => func.id.as_ref().map(|id| Arc::from(id.name.as_str())),
            FunctionNode::Arrow(_) => None,
        };
        // A nested function value has no index entry of its own — its
        // control skeleton AND its hoisted nested-declaration shadow set
        // come from the SAME single inventory walk over its own body, so
        // the one lexical binding authority applies inside nested values
        // exactly as at the root.
        let (control, nested_fn_shadows): (Arc<[FunctionControlRegion]>, Vec<Arc<str>>) =
            match node.body() {
                Some(body) => {
                    let inventory = inventory_statement_list(&body.statements);
                    (
                        Arc::from(inventory.control),
                        inventory.nested_function_names,
                    )
                }
                None => (Arc::from(Vec::new()), Vec::new()),
            };
        let mut nested = Lowerer {
            source: self.source,
            selection: None,
            params: &params,
            self_name: self_name.as_deref(),
            scopes: vec![nested_scopes],
            fn_shadows: &nested_fn_shadows,
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
                    Statement::ExpressionStatement(expression) => {
                        nested.lower_expr(&expression.expression, ExprMode::ArrowBody)
                    }
                    _ => SliceExpr::Any,
                });
            SliceRegion {
                statements: Arc::from(
                    argument
                        .map(|argument| SliceStatement::Return {
                            argument: Some(argument),
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
    /// [`SliceExpr::Any`].
    fn lower_leaf(&mut self, expr: &Expression<'_>, mode: ExprMode) -> SliceExpr {
        let ty = self.leaf_type(expr, mode);
        if is_any(&ty) {
            SliceExpr::Any
        } else {
            SliceExpr::Type(ty)
        }
    }

    /// The shared shallow-pass per-expression lowering for the position
    /// (`infer_declaration_expression_type`: literal-preserving for arrow
    /// bodies and `const` initializers, widening for return / `let` /
    /// `var` positions). Budget exhaustion degrades the one expression to
    /// `any` and records the typed budget edge.
    fn leaf_type(&mut self, expr: &Expression<'_>, mode: ExprMode) -> TypeExpr {
        let preserve_literal = matches!(
            mode,
            ExprMode::ArrowBody | ExprMode::BindingInit(SliceBindingKind::Const)
        );
        infer_declaration_expression_type(expr, self.source, preserve_literal).unwrap_or_else(
            |reason| {
                if self.budget_failure.is_none() {
                    self.budget_failure = Some(reason);
                }
                TypeExpr::Primitive(PrimitiveName::Any)
            },
        )
    }
}

fn is_any(ty: &TypeExpr) -> bool {
    matches!(ty, TypeExpr::Primitive(PrimitiveName::Any))
}
