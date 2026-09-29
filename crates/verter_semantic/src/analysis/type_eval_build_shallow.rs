//! The shallow per-expression inference, run from explicit stacks.
//!
//! An expression nested in an expression costs no native level: every
//! construct whose type is built from its parts' types — a conditional, an
//! array or object literal, a template in a const context, an arrow, a
//! call, `!`, `&&` / `||` — is a frame of one task stack, resumed with each
//! part's inferred value. The same holds for widening a lowered type's
//! literals. The only bound is the inference's work budget, which every
//! visited expression, member and parameter charges.

use super::*;

/// What a step of the inference produces: a type, or the object, signature
/// or parameter list an extraction builds.
pub(super) enum Value {
    Type(TypeExpr),
    Object(ObjectExpr),
    Signature(LoweredSignatureParts),
    Params(Vec<FunctionParam>),
}

impl Value {
    pub(super) fn into_type(self) -> TypeExpr {
        match self {
            Value::Type(ty) => ty,
            _ => unreachable!("a type position receives a type"),
        }
    }

    pub(super) fn into_object(self) -> ObjectExpr {
        match self {
            Value::Object(object) => object,
            _ => unreachable!("an object extraction receives an object"),
        }
    }

    pub(super) fn into_signature(self) -> LoweredSignatureParts {
        match self {
            Value::Signature(signature) => signature,
            _ => unreachable!("a signature extraction receives a signature"),
        }
    }

    pub(super) fn into_params(self) -> Vec<FunctionParam> {
        match self {
            Value::Params(params) => params,
            _ => unreachable!("a parameter lowering receives parameters"),
        }
    }
}

