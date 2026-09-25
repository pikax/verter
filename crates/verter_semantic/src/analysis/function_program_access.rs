//! Authored access roots for the retained function inventory.

use oxc_ast::ast::{
    AssignmentTarget, AssignmentTargetMaybeDefault, AssignmentTargetProperty, Expression,
    IdentifierReference, SimpleAssignmentTarget,
};
use oxc_span::GetSpan;
use std::sync::Arc;

use super::{
    FunctionReadRole, FunctionReferenceBinding, FunctionReferenceRecord, FunctionWriteKind,
    FunctionWriteTarget,
};

fn named(identifier: &IdentifierReference<'_>, kind: FunctionWriteKind) -> FunctionWriteTarget {
    FunctionWriteTarget::Binding {
        reference: FunctionReferenceRecord {
            name: Arc::from(identifier.name.as_str()),
            span: identifier.span.into(),
            binding: FunctionReferenceBinding::Free,
            read_role: None,
            path: Arc::from([]),
        },
        kind,
    }
}

pub(crate) fn static_member_reference(
    member: &oxc_ast::ast::StaticMemberExpression<'_>,
) -> Option<FunctionReferenceRecord> {
    let mut path = Vec::new();
    let mut current = member;
    loop {
        path.push(Arc::from(current.property.name.as_str()));
        match &current.object {
            Expression::Identifier(identifier) => {
                path.reverse();
                return Some(FunctionReferenceRecord {
                    name: Arc::from(identifier.name.as_str()),
                    span: identifier.span.into(),
                    binding: FunctionReferenceBinding::Free,
                    read_role: Some(FunctionReadRole::Value),
                    path: path.into(),
                });
            }
            Expression::StaticMemberExpression(parent) => current = parent,
            _ => return None,
        }
    }
}

/// The root a write through `expression` reaches. A type assertion between
/// the root and the written position (`(x as T) = v`) makes a whole write
/// [`FunctionWriteTarget::Asserted`]; parentheses and a non-null assertion
/// (`x! = v`) keep it an assignment of the root, as the checker's
/// `getAssignmentTargetKind` walks through exactly those two.
fn expression_target(
    mut expression: &Expression<'_>,
    mut kind: FunctionWriteKind,
    mut asserted: bool,
) -> FunctionWriteTarget {
    let span = expression.span().into();
    loop {
        expression = match expression {
            Expression::Identifier(_) if asserted && kind == FunctionWriteKind::Whole => {
                return FunctionWriteTarget::Asserted { span }
            }
            Expression::Identifier(identifier) => return named(identifier, kind),
            Expression::StaticMemberExpression(member) => {
                kind = FunctionWriteKind::Member;
                &member.object
            }
            Expression::ComputedMemberExpression(member) => {
                kind = FunctionWriteKind::Member;
                &member.object
            }
            Expression::PrivateFieldExpression(member) => {
                kind = FunctionWriteKind::Member;
                &member.object
            }
            Expression::ParenthesizedExpression(inner) => &inner.expression,
            Expression::TSAsExpression(inner) => {
                asserted = true;
                &inner.expression
            }
            Expression::TSSatisfiesExpression(inner) => {
                asserted = true;
                &inner.expression
            }
            Expression::TSTypeAssertion(inner) => {
                asserted = true;
                &inner.expression
            }
            Expression::TSNonNullExpression(inner) => &inner.expression,
            Expression::TSInstantiationExpression(inner) => &inner.expression,
            _ => return FunctionWriteTarget::Unsupported { span },
        };
    }
}

pub(crate) fn expression_root<'a, 'ast>(
    mut expression: &'a Expression<'ast>,
) -> Option<&'a IdentifierReference<'ast>> {
    loop {
        expression = match expression {
            Expression::Identifier(identifier) => return Some(identifier),
            Expression::StaticMemberExpression(member) => &member.object,
            Expression::ComputedMemberExpression(member) => &member.object,
            Expression::PrivateFieldExpression(member) => &member.object,
            Expression::ParenthesizedExpression(inner) => &inner.expression,
            Expression::TSAsExpression(inner) => &inner.expression,
            Expression::TSSatisfiesExpression(inner) => &inner.expression,
            Expression::TSNonNullExpression(inner) => &inner.expression,
            Expression::TSTypeAssertion(inner) => &inner.expression,
            Expression::TSInstantiationExpression(inner) => &inner.expression,
            _ => return None,
        };
    }
}

