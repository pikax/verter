//! Span adjustment utilities for OXC AST nodes.
//!
//! These utilities allow adjusting spans in AST nodes after parsing
//! substrings, so the spans reflect positions in the original source.

use oxc_ast::ast::{
    Argument, ArrayExpression, ArrayExpressionElement, ArrayPattern, ArrowFunctionExpression,
    AssignmentTarget, AssignmentTargetMaybeDefault, AssignmentTargetProperty, AssignmentTargetRest,
    AwaitExpression, BinaryExpression, BindingPattern, BindingProperty, BindingRestElement,
    CallExpression, ChainElement, ChainExpression, ComputedMemberExpression, ConditionalExpression,
    ExportDefaultDeclarationKind, Expression, ForStatementInit, FormalParameter, FormalParameters,
    ImportExpression, LogicalExpression, MemberExpression, NewExpression, ObjectExpression,
    ObjectPattern, ObjectPropertyKind, ParenthesizedExpression, PrivateFieldExpression, Program,
    PropertyKey, SequenceExpression, SimpleAssignmentTarget, SpreadElement, Statement,
    StaticMemberExpression, TSAsExpression, TSNonNullExpression, TSSatisfiesExpression,
    TSTypeAssertion, TaggedTemplateExpression, TemplateLiteral, UnaryExpression, UpdateExpression,
    YieldExpression,
};
use oxc_span::{GetSpanMut, Span};

/// Adjust a span by adding an offset to both start and end.
#[inline]
pub(super) fn adjust_span(span: &mut Span, offset: u32) {
    span.start += offset;
    span.end += offset;
}

/// Normalize retained template expression spans, including nested type arguments.
pub fn adjust_expression_spans(expr: &mut Expression<'_>, offset: u32) {
    if offset != 0 {
        use oxc_ast_visit::VisitMut;
        SpanShift {
            wrapper: 0,
            file: offset,
        }
        .visit_expression(expr);
    }
}

/// One coordinate conversion for retained template expression and slot spans.
/// Synthetic wrapper boundaries map to the start of the authored window.
pub(super) struct SpanShift {
    pub(super) wrapper: u32,
    pub(super) file: u32,
}
impl<'a> oxc_ast_visit::VisitMut<'a> for SpanShift {
    fn visit_span(&mut self, span: &mut Span) {
        span.start = span.start.saturating_sub(self.wrapper) + self.file;
        span.end = span.end.saturating_sub(self.wrapper) + self.file;
    }
    fn visit_ts_type_annotation(&mut self, annotation: &mut oxc_ast::ast::TSTypeAnnotation<'a>) {
        // Annotation positions preserve their published wrapper prefix; nested
        // type references remain parse-relative for the existing extractor.
        adjust_span(&mut annotation.span, self.file);
    }
}

/// Pending nodes of the Program coordinate conversion. Each task borrows a
/// disjoint subtree; scalar span edits keep the existing policy, including
/// intentionally unshifted type interiors and expression-only function shells.
enum SpanTask<'n, 'a> {
    ProgramExpression(&'n mut Expression<'a>),
    TemplateLiteral(&'n mut TemplateLiteral<'a>),
    ArrayExpression(&'n mut ArrayExpression<'a>),
    SpreadElement(&'n mut SpreadElement<'a>),
    ObjectExpression(&'n mut ObjectExpression<'a>),
    PropertyKey(&'n mut PropertyKey<'a>),
    ParenthesizedExpression(&'n mut ParenthesizedExpression<'a>),
    SequenceExpression(&'n mut SequenceExpression<'a>),
    StaticMemberExpression(&'n mut StaticMemberExpression<'a>),
    ComputedMemberExpression(&'n mut ComputedMemberExpression<'a>),
    PrivateFieldExpression(&'n mut PrivateFieldExpression<'a>),
    CallExpression(&'n mut CallExpression<'a>),
    Argument(&'n mut Argument<'a>),
    NewExpression(&'n mut NewExpression<'a>),
    TaggedTemplateExpression(&'n mut TaggedTemplateExpression<'a>),
    UnaryExpression(&'n mut UnaryExpression<'a>),
    UpdateExpression(&'n mut UpdateExpression<'a>),
    MemberExpression(&'n mut MemberExpression<'a>),
    BinaryExpression(&'n mut BinaryExpression<'a>),
    LogicalExpression(&'n mut LogicalExpression<'a>),
    ConditionalExpression(&'n mut ConditionalExpression<'a>),
    ArrowFunctionExpression(&'n mut ArrowFunctionExpression<'a>),
    YieldExpression(&'n mut YieldExpression<'a>),
    AwaitExpression(&'n mut AwaitExpression<'a>),
    ChainExpression(&'n mut ChainExpression<'a>),
    TsAsExpression(&'n mut TSAsExpression<'a>),
    TsSatisfiesExpression(&'n mut TSSatisfiesExpression<'a>),
    TsNonNullExpression(&'n mut TSNonNullExpression<'a>),
    TsTypeAssertion(&'n mut TSTypeAssertion<'a>),
    ImportExpression(&'n mut ImportExpression<'a>),
    AssignmentTarget(&'n mut AssignmentTarget<'a>),
    AssignmentTargetRest(&'n mut AssignmentTargetRest<'a>),
    AssignmentTargetMaybeDefault(&'n mut AssignmentTargetMaybeDefault<'a>),
    AssignmentTargetProperty(&'n mut AssignmentTargetProperty<'a>),
    SimpleAssignmentTarget(&'n mut SimpleAssignmentTarget<'a>),
    FormalParameters(&'n mut FormalParameters<'a>),
    FormalParameter(&'n mut FormalParameter<'a>),
    BindingRestElement(&'n mut BindingRestElement<'a>),
    BindingPattern(&'n mut BindingPattern<'a>),
    ObjectPattern(&'n mut ObjectPattern<'a>),
    BindingProperty(&'n mut BindingProperty<'a>),
    ArrayPattern(&'n mut ArrayPattern<'a>),
    Program(&'n mut Program<'a>),
    Statement(&'n mut Statement<'a>),
    ForStatementInit(&'n mut ForStatementInit<'a>),
    ForStatementLeft(&'n mut oxc_ast::ast::ForStatementLeft<'a>),
    VariableDeclaration(&'n mut oxc_ast::ast::VariableDeclaration<'a>),
    Function(&'n mut oxc_ast::ast::Function<'a>),
    Class(&'n mut oxc_ast::ast::Class<'a>),
    Declaration(&'n mut oxc_ast::ast::Declaration<'a>),
}

