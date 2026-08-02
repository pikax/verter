//! Whole-function flow IR — the OWNED, arena-free, lazy per-function body
//! artifact of the flow-return substrate.
//!
//! The [`FunctionProgramIndex`](verter_semantic::analysis::function_program::FunctionProgramIndex)
//! is the eager STRUCTURAL inventory (identities + locators, no lowered
//! types). THIS module is the sole lazy body artifact: on first demand for
//! one function it reborrows the retained parse snapshot ONCE (through the
//! memo's lease-only run, exactly like every other body product) and lowers
//! the complete demanded function into owned typed IR with a block/if
//! control-flow tree.
//!
//! Control semantics: sequential region evaluation (a terminal return or
//! throw ends the region; statements after it are unreachable and
//! dropped), an `if` whose arms both terminate cannot fall through, blocks
//! nest, return-free loop/labeled constructs are fall-through transparent,
//! and return-bearing loop/labeled constructs, `switch`, `try`, `with`,
//! jumps, and module-level statements are UNSUPPORTED — typed,
//! fail-closed: the region is produced up to the first
//! [`FlowIrStatement::Unsupported`] marker and
//! the marker propagates to the root so the evaluator degrades the whole
//! result.
//!
//! Expression lowering reuses the scanner's expression lowering for every
//! supported form; the flow-only differences are explicit IR carriers:
//! parameter references become [`FlowIrExpr::Param`], simple local bindings
//! become [`FlowIrExpr::Local`] (reaching definitions resolved by the
//! evaluator), a bare-identifier call resolves through ONE lexical binding
//! authority — a hoisted nested function declaration of the same name is
//! [`FlowIrExpr::LocalFunctionShadow`] (fail-closed), a parameter or
//! in-scope local is [`FlowIrExpr::CallOnBinding`], a call to the function
//! itself is [`FlowIrExpr::DirectSelfCall`] (a recursion hold), an index
//! exact direct call is [`FlowIrExpr::DirectCall`] — and any other call
//! rides the scanner's symbolic `ReturnType<typeof …>` carrier (or `any`
//! for an unrepresentable callee) as [`FlowIrExpr::SymbolicCall`].

use std::sync::Arc;

use oxc_ast::ast::{
    ArrowFunctionExpression, BindingPattern, Class, ClassElement, Declaration,
    ExportDefaultDeclarationKind, Expression, FormalParameters, Function, FunctionBody,
    ObjectExpression, ObjectPropertyKind, Program, PropertyKey, PropertyKind, Statement,
    TSModuleDeclaration, TSModuleDeclarationBody, TSTypeParameterDeclaration, VariableDeclaration,
    VariableDeclarationKind,
};
use oxc_span::GetSpan;
use verter_semantic::analysis::function_program::{
    inventory_statement_list, FunctionBindingKind, FunctionBodyLocator, FunctionControlRegion,
    FunctionDescentStep, FunctionProgramEntry,
};
use verter_semantic::analysis::type_eval_build::{
    infer_declaration_expression_type, infer_expression_type,
};
use verter_type_expr::{PrimitiveName, TypeExpr};
use verter_type_expr_oxc::lower_ts_type;

/// The OWNED lazy body IR of one demanded function: lowered parameters,
/// the root body region, and the region's reachability result.
#[derive(Debug, Clone, PartialEq)]
pub struct WholeFunctionFlowIrNode {
    /// Formal parameters in source order (rest parameter last).
    pub params: Arc<[FlowIrParam]>,
    /// The function's OWN type parameters (the root signature's binders —
    /// the evaluator lowers parameters and body leaves under them, never
    /// under an outer same-name resolution).
    pub type_parameters: Arc<[FlowIrTypeParam]>,
    /// The root region (the function body statement list). An
    /// expression-bodied arrow lowers to a single `return` of the
    /// expression.
    pub body: FlowIrRegion,
    /// Whether execution can reach past the body without a `return`.
    pub can_fall_through: bool,
    /// A budget edge one leaf's expression lowering hit (the expression
    /// itself degrades to `any`, the whole evaluation fails with the typed
    /// budget reason — the scanner's `Unavailable` verdict for the same
    /// leaf).
    pub budget_failure: Option<verter_type_expr::facts::InferenceUnavailableReason>,
}