/// One step of the inference.
pub(super) enum Task<'a> {
    /// A declaration-position expression under its top-level literal
    /// policy.
    Declaration(&'a Expression<'a>, TopLevelLiteralPolicy),
    /// A value expression whose object-literal members follow the policy.
    Value(&'a Expression<'a>, MemberLiteralPolicy),
    /// [`Self::Value`] for the expression whose whole identifier read or
    /// type query the caller asked for: its parentheses, `satisfies` and
    /// assertions report it.
    RootValue(&'a Expression<'a>, MemberLiteralPolicy),
    /// The type on top, a fresh top-level literal widened when the policy
    /// widens it.
    WidenTopLevel(TopLevelLiteralPolicy),
    /// A conditional's two branch types, joined.
    Branches,
    /// `boolean` when each of the last `count` types is `boolean`.
    BooleanOver(usize),
    /// A call's value over the callee type on top.
    CallReturn,
    DeclarationArray(DeclarationArrayFrame<'a>),
    ValueArray(ValueArrayFrame<'a>),
    ConstTemplate(TemplateFrame<'a>),
    Object(ObjectFrame<'a>),
    Function(FunctionFrame<'a>),
    Arrow(ArrowFrame<'a>),
    Params(ParamsFrame<'a>),
}

/// What a frame needs next: a part inferred, or nothing — its value.
enum Step<'a> {
    Descend(Task<'a>),
    Done(Value),
}

/// A construct inferred from its parts, one part at a time.
trait Frame<'a>: Sized {
    /// Whether the frame asked for a part it has not received.
    fn awaiting(&self) -> bool;
    /// Resume with the part asked for last `delivered`, until the frame
    /// needs another part or has its value.
    fn step(
        &mut self,
        delivered: Option<Value>,
        source: &str,
        budget: &mut InferenceBudget,
    ) -> InferenceResult<Step<'a>>;
}

/// Run the inference from `first` to its value. `read_root` receives the
/// whole identifier read or type query of a [`Task::RootValue`] chain.
pub(super) fn run<'a>(
    first: Task<'a>,
    source: &str,
    budget: &mut InferenceBudget,
    mut read_root: Option<&mut IndexedValueReadRoot>,
) -> InferenceResult<Value> {
    let mut tasks = vec![first];
    let mut values: Vec<Value> = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Declaration(expr, policy) => {
                declaration(expr, policy, source, budget, &mut tasks)?;
            }
            Task::Value(expr, policy) => {
                value(
                    expr,
                    policy,
                    false,
                    source,
                    budget,
                    None,
                    &mut tasks,
                    &mut values,
                )?;
            }
            Task::RootValue(expr, policy) => value(
                expr,
                policy,
                true,
                source,
                budget,
                read_root.as_deref_mut(),
                &mut tasks,
                &mut values,
            )?,
            Task::WidenTopLevel(policy) => {
                let ty = pop_type(&mut values);
                values.push(Value::Type(match policy {
                    TopLevelLiteralPolicy::Widen => widen_shallow_literal(ty),
                    TopLevelLiteralPolicy::Preserve => ty,
                }));
            }
            Task::Branches => {
                let alternate = pop_type(&mut values);
                let consequent = pop_type(&mut values);
                values.push(Value::Type(TypeExpr::union(vec![consequent, alternate])));
            }
            Task::BooleanOver(count) => {
                let operands: Vec<TypeExpr> = values
                    .split_off(values.len() - count)
                    .into_iter()
                    .map(Value::into_type)
                    .collect();
                values.push(Value::Type(boolean_or_unmodeled(&operands, budget)));
            }
            Task::CallReturn => {
                let callee = pop_type(&mut values);
                values.push(Value::Type(
                    if matches!(callee, TypeExpr::Primitive(PrimitiveName::Any)) {
                        TypeExpr::Primitive(PrimitiveName::Any)
                    } else {
                        call_return_carrier(callee)
                    },
                ));
            }
            Task::DeclarationArray(frame) => resume(
                frame,
                Task::DeclarationArray,
                source,
                budget,
                &mut tasks,
                &mut values,
            )?,
            Task::ValueArray(frame) => resume(
                frame,
                Task::ValueArray,
                source,
                budget,
                &mut tasks,
                &mut values,
            )?,
            Task::ConstTemplate(frame) => resume(
                frame,
                Task::ConstTemplate,
                source,
                budget,
                &mut tasks,
                &mut values,
            )?,
            Task::Object(frame) => {
                resume(frame, Task::Object, source, budget, &mut tasks, &mut values)?;
            }
            Task::Function(frame) => {
                resume(
                    frame,
                    Task::Function,
                    source,
                    budget,
                    &mut tasks,
                    &mut values,
                )?;
            }
            Task::Arrow(frame) => {
                resume(frame, Task::Arrow, source, budget, &mut tasks, &mut values)?;
            }
            Task::Params(frame) => {
                resume(frame, Task::Params, source, budget, &mut tasks, &mut values)?;
            }
        }
    }
    Ok(values
        .pop()
        .expect("the inference's value is the one value left"))
}

fn pop_type(values: &mut Vec<Value>) -> TypeExpr {
    values.pop().expect("a part's type").into_type()
}

/// Resume `frame`, putting it back beneath the part it asks for next, or
/// its value on the value stack.
fn resume<'a, F: Frame<'a>>(
    mut frame: F,
    wrap: fn(F) -> Task<'a>,
    source: &str,
    budget: &mut InferenceBudget,
    tasks: &mut Vec<Task<'a>>,
    values: &mut Vec<Value>,
) -> InferenceResult<()> {
    let delivered = frame
        .awaiting()
        .then(|| values.pop().expect("the part the frame asked for"));
    match frame.step(delivered, source, budget)? {
        Step::Descend(part) => {
            tasks.push(wrap(frame));
            tasks.push(part);
        }
        Step::Done(value) => values.push(value),
    }
    Ok(())
}

/// A declaration-position expression: the top level under `policy`. A
/// const assertion pins its whole operand; a type assertion's type is not a
/// fresh literal; parentheses pass the policy through; each branch of a
/// conditional is a top level under the same policy; an array literal's
/// elements always widen.
fn declaration<'a>(
    expr: &'a Expression<'a>,
    policy: TopLevelLiteralPolicy,
    source: &str,
    budget: &mut InferenceBudget,
    tasks: &mut Vec<Task<'a>>,
) -> InferenceResult<()> {
    budget.visit()?;
    if expr_is_const_asserted(expr, source) {
        tasks.push(Task::Value(expr, MemberLiteralPolicy::Widen));
        return Ok(());
    }
    match expr {
        Expression::TSAsExpression(_) | Expression::TSTypeAssertion(_) => {
            tasks.push(Task::Value(expr, MemberLiteralPolicy::Widen));
        }
        Expression::ParenthesizedExpression(parenthesized) => {
            tasks.push(Task::Declaration(&parenthesized.expression, policy));
        }
        Expression::ConditionalExpression(conditional) => {
            tasks.push(Task::Branches);
            tasks.push(Task::Declaration(&conditional.alternate, policy));
            tasks.push(Task::Declaration(&conditional.consequent, policy));
        }
        Expression::ArrayExpression(array) => {
            tasks.push(Task::DeclarationArray(DeclarationArrayFrame {
                array,
                next: 0,
                element_types: Vec::new(),
                awaiting: None,
            }));
        }
        _ => {
            tasks.push(Task::WidenTopLevel(policy));
            tasks.push(Task::Value(expr, MemberLiteralPolicy::Widen));
        }
    }
    Ok(())
}

/// A value expression under the member-literal `policy`: its type pushed
/// on `values`, or the tasks that build it pushed on `tasks`. `root` marks
/// the chain whose whole identifier read or type query `read_root`
/// receives.
#[allow(clippy::too_many_arguments)]
fn value<'a>(
    expr: &'a Expression<'a>,
    policy: MemberLiteralPolicy,
    root: bool,
    source: &str,
    budget: &mut InferenceBudget,
    read_root: Option<&mut IndexedValueReadRoot>,
    tasks: &mut Vec<Task<'a>>,
    values: &mut Vec<Value>,
) -> InferenceResult<()> {
    budget.visit()?;
    let same_chain = |expr: &'a Expression<'a>, policy: MemberLiteralPolicy| {
        if root {
            Task::RootValue(expr, policy)
        } else {
            Task::Value(expr, policy)
        }
    };
    match value_inference_carrier(expr) {
        ValueInferenceCarrier::Parenthesized(inner) => {
            tasks.push(same_chain(inner, policy));
            return Ok(());
        }
        ValueInferenceCarrier::Satisfies(inner) => {
            let inner_policy = if policy == MemberLiteralPolicy::ConstAssert {
                policy
            } else {
                MemberLiteralPolicy::Preserve
            };
            tasks.push(same_chain(inner, inner_policy));
            return Ok(());
        }
        ValueInferenceCarrier::Assertion {
            operand,
            annotation,
        } => {
            if verter_type_expr_oxc::is_const_assertion_type(annotation) {
                tasks.push(same_chain(operand, MemberLiteralPolicy::ConstAssert));
                return Ok(());
            }
            let mut query = None;
            let asserted = verter_type_expr_oxc::lower_ts_type_with_whole_query(
                annotation,
                source,
                read_root.as_ref().map(|_| &mut query),
            );
            if let (Some(root), Some(query)) = (read_root, query) {
                *root = IndexedValueReadRoot::SourceTypeQuery(query);
            }
            values.push(Value::Type(asserted));
            return Ok(());
        }
        ValueInferenceCarrier::Value => {}
    }
    let ty = match expr {
        // `undefined` is an IDENTIFIER in the grammar (unlike the `null`
        // literal) but its value position IS the `undefined` type — never a
        // resolvable file-scope value path.
        Expression::Identifier(ident) if ident.name == "undefined" => {
            TypeExpr::Primitive(PrimitiveName::Undefined)
        }
        Expression::Identifier(ident) => {
            if let Some(read_root) = read_root {
                *read_root = IndexedValueReadRoot::Identifier(ident.span.into());
            }
            TypeExpr::TypeOf(ValueRef {
                path: vec![ident.name.as_str().to_string()],
                type_args: Vec::new(),
            })
        }
        Expression::StringLiteral(s) => TypeExpr::string_literal(s.value.as_str()),
        Expression::NumericLiteral(n) => TypeExpr::number_literal(n.value),
        Expression::BigIntLiteral(b) => {
            TypeExpr::Literal(verter_type_expr::LiteralValue::BigInt(b.value.to_string()))
        }
        // A signed numeric literal (`-1`, `+1`) and a negated bigint literal
        // (`-1n`) are literals of their own value.
        Expression::UnaryExpression(unary)
            if matches!(
                (unary.operator, &unary.argument),
                (
                    UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus,
                    Expression::NumericLiteral(_)
                ) | (UnaryOperator::UnaryNegation, Expression::BigIntLiteral(_))
            ) =>
        {
            match &unary.argument {
                Expression::NumericLiteral(n) if unary.operator == UnaryOperator::UnaryNegation => {
                    TypeExpr::number_literal(-n.value)
                }
                Expression::NumericLiteral(n) => TypeExpr::number_literal(n.value),
                Expression::BigIntLiteral(b) => TypeExpr::Literal(
                    verter_type_expr::LiteralValue::BigInt(format!("-{}", b.value)),
                ),
                _ => unreachable!("the guard admits only numeric and bigint literal operands"),
            }
        }
        Expression::BooleanLiteral(b) => TypeExpr::boolean_literal(b.value),
        Expression::NullLiteral(_) => TypeExpr::Primitive(PrimitiveName::Null),
        // `void x` evaluates its operand and produces `undefined`.
        Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::Void => {
            TypeExpr::Primitive(PrimitiveName::Undefined)
        }
        // `typeof x` over an operand it only reads is the checker's union
        // of the `typeof` result strings (`typeofType`), a regular literal
        // union no position widens.
        Expression::UnaryExpression(unary)
            if unary.operator == UnaryOperator::Typeof
                && typeof_operand_only_reads(&unary.argument) =>
        {
            TypeExpr::union(
                [
                    "string",
                    "number",
                    "bigint",
                    "boolean",
                    "symbol",
                    "undefined",
                    "object",
                    "function",
                ]
                .into_iter()
                .map(TypeExpr::string_literal)
                .collect(),
            )
        }
        // Both arms contribute to this composite; neither is its whole
        // identifier origin.
        Expression::ConditionalExpression(conditional) => {
            tasks.push(Task::Branches);
            tasks.push(Task::Value(&conditional.alternate, policy));
            tasks.push(Task::Value(&conditional.consequent, policy));
            return Ok(());
        }
        Expression::ArrayExpression(array) => {
            tasks.push(Task::ValueArray(ValueArrayFrame::new(
                array, policy, budget,
            )));
            return Ok(());
        }
        Expression::ObjectExpression(object) => {
            tasks.push(Task::Object(ObjectFrame::new(
                object, policy, true, budget,
            )?));
            return Ok(());
        }
        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => {
            let mut value = String::new();
            for quasi in &tpl.quasis {
                value.push_str(quasi.value.raw.as_str());
            }
            TypeExpr::string_literal(value)
        }
        // In a const context a template is the template literal type of its
        // holes' literal types (`` `x${1}` as const `` is `"x1"`), when every
        // hole is a literal or primitive type this inference reads; a hole
        // naming a value keeps the template a `string`.
        Expression::TemplateLiteral(tpl) if policy == MemberLiteralPolicy::ConstAssert => {
            tasks.push(Task::ConstTemplate(TemplateFrame::new(tpl, policy)));
            return Ok(());
        }
        Expression::TemplateLiteral(_) => TypeExpr::Primitive(PrimitiveName::String),
        Expression::ArrowFunctionExpression(arrow) => {
            tasks.push(Task::Arrow(ArrowFrame::new(arrow, true, budget)?));
            return Ok(());
        }
        Expression::StaticMemberExpression(member) => {
            // obj.foo → typeof obj.foo (build a dotted path)
            let mut path = Vec::new();
            collect_static_member_path_with_budget(member, &mut path, budget)?;
            if path.is_empty() {
                budget.used_unmodeled_fallback = true;
                TypeExpr::Primitive(PrimitiveName::Any)
            } else {
                TypeExpr::TypeOf(ValueRef {
                    path,
                    type_args: Vec::new(),
                })
            }
        }
        // fn() → ReturnType<typeof fn>
        Expression::CallExpression(call) => {
            tasks.push(Task::CallReturn);
            tasks.push(Task::Value(&call.callee, policy));
            return Ok(());
        }
        // An equality, relational, `instanceof` or `in` comparison is
        // `boolean` whatever its operands are: neither operand provides the
        // comparison's value.
        Expression::BinaryExpression(binary) if binary_operator_is_comparison(binary.operator) => {
            TypeExpr::Primitive(PrimitiveName::Boolean)
        }
        // `#field in object` is an `in` test: `boolean`.
        Expression::PrivateInExpression(_) => TypeExpr::Primitive(PrimitiveName::Boolean),
        // `!operand`, and `a && b` / `a || b`, over `boolean` operands are
        // `boolean`. Any other operand keeps the unmodeled fallback: the
        // result then depends on the operand's truthiness facts (`!` over an
        // always-truthy operand is `false`) or is the operand's own value.
        Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::LogicalNot => {
            tasks.push(Task::BooleanOver(1));
            tasks.push(Task::Value(&unary.argument, MemberLiteralPolicy::Widen));
            return Ok(());
        }
        Expression::LogicalExpression(logical)
            if matches!(
                logical.operator,
                oxc_ast::ast::LogicalOperator::And | oxc_ast::ast::LogicalOperator::Or
            ) =>
        {
            tasks.push(Task::BooleanOver(2));
            tasks.push(Task::Value(&logical.right, MemberLiteralPolicy::Widen));
            tasks.push(Task::Value(&logical.left, MemberLiteralPolicy::Widen));
            return Ok(());
        }
        _ => {
            budget.used_unmodeled_fallback = true;
            TypeExpr::Primitive(PrimitiveName::Any)
        }
    };
    values.push(Value::Type(ty));
    Ok(())
}