fn adjust_pending_spans(task: SpanTask<'_, '_>, offset: u32) {
    if offset == 0 {
        return;
    }
    let mut stack = vec![task];
    while let Some(task) = stack.pop() {
        match task {
            SpanTask::ProgramExpression(expr) => {
                match expr {
                    Expression::Identifier(id) => {
                        let id = &mut **id;
                        adjust_span(&mut id.span, offset);
                    }
                    Expression::BooleanLiteral(lit) => {
                        let lit = &mut **lit;
                        adjust_span(&mut lit.span, offset);
                    }
                    Expression::NullLiteral(lit) => {
                        let lit = &mut **lit;
                        adjust_span(&mut lit.span, offset);
                    }
                    Expression::NumericLiteral(lit) => {
                        let lit = &mut **lit;
                        adjust_span(&mut lit.span, offset);
                    }
                    Expression::BigIntLiteral(lit) => {
                        let lit = &mut **lit;
                        adjust_span(&mut lit.span, offset);
                    }
                    Expression::StringLiteral(lit) => {
                        let lit = &mut **lit;
                        adjust_span(&mut lit.span, offset);
                    }
                    Expression::RegExpLiteral(lit) => {
                        let lit = &mut **lit;
                        adjust_span(&mut lit.span, offset);
                    }
                    Expression::TemplateLiteral(tpl) => {
                        let tpl = &mut **tpl;
                        stack.push(SpanTask::TemplateLiteral(tpl));
                    }
                    Expression::ArrayExpression(arr) => {
                        let arr = &mut **arr;
                        stack.push(SpanTask::ArrayExpression(arr));
                    }
                    Expression::ObjectExpression(obj) => {
                        let obj = &mut **obj;
                        stack.push(SpanTask::ObjectExpression(obj));
                    }
                    Expression::ParenthesizedExpression(paren) => {
                        let paren = &mut **paren;
                        stack.push(SpanTask::ParenthesizedExpression(paren));
                    }
                    Expression::SequenceExpression(seq) => {
                        let seq = &mut **seq;
                        stack.push(SpanTask::SequenceExpression(seq));
                    }
                    Expression::StaticMemberExpression(mem) => {
                        let mem = &mut **mem;
                        stack.push(SpanTask::StaticMemberExpression(mem));
                    }
                    Expression::ComputedMemberExpression(mem) => {
                        let mem = &mut **mem;
                        stack.push(SpanTask::ComputedMemberExpression(mem));
                    }
                    Expression::PrivateFieldExpression(mem) => {
                        let mem = &mut **mem;
                        stack.push(SpanTask::PrivateFieldExpression(mem));
                    }
                    Expression::CallExpression(call) => {
                        let call = &mut **call;
                        stack.push(SpanTask::CallExpression(call));
                    }
                    Expression::NewExpression(new) => {
                        let new = &mut **new;
                        stack.push(SpanTask::NewExpression(new));
                    }
                    Expression::TaggedTemplateExpression(tagged) => {
                        let tagged = &mut **tagged;
                        stack.push(SpanTask::TaggedTemplateExpression(tagged));
                    }
                    Expression::UnaryExpression(unary) => {
                        let unary = &mut **unary;
                        stack.push(SpanTask::UnaryExpression(unary));
                    }
                    Expression::UpdateExpression(update) => {
                        let update = &mut **update;
                        stack.push(SpanTask::UpdateExpression(update));
                    }
                    Expression::BinaryExpression(binary) => {
                        let binary = &mut **binary;
                        stack.push(SpanTask::BinaryExpression(binary));
                    }
                    Expression::LogicalExpression(logical) => {
                        let logical = &mut **logical;
                        stack.push(SpanTask::LogicalExpression(logical));
                    }
                    Expression::ConditionalExpression(cond) => {
                        let cond = &mut **cond;
                        stack.push(SpanTask::ConditionalExpression(cond));
                    }
                    Expression::ArrowFunctionExpression(arrow) => {
                        let arrow = &mut **arrow;
                        stack.push(SpanTask::ArrowFunctionExpression(arrow));
                    }
                    Expression::YieldExpression(yield_expr) => {
                        let yield_expr = &mut **yield_expr;
                        stack.push(SpanTask::YieldExpression(yield_expr));
                    }
                    Expression::AwaitExpression(await_expr) => {
                        let await_expr = &mut **await_expr;
                        stack.push(SpanTask::AwaitExpression(await_expr));
                    }
                    Expression::ChainExpression(chain) => {
                        let chain = &mut **chain;
                        stack.push(SpanTask::ChainExpression(chain));
                    }
                    Expression::TSAsExpression(ts_as) => {
                        let ts_as = &mut **ts_as;
                        stack.push(SpanTask::TsAsExpression(ts_as));
                    }
                    Expression::TSSatisfiesExpression(ts_sat) => {
                        let ts_sat = &mut **ts_sat;
                        stack.push(SpanTask::TsSatisfiesExpression(ts_sat));
                    }
                    Expression::TSNonNullExpression(ts_non_null) => {
                        let ts_non_null = &mut **ts_non_null;
                        stack.push(SpanTask::TsNonNullExpression(ts_non_null));
                    }
                    Expression::TSTypeAssertion(ts_assert) => {
                        let ts_assert = &mut **ts_assert;
                        stack.push(SpanTask::TsTypeAssertion(ts_assert));
                    }
                    Expression::ImportExpression(import) => {
                        let import = &mut **import;
                        stack.push(SpanTask::ImportExpression(import));
                    }
                    Expression::ThisExpression(this) => {
                        let this = &mut **this;
                        adjust_span(&mut this.span, offset);
                    }
                    Expression::Super(sup) => {
                        let sup = &mut **sup;
                        adjust_span(&mut sup.span, offset);
                    }
                    Expression::ImportMeta(meta) => {
                        let meta = &mut **meta;
                        adjust_span(&mut meta.span, offset);
                    }
                    Expression::NewTarget(meta) => {
                        let meta = &mut **meta;
                        adjust_span(&mut meta.span, offset);
                    }
                    // FunctionExpression, ClassExpression, etc. are less common in v-for/v-slot
                    // but we handle them for completeness
                    Expression::FunctionExpression(func) => {
                        let func = &mut **func;
                        adjust_span(&mut func.span, offset);
                        // Could recurse into body, params, etc. if needed
                    }
                    Expression::ClassExpression(class) => {
                        let class = &mut **class;
                        adjust_span(&mut class.span, offset);
                    }
                    Expression::AssignmentExpression(assign) => {
                        let assign = &mut **assign;
                        adjust_span(&mut assign.span, offset);
                        stack.push(SpanTask::AssignmentTarget(&mut assign.left));
                        stack.push(SpanTask::ProgramExpression(&mut assign.right));
                    }
                    Expression::JSXElement(_)
                    | Expression::JSXFragment(_)
                    | Expression::PrivateInExpression(_)
                    | Expression::TSInstantiationExpression(_)
                    | Expression::V8IntrinsicExpression(_) => {
                        // These are less common in Vue templates, skip for now
                    }
                }
            }
            SpanTask::TemplateLiteral(tpl) => {
                adjust_span(&mut tpl.span, offset);
                for quasi in &mut tpl.quasis {
                    adjust_span(&mut quasi.span, offset);
                }
                for expr in &mut tpl.expressions {
                    stack.push(SpanTask::ProgramExpression(expr));
                }
            }
            SpanTask::ArrayExpression(arr) => {
                adjust_span(&mut arr.span, offset);
                for elem in &mut arr.elements {
                    match elem {
                        ArrayExpressionElement::SpreadElement(spread) => {
                            let spread = &mut **spread;
                            stack.push(SpanTask::SpreadElement(spread));
                        }
                        ArrayExpressionElement::Elision(elision) => {
                            let elision = &mut **elision;
                            adjust_span(&mut elision.span, offset);
                        }
                        elem => {
                            if let Some(expr) = elem.as_expression_mut() {
                                stack.push(SpanTask::ProgramExpression(expr));
                            }
                        }
                    }
                }
            }
            SpanTask::SpreadElement(spread) => {
                adjust_span(&mut spread.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut spread.argument));
            }
            SpanTask::ObjectExpression(obj) => {
                adjust_span(&mut obj.span, offset);
                for prop in &mut obj.properties {
                    match prop {
                        ObjectPropertyKind::ObjectProperty(p) => {
                            let p = &mut **p;
                            adjust_span(&mut p.span, offset);
                            stack.push(SpanTask::PropertyKey(&mut p.key));
                            stack.push(SpanTask::ProgramExpression(&mut p.value));
                        }
                        ObjectPropertyKind::SpreadProperty(spread) => {
                            let spread = &mut **spread;
                            adjust_span(&mut spread.span, offset);
                            stack.push(SpanTask::ProgramExpression(&mut spread.argument));
                        }
                    }
                }
            }
            SpanTask::PropertyKey(key) => match key {
                PropertyKey::StaticIdentifier(ident) => adjust_span(&mut ident.span, offset),
                PropertyKey::PrivateIdentifier(ident) => adjust_span(&mut ident.span, offset),
                key => {
                    if let Some(key_expr) = key.as_expression_mut() {
                        stack.push(SpanTask::ProgramExpression(key_expr));
                    }
                }
            },
            SpanTask::ParenthesizedExpression(paren) => {
                adjust_span(&mut paren.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut paren.expression));
            }
            SpanTask::SequenceExpression(seq) => {
                adjust_span(&mut seq.span, offset);
                for expr in &mut seq.expressions {
                    stack.push(SpanTask::ProgramExpression(expr));
                }
            }
            SpanTask::StaticMemberExpression(mem) => {
                adjust_span(&mut mem.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut mem.object));
                adjust_span(&mut mem.property.span, offset);
            }
            SpanTask::ComputedMemberExpression(mem) => {
                adjust_span(&mut mem.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut mem.object));
                stack.push(SpanTask::ProgramExpression(&mut mem.expression));
            }
            SpanTask::PrivateFieldExpression(mem) => {
                adjust_span(&mut mem.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut mem.object));
                adjust_span(&mut mem.field.span, offset);
            }
            SpanTask::CallExpression(call) => {
                adjust_span(&mut call.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut call.callee));
                for arg in &mut call.arguments {
                    stack.push(SpanTask::Argument(arg));
                }
            }
            SpanTask::Argument(arg) => match arg {
                Argument::SpreadElement(spread) => {
                    let spread = &mut **spread;
                    stack.push(SpanTask::SpreadElement(spread));
                }
                arg => {
                    if let Some(expr) = arg.as_expression_mut() {
                        stack.push(SpanTask::ProgramExpression(expr));
                    }
                }
            },
            SpanTask::NewExpression(new) => {
                adjust_span(&mut new.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut new.callee));
                for arg in &mut new.arguments {
                    stack.push(SpanTask::Argument(arg));
                }
            }
            SpanTask::TaggedTemplateExpression(tagged) => {
                adjust_span(&mut tagged.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut tagged.tag));
                stack.push(SpanTask::TemplateLiteral(&mut tagged.quasi));
            }
            SpanTask::UnaryExpression(unary) => {
                adjust_span(&mut unary.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut unary.argument));
            }
            SpanTask::UpdateExpression(update) => {
                adjust_span(&mut update.span, offset);
                match &mut update.argument {
                    SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                        let id = &mut **id;
                        adjust_span(&mut id.span, offset);
                    }
                    SimpleAssignmentTarget::TSAsExpression(ts_as) => {
                        let ts_as = &mut **ts_as;
                        stack.push(SpanTask::TsAsExpression(ts_as));
                    }
                    SimpleAssignmentTarget::TSSatisfiesExpression(ts_sat) => {
                        let ts_sat = &mut **ts_sat;
                        stack.push(SpanTask::TsSatisfiesExpression(ts_sat));
                    }
                    SimpleAssignmentTarget::TSNonNullExpression(ts_non_null) => {
                        let ts_non_null = &mut **ts_non_null;
                        stack.push(SpanTask::TsNonNullExpression(ts_non_null));
                    }
                    SimpleAssignmentTarget::TSTypeAssertion(ts_assert) => {
                        let ts_assert = &mut **ts_assert;
                        stack.push(SpanTask::TsTypeAssertion(ts_assert));
                    }
                    argument => {
                        if let Some(mem) = argument.as_member_expression_mut() {
                            stack.push(SpanTask::MemberExpression(mem));
                        }
                    }
                }
            }
            SpanTask::MemberExpression(mem) => match mem {
                MemberExpression::ComputedMemberExpression(computed) => {
                    let computed = &mut **computed;
                    stack.push(SpanTask::ComputedMemberExpression(computed));
                }
                MemberExpression::StaticMemberExpression(static_mem) => {
                    let static_mem = &mut **static_mem;
                    stack.push(SpanTask::StaticMemberExpression(static_mem));
                }
                MemberExpression::PrivateFieldExpression(private) => {
                    let private = &mut **private;
                    stack.push(SpanTask::PrivateFieldExpression(private));
                }
            },
            SpanTask::BinaryExpression(binary) => {
                adjust_span(&mut binary.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut binary.left));
                stack.push(SpanTask::ProgramExpression(&mut binary.right));
            }
            SpanTask::LogicalExpression(logical) => {
                adjust_span(&mut logical.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut logical.left));
                stack.push(SpanTask::ProgramExpression(&mut logical.right));
            }
            SpanTask::ConditionalExpression(cond) => {
                adjust_span(&mut cond.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut cond.test));
                stack.push(SpanTask::ProgramExpression(&mut cond.consequent));
                stack.push(SpanTask::ProgramExpression(&mut cond.alternate));
            }
            SpanTask::ArrowFunctionExpression(arrow) => {
                adjust_span(&mut arrow.span, offset);
                if let Some(tp) = &mut arrow.type_parameters {
                    adjust_span(&mut tp.span, offset);
                }
                if let Some(rt) = &mut arrow.return_type {
                    adjust_span(&mut rt.span, offset);
                }
                stack.push(SpanTask::FormalParameters(&mut arrow.params));
                match &mut arrow.body {
                    oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) => {
                        let body = &mut **body;
                        // Block body: (x) => { return x + 1; }
                        adjust_span(&mut body.span, offset);
                        for stmt in &mut body.statements {
                            stack.push(SpanTask::Statement(stmt));
                        }
                    }
                    body => {
                        // Expression body: (x) => x + 1
                        if let Some(expression) = body.as_expression_mut() {
                            stack.push(SpanTask::ProgramExpression(expression));
                        }
                    }
                }
            }
            SpanTask::YieldExpression(yield_expr) => {
                adjust_span(&mut yield_expr.span, offset);
                if let Some(arg) = &mut yield_expr.argument {
                    stack.push(SpanTask::ProgramExpression(arg));
                }
            }
            SpanTask::AwaitExpression(await_expr) => {
                adjust_span(&mut await_expr.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut await_expr.argument));
            }
            SpanTask::ChainExpression(chain) => {
                adjust_span(&mut chain.span, offset);
                match &mut chain.expression {
                    ChainElement::CallExpression(call) => {
                        let call = &mut **call;
                        stack.push(SpanTask::CallExpression(call));
                    }
                    ChainElement::TSNonNullExpression(ts_non_null) => {
                        let ts_non_null = &mut **ts_non_null;
                        stack.push(SpanTask::TsNonNullExpression(ts_non_null));
                    }
                    expression => {
                        if let Some(mem) = expression.as_member_expression_mut() {
                            stack.push(SpanTask::MemberExpression(mem));
                        }
                    }
                }
            }
            SpanTask::TsAsExpression(ts_as) => {
                adjust_span(&mut ts_as.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut ts_as.expression));
                // Type annotation spans could also be adjusted if needed
            }
            SpanTask::TsSatisfiesExpression(ts_sat) => {
                adjust_span(&mut ts_sat.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut ts_sat.expression));
            }
            SpanTask::TsNonNullExpression(ts_non_null) => {
                adjust_span(&mut ts_non_null.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut ts_non_null.expression));
            }
            SpanTask::TsTypeAssertion(ts_assert) => {
                adjust_span(&mut ts_assert.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut ts_assert.expression));
            }
            SpanTask::ImportExpression(import) => {
                adjust_span(&mut import.span, offset);
                stack.push(SpanTask::ProgramExpression(&mut import.source));
                if let Some(options) = &mut import.options {
                    stack.push(SpanTask::ProgramExpression(options));
                }
            }
            SpanTask::AssignmentTarget(target) => match target {
                AssignmentTarget::ArrayAssignmentTarget(arr) => {
                    let arr = &mut **arr;
                    adjust_span(&mut arr.span, offset);
                    for elem in arr.elements.iter_mut().flatten() {
                        stack.push(SpanTask::AssignmentTargetMaybeDefault(elem));
                    }
                    if let Some(rest) = &mut arr.rest {
                        stack.push(SpanTask::AssignmentTargetRest(rest));
                    }
                }
                AssignmentTarget::ObjectAssignmentTarget(obj) => {
                    let obj = &mut **obj;
                    adjust_span(&mut obj.span, offset);
                    for prop in &mut obj.properties {
                        stack.push(SpanTask::AssignmentTargetProperty(prop));
                    }
                    if let Some(rest) = &mut obj.rest {
                        stack.push(SpanTask::AssignmentTargetRest(rest));
                    }
                }
                target => {
                    if let Some(simple) = target.as_simple_assignment_target_mut() {
                        stack.push(SpanTask::SimpleAssignmentTarget(simple));
                    }
                }
            },
            SpanTask::AssignmentTargetRest(rest) => {
                adjust_span(&mut rest.span, offset);
                stack.push(SpanTask::AssignmentTarget(&mut rest.target));
            }
            SpanTask::AssignmentTargetMaybeDefault(target) => match target {
                AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(with_default) => {
                    let with_default = &mut **with_default;
                    adjust_span(&mut with_default.span, offset);
                    stack.push(SpanTask::AssignmentTarget(&mut with_default.binding));
                    stack.push(SpanTask::ProgramExpression(&mut with_default.init));
                }
                target => {
                    if let Some(simple) = target.as_assignment_target_mut() {
                        stack.push(SpanTask::AssignmentTarget(simple));
                    }
                }
            },
            SpanTask::AssignmentTargetProperty(prop) => {
                match prop {
                    AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(id) => {
                        let id = &mut **id;
                        adjust_span(&mut id.span, offset);
                        adjust_span(&mut id.binding.span, offset);
                        if let Some(init) = &mut id.init {
                            stack.push(SpanTask::ProgramExpression(init));
                        }
                    }
                    AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                        let p = &mut **p;
                        adjust_span(&mut p.span, offset);
                        // Name might not be convertible to expression (e.g., shorthand properties)
                        if let Some(name_expr) = p.name.as_expression_mut() {
                            stack.push(SpanTask::ProgramExpression(name_expr));
                        }
                        stack.push(SpanTask::AssignmentTargetMaybeDefault(&mut p.binding));
                    }
                }
            }
            SpanTask::SimpleAssignmentTarget(target) => match target {
                SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                    let id = &mut **id;
                    adjust_span(&mut id.span, offset);
                }
                SimpleAssignmentTarget::TSAsExpression(ts_as) => {
                    let ts_as = &mut **ts_as;
                    stack.push(SpanTask::TsAsExpression(ts_as));
                }
                SimpleAssignmentTarget::TSSatisfiesExpression(ts_sat) => {
                    let ts_sat = &mut **ts_sat;
                    stack.push(SpanTask::TsSatisfiesExpression(ts_sat));
                }
                SimpleAssignmentTarget::TSNonNullExpression(ts_non_null) => {
                    let ts_non_null = &mut **ts_non_null;
                    stack.push(SpanTask::TsNonNullExpression(ts_non_null));
                }
                SimpleAssignmentTarget::TSTypeAssertion(ts_assert) => {
                    let ts_assert = &mut **ts_assert;
                    stack.push(SpanTask::TsTypeAssertion(ts_assert));
                }
                target => {
                    if let Some(mem) = target.as_member_expression_mut() {
                        stack.push(SpanTask::MemberExpression(mem));
                    }
                }
            },
            SpanTask::FormalParameters(params) => {
                adjust_span(&mut params.span, offset);
                for param in &mut params.items {
                    stack.push(SpanTask::FormalParameter(param));
                }
                if let Some(rest) = &mut params.rest {
                    adjust_span(&mut rest.span, offset);
                    if let Some(ta) = &mut rest.type_annotation {
                        adjust_span(&mut ta.span, offset);
                    }
                    stack.push(SpanTask::BindingPattern(&mut rest.rest.argument));
                }
            }
            SpanTask::FormalParameter(param) => {
                adjust_span(&mut param.span, offset);
                stack.push(SpanTask::BindingPattern(&mut param.pattern));
                if let Some(ta) = &mut param.type_annotation {
                    adjust_span(&mut ta.span, offset);
                }
                if let Some(init) = &mut param.initializer {
                    stack.push(SpanTask::ProgramExpression(init));
                }
            }
            SpanTask::BindingRestElement(rest) => {
                adjust_span(&mut rest.span, offset);
                stack.push(SpanTask::BindingPattern(&mut rest.argument));
            }
            SpanTask::BindingPattern(pattern) => {
                match pattern {
                    BindingPattern::BindingIdentifier(id) => {
                        let id = &mut **id;
                        adjust_span(&mut id.span, offset);
                    }
                    BindingPattern::ObjectPattern(obj) => {
                        let obj = &mut **obj;
                        stack.push(SpanTask::ObjectPattern(obj));
                    }
                    BindingPattern::ArrayPattern(arr) => {
                        let arr = &mut **arr;
                        stack.push(SpanTask::ArrayPattern(arr));
                    }
                    BindingPattern::AssignmentPattern(assign) => {
                        let assign = &mut **assign;
                        adjust_span(&mut assign.span, offset);
                        stack.push(SpanTask::BindingPattern(&mut assign.left));
                        stack.push(SpanTask::ProgramExpression(&mut assign.right));
                    }
                }
                // Type annotation spans could be adjusted if needed
            }
            SpanTask::ObjectPattern(obj) => {
                adjust_span(&mut obj.span, offset);
                for prop in &mut obj.properties {
                    stack.push(SpanTask::BindingProperty(prop));
                }
                if let Some(rest) = &mut obj.rest {
                    stack.push(SpanTask::BindingRestElement(rest));
                }
            }
            SpanTask::BindingProperty(prop) => {
                adjust_span(&mut prop.span, offset);
                // Key might not be convertible to expression (e.g., shorthand properties)
                if let Some(key_expr) = prop.key.as_expression_mut() {
                    stack.push(SpanTask::ProgramExpression(key_expr));
                }
                stack.push(SpanTask::BindingPattern(&mut prop.value));
            }
            SpanTask::ArrayPattern(arr) => {
                adjust_span(&mut arr.span, offset);
                for elem in arr.elements.iter_mut().flatten() {
                    stack.push(SpanTask::BindingPattern(elem));
                }
                if let Some(rest) = &mut arr.rest {
                    stack.push(SpanTask::BindingRestElement(rest));
                }
            }
            SpanTask::Program(program) => {
                adjust_span(&mut program.span, offset);
                for stmt in &mut program.body {
                    stack.push(SpanTask::Statement(stmt));
                }
            }
            SpanTask::Statement(stmt) => {
                match stmt {
                    // ---- Core statements ----
                    Statement::ExpressionStatement(s) => {
                        let s = &mut **s;
                        adjust_span(&mut s.span, offset);
                        stack.push(SpanTask::ProgramExpression(&mut s.expression));
                    }
                    Statement::BlockStatement(block) => {
                        let block = &mut **block;
                        adjust_span(&mut block.span, offset);
                        for s in &mut block.body {
                            stack.push(SpanTask::Statement(s));
                        }
                    }
                    Statement::ReturnStatement(s) => {
                        let s = &mut **s;
                        adjust_span(&mut s.span, offset);
                        if let Some(arg) = &mut s.argument {
                            stack.push(SpanTask::ProgramExpression(arg));
                        }
                    }
                    Statement::IfStatement(s) => {
                        let s = &mut **s;
                        adjust_span(&mut s.span, offset);
                        stack.push(SpanTask::ProgramExpression(&mut s.test));
                        stack.push(SpanTask::Statement(&mut s.consequent));
                        if let Some(alt) = &mut s.alternate {
                            stack.push(SpanTask::Statement(alt));
                        }
                    }
                    Statement::ForStatement(s) => {
                        let s = &mut **s;
                        adjust_span(&mut s.span, offset);
                        if let Some(init) = &mut s.init {
                            stack.push(SpanTask::ForStatementInit(init));
                        }
                        if let Some(test) = &mut s.test {
                            stack.push(SpanTask::ProgramExpression(test));
                        }
                        if let Some(update) = &mut s.update {
                            stack.push(SpanTask::ProgramExpression(update));
                        }
                        stack.push(SpanTask::Statement(&mut s.body));
                    }
                    Statement::ForInStatement(s) => {
                        let s = &mut **s;
                        adjust_span(&mut s.span, offset);
                        stack.push(SpanTask::ForStatementLeft(&mut s.left));
                        stack.push(SpanTask::ProgramExpression(&mut s.right));
                        stack.push(SpanTask::Statement(&mut s.body));
                    }
                    Statement::ForOfStatement(s) => {
                        let s = &mut **s;
                        adjust_span(&mut s.span, offset);
                        stack.push(SpanTask::ForStatementLeft(&mut s.left));
                        stack.push(SpanTask::ProgramExpression(&mut s.right));
                        stack.push(SpanTask::Statement(&mut s.body));
                    }
                    Statement::WhileStatement(s) => {
                        let s = &mut **s;
                        adjust_span(&mut s.span, offset);
                        stack.push(SpanTask::ProgramExpression(&mut s.test));
                        stack.push(SpanTask::Statement(&mut s.body));
                    }
                    Statement::DoWhileStatement(s) => {
                        let s = &mut **s;
                        adjust_span(&mut s.span, offset);
                        stack.push(SpanTask::Statement(&mut s.body));
                        stack.push(SpanTask::ProgramExpression(&mut s.test));
                    }
                    Statement::SwitchStatement(s) => {
                        let s = &mut **s;
                        adjust_span(&mut s.span, offset);
                        stack.push(SpanTask::ProgramExpression(&mut s.discriminant));
                        for case in &mut s.cases {
                            adjust_span(&mut case.span, offset);
                            if let Some(test) = &mut case.test {
                                stack.push(SpanTask::ProgramExpression(test));
                            }
                            for stmt in &mut case.consequent {
                                stack.push(SpanTask::Statement(stmt));
                            }
                        }
                    }
                    Statement::TryStatement(s) => {
                        let s = &mut **s;
                        adjust_span(&mut s.span, offset);
                        adjust_span(&mut s.block.span, offset);
                        for stmt in &mut s.block.body {
                            stack.push(SpanTask::Statement(stmt));
                        }
                        if let Some(handler) = &mut s.handler {
                            let handler = &mut **handler;
                            adjust_span(&mut handler.span, offset);
                            if let Some(param) = &mut handler.param {
                                stack.push(SpanTask::BindingPattern(&mut param.pattern));
                            }
                            adjust_span(&mut handler.body.span, offset);
                            for stmt in &mut handler.body.body {
                                stack.push(SpanTask::Statement(stmt));
                            }
                        }
                        if let Some(finalizer) = &mut s.finalizer {
                            adjust_span(&mut finalizer.span, offset);
                            for stmt in &mut finalizer.body {
                                stack.push(SpanTask::Statement(stmt));
                            }
                        }
                    }
                    Statement::ThrowStatement(s) => {
                        let s = &mut **s;
                        adjust_span(&mut s.span, offset);
                        stack.push(SpanTask::ProgramExpression(&mut s.argument));
                    }
                    Statement::LabeledStatement(s) => {
                        let s = &mut **s;
                        adjust_span(&mut s.span, offset);
                        adjust_span(&mut s.label.span, offset);
                        stack.push(SpanTask::Statement(&mut s.body));
                    }
                    Statement::BreakStatement(s) => {
                        let s = &mut **s;
                        adjust_span(&mut s.span, offset);
                        if let Some(label) = &mut s.label {
                            adjust_span(&mut label.span, offset);
                        }
                    }
                    Statement::ContinueStatement(s) => {
                        let s = &mut **s;
                        adjust_span(&mut s.span, offset);
                        if let Some(label) = &mut s.label {
                            adjust_span(&mut label.span, offset);
                        }
                    }
                    Statement::EmptyStatement(s) => {
                        let s = &mut **s;
                        adjust_span(&mut s.span, offset);
                    }
                    Statement::WithStatement(s) => {
                        let s = &mut **s;
                        adjust_span(&mut s.span, offset);
                        stack.push(SpanTask::ProgramExpression(&mut s.object));
                        stack.push(SpanTask::Statement(&mut s.body));
                    }
                    Statement::DebuggerStatement(s) => {
                        let s = &mut **s;
                        adjust_span(&mut s.span, offset);
                    }

                    // ---- Declarations ----
                    Statement::VariableDeclaration(decl) => {
                        let decl = &mut **decl;
                        stack.push(SpanTask::VariableDeclaration(decl));
                    }
                    Statement::FunctionDeclaration(func) => {
                        let func = &mut **func;
                        stack.push(SpanTask::Function(func));
                    }
                    Statement::ClassDeclaration(class) => {
                        let class = &mut **class;
                        stack.push(SpanTask::Class(class));
                    }

                    // ---- Module declarations ----
                    Statement::ImportDeclaration(decl) => {
                        let decl = &mut **decl;
                        adjust_span(&mut decl.span, offset);
                        adjust_span(&mut decl.source.span, offset);
                        if let Some(specifiers) = &mut decl.specifiers {
                            for spec in specifiers {
                                match spec {
                        oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            let s = &mut **s;
                            adjust_span(&mut s.span, offset);
                            adjust_span(s.imported.span_mut(), offset);
                            adjust_span(&mut s.local.span, offset);
                        }
                        oxc_ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                            let s = &mut **s;
                            adjust_span(&mut s.span, offset);
                            adjust_span(&mut s.local.span, offset);
                        }
                        oxc_ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                            let s = &mut **s;
                            adjust_span(&mut s.span, offset);
                            adjust_span(&mut s.local.span, offset);
                        }
                    }
                            }
                        }
                        if let Some(with_clause) = &mut decl.with_clause {
                            adjust_span(&mut with_clause.span, offset);
                        }
                    }
                    Statement::ExportAllDeclaration(decl) => {
                        let decl = &mut **decl;
                        adjust_span(&mut decl.span, offset);
                        adjust_span(&mut decl.source.span, offset);
                        if let Some(exported) = &mut decl.exported {
                            adjust_span(exported.span_mut(), offset);
                        }
                    }
                    Statement::ExportDefaultDeclaration(decl) => {
                        let decl = &mut **decl;
                        adjust_span(&mut decl.span, offset);
                        match &mut decl.declaration {
                            ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                                let func = &mut **func;
                                stack.push(SpanTask::Function(func));
                            }
                            ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                                let class = &mut **class;
                                stack.push(SpanTask::Class(class));
                            }
                            declaration => {
                                if let Some(expr) = declaration.as_expression_mut() {
                                    stack.push(SpanTask::ProgramExpression(expr));
                                }
                            }
                        }
                    }
                    Statement::ExportDeclaration(decl) => {
                        let decl = &mut **decl;
                        adjust_span(&mut decl.span, offset);
                        stack.push(SpanTask::Declaration(&mut decl.declaration));
                    }
                    Statement::ExportNamedDeclaration(decl) => {
                        let decl = &mut **decl;
                        adjust_span(&mut decl.span, offset);
                        for spec in &mut decl.specifiers {
                            adjust_span(&mut spec.span, offset);
                            adjust_span(spec.local.span_mut(), offset);
                            adjust_span(spec.exported.span_mut(), offset);
                        }
                    }
                    Statement::ExportFromDeclaration(decl) => {
                        let decl = &mut **decl;
                        adjust_span(&mut decl.span, offset);
                        for spec in &mut decl.specifiers {
                            adjust_span(&mut spec.span, offset);
                            adjust_span(spec.local.span_mut(), offset);
                            adjust_span(spec.exported.span_mut(), offset);
                        }
                        adjust_span(&mut decl.source.span, offset);
                    }

                    // ---- TypeScript declarations ----
                    Statement::TSTypeAliasDeclaration(d) => {
                        let d = &mut **d;
                        adjust_span(&mut d.span, offset);
                        adjust_span(&mut d.id.span, offset);
                    }
                    Statement::TSInterfaceDeclaration(d) => {
                        let d = &mut **d;
                        adjust_span(&mut d.span, offset);
                        adjust_span(&mut d.id.span, offset);
                    }
                    Statement::TSEnumDeclaration(d) => {
                        let d = &mut **d;
                        adjust_span(&mut d.span, offset);
                        adjust_span(&mut d.id.span, offset);
                        for member in &mut d.body.members {
                            adjust_span(&mut member.span, offset);
                            if let Some(init) = &mut member.initializer {
                                stack.push(SpanTask::ProgramExpression(init));
                            }
                        }
                    }
                    Statement::TSExternalModuleDeclaration(d) => {
                        let d = &mut **d;
                        adjust_span(&mut d.span, offset);
                    }
                    Statement::TSNamespaceDeclaration(d) => {
                        let d = &mut **d;
                        adjust_span(&mut d.span, offset);
                    }
                    Statement::TSImportEqualsDeclaration(d) => {
                        let d = &mut **d;
                        adjust_span(&mut d.span, offset);
                        adjust_span(&mut d.id.span, offset);
                    }
                    Statement::TSExportAssignment(d) => {
                        let d = &mut **d;
                        adjust_span(&mut d.span, offset);
                        stack.push(SpanTask::ProgramExpression(&mut d.expression));
                    }
                    Statement::TSNamespaceExportDeclaration(d) => {
                        let d = &mut **d;
                        adjust_span(&mut d.span, offset);
                        adjust_span(&mut d.id.span, offset);
                    }
                    Statement::TSGlobalDeclaration(d) => {
                        let d = &mut **d;
                        adjust_span(&mut d.span, offset);
                    }
                }
            }
            SpanTask::ForStatementInit(init) => match init {
                ForStatementInit::VariableDeclaration(decl) => {
                    let decl = &mut **decl;
                    stack.push(SpanTask::VariableDeclaration(decl));
                }
                init => {
                    if let Some(expr) = init.as_expression_mut() {
                        stack.push(SpanTask::ProgramExpression(expr));
                    }
                }
            },
            SpanTask::ForStatementLeft(left) => match left {
                oxc_ast::ast::ForStatementLeft::VariableDeclaration(decl) => {
                    let decl = &mut **decl;
                    stack.push(SpanTask::VariableDeclaration(decl));
                }
                left => {
                    if let Some(target) = left.as_assignment_target_mut() {
                        stack.push(SpanTask::AssignmentTarget(target));
                    }
                }
            },
            SpanTask::VariableDeclaration(decl) => {
                adjust_span(&mut decl.span, offset);
                for declarator in &mut decl.declarations {
                    adjust_span(&mut declarator.span, offset);
                    stack.push(SpanTask::BindingPattern(&mut declarator.id));
                    if let Some(ta) = &mut declarator.type_annotation {
                        adjust_span(&mut ta.span, offset);
                    }
                    if let Some(init) = &mut declarator.init {
                        stack.push(SpanTask::ProgramExpression(init));
                    }
                }
            }
            SpanTask::Function(func) => {
                adjust_span(&mut func.span, offset);
                if let Some(id) = &mut func.id {
                    adjust_span(&mut id.span, offset);
                }
                if let Some(tp) = &mut func.type_parameters {
                    adjust_span(&mut tp.span, offset);
                }
                if let Some(rt) = &mut func.return_type {
                    adjust_span(&mut rt.span, offset);
                }
                stack.push(SpanTask::FormalParameters(&mut func.params));
                if let Some(body) = &mut func.body {
                    adjust_span(&mut body.span, offset);
                    for stmt in &mut body.statements {
                        stack.push(SpanTask::Statement(stmt));
                    }
                }
            }
            SpanTask::Class(class) => {
                adjust_span(&mut class.span, offset);
                if let Some(id) = &mut class.id {
                    adjust_span(&mut id.span, offset);
                }
                if let Some(tp) = &mut class.type_parameters {
                    adjust_span(&mut tp.span, offset);
                }
                if let Some(sta) = class
                    .heritage
                    .as_mut()
                    .and_then(|h| h.type_arguments.as_mut())
                {
                    adjust_span(&mut sta.span, offset);
                }
                for impl_clause in &mut class.implements {
                    adjust_span(&mut impl_clause.span, offset);
                }
                if let Some(heritage) = &mut class.heritage {
                    stack.push(SpanTask::ProgramExpression(&mut heritage.expression));
                }
                adjust_span(&mut class.body.span, offset);
                for element in &mut class.body.body {
                    match element {
                        oxc_ast::ast::ClassElement::MethodDefinition(method) => {
                            let method = &mut **method;
                            adjust_span(&mut method.span, offset);
                            if let Some(key_expr) = method.key.as_expression_mut() {
                                stack.push(SpanTask::ProgramExpression(key_expr));
                            }
                            stack.push(SpanTask::Function(&mut method.value));
                        }
                        oxc_ast::ast::ClassElement::PropertyDefinition(prop) => {
                            let prop = &mut **prop;
                            adjust_span(&mut prop.span, offset);
                            if let Some(key_expr) = prop.key.as_expression_mut() {
                                stack.push(SpanTask::ProgramExpression(key_expr));
                            }
                            if let Some(ta) = &mut prop.type_annotation {
                                adjust_span(&mut ta.span, offset);
                            }
                            if let Some(value) = &mut prop.value {
                                stack.push(SpanTask::ProgramExpression(value));
                            }
                        }
                        oxc_ast::ast::ClassElement::StaticBlock(block) => {
                            let block = &mut **block;
                            adjust_span(&mut block.span, offset);
                            for stmt in &mut block.body {
                                stack.push(SpanTask::Statement(stmt));
                            }
                        }
                        oxc_ast::ast::ClassElement::AccessorProperty(prop) => {
                            let prop = &mut **prop;
                            adjust_span(&mut prop.span, offset);
                            if let Some(key_expr) = prop.key.as_expression_mut() {
                                stack.push(SpanTask::ProgramExpression(key_expr));
                            }
                            if let Some(ta) = &mut prop.type_annotation {
                                adjust_span(&mut ta.span, offset);
                            }
                            if let Some(value) = &mut prop.value {
                                stack.push(SpanTask::ProgramExpression(value));
                            }
                        }
                        oxc_ast::ast::ClassElement::TSIndexSignature(sig) => {
                            let sig = &mut **sig;
                            adjust_span(&mut sig.span, offset);
                        }
                    }
                }
            }
            SpanTask::Declaration(decl) => match decl {
                oxc_ast::ast::Declaration::VariableDeclaration(d) => {
                    let d = &mut **d;
                    stack.push(SpanTask::VariableDeclaration(d));
                }
                oxc_ast::ast::Declaration::FunctionDeclaration(f) => {
                    let f = &mut **f;
                    stack.push(SpanTask::Function(f));
                }
                oxc_ast::ast::Declaration::ClassDeclaration(c) => {
                    let c = &mut **c;
                    stack.push(SpanTask::Class(c));
                }
                oxc_ast::ast::Declaration::TSTypeAliasDeclaration(d) => {
                    let d = &mut **d;
                    adjust_span(&mut d.span, offset);
                    adjust_span(&mut d.id.span, offset);
                }
                oxc_ast::ast::Declaration::TSInterfaceDeclaration(d) => {
                    let d = &mut **d;
                    adjust_span(&mut d.span, offset);
                    adjust_span(&mut d.id.span, offset);
                }
                oxc_ast::ast::Declaration::TSEnumDeclaration(d) => {
                    let d = &mut **d;
                    adjust_span(&mut d.span, offset);
                    adjust_span(&mut d.id.span, offset);
                    for member in &mut d.body.members {
                        adjust_span(&mut member.span, offset);
                        if let Some(init) = &mut member.initializer {
                            stack.push(SpanTask::ProgramExpression(init));
                        }
                    }
                }
                oxc_ast::ast::Declaration::TSExternalModuleDeclaration(d) => {
                    let d = &mut **d;
                    adjust_span(&mut d.span, offset);
                }
                oxc_ast::ast::Declaration::TSNamespaceDeclaration(d) => {
                    let d = &mut **d;
                    adjust_span(&mut d.span, offset);
                }
                oxc_ast::ast::Declaration::TSGlobalDeclaration(d) => {
                    let d = &mut **d;
                    adjust_span(&mut d.span, offset);
                }
                oxc_ast::ast::Declaration::TSImportEqualsDeclaration(d) => {
                    let d = &mut **d;
                    adjust_span(&mut d.span, offset);
                    adjust_span(&mut d.id.span, offset);
                }
            },
        }
    }
}