/// One formal parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowIrParam {
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
pub struct FlowIrRegion {
    /// The reachable statements, in source order. Statements after a
    /// terminal path (return / throw / unsupported construct) are
    /// unreachable and dropped.
    pub statements: Arc<[FlowIrStatement]>,
    /// Whether execution can reach past this region without a `return`.
    pub can_fall_through: bool,
}

/// One statement of the flow IR.
#[derive(Debug, Clone, PartialEq)]
pub enum FlowIrStatement {
    /// A `return` (bare `return;` carries no argument).
    Return {
        /// The lowered return argument, when present.
        argument: Option<FlowIrExpr>,
    },
    /// An `if` statement. No guard narrowing: the test lowers as a plain
    /// expression and each arm is its own region.
    If {
        /// The lowered test expression.
        test: FlowIrExpr,
        /// The consequent region.
        consequent: Box<FlowIrRegion>,
        /// The alternate region, when an `else` exists.
        alternate: Option<Box<FlowIrRegion>>,
    },
    /// A nested block, as its own region.
    Block(FlowIrRegion),
    /// A `const` / `let` / `var` declarator with an identifier binding.
    Binding {
        /// The binding name.
        name: Arc<str>,
        /// The declaration kind.
        kind: FlowIrBindingKind,
        /// The lowered initializer, when present.
        init: Option<FlowIrExpr>,
    },
    /// An expression statement (evaluation effects; no return
    /// contribution).
    Effect(FlowIrExpr),
    /// A return-free loop or labeled construct: effectful but fall-through
    /// transparent.
    TransparentLoop,
    /// An unsupported construct (return-bearing loop/labeled, `switch`,
    /// `try`, `with`, a `break`/`continue` jump, a module-level
    /// statement). The whole function is unsupported: the region is
    /// produced up to this marker and the evaluator degrades the whole
    /// result.
    Unsupported(FlowIrUnsupported),
}

/// The kind of one local binding declarator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowIrBindingKind {
    /// A `const` (or `using`) declarator.
    Const,
    /// A `let` declarator.
    Let,
    /// A `var` declarator.
    Var,
}

/// One expression of the flow IR.
#[derive(Debug, Clone, PartialEq)]
pub enum FlowIrExpr {
    /// A fully lowered leaf: literals, arrays, objects (spread members ride
    /// as `ObjectMember::Spread` for later delegation to the object-spread
    /// projection), templates, `typeof` paths, `as` / `satisfies` /
    /// parenthesized results — exactly the scanner's expression lowering.
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
    /// computed keys, and method / accessor members keep the scanner's
    /// whole-literal lowering.
    Object {
        /// The members in source order.
        members: Arc<[FlowIrObjectMember]>,
    },
    /// A nested function VALUE (a function / arrow expression or an
    /// object-literal method in any expression position): its parameters
    /// and OWNED body region, lowered inline — the evaluator answers its
    /// body-derived return through the same flow evaluation, never a body
    /// scan and never a leaf fallback.
    NestedFunctionValue {
        /// The nested function's formal parameters (rest last).
        params: Arc<[FlowIrParam]>,
        /// The nested function's own type parameters (the signature's own
        /// binders — carried so the composed signature keeps `<T>`).
        type_parameters: Arc<[FlowIrTypeParam]>,
        /// The nested function's body region (an expression-bodied arrow
        /// lowers to a single `return` of the expression).
        body: FlowIrRegion,
        /// Whether execution can reach past the nested body without a
        /// `return`.
        can_fall_through: bool,
    },
    /// A direct call on a nested function value (an IIFE) — the call's
    /// value is the nested function's evaluated return.
    NestedCall(Box<FlowIrExpr>),
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
    /// A call lowered to the scanner's symbolic `ReturnType<typeof …>`
    /// carrier.
    SymbolicCall(TypeExpr),
    /// An expression the scanner cannot represent (its `any` fallback),
    /// including a call with an unrepresentable callee.
    Any,
}