/// A declaration-position array literal: its elements always widen, a
/// spread contributes its source's element types, and with
/// `strictNullChecks` off a bare nullish element is dropped beside another
/// element (see [`NestedNullishLiterals`]).
pub(super) struct DeclarationArrayFrame<'a> {
    array: &'a oxc_ast::ast::ArrayExpression<'a>,
    next: usize,
    element_types: Vec<TypeExpr>,
    /// Whether the part asked for is a spread's source.
    awaiting: Option<bool>,
}

impl<'a> Frame<'a> for DeclarationArrayFrame<'a> {
    fn awaiting(&self) -> bool {
        self.awaiting.is_some()
    }

    fn step(
        &mut self,
        delivered: Option<Value>,
        _source: &str,
        budget: &mut InferenceBudget,
    ) -> InferenceResult<Step<'a>> {
        if let Some(delivered) = delivered {
            let ty = delivered.into_type();
            if self.awaiting.take() == Some(true) {
                spread_element_types(&mut self.element_types, &ty);
            } else {
                append_union_members(&mut self.element_types, ty);
            }
        }
        while let Some(element) = self.array.elements.get(self.next) {
            self.next += 1;
            if budget.nested_nullish == NestedNullishLiterals::WidenToAny
                && element
                    .as_expression()
                    .is_some_and(expr_is_widening_nullish)
            {
                continue;
            }
            match element {
                oxc_ast::ast::ArrayExpressionElement::SpreadElement(spread) => {
                    self.awaiting = Some(true);
                    return Ok(Step::Descend(Task::Declaration(
                        &spread.argument,
                        TopLevelLiteralPolicy::Widen,
                    )));
                }
                oxc_ast::ast::ArrayExpressionElement::Elision(_) => {}
                _ => {
                    if let Some(expression) = element.as_expression() {
                        self.awaiting = Some(false);
                        return Ok(Step::Descend(Task::Declaration(
                            expression,
                            TopLevelLiteralPolicy::Widen,
                        )));
                    }
                }
            }
        }
        Ok(Step::Done(Value::Type(element_union_array(
            std::mem::take(&mut self.element_types),
        ))))
    }
}

