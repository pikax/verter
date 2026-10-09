//! The class-evaluation positions of a class DECLARATION that run inline in
//! the enclosing frame's flow.
//!
//! A class declaration evaluates its `extends` value and then its static
//! elements, in source order, at the statement, and the checker flows a
//! write there into every read that follows (measured on 7.0.2: after
//! `class C extends (x = true, B) { static { x = 1; } }` a read of `x` is
//! `number`). A static block completes like an inlined block: its normal
//! completions join into the statement's continuation, and when none
//! exists the code after the class is unreachable.
//!
//! The flow skeleton and the slice content both decide, through
//! [`inline_class_evaluation`], which positions of one declaration run
//! inline, so a position the skeleton records as this frame's footprint is
//! exactly a position the content half lowers. A declaration outside the
//! supported shape keeps its class subtree out of the skeleton, and the
//! content half's fail-closed effect scan answers for it.

use oxc_ast::ast::{
    ArrowFunctionExpression, CatchParameter, Class, ClassElement, Expression, Function, NewTarget,
    Statement, StaticBlock, Super, ThisExpression, VariableDeclaration,
};
use oxc_ast_visit::{walk, Visit};

/// The positions of one class declaration that run inline, in source order:
/// the `extends` value first, then each static block.
pub struct InlineClassEvaluation<'c, 'a> {
    /// The `extends` value, when it is an effect-bearing form (a sequence
    /// or an assignment) the enclosing flow applies.
    pub heritage: Option<&'c Expression<'a>>,
    /// The static blocks, in source order.
    pub static_blocks: Vec<&'c StaticBlock<'a>>,
}

/// The inline class-evaluation positions of `class`, a class DECLARATION,
/// or `None` when it has none or any of its class-evaluation positions is
/// outside the supported shape.
///
/// Supported: no decorators; an `extends` value that is either free of
/// effects or a sequence / assignment (applied inline); computed keys and
/// static initializers free of effects (a write there never retypes an
/// enclosing binding, measured on 7.0.2); and static blocks whose bodies
/// declare nothing, read no `this` / `super` / `new.target`, and create no
/// function or class (a static block keeps its own scope and receiver,
/// which the enclosing frame does not model).
pub fn inline_class_evaluation<'c, 'a>(
    class: &'c Class<'a>,
) -> Option<InlineClassEvaluation<'c, 'a>> {
    if class.declare || !class.decorators.is_empty() {
        return None;
    }
    let heritage = match class.heritage.as_ref().map(|heritage| &heritage.expression) {
        None => None,
        Some(expression) => match unwrap_parens(expression) {
            Expression::SequenceExpression(_) | Expression::AssignmentExpression(_) => {
                Some(expression)
            }
            _ if runs_no_effect(|scan| scan.visit_expression(expression)) => None,
            _ => return None,
        },
    };
    let mut static_blocks = Vec::new();
    for element in &class.body.body {
        match element {
            ClassElement::StaticBlock(block) => {
                let mut support = StaticBlockSupport::default();
                support.visit_statements(&block.body);
                if !support.supported {
                    return None;
                }
                static_blocks.push(&**block);
            }
            ClassElement::MethodDefinition(method) => {
                if !method.decorators.is_empty()
                    || (method.computed
                        && !runs_no_effect(|scan| scan.visit_property_key(&method.key)))
                {
                    return None;
                }
            }
            ClassElement::PropertyDefinition(property) => {
                if !property.decorators.is_empty()
                    || (property.computed
                        && !runs_no_effect(|scan| scan.visit_property_key(&property.key)))
                {
                    return None;
                }
                if let (true, Some(value)) = (property.r#static, property.value.as_ref()) {
                    if !runs_no_effect(|scan| scan.visit_expression(value)) {
                        return None;
                    }
                }
            }
            ClassElement::AccessorProperty(property) => {
                if !property.decorators.is_empty()
                    || (property.computed
                        && !runs_no_effect(|scan| scan.visit_property_key(&property.key)))
                {
                    return None;
                }
                if let (true, Some(value)) = (property.r#static, property.value.as_ref()) {
                    if !runs_no_effect(|scan| scan.visit_expression(value)) {
                        return None;
                    }
                }
            }
            ClassElement::TSIndexSignature(_) => {}
        }
    }
    (heritage.is_some() || !static_blocks.is_empty()).then_some(InlineClassEvaluation {
        heritage,
        static_blocks,
    })
}

