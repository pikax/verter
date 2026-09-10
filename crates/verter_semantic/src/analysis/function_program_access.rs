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

fn expression_target(
    mut expression: &Expression<'_>,
    mut kind: FunctionWriteKind,
) -> FunctionWriteTarget {
    let span = expression.span().into();
    loop {
        expression = match expression {
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
            Expression::TSAsExpression(inner) => &inner.expression,
            Expression::TSSatisfiesExpression(inner) => &inner.expression,
            Expression::TSNonNullExpression(inner) => &inner.expression,
            Expression::TSTypeAssertion(inner) => &inner.expression,
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
            expression_target(&member.object, FunctionWriteKind::Member)
        }
        SimpleAssignmentTarget::ComputedMemberExpression(member) => {
            expression_target(&member.object, FunctionWriteKind::Member)
        }
        SimpleAssignmentTarget::PrivateFieldExpression(member) => {
            expression_target(&member.object, FunctionWriteKind::Member)
        }
        SimpleAssignmentTarget::TSAsExpression(inner) => {
            expression_target(&inner.expression, FunctionWriteKind::Whole)
        }
        SimpleAssignmentTarget::TSSatisfiesExpression(inner) => {
            expression_target(&inner.expression, FunctionWriteKind::Whole)
        }
        SimpleAssignmentTarget::TSNonNullExpression(inner) => {
            expression_target(&inner.expression, FunctionWriteKind::Whole)
        }
        SimpleAssignmentTarget::TSTypeAssertion(inner) => {
            expression_target(&inner.expression, FunctionWriteKind::Whole)
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