/// A spread's source contributes its element types (array / tuple /
/// union-of-those), else `any`.
fn spread_element_types(element_types: &mut Vec<TypeExpr>, source: &TypeExpr) {
    if let Some(elements) = collect_array_element_types_from_type(source) {
        element_types.extend(elements);
    } else {
        element_types.push(TypeExpr::Primitive(PrimitiveName::Any));
    }
}

/// The mutable array of the union of `element_types`, `any[]` for none.
fn element_union_array(element_types: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr::Array {
        element: Arc::new(if element_types.is_empty() {
            TypeExpr::Primitive(PrimitiveName::Any)
        } else {
            TypeExpr::union(element_types)
        }),
        readonly: false,
    }
}

/// A value-position array literal. A literal-preserving position keeps its
/// POSITIONAL structure as a tuple (`readonly` under `as const`); a spread
/// or an elision makes the positions non-recoverable, so the element-union
/// array is the sound form there.
pub(super) struct ValueArrayFrame<'a> {
    array: &'a oxc_ast::ast::ArrayExpression<'a>,
    policy: MemberLiteralPolicy,
    /// `Some(readonly)` when the literal is a tuple.
    tuple: Option<bool>,
    widen_nullish: bool,
    next: usize,
    tuple_elements: Vec<TupleElement>,
    element_types: Vec<TypeExpr>,
    /// Whether the part asked for is a spread's source.
    awaiting: Option<bool>,
}

impl<'a> ValueArrayFrame<'a> {
    fn new(
        array: &'a oxc_ast::ast::ArrayExpression<'a>,
        policy: MemberLiteralPolicy,
        budget: &InferenceBudget,
    ) -> Self {
        let positional = !array.elements.iter().any(|element| {
            matches!(
                element,
                oxc_ast::ast::ArrayExpressionElement::SpreadElement(_)
                    | oxc_ast::ast::ArrayExpressionElement::Elision(_)
            )
        });
        let tuple = policy.array_literal_is_tuple().filter(|_| positional);
        Self {
            array,
            policy,
            tuple,
            widen_nullish: budget.nested_nullish == NestedNullishLiterals::WidenToAny,
            next: 0,
            tuple_elements: Vec::new(),
            element_types: Vec::new(),
            awaiting: None,
        }
    }
}

impl<'a> Frame<'a> for ValueArrayFrame<'a> {
    fn awaiting(&self) -> bool {
        self.awaiting.is_some()
    }

    fn step(
        &mut self,
        delivered: Option<Value>,
        _source: &str,
        _budget: &mut InferenceBudget,
    ) -> InferenceResult<Step<'a>> {
        if let Some(readonly) = self.tuple {
            if let Some(delivered) = delivered {
                self.awaiting = None;
                self.tuple_elements
                    .push(tuple_element(delivered.into_type()));
            }
            while let Some(element) = self.array.elements.get(self.next) {
                self.next += 1;
                let Some(expr) = element.as_expression() else {
                    continue;
                };
                if self.widen_nullish && expr_is_widening_nullish(expr) {
                    self.tuple_elements
                        .push(tuple_element(TypeExpr::Primitive(PrimitiveName::Any)));
                    continue;
                }
                self.awaiting = Some(false);
                return Ok(Step::Descend(Task::Value(expr, self.policy)));
            }
            return Ok(Step::Done(Value::Type(TypeExpr::Tuple {
                elements: Arc::from(std::mem::take(&mut self.tuple_elements).into_boxed_slice()),
                readonly,
            })));
        }
        if let Some(delivered) = delivered {
            let ty = delivered.into_type();
            if self.awaiting.take() == Some(true) {
                spread_element_types(&mut self.element_types, &ty);
            } else {
                append_union_members(&mut self.element_types, ty);
            }
        }
        // `strictNullChecks` off: a bare nullish element adds nothing to
        // the element union beside another element, and an array of
        // nothing else widens to `any[]` exactly as an empty one does.
        while let Some(element) = self.array.elements.get(self.next) {
            self.next += 1;
            if self.widen_nullish
                && element
                    .as_expression()
                    .is_some_and(expr_is_widening_nullish)
            {
                continue;
            }
            match element {
                // The spread source always infers under the plain widen
                // context.
                oxc_ast::ast::ArrayExpressionElement::SpreadElement(spread) => {
                    self.awaiting = Some(true);
                    return Ok(Step::Descend(Task::Value(
                        &spread.argument,
                        MemberLiteralPolicy::Widen,
                    )));
                }
                oxc_ast::ast::ArrayExpressionElement::Elision(_) => {}
                _ => {
                    if let Some(expr) = element.as_expression() {
                        self.awaiting = Some(false);
                        return Ok(Step::Descend(Task::Value(expr, self.policy)));
                    }
                }
            }
        }
        Ok(Step::Done(Value::Type(element_union_array(
            std::mem::take(&mut self.element_types),
        ))))
    }
}

fn tuple_element(ty: TypeExpr) -> TupleElement {
    TupleElement {
        label: None,
        ty,
        optional: false,
        rest: false,
    }
}

/// A template with holes in a const context: the template literal type of
/// its holes' literal types, or `string` when a hole is not a literal or
/// primitive type.
pub(super) struct TemplateFrame<'a> {
    tpl: &'a oxc_ast::ast::TemplateLiteral<'a>,
    policy: MemberLiteralPolicy,
    holes: Vec<TypeExpr>,
    awaiting: bool,
}

impl<'a> TemplateFrame<'a> {
    fn new(tpl: &'a oxc_ast::ast::TemplateLiteral<'a>, policy: MemberLiteralPolicy) -> Self {
        Self {
            tpl,
            policy,
            holes: Vec::with_capacity(tpl.expressions.len()),
            awaiting: false,
        }
    }
}