/// One type parameter of a nested function value.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowIrTypeParam {
    /// The parameter name.
    pub name: Arc<str>,
    /// The lowered constraint, when authored.
    pub constraint: Option<TypeExpr>,
    /// The lowered default, when authored.
    pub default: Option<TypeExpr>,
}

/// One member of a structurally lowered object-literal return.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowIrObjectMember {
    /// The static string key.
    pub key: Arc<str>,
    /// The member value.
    pub value: FlowIrExpr,
    /// The authored method / accessor kind (`None` for a plain property).
    pub method_kind: Option<verter_type_expr::ObjectMethodKind>,
    /// The authored member spans (declaration / name) — they keep two
    /// same-shaped return objects at distinct source sites distinct at
    /// interning (the scanner's member spans do the same).
    pub spans: verter_type_expr::MemberSpans,
}

/// The unsupported-construct classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowIrUnsupported {
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

/// The function node a locator descent lands on.
enum FunctionNode<'a> {
    /// A `function` declaration / expression or a class/object method.
    Function(&'a Function<'a>),
    /// An arrow function.
    Arrow(&'a ArrowFunctionExpression<'a>),
}

impl<'a> FunctionNode<'a> {
    fn params(&self) -> &'a FormalParameters<'a> {
        match self {
            Self::Function(func) => &func.params,
            Self::Arrow(arrow) => &arrow.params,
        }
    }

    fn body(&self) -> Option<&'a FunctionBody<'a>> {
        match self {
            Self::Function(func) => func.body.as_deref(),
            Self::Arrow(arrow) => Some(&arrow.body),
        }
    }

    /// Whether this is an expression-bodied arrow (`(x) => x * 2`).
    fn is_expression_body(&self) -> bool {
        matches!(self, Self::Arrow(arrow) if arrow.expression)
    }

    /// The function's own type parameter clause, when authored.
    fn type_parameters(&self) -> Option<&TSTypeParameterDeclaration<'_>> {
        match self {
            Self::Function(func) => func.type_parameters.as_deref(),
            Self::Arrow(arrow) => arrow.type_parameters.as_deref(),
        }
    }
}