/// Whether evaluating `expression` runs a call, construction or write (a
/// nested class counts: it runs its own class-evaluation positions).
pub fn expression_runs_effects(expression: &Expression<'_>) -> bool {
    !runs_no_effect(|scan| scan.visit_expression(expression))
}

fn unwrap_parens<'c, 'a>(mut expression: &'c Expression<'a>) -> &'c Expression<'a> {
    while let Expression::ParenthesizedExpression(inner) = expression {
        expression = &inner.expression;
    }
    expression
}

/// Whether the positions `visit` walks run no call, construction or write
/// at class evaluation. A function value runs nothing until called; a
/// nested class runs its own class-evaluation positions, so it counts as
/// an effect.
fn runs_no_effect(visit: impl FnOnce(&mut EffectScan)) -> bool {
    let mut scan = EffectScan::default();
    visit(&mut scan);
    !scan.effect
}

#[derive(Default)]
struct EffectScan {
    effect: bool,
}

impl<'a> Visit<'a> for EffectScan {
    fn visit_call_expression(&mut self, _it: &oxc_ast::ast::CallExpression<'a>) {
        self.effect = true;
    }
    fn visit_new_expression(&mut self, _it: &oxc_ast::ast::NewExpression<'a>) {
        self.effect = true;
    }
    fn visit_tagged_template_expression(
        &mut self,
        _it: &oxc_ast::ast::TaggedTemplateExpression<'a>,
    ) {
        self.effect = true;
    }
    fn visit_assignment_expression(&mut self, _it: &oxc_ast::ast::AssignmentExpression<'a>) {
        self.effect = true;
    }
    fn visit_update_expression(&mut self, _it: &oxc_ast::ast::UpdateExpression<'a>) {
        self.effect = true;
    }
    fn visit_await_expression(&mut self, _it: &oxc_ast::ast::AwaitExpression<'a>) {
        self.effect = true;
    }
    fn visit_yield_expression(&mut self, _it: &oxc_ast::ast::YieldExpression<'a>) {
        self.effect = true;
    }
    fn visit_import_expression(&mut self, _it: &oxc_ast::ast::ImportExpression<'a>) {
        self.effect = true;
    }
    fn visit_class(&mut self, _it: &Class<'a>) {
        self.effect = true;
    }
    fn visit_function(&mut self, _it: &Function<'a>, _flags: oxc_syntax::scope::ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _it: &ArrowFunctionExpression<'a>) {}
}

/// Whether a static block's body is one the enclosing frame models: it
/// declares no binding of its own and reads no class receiver.
struct StaticBlockSupport {
    supported: bool,
}

impl Default for StaticBlockSupport {
    fn default() -> Self {
        Self { supported: true }
    }
}

impl<'a> Visit<'a> for StaticBlockSupport {
    fn visit_statement(&mut self, it: &Statement<'a>) {
        match it {
            Statement::FunctionDeclaration(_)
            | Statement::ClassDeclaration(_)
            | Statement::ReturnStatement(_)
            | Statement::TSTypeAliasDeclaration(_)
            | Statement::TSInterfaceDeclaration(_)
            | Statement::TSEnumDeclaration(_)
            | Statement::TSNamespaceDeclaration(_)
            | Statement::TSGlobalDeclaration(_)
            | Statement::TSExternalModuleDeclaration(_)
            | Statement::TSImportEqualsDeclaration(_)
            | Statement::WithStatement(_) => self.supported = false,
            _ => walk::walk_statement(self, it),
        }
    }
    fn visit_variable_declaration(&mut self, _it: &VariableDeclaration<'a>) {
        self.supported = false;
    }
    fn visit_catch_parameter(&mut self, _it: &CatchParameter<'a>) {
        self.supported = false;
    }
    fn visit_this_expression(&mut self, _it: &ThisExpression) {
        self.supported = false;
    }
    fn visit_super(&mut self, _it: &Super) {
        self.supported = false;
    }
    fn visit_new_target(&mut self, _it: &NewTarget) {
        self.supported = false;
    }
    fn visit_class(&mut self, _it: &Class<'a>) {
        self.supported = false;
    }
    fn visit_function(&mut self, _it: &Function<'a>, _flags: oxc_syntax::scope::ScopeFlags) {
        self.supported = false;
    }
    fn visit_arrow_function_expression(&mut self, _it: &ArrowFunctionExpression<'a>) {
        self.supported = false;
    }
}