impl<'a> Frame<'a> for TemplateFrame<'a> {
    fn awaiting(&self) -> bool {
        self.awaiting
    }

    fn step(
        &mut self,
        delivered: Option<Value>,
        _source: &str,
        _budget: &mut InferenceBudget,
    ) -> InferenceResult<Step<'a>> {
        if let Some(delivered) = delivered {
            self.awaiting = false;
            self.holes.push(delivered.into_type());
        }
        if let Some(hole) = self.tpl.expressions.get(self.holes.len()) {
            self.awaiting = true;
            return Ok(Step::Descend(Task::Value(hole, self.policy)));
        }
        let scalar = |ty: &TypeExpr| matches!(ty, TypeExpr::Literal(_) | TypeExpr::Primitive(_));
        if !self.holes.iter().all(|ty| match ty {
            TypeExpr::Union(members) => members.iter().all(scalar),
            other => scalar(other),
        }) {
            return Ok(Step::Done(Value::Type(TypeExpr::Primitive(
                PrimitiveName::String,
            ))));
        }
        Ok(Step::Done(Value::Type(TypeExpr::TemplateLiteral {
            quasis: self
                .tpl
                .quasis
                .iter()
                .map(|quasi| quasi.value.raw.to_string())
                .collect(),
            expressions: Arc::from(std::mem::take(&mut self.holes).into_boxed_slice()),
        })))
    }
}

/// The member an [`ObjectFrame`] waits on.
enum ObjectAwait<'a> {
    /// A method's (or accessor's) signature.
    Method {
        key: TypeAuthoredPropertyKey,
        property: &'a oxc_ast::ast::ObjectProperty<'a>,
        function: &'a Function<'a>,
    },
    /// A data member's value, `readonly` under a whole-object `as const`.
    Property {
        key: TypeAuthoredPropertyKey,
        property: &'a oxc_ast::ast::ObjectProperty<'a>,
        readonly: bool,
    },
    /// A spread's operand.
    Spread,
}

/// An object literal EXPRESSION extracted into the ordered pre-fold IR:
/// every direct member is minted `FreshOwn` and every spread rides an
/// [`ObjectMember::Spread`] entry holding the operand's inferred type — all
/// in source order. The producer NEVER folds: taint/overlap semantics are
/// the shared spread materializer's decision at graph-lowering time.
pub(super) struct ObjectFrame<'a> {
    object: &'a ObjectExpression<'a>,
    policy: MemberLiteralPolicy,
    /// Whether the literal's value is its type, rather than the extracted
    /// object itself.
    as_type: bool,
    next: usize,
    members: Vec<ObjectMember>,
    awaiting: Option<ObjectAwait<'a>>,
}

impl<'a> ObjectFrame<'a> {
    pub(super) fn new(
        object: &'a ObjectExpression<'a>,
        policy: MemberLiteralPolicy,
        as_type: bool,
        budget: &mut InferenceBudget,
    ) -> InferenceResult<Self> {
        budget.visit()?;
        Ok(Self {
            object,
            policy,
            as_type,
            next: 0,
            members: Vec::new(),
            awaiting: None,
        })
    }
}

/// A data member written directly in the literal: a fresh excess-property
/// candidate until a later spread overlaps it (the fold's decision).
fn data_member(
    key: TypeAuthoredPropertyKey,
    property: &oxc_ast::ast::ObjectProperty<'_>,
    ty: TypeExpr,
    readonly: bool,
) -> ObjectMember {
    let spans = MemberSpans {
        declaration: Some(property.span.into()),
        name: Some(property.key.span().into()),
        // Value-inferred property: there is no source type annotation to
        // anchor.
        type_annotation: None,
    };
    ObjectMember::Property(
        verter_type_expr::ObjectProperty::with_key_spans_public(key, ty, false, readonly, spans)
            .with_excess_origin(verter_type_expr::ExcessPropertyOrigin::FreshOwn),
    )
}

/// A method written directly in the literal, from its `signature`.
fn method_member(
    key: TypeAuthoredPropertyKey,
    property: &oxc_ast::ast::ObjectProperty<'_>,
    function: &Function<'_>,
    signature: LoweredSignatureParts,
) -> ObjectMember {
    let spans = MemberSpans {
        declaration: Some(property.span.into()),
        name: Some(property.key.span().into()),
        type_annotation: None,
    };
    let mut method = MethodSignature::with_key_spans_public(
        key,
        FunctionExpr::with_spans(
            signature.parameters,
            signature.return_type.map(Arc::new),
            signature.type_parameters,
            FunctionSpans {
                signature: Some(function.span.into()),
                return_type: function
                    .return_type
                    .as_ref()
                    .map(|return_type| return_type.type_annotation.span().into()),
            },
        )
        .with_predicate(signature.predicate),
        false,
        spans,
    )
    .with_excess_origin(verter_type_expr::ExcessPropertyOrigin::FreshOwn);
    method.method_kind = match property.kind {
        PropertyKind::Get => ObjectMethodKind::Get,
        PropertyKind::Set => ObjectMethodKind::Set,
        PropertyKind::Init => ObjectMethodKind::Method,
    };
    method.has_implementation_body = signature.has_implementation_body;
    ObjectMember::Method(method)
}