/// Lower one function node's own type parameter clause (name, lowered
/// constraint, lowered default) — shared by the whole-function node and
/// every nested function value.
fn lower_flow_type_params(node: &FunctionNode<'_>, source: &str) -> Vec<FlowIrTypeParam> {
    node.type_parameters()
        .map(|declaration| {
            declaration
                .params
                .iter()
                .map(|param| FlowIrTypeParam {
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

/// Build the whole-function flow IR for one indexed function entry against
/// the retained parse snapshot. Runs inside the memo's lease-only job:
/// pure, owned output, no host re-entry. Returns `None` on any locator
/// miss (a typed miss, never a panic).
pub(crate) fn build_whole_function_flow_ir(
    program: &Program<'_>,
    source: &str,
    entry: &FunctionProgramEntry,
) -> Option<WholeFunctionFlowIrNode> {
    let (node, self_name) = resolve_function_node(program, &entry.locator)?;
    let params = lower_params(node.params(), source);
    let type_parameters = lower_flow_type_params(&node, source);
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
        // scanner's arrow-body behavior: the expression cannot fall
        // through).
        let statement = body.statements.first()?;
        let Statement::ExpressionStatement(expression) = statement else {
            return None;
        };
        let argument = lowerer.lower_expr(&expression.expression, ExprMode::ArrowBody);
        FlowIrRegion {
            statements: Arc::from([FlowIrStatement::Return {
                argument: Some(argument),
            }]),
            can_fall_through: false,
        }
    } else {
        lowerer.lower_region(&body.statements).region
    };
    let budget_failure = lowerer.budget_failure;
    Some(WholeFunctionFlowIrNode {
        can_fall_through: region.can_fall_through,
        params: Arc::from(params.into_boxed_slice()),
        type_parameters: Arc::from(type_parameters.into_boxed_slice()),
        body: region,
        budget_failure,
    })
}

/// The declaration view of a statement, unwrapping the export wrappers the
/// locator descent does not record (the index discovers through
/// `export { … }` / `export default` transparently).
enum DeclRef<'a> {
    /// A function declaration.
    Function(&'a Function<'a>),
    /// A variable declaration.
    Variable(&'a VariableDeclaration<'a>),
    /// A class declaration.
    Class(&'a Class<'a>),
    /// A namespace (`TSModuleDeclaration`).
    Module(&'a TSModuleDeclaration<'a>),
    /// An `export default { … }` object expression.
    ExportDefaultObject(&'a ObjectExpression<'a>),
}

fn declaration_of<'a>(statement: &'a Statement<'a>) -> Option<DeclRef<'a>> {
    match statement {
        Statement::FunctionDeclaration(func) => Some(DeclRef::Function(func)),
        Statement::VariableDeclaration(decl) => Some(DeclRef::Variable(decl)),
        Statement::ClassDeclaration(class) => Some(DeclRef::Class(class)),
        Statement::TSModuleDeclaration(module) => Some(DeclRef::Module(module)),
        Statement::ExportNamedDeclaration(export) => match export.declaration.as_ref()? {
            Declaration::FunctionDeclaration(func) => Some(DeclRef::Function(func)),
            Declaration::VariableDeclaration(decl) => Some(DeclRef::Variable(decl)),
            Declaration::ClassDeclaration(class) => Some(DeclRef::Class(class)),
            Declaration::TSModuleDeclaration(module) => Some(DeclRef::Module(module)),
            _ => None,
        },
        Statement::ExportDefaultDeclaration(export) => match &export.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                Some(DeclRef::Function(func))
            }
            ExportDefaultDeclarationKind::ClassDeclaration(class) => Some(DeclRef::Class(class)),
            other => match other.as_expression() {
                Some(Expression::ObjectExpression(obj)) => Some(DeclRef::ExportDefaultObject(obj)),
                _ => None,
            },
        },
        _ => None,
    }
}

/// The function node behind an initializer expression: an arrow or a
/// Unwrap a parenthesized expression (the IIFE callee shape).
fn unwrap_parenthesized<'a>(expression: &'a Expression<'a>) -> &'a Expression<'a> {
    match expression {
        Expression::ParenthesizedExpression(paren) => unwrap_parenthesized(&paren.expression),
        inner => inner,
    }
}

/// function expression, nothing else.
fn function_from_expression<'a>(expression: &'a Expression<'a>) -> Option<FunctionNode<'a>> {
    match expression {
        Expression::FunctionExpression(func) => Some(FunctionNode::Function(func)),
        Expression::ArrowFunctionExpression(arrow) => Some(FunctionNode::Arrow(arrow)),
        _ => None,
    }
}