pub(super) fn simple_assignment_target(target: &SimpleAssignmentTarget<'_>) -> FunctionWriteTarget {
    match target {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) => {
            named(identifier, FunctionWriteKind::Whole)
        }
        SimpleAssignmentTarget::StaticMemberExpression(member) => {
            expression_target(&member.object, FunctionWriteKind::Member, false)
        }
        SimpleAssignmentTarget::ComputedMemberExpression(member) => {
            expression_target(&member.object, FunctionWriteKind::Member, false)
        }
        SimpleAssignmentTarget::PrivateFieldExpression(member) => {
            expression_target(&member.object, FunctionWriteKind::Member, false)
        }
        SimpleAssignmentTarget::TSAsExpression(inner) => {
            expression_target(&inner.expression, FunctionWriteKind::Whole, true)
        }
        SimpleAssignmentTarget::TSSatisfiesExpression(inner) => {
            expression_target(&inner.expression, FunctionWriteKind::Whole, true)
        }
        SimpleAssignmentTarget::TSNonNullExpression(inner) => {
            expression_target(&inner.expression, FunctionWriteKind::Whole, false)
        }
        SimpleAssignmentTarget::TSTypeAssertion(inner) => {
            expression_target(&inner.expression, FunctionWriteKind::Whole, true)
        }
    }
}

pub(super) fn assignment_targets(target: &AssignmentTarget<'_>) -> Vec<FunctionWriteTarget> {
    let mut targets = Vec::new();
    collect_targets(target, &mut targets);
    targets
}

fn collect_default_target(
    target: &AssignmentTargetMaybeDefault<'_>,
    out: &mut Vec<FunctionWriteTarget>,
) {
    match target {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) => {
            collect_targets(&default.binding, out)
        }
        _ => collect_targets(target.to_assignment_target(), out),
    }
}

fn collect_targets(target: &AssignmentTarget<'_>, out: &mut Vec<FunctionWriteTarget>) {
    match target {
        AssignmentTarget::ArrayAssignmentTarget(array) => {
            for element in array.elements.iter().flatten() {
                collect_default_target(element, out);
            }
            if let Some(rest) = &array.rest {
                collect_targets(&rest.target, out);
            }
        }
        AssignmentTarget::ObjectAssignmentTarget(object) => {
            for property in &object.properties {
                match property {
                    AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(identifier) => {
                        out.push(named(&identifier.binding, FunctionWriteKind::Whole))
                    }
                    AssignmentTargetProperty::AssignmentTargetPropertyProperty(property) => {
                        collect_default_target(&property.binding, out)
                    }
                }
            }
            if let Some(rest) = &object.rest {
                collect_targets(&rest.target, out);
            }
        }
        _ => out.push(simple_assignment_target(
            target.to_simple_assignment_target(),
        )),
    }
}

/// The whole-binding assignments code no index entry serves (a local class
/// declaration's members, a class's instance initializers, a callable in a
/// parameter list) makes to names it does not itself declare: the
/// assignments that reach the enclosing frames, which the checker's
/// `isSymbolAssigned` counts wherever in the declaring function they
/// occur. Names resolve by the lexical scopes of the walked code alone
/// (parameters, `var` declarations hoisted to their function, block-scoped
/// declarations, catch parameters, nested function and class names); an
/// assignment none of them declares escapes, reported by name and span for
/// the enclosing frame's resolution.
pub(crate) struct EscapingAssignments {
    scopes: Vec<EscapeScope>,
    stack: Vec<usize>,
    writes: Vec<(FunctionReferenceRecord, usize)>,
    /// The span of a declaration name the walk binds itself, in the scope
    /// the checker binds it in, so the generic binding visit skips it.
    bound_elsewhere: Option<oxc_span::Span>,
    /// Set while a `var` declaration's names are visited: they bind in the
    /// nearest function scope.
    var_declaration: bool,
    /// A class expression's name, bound in the class scope the walk
    /// enters next.
    class_expression_name: Option<Arc<str>>,
}

struct EscapeScope {
    parent: Option<usize>,
    function: bool,
    names: Vec<Arc<str>>,
}

impl Default for EscapingAssignments {
    fn default() -> Self {
        Self {
            scopes: vec![EscapeScope {
                parent: None,
                function: true,
                names: Vec::new(),
            }],
            stack: vec![0],
            writes: Vec::new(),
            bound_elsewhere: None,
            var_declaration: false,
            class_expression_name: None,
        }
    }
}

impl EscapingAssignments {
    /// The assignments that escape every scope the walk declares.
    pub(crate) fn into_escaping(self) -> Vec<FunctionReferenceRecord> {
        let Self { scopes, writes, .. } = self;
        writes
            .into_iter()
            .filter(|(reference, scope)| {
                let mut current = Some(*scope);
                while let Some(index) = current {
                    if scopes[index].names.contains(&reference.name) {
                        return false;
                    }
                    current = scopes[index].parent;
                }
                true
            })
            .map(|(reference, _)| reference)
            .collect()
    }

    fn current(&self) -> usize {
        *self
            .stack
            .last()
            .expect("the outermost scope is never left")
    }