impl<'a> Frame<'a> for ObjectFrame<'a> {
    fn awaiting(&self) -> bool {
        self.awaiting.is_some()
    }

    fn step(
        &mut self,
        delivered: Option<Value>,
        source: &str,
        budget: &mut InferenceBudget,
    ) -> InferenceResult<Step<'a>> {
        if let Some(delivered) = delivered {
            let member = match self
                .awaiting
                .take()
                .expect("a member delivered to a request")
            {
                ObjectAwait::Method {
                    key,
                    property,
                    function,
                } => method_member(key, property, function, delivered.into_signature()),
                ObjectAwait::Property {
                    key,
                    property,
                    readonly,
                } => data_member(key, property, delivered.into_type(), readonly),
                ObjectAwait::Spread => {
                    ObjectMember::Spread(verter_type_expr::SpreadMember::new(delivered.into_type()))
                }
            };
            self.members.push(member);
        }
        while let Some(member) = self.object.properties.get(self.next) {
            self.next += 1;
            match member {
                ObjectPropertyKind::ObjectProperty(property) => {
                    let key = lower_property_key(&property.key, source);
                    if property.method || !matches!(property.kind, PropertyKind::Init) {
                        let Expression::FunctionExpression(function) = &property.value else {
                            continue;
                        };
                        self.awaiting = Some(ObjectAwait::Method {
                            key,
                            property,
                            function,
                        });
                        return Ok(Step::Descend(Task::Function(FunctionFrame::new(
                            function, budget,
                        )?)));
                    }
                    // `readonly` comes ONLY from a WHOLE-OBJECT `as const`
                    // (the enclosing policy). A per-property `as const`
                    // (`{ tag: "x" as const }`) narrows the VALUE to a
                    // literal but does NOT add the `readonly` modifier.
                    let readonly = self.policy == MemberLiteralPolicy::ConstAssert;
                    let value = &property.value;
                    // A bare nullish member value under `strictNullChecks`
                    // off widens to `any` whatever the member policy — an
                    // `as const` object keeps it `readonly` but not `null`
                    // (TypeScript 7.0.2: `{ a: null } as const` is
                    // `{ readonly a: any }`).
                    if budget.nested_nullish == NestedNullishLiterals::WidenToAny
                        && expr_is_widening_nullish(value)
                    {
                        budget.visit()?;
                        self.members.push(data_member(
                            key,
                            property,
                            TypeExpr::Primitive(PrimitiveName::Any),
                            readonly,
                        ));
                        continue;
                    }
                    // The value (and its NESTED members) is inferred under a
                    // const context when the whole object is `as const` OR
                    // this property carries its own `as const`. A fresh
                    // TOP-LEVEL literal widens only under a plain `Widen`
                    // context; `Preserve` (satisfies) and `ConstAssert` keep
                    // it.
                    let value_policy = if expr_is_const_asserted(value, source) {
                        MemberLiteralPolicy::ConstAssert
                    } else {
                        self.policy
                    };
                    self.awaiting = Some(ObjectAwait::Property {
                        key,
                        property,
                        readonly,
                    });
                    return Ok(Step::Descend(
                        if value_policy == MemberLiteralPolicy::Widen {
                            Task::Declaration(value, TopLevelLiteralPolicy::Widen)
                        } else {
                            Task::Value(value, value_policy)
                        },
                    ));
                }
                ObjectPropertyKind::SpreadProperty(spread) => {
                    self.awaiting = Some(ObjectAwait::Spread);
                    return Ok(Step::Descend(Task::Value(&spread.argument, self.policy)));
                }
            }
        }
        let object = ObjectExpr {
            properties: std::mem::take(&mut self.members),
        };
        Ok(Step::Done(if self.as_type {
            Value::Type(TypeExpr::Object(Arc::new(object)))
        } else {
            Value::Object(object)
        }))
    }
}

/// A function's signature: its parameters lowered, its return carrier the
/// AUTHORED annotation only — an unannotated function's return is
/// body-derived and names its served function position, never a body scan.
pub(super) struct FunctionFrame<'a> {
    function: &'a Function<'a>,
    awaiting: bool,
}

impl<'a> FunctionFrame<'a> {
    pub(super) fn new(
        function: &'a Function<'a>,
        budget: &mut InferenceBudget,
    ) -> InferenceResult<Self> {
        budget.visit()?;
        Ok(Self {
            function,
            awaiting: false,
        })
    }
}

impl<'a> Frame<'a> for FunctionFrame<'a> {
    fn awaiting(&self) -> bool {
        self.awaiting
    }

    fn step(
        &mut self,
        delivered: Option<Value>,
        source: &str,
        budget: &mut InferenceBudget,
    ) -> InferenceResult<Step<'a>> {
        let function = self.function;
        let Some(delivered) = delivered else {
            self.awaiting = true;
            return Ok(Step::Descend(Task::Params(ParamsFrame::new(
                &function.params,
                function.this_param.as_deref(),
                source,
                budget,
            )?)));
        };
        let (return_type, predicate) = match function.return_type.as_ref() {
            Some(return_type) => {
                let (return_type, predicate) =
                    lower_return_annotation(&return_type.type_annotation, source);
                (Some(return_type), predicate)
            }
            None => (None, None),
        };
        Ok(Step::Done(Value::Signature(LoweredSignatureParts {
            parameters: delivered.into_params(),
            return_type,
            predicate,
            type_parameters: function
                .type_parameters
                .as_ref()
                .map(|tp| lower_type_param_decls(tp, source))
                .unwrap_or_default(),
            has_implementation_body: function.body.is_some(),
            has_authored_return: function.return_type.is_some(),
            jsdoc_return: false,
            origin: LoweredSignatureOrigin::DeclBody,
        })))
    }
}

/// An arrow's signature. An AUTHORED annotation is the declared carrier; an
/// expression-bodied arrow's body IS one expression — the value inference
/// answers it directly (there is no statement scan); a block-bodied
/// arrow's return is body-derived and names its served function position.
pub(super) struct ArrowFrame<'a> {
    arrow: &'a ArrowFunctionExpression<'a>,
    /// Whether the arrow's value is its function type, rather than the
    /// extracted signature itself.
    as_type: bool,
    parameters: Option<Vec<FunctionParam>>,
    awaiting: bool,
}

impl<'a> ArrowFrame<'a> {
    pub(super) fn new(
        arrow: &'a ArrowFunctionExpression<'a>,
        as_type: bool,
        budget: &mut InferenceBudget,
    ) -> InferenceResult<Self> {
        budget.visit()?;
        Ok(Self {
            arrow,
            as_type,
            parameters: None,
            awaiting: false,
        })
    }

    /// The arrow's value, its body inferred to `body_type` (`None` for an
    /// annotated or block-bodied arrow).
    fn done(&mut self, body_type: Option<TypeExpr>, source: &str) -> Value {
        let arrow = self.arrow;
        let mut predicate = None;
        let return_type = match &arrow.return_type {
            Some(annotation) => {
                let (return_type, authored_predicate) =
                    lower_return_annotation(&annotation.type_annotation, source);
                predicate = authored_predicate;
                Some(return_type)
            }
            None => body_type,
        };
        let signature = LoweredSignatureParts {
            parameters: self.parameters.take().expect("the arrow's parameters"),
            return_type,
            predicate,
            type_parameters: arrow
                .type_parameters
                .as_ref()
                .map(|tp| lower_type_param_decls(tp, source))
                .unwrap_or_default(),
            // An arrow function always carries an implementation body
            // (expression or block form).
            has_implementation_body: true,
            has_authored_return: arrow.return_type.is_some(),
            jsdoc_return: false,
            origin: LoweredSignatureOrigin::DeclBody,
        };
        if !self.as_type {
            return Value::Signature(signature);
        }
        let spans = FunctionSpans {
            signature: Some(arrow.span.into()),
            return_type: arrow
                .return_type
                .as_ref()
                .map(|rt| rt.type_annotation.span().into()),
        };
        Value::Type(TypeExpr::Function(Arc::new(
            FunctionExpr::with_spans(
                signature.parameters,
                signature.return_type.map(Arc::new),
                signature.type_parameters,
                spans,
            )
            .with_predicate(signature.predicate),
        )))
    }
}