/// Resolve one function's locator against the retained snapshot: the
/// contributing top-level statement, then the ordinal descent. Also
/// derives the function's bare-identifier SELF name for direct-recursion
/// detection: a function declaration contributes its id, a variable
/// initializer its declarator binding; class members and object-literal
/// members have no bare-identifier self name. Any miss is a typed `None`.
fn resolve_function_node<'a>(
    program: &'a Program<'a>,
    locator: &FunctionBodyLocator,
) -> Option<(FunctionNode<'a>, Option<Arc<str>>)> {
    let mut statement = program
        .body
        .get(locator.contributor.contributor_index as usize)?;
    let mut steps = locator.descent.iter();
    loop {
        match steps.next()? {
            FunctionDescentStep::NamespaceMember { statement_ordinal } => {
                let DeclRef::Module(module) = declaration_of(statement)? else {
                    return None;
                };
                let Some(TSModuleDeclarationBody::TSModuleBlock(block)) = module.body.as_ref()
                else {
                    return None;
                };
                statement = block.body.get(*statement_ordinal as usize)?;
            }
            FunctionDescentStep::FunctionDeclaration => {
                // Terminal step: the statement IS the function declaration.
                if steps.next().is_some() {
                    return None;
                }
                let DeclRef::Function(func) = declaration_of(statement)? else {
                    return None;
                };
                let self_name = func.id.as_ref().map(|id| Arc::from(id.name.as_str()));
                return Some((FunctionNode::Function(func), self_name));
            }
            FunctionDescentStep::VariableInitializer { declarator_ordinal } => {
                let DeclRef::Variable(var_decl) = declaration_of(statement)? else {
                    return None;
                };
                let declarator = var_decl.declarations.get(*declarator_ordinal as usize)?;
                let self_name = match &declarator.id {
                    BindingPattern::BindingIdentifier(id) => Some(Arc::from(id.name.as_str())),
                    _ => None,
                };
                let init = declarator.init.as_ref()?;
                match steps.next() {
                    None => {
                        return Some((function_from_expression(init)?, self_name));
                    }
                    Some(FunctionDescentStep::ObjectMember { member_ordinal }) => {
                        // Terminal step: the object-literal member inside
                        // the current initializer object expression.
                        if steps.next().is_some() {
                            return None;
                        }
                        let Expression::ObjectExpression(obj) = init else {
                            return None;
                        };
                        let prop = obj.properties.get(*member_ordinal as usize)?;
                        let ObjectPropertyKind::ObjectProperty(property) = prop else {
                            return None;
                        };
                        // Object members have no bare-identifier self name.
                        return Some((function_from_expression(&property.value)?, None));
                    }
                    Some(_) => return None,
                }
            }
            FunctionDescentStep::ClassMember { member_ordinal } => {
                // Terminal step: the class member at `member_ordinal`.
                if steps.next().is_some() {
                    return None;
                }
                let DeclRef::Class(class) = declaration_of(statement)? else {
                    return None;
                };
                let element = class.body.body.get(*member_ordinal as usize)?;
                let node = match element {
                    ClassElement::MethodDefinition(method) => FunctionNode::Function(&method.value),
                    ClassElement::PropertyDefinition(property) => {
                        function_from_expression(property.value.as_ref()?)?
                    }
                    _ => return None,
                };
                // Class members have no bare-identifier self name.
                return Some((node, None));
            }
            FunctionDescentStep::ExportDefaultObjectMember { member_ordinal } => {
                // Terminal step: the object-literal method at
                // `member_ordinal` inside the `export default { … }`
                // object expression.
                if steps.next().is_some() {
                    return None;
                }
                let DeclRef::ExportDefaultObject(obj) = declaration_of(statement)? else {
                    return None;
                };
                let prop = obj.properties.get(*member_ordinal as usize)?;
                let ObjectPropertyKind::ObjectProperty(property) = prop else {
                    return None;
                };
                // Object members have no bare-identifier self name.
                return Some((function_from_expression(&property.value)?, None));
            }
            FunctionDescentStep::ObjectMember { .. } => {
                // Only valid immediately after a VariableInitializer step
                // (handled there).
                return None;
            }
        }
    }
}