    fn declare(&mut self, name: &str, var: bool) {
        let mut scope = self.current();
        if var {
            while !self.scopes[scope].function {
                scope = self.scopes[scope]
                    .parent
                    .expect("the outermost scope is a function scope");
            }
        }
        self.scopes[scope].names.push(Arc::from(name));
    }

    fn record(&mut self, targets: Vec<FunctionWriteTarget>) {
        let scope = self.current();
        for target in targets {
            if let FunctionWriteTarget::Binding {
                reference,
                kind: FunctionWriteKind::Whole,
            } = target
            {
                self.writes.push((reference, scope));
            }
        }
    }
}

impl<'a> oxc_ast_visit::Visit<'a> for EscapingAssignments {
    fn enter_scope(
        &mut self,
        flags: oxc_syntax::scope::ScopeFlags,
        _scope_id: &std::cell::Cell<Option<oxc_syntax::scope::ScopeId>>,
    ) {
        let parent = self.current();
        self.scopes.push(EscapeScope {
            parent: Some(parent),
            function: flags.is_function() || flags.is_class_static_block(),
            names: self.class_expression_name.take().into_iter().collect(),
        });
        self.stack.push(self.scopes.len() - 1);
    }

    fn leave_scope(&mut self) {
        self.stack.pop();
    }

    fn visit_ts_type(&mut self, _it: &oxc_ast::ast::TSType<'a>) {}

    fn visit_ts_type_annotation(&mut self, _it: &oxc_ast::ast::TSTypeAnnotation<'a>) {}

    fn visit_ts_type_parameter_declaration(
        &mut self,
        _it: &oxc_ast::ast::TSTypeParameterDeclaration<'a>,
    ) {
    }

    fn visit_ts_this_parameter(&mut self, _it: &oxc_ast::ast::TSThisParameter<'a>) {}

    fn visit_binding_identifier(&mut self, it: &oxc_ast::ast::BindingIdentifier<'a>) {
        if self.bound_elsewhere == Some(it.span) {
            return;
        }
        self.declare(it.name.as_str(), self.var_declaration);
    }

    fn visit_function(
        &mut self,
        it: &oxc_ast::ast::Function<'a>,
        flags: oxc_syntax::scope::ScopeFlags,
    ) {
        // A declaration's name binds in the enclosing scope, an
        // expression's in its own.
        let previous = self.bound_elsewhere;
        if let (true, Some(id)) = (it.is_declaration(), &it.id) {
            self.declare(id.name.as_str(), false);
            self.bound_elsewhere = Some(id.span);
        }
        oxc_ast_visit::walk::walk_function(self, it, flags);
        self.bound_elsewhere = previous;
    }

    fn visit_class(&mut self, it: &oxc_ast::ast::Class<'a>) {
        // A declaration's name binds in the enclosing scope, an
        // expression's inside the class.
        let previous = self.bound_elsewhere;
        if let Some(id) = &it.id {
            if it.r#type == oxc_ast::ast::ClassType::ClassDeclaration {
                self.declare(id.name.as_str(), false);
            } else {
                self.class_expression_name = Some(Arc::from(id.name.as_str()));
            }
            self.bound_elsewhere = Some(id.span);
        }
        oxc_ast_visit::walk::walk_class(self, it);
        self.bound_elsewhere = previous;
    }

    fn visit_variable_declaration(&mut self, it: &oxc_ast::ast::VariableDeclaration<'a>) {
        for declarator in &it.declarations {
            let previous = self.var_declaration;
            self.var_declaration = it.kind == oxc_ast::ast::VariableDeclarationKind::Var;
            self.visit_binding_pattern(&declarator.id);
            self.var_declaration = previous;
            if let Some(init) = &declarator.init {
                self.visit_expression(init);
            }
        }
    }

    fn visit_assignment_expression(&mut self, it: &oxc_ast::ast::AssignmentExpression<'a>) {
        self.record(assignment_targets(&it.left));
        oxc_ast_visit::walk::walk_assignment_expression(self, it);
    }

    fn visit_update_expression(&mut self, it: &oxc_ast::ast::UpdateExpression<'a>) {
        self.record(vec![simple_assignment_target(&it.argument)]);
        oxc_ast_visit::walk::walk_update_expression(self, it);
    }

    fn visit_for_in_statement(&mut self, it: &oxc_ast::ast::ForInStatement<'a>) {
        if let Some(target) = it.left.as_assignment_target() {
            self.record(assignment_targets(target));
        }
        oxc_ast_visit::walk::walk_for_in_statement(self, it);
    }

    fn visit_for_of_statement(&mut self, it: &oxc_ast::ast::ForOfStatement<'a>) {
        if let Some(target) = it.left.as_assignment_target() {
            self.record(assignment_targets(target));
        }
        oxc_ast_visit::walk::walk_for_of_statement(self, it);
    }
}