impl<'a> Frame<'a> for ArrowFrame<'a> {
    fn awaiting(&self) -> bool {
        self.awaiting
    }

    fn step(
        &mut self,
        delivered: Option<Value>,
        source: &str,
        budget: &mut InferenceBudget,
    ) -> InferenceResult<Step<'a>> {
        self.awaiting = false;
        let Some(delivered) = delivered else {
            self.awaiting = true;
            return Ok(Step::Descend(Task::Params(ParamsFrame::new(
                &self.arrow.params,
                None,
                source,
                budget,
            )?)));
        };
        if self.parameters.is_some() {
            // The expression body's type.
            return Ok(Step::Done(self.done(Some(delivered.into_type()), source)));
        }
        self.parameters = Some(delivered.into_params());
        if self.arrow.return_type.is_none() {
            if let Some(expression) = self.arrow.get_expression() {
                self.awaiting = true;
                return Ok(Step::Descend(Task::Value(
                    expression,
                    MemberLiteralPolicy::Widen,
                )));
            }
        }
        Ok(Step::Done(self.done(None, source)))
    }
}

/// The parameter a [`ParamsFrame`] waits on: its initializer's type
/// completes it.
struct ParamAwait<'a> {
    param: &'a oxc_ast::ast::FormalParameter<'a>,
    name: Option<String>,
}

/// A parameter list lowered: an annotated parameter's declared type, an
/// unannotated one's initializer inferred as a widening declaration, else
/// `any`.
pub(super) struct ParamsFrame<'a> {
    params: &'a FormalParameters<'a>,
    next: usize,
    lowered: Vec<FunctionParam>,
    awaiting: Option<ParamAwait<'a>>,
}

impl<'a> ParamsFrame<'a> {
    pub(super) fn new(
        params: &'a FormalParameters<'a>,
        this_param: Option<&TSThisParameter<'_>>,
        source: &str,
        budget: &mut InferenceBudget,
    ) -> InferenceResult<Self> {
        budget.visit()?;
        let mut lowered = Vec::with_capacity(
            params.items.len()
                + usize::from(params.rest.is_some())
                + usize::from(this_param.is_some()),
        );
        if let Some(this) = this_param {
            lowered.push(lower_this_param(this, source));
        }
        Ok(Self {
            params,
            next: 0,
            lowered,
            awaiting: None,
        })
    }

    fn push(
        &mut self,
        param: &oxc_ast::ast::FormalParameter<'_>,
        name: Option<String>,
        ty: TypeExpr,
    ) {
        // The OXC structural fact: did this parameter carry an explicit TS
        // type annotation? Captured here (the AST node is in hand), it is
        // the sole authority for JSDoc `@param` precedence downstream — an
        // explicit `: any` lowers to `Primitive(Any)` exactly like a missing
        // annotation, so the lowered `ty` cannot distinguish the two.
        let mut lowered = FunctionParam::with_span(
            name,
            ty,
            param.optional || param.initializer.is_some(),
            false,
            Some(param.span.into()),
            param.type_annotation.is_some(),
        );
        lowered.is_parameter_property =
            param.accessibility.is_some() || param.readonly || param.r#override;
        self.lowered.push(lowered);
    }
}

impl<'a> Frame<'a> for ParamsFrame<'a> {
    fn awaiting(&self) -> bool {
        self.awaiting.is_some()
    }

    fn step(
        &mut self,
        delivered: Option<Value>,
        source: &str,
        budget: &mut InferenceBudget,
    ) -> InferenceResult<Step<'a>> {
        if let Some(delivered) = delivered {
            let ParamAwait { param, name } = self
                .awaiting
                .take()
                .expect("a parameter delivered to a request");
            self.push(param, name, delivered.into_type());
        }
        while let Some(param) = self.params.items.get(self.next) {
            self.next += 1;
            budget.visit()?;
            let name = match &param.pattern {
                BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
                _ => None,
            };
            let ty = if let Some(annotation) = &param.type_annotation {
                lower_ts_type(&annotation.type_annotation, source)
            } else if let Some(initializer) = &param.initializer {
                self.awaiting = Some(ParamAwait { param, name });
                return Ok(Step::Descend(Task::Declaration(
                    initializer,
                    TopLevelLiteralPolicy::Widen,
                )));
            } else {
                TypeExpr::Primitive(PrimitiveName::Any)
            };
            self.push(param, name, ty);
        }
        if let Some(rest) = &self.params.rest {
            budget.visit()?;
            let name = match &rest.rest.argument {
                BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
                _ => None,
            };
            let ty = rest
                .type_annotation
                .as_ref()
                .map(|ta| lower_ts_type(&ta.type_annotation, source))
                .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
            self.lowered.push(FunctionParam::with_span(
                name,
                ty,
                false,
                true,
                Some(rest.span.into()),
                rest.type_annotation.is_some(),
            ));
        }
        Ok(Step::Done(Value::Params(std::mem::take(&mut self.lowered))))
    }
}

/// One step of widening a lowered type's literals: a type to widen, or a
/// type whose `parts` widened types are the last ones produced, to rebuild
/// from them.
enum Widening {
    Visit(TypeExpr),
    Rebuild { ty: TypeExpr, parts: usize },
}

/// `ty` with every fresh literal it holds widened to its primitive — through
/// unions, intersections, arrays, tuples, object members and function
/// results — from a work list, so a type nested any number of levels deep
/// costs no native level per level.
pub(super) fn widen_literal_type(
    ty: TypeExpr,
    budget: &mut InferenceBudget,
) -> InferenceResult<TypeExpr> {
    let mut tasks = vec![Widening::Visit(ty)];
    let mut values: Vec<TypeExpr> = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            Widening::Visit(ty) => {
                budget.visit()?;
                let Some(parts) = widening_parts(&ty, budget)? else {
                    values.push(widened_leaf(ty));
                    continue;
                };
                tasks.push(Widening::Rebuild {
                    ty,
                    parts: parts.len(),
                });
                tasks.extend(parts.into_iter().rev().map(Widening::Visit));
            }
            Widening::Rebuild { ty, parts } => {
                let widened = values.split_off(values.len() - parts);
                values.push(rebuild_widened(&ty, widened));
            }
        }
    }
    Ok(values.pop().expect("the widened type"))
}