/// Lower the formal parameters: binding name, optional/rest flags, and the
/// parameter type — the authored TS annotation through `lower_ts_type`,
/// else the default initializer's inferred type (the scanner's parameter
/// rule), else `any`.
fn lower_params(params: &FormalParameters<'_>, source: &str) -> Vec<FlowIrParam> {
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
        out.push(FlowIrParam {
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
        out.push(FlowIrParam {
            name,
            optional: false,
            rest: true,
            ty,
        });
    }
    out
}

/// The expression-lowering position, selecting which of the scanner's two
/// entry points a leaf lowers through (and its widening behavior).
#[derive(Clone, Copy)]
enum ExprMode {
    /// Return-argument position: declaration lowering (`preserve_literal =
    /// false`), exactly the scanner's return-argument behavior — a fresh
    /// top-level literal widens to its primitive.
    Return,
    /// Arrow expression-body position: expression lowering, exactly the
    /// scanner's arrow-body behavior — a standalone literal keeps its
    /// literal type.
    ArrowBody,
    /// Binding-initializer position: the scanner's value-declaration
    /// initializer behavior — `const` preserves the literal, `let` / `var`
    /// widen.
    BindingInit(FlowIrBindingKind),
    /// Effect / test position: declaration lowering (the value is not a
    /// return contribution).
    Plain,
}

/// The region lowering result: the region plus whether any nested lowering
/// hit an unsupported construct (the marker is in the tree; the flag
/// propagates so the root region stops at the same point).
struct LoweredRegion {
    region: FlowIrRegion,
    hit_unsupported: bool,
}

/// The statement/expression lowering state: scanner entry points, the
/// function's parameters (for [`FlowIrExpr::Param`] ordinals), its
/// bare-identifier self name (for [`FlowIrExpr::DirectSelfCall`]), the
/// hoisted nested function declaration names (for
/// [`FlowIrExpr::LocalFunctionShadow`]), and the local-binding scope stack
/// (for [`FlowIrExpr::Local`]). One scope frame per region; a region
/// PRE-DECLARES every lexical name its own statements bind (a forward
/// reference is in scope but unbound at evaluation — the TDZ-honest `any`)
/// and a binding's reaching definition still enters AFTER its initializer
/// is lowered.
struct Lowerer<'a> {
    source: &'a str,
    params: &'a [FlowIrParam],
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
    /// The first budget edge a leaf's expression lowering hit.
    budget_failure: Option<verter_type_expr::facts::InferenceUnavailableReason>,
}