/// Adjust formal parameters using the Program coordinate policy.
pub fn adjust_formal_parameters_spans(params: &mut FormalParameters<'_>, offset: u32) {
    adjust_pending_spans(SpanTask::FormalParameters(params), offset);
}

/// Adjust a Program while preserving its expression and type coordinate policy.
pub fn adjust_program_spans(program: &mut Program<'_>, offset: u32) {
    adjust_pending_spans(SpanTask::Program(program), offset);
}

// =============================================================================
// Diagnostic span adjustment
// =============================================================================

/// Adjust all label spans in a list of diagnostics by adding the given offset.
///
/// OXC parser errors contain `LabeledSpan`s with offsets relative to the parsed
/// substring. This function shifts them so they reference positions in the
/// original source file.
pub fn adjust_diagnostics_spans(errors: &mut [oxc_diagnostics::OxcDiagnostic], offset: u32) {
    if offset == 0 {
        return;
    }
    for diag in errors.iter_mut() {
        for label in diag.labels.iter_mut() {
            *label = oxc_diagnostics::LabeledSpan::new(
                label.label().map(|s| s.to_string()),
                label.offset() + offset,
                label.len(),
            );
        }
    }
}

#[cfg(test)]
mod coordinate_policy_tests {
    use super::*;
    use oxc_span::GetSpan;

    #[test]
    fn program_macro_type_arguments_keep_their_existing_coordinate_policy() {
        let allocator = oxc_allocator::Allocator::default();
        let source = "const props = defineProps<Props>()";
        let mut parsed =
            crate::oxc_parse::Parser::new(&allocator, source, oxc_span::SourceType::ts()).parse();
        assert!(parsed.diagnostics.is_empty());
        adjust_program_spans(&mut parsed.program, 17);
        let Statement::VariableDeclaration(declaration) = &parsed.program.body[0] else {
            panic!("variable declaration");
        };
        let Expression::CallExpression(call) = declaration.declarations[0].init.as_ref().unwrap()
        else {
            panic!("macro call");
        };
        assert_eq!(
            call.type_arguments.as_ref().unwrap().span.start,
            source.find("<Props>").unwrap() as u32
        );
        assert_eq!(
            call.callee.span().start,
            17 + source.find("defineProps").unwrap() as u32
        );
    }
}