/// A type with no parts to widen, widened: a fresh literal is its
/// primitive, anything else itself.
fn widened_leaf(ty: TypeExpr) -> TypeExpr {
    match &ty {
        TypeExpr::Literal(verter_type_expr::LiteralValue::String(_)) => {
            TypeExpr::Primitive(PrimitiveName::String)
        }
        TypeExpr::Literal(verter_type_expr::LiteralValue::Number(_)) => {
            TypeExpr::Primitive(PrimitiveName::Number)
        }
        TypeExpr::Literal(verter_type_expr::LiteralValue::Boolean(_)) => {
            TypeExpr::Primitive(PrimitiveName::Boolean)
        }
        TypeExpr::Literal(verter_type_expr::LiteralValue::BigInt(_)) => {
            TypeExpr::Primitive(PrimitiveName::BigInt)
        }
        _ => ty,
    }
}

/// The parts of `ty` whose literals widen, in the order
/// [`rebuild_widened`] reads them back; `None` for a type with no parts.
/// Each object member charges the budget as one more visit.
fn widening_parts(
    ty: &TypeExpr,
    budget: &mut InferenceBudget,
) -> InferenceResult<Option<Vec<TypeExpr>>> {
    let return_type =
        |function: &FunctionExpr| function.return_type.as_deref().cloned().into_iter();
    Ok(Some(match ty {
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            members.iter().cloned().collect()
        }
        TypeExpr::Array { element, .. } => vec![element.as_ref().clone()],
        TypeExpr::Tuple { elements, .. } => {
            elements.iter().map(|element| element.ty.clone()).collect()
        }
        TypeExpr::Object(object) => {
            let mut parts = Vec::with_capacity(object.properties.len());
            for member in &object.properties {
                budget.visit()?;
                match member {
                    ObjectMember::Property(property) => parts.push(property.ty.clone()),
                    ObjectMember::Spread(spread) => parts.push(spread.ty.clone()),
                    ObjectMember::IndexSignature(signature) => {
                        parts.push(signature.value_type.clone());
                    }
                    ObjectMember::CallSignature(function)
                    | ObjectMember::ConstructSignature(function) => {
                        parts.extend(return_type(function));
                    }
                    ObjectMember::Method(method) => parts.extend(return_type(&method.function)),
                }
            }
            parts
        }
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
            return_type(function).collect()
        }
        _ => return Ok(None),
    }))
}

/// `ty` rebuilt from its parts' `widened` types, in [`widening_parts`]'
/// order: a union deduplicated, a constructor type kept a constructor type,
/// every other axis carried over.
fn rebuild_widened(ty: &TypeExpr, widened: Vec<TypeExpr>) -> TypeExpr {
    let mut widened = widened.into_iter();
    let mut next = move || widened.next().expect("a widened part");
    match ty {
        TypeExpr::Union(members) => {
            let members: Vec<TypeExpr> = members.iter().map(|_| next()).collect();
            TypeExpr::union(dedupe_type_exprs(members))
        }
        TypeExpr::Intersection(members) => {
            TypeExpr::intersection(members.iter().map(|_| next()).collect())
        }
        TypeExpr::Array { readonly, .. } => TypeExpr::Array {
            element: Arc::new(next()),
            readonly: *readonly,
        },
        TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
            elements: Arc::from(
                elements
                    .iter()
                    .cloned()
                    .map(|mut element| {
                        element.ty = next();
                        element
                    })
                    .collect::<Vec<_>>(),
            ),
            readonly: *readonly,
        },
        TypeExpr::Object(object) => {
            let properties = object
                .properties
                .iter()
                .cloned()
                .map(|member| match member {
                    ObjectMember::Property(mut property) => {
                        property.ty = next();
                        ObjectMember::Property(property)
                    }
                    // Widening the pre-fold operand is the fold-equivalent
                    // of widening the spread-produced members: an inline
                    // literal operand's members widen; a reference operand
                    // passes through unchanged.
                    ObjectMember::Spread(mut spread) => {
                        spread.ty = next();
                        ObjectMember::Spread(spread)
                    }
                    ObjectMember::IndexSignature(mut signature) => {
                        signature.value_type = next();
                        ObjectMember::IndexSignature(signature)
                    }
                    ObjectMember::CallSignature(function) => {
                        let return_type = function.return_type.as_ref().map(|_| Arc::new(next()));
                        ObjectMember::CallSignature(
                            FunctionExpr::with_spans(
                                function.parameters,
                                return_type,
                                function.type_parameters,
                                function.spans,
                            )
                            .with_predicate(function.predicate),
                        )
                    }
                    ObjectMember::ConstructSignature(function) => {
                        let is_abstract = function.is_abstract;
                        let return_type = function.return_type.as_ref().map(|_| Arc::new(next()));
                        ObjectMember::ConstructSignature(
                            FunctionExpr::with_spans(
                                function.parameters,
                                return_type,
                                function.type_parameters,
                                function.spans,
                            )
                            .with_predicate(function.predicate)
                            .with_abstract(is_abstract),
                        )
                    }
                    ObjectMember::Method(mut method) => {
                        let return_type = method
                            .function
                            .return_type
                            .as_ref()
                            .map(|_| Arc::new(next()));
                        method.function = FunctionExpr::with_spans(
                            method.function.parameters,
                            return_type,
                            method.function.type_parameters,
                            method.function.spans,
                        )
                        .with_predicate(method.function.predicate);
                        ObjectMember::Method(method)
                    }
                })
                .collect();
            TypeExpr::Object(Arc::new(ObjectExpr { properties }))
        }
        TypeExpr::Function(function) => TypeExpr::Function(Arc::new(
            FunctionExpr::with_spans(
                function.parameters.clone(),
                function.return_type.as_ref().map(|_| Arc::new(next())),
                function.type_parameters.clone(),
                function.spans,
            )
            .with_predicate(function.predicate.clone())
            .with_abstract(function.is_abstract),
        )),
        // A bare constructor type (`new (...) => R`) carries the same
        // `FunctionExpr` payload as a function type, so its literal members
        // widen identically; it stays a constructor type.
        TypeExpr::ConstructorType(function) => TypeExpr::ConstructorType(Arc::new(
            FunctionExpr::with_spans(
                function.parameters.clone(),
                function.return_type.as_ref().map(|_| Arc::new(next())),
                function.type_parameters.clone(),
                function.spans,
            )
            .with_predicate(function.predicate.clone()),
        )),
        _ => unreachable!("only a type with parts is rebuilt"),
    }
}