impl Lowerer<'_> {
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
        // same-name callee.
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
        let mut out: Vec<FlowIrStatement> = Vec::new();
        let mut can_fall_through = true;
        let mut hit_unsupported = false;
        for statement in statements {
            if !can_fall_through {
                break;
            }
            match statement {
                Statement::ReturnStatement(ret) => {
                    let argument = ret
                        .argument
                        .as_ref()
                        .map(|arg| self.lower_expr(arg, ExprMode::Return));
                    out.push(FlowIrStatement::Return { argument });
                    can_fall_through = false;
                }
                Statement::BlockStatement(block) => {
                    let child = self.lower_region(&block.body);
                    can_fall_through = child.region.can_fall_through;
                    hit_unsupported = child.hit_unsupported;
                    out.push(FlowIrStatement::Block(child.region));
                }
                Statement::IfStatement(if_stmt) => {
                    let test = self.lower_expr(&if_stmt.test, ExprMode::Plain);
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
                    out.push(FlowIrStatement::If {
                        test,
                        consequent: Box::new(consequent.region),
                        alternate: alternate.map(|region| Box::new(region.region)),
                    });
                }
                Statement::VariableDeclaration(decl) => {
                    let kind = match decl.kind {
                        VariableDeclarationKind::Const
                        | VariableDeclarationKind::Using
                        | VariableDeclarationKind::AwaitUsing => FlowIrBindingKind::Const,
                        VariableDeclarationKind::Let => FlowIrBindingKind::Let,
                        VariableDeclarationKind::Var => FlowIrBindingKind::Var,
                    };
                    for declarator in &decl.declarations {
                        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                            // Destructuring declarators are not simple
                            // local reaching definitions.
                            continue;
                        };
                        let init = declarator
                            .init
                            .as_ref()
                            .map(|expr| self.lower_expr(expr, ExprMode::BindingInit(kind)));
                        let name: Arc<str> = Arc::from(id.name.as_str());
                        out.push(FlowIrStatement::Binding {
                            name: Arc::clone(&name),
                            kind,
                            init,
                        });
                        if let Some(frame) = self.scopes.last_mut() {
                            frame.push(name);
                        }
                    }
                }
                Statement::ExpressionStatement(expression) => {
                    out.push(FlowIrStatement::Effect(
                        self.lower_expr(&expression.expression, ExprMode::Plain),
                    ));
                }
                // A `throw` terminates the region path without contributing
                // a return arm (the scanner's `ThrowStatement → Ok(false)`).
                Statement::ThrowStatement(_) => {
                    can_fall_through = false;
                }
                Statement::DoWhileStatement(_)
                | Statement::ForInStatement(_)
                | Statement::ForOfStatement(_)
                | Statement::ForStatement(_)
                | Statement::WhileStatement(_) => {
                    if self.control_has_return(statement) {
                        out.push(FlowIrStatement::Unsupported(FlowIrUnsupported::Loop));
                        hit_unsupported = true;
                        can_fall_through = false;
                    } else {
                        out.push(FlowIrStatement::TransparentLoop);
                    }
                }
                Statement::LabeledStatement(_) => {
                    if self.control_has_return(statement) {
                        out.push(FlowIrStatement::Unsupported(FlowIrUnsupported::Labeled));
                        hit_unsupported = true;
                        can_fall_through = false;
                    } else {
                        out.push(FlowIrStatement::TransparentLoop);
                    }
                }
                Statement::SwitchStatement(_) => {
                    out.push(FlowIrStatement::Unsupported(FlowIrUnsupported::Switch));
                    hit_unsupported = true;
                    can_fall_through = false;
                }
                Statement::TryStatement(_) => {
                    out.push(FlowIrStatement::Unsupported(FlowIrUnsupported::Try));
                    hit_unsupported = true;
                    can_fall_through = false;
                }
                Statement::WithStatement(_) => {
                    out.push(FlowIrStatement::Unsupported(FlowIrUnsupported::With));
                    hit_unsupported = true;
                    can_fall_through = false;
                }
                Statement::BreakStatement(_) | Statement::ContinueStatement(_) => {
                    out.push(FlowIrStatement::Unsupported(FlowIrUnsupported::Jump));
                    hit_unsupported = true;
                    can_fall_through = false;
                }
                Statement::ImportDeclaration(_)
                | Statement::ExportAllDeclaration(_)
                | Statement::ExportDefaultDeclaration(_)
                | Statement::ExportNamedDeclaration(_)
                | Statement::TSExportAssignment(_)
                | Statement::TSNamespaceExportDeclaration(_) => {
                    out.push(FlowIrStatement::Unsupported(
                        FlowIrUnsupported::ModuleDeclaration,
                    ));
                    hit_unsupported = true;
                    can_fall_through = false;
                }
                // Declaration / no-op statements: transparent (no return
                // contribution, no flow-IR statement).
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
            region: FlowIrRegion {
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
    /// lowers STRUCTURALLY (each member value is a flow expression); a
    /// bare-identifier call to the function itself becomes the recursion
    /// hold; every other form reuses the scanner's expression lowering for
    /// the position.
    fn lower_expr(&mut self, expr: &Expression<'_>, mode: ExprMode) -> FlowIrExpr {
        match expr {
            Expression::Identifier(identifier) => {
                let name = identifier.name.as_str();
                if let Some(ordinal) = self.param_ordinal(name) {
                    return FlowIrExpr::Param { ordinal };
                }
                if self.is_local_in_scope(name) {
                    return FlowIrExpr::Local {
                        name: Arc::from(name),
                    };
                }
                self.lower_leaf(expr, mode)
            }
            Expression::ParenthesizedExpression(paren) => {
                // A parenthesized wrapper is structurally transparent (the
                // scanner unwraps it the same way).
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
                    // A method / accessor member with a body is a nested
                    // function value (its return evaluates inline through
                    // the same flow machinery); a method without a body
                    // keeps the scanner's whole-literal lowering.
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
                        members.push(FlowIrObjectMember {
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
                        (true, FlowIrExpr::Type(ty)) => FlowIrExpr::Type(
                            verter_semantic::analysis::type_eval_build::widen_shallow_literal(ty),
                        ),
                        (_, value) => value,
                    };
                    members.push(FlowIrObjectMember {
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
                    FlowIrExpr::Object {
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
                FlowIrExpr::NestedCall(Box::new(function))
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
                        return FlowIrExpr::LocalFunctionShadow;
                    }
                    // 2. A parameter SHADOWS the declaration: the call goes
                    //    through the binding's signature, never a flow
                    //    obligation edge.
                    if let Some(ordinal) = self.param_ordinal(name) {
                        return FlowIrExpr::CallOnBinding {
                            param: Some(ordinal),
                            name: Arc::from(name),
                        };
                    }
                    // 3. An in-scope local (pre-declared or already bound)
                    //    shadows the declaration the same way.
                    if self.is_local_in_scope(name) {
                        return FlowIrExpr::CallOnBinding {
                            param: None,
                            name: Arc::from(name),
                        };
                    }
                    // 4. A bare-identifier call to the function itself — a
                    //    direct same-slot recursion hold.
                    if Some(name) == self.self_name {
                        return FlowIrExpr::DirectSelfCall;
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
                        return FlowIrExpr::DirectCall(direct.target.clone());
                    }
                }
                let ty = self.scanner_type(expr, mode);
                if is_any(&ty) {
                    FlowIrExpr::Any
                } else {
                    FlowIrExpr::SymbolicCall(ty)
                }
            }
            _ => self.lower_leaf(expr, mode),
        }
    }

    /// Lower a NESTED function node (a function / arrow expression or an
    /// object-literal method) into an owned nested function value: the
    /// nested body's statements lower under the nested function's OWN
    /// parameter scope — outer parameters and locals are NOT in scope
    /// (closure capture stays the leaf fallback).
    fn lower_nested_function(&mut self, node: &FunctionNode<'_>) -> FlowIrExpr {
        let params = lower_params(node.params(), self.source);
        let type_parameters = lower_flow_type_params(node, self.source);
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
                    _ => FlowIrExpr::Any,
                });
            FlowIrRegion {
                statements: Arc::from(
                    argument
                        .map(|argument| FlowIrStatement::Return {
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
                None => FlowIrRegion {
                    statements: Arc::from(Vec::new().into_boxed_slice()),
                    can_fall_through: true,
                },
            }
        };
        if let Some(reason) = nested.budget_failure {
            self.budget_failure.get_or_insert(reason);
        }
        FlowIrExpr::NestedFunctionValue {
            params: Arc::from(params.into_boxed_slice()),
            type_parameters: Arc::from(type_parameters.into_boxed_slice()),
            can_fall_through: region.can_fall_through,
            body: region,
        }
    }

    /// Lower a leaf expression through the scanner, wrapping the result.
    /// The scanner's `any` fallback surfaces as [`FlowIrExpr::Any`].
    fn lower_leaf(&mut self, expr: &Expression<'_>, mode: ExprMode) -> FlowIrExpr {
        let ty = self.scanner_type(expr, mode);
        if is_any(&ty) {
            FlowIrExpr::Any
        } else {
            FlowIrExpr::Type(ty)
        }
    }

    /// The scanner's expression lowering for the position. Budget
    /// exhaustion degrades the one expression to `any` — the same carrier
    /// the scanner emits for unrepresentable forms — and records the typed
    /// budget edge (the scanner's `Unavailable` verdict for the same leaf).
    fn scanner_type(&mut self, expr: &Expression<'_>, mode: ExprMode) -> TypeExpr {
        let result = match mode {
            ExprMode::Return
            | ExprMode::Plain
            | ExprMode::BindingInit(FlowIrBindingKind::Let)
            | ExprMode::BindingInit(FlowIrBindingKind::Var) => {
                infer_declaration_expression_type(expr, self.source, false)
            }
            ExprMode::ArrowBody | ExprMode::BindingInit(FlowIrBindingKind::Const) => {
                infer_expression_type(expr, self.source)
            }
        };
        result.unwrap_or_else(|reason| {
            if self.budget_failure.is_none() {
                self.budget_failure = Some(reason);
            }
            TypeExpr::Primitive(PrimitiveName::Any)
        })
    }
}

fn is_any(ty: &TypeExpr) -> bool {
    matches!(ty, TypeExpr::Primitive(PrimitiveName::Any))
}
