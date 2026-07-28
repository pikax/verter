//! Exact syntactic locations of conditional-`infer` declarations.
//!
//! This is the canonical typed child traversal used both to index authored
//! binder locations and to predeclare the binders introduced by a
//! conditional's `extends` pattern. Keeping those operations on one closed
//! `TypeExpr` match prevents identity assignment and declaration discovery
//! from drifting onto subtly different walks.

use std::sync::Arc;

use verter_type_expr::{FunctionExpr, ObjectMember, TypeExpr};

/// One typed edge in the syntax tree containing an `infer` declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum InferSyntaxPathStep {
    ParenthesizedInner,
    RestInner,
    KeyOfOperand,
    ArrayElement,
    UnionArm(u32),
    IntersectionArm(u32),
    TupleElement(u32),
    RefTypeArgument(u32),
    ImportTypeArgument(u32),
    TypeOfTypeArgument(u32),
    IndexedAccessObject,
    IndexedAccessIndex,
    ConditionalCheck,
    ConditionalExtends,
    ConditionalTrue,
    ConditionalFalse,
    MappedSource,
    MappedValue,
    MappedNameType,
    TemplateExpression(u32),
    TypeParameterConstraint,
    TypeParameterDefault,
    RecursiveRefTypeArgument(u32),
    RecursiveConditionalCheck(u32),
    RecursiveConditionalExtends(u32),
    FunctionTypeParameterConstraint(u32),
    FunctionTypeParameterDefault(u32),
    FunctionParameter(u32),
    FunctionReturn,
    ObjectProperty(u32),
    ObjectSpread(u32),
    ObjectIndexKey(u32),
    ObjectIndexValue(u32),
    ObjectMethodTypeParameterConstraint { member: u32, parameter: u32 },
    ObjectMethodTypeParameterDefault { member: u32, parameter: u32 },
    ObjectMethodParameter { member: u32, parameter: u32 },
    ObjectMethodReturn(u32),
    ObjectCallTypeParameterConstraint { member: u32, parameter: u32 },
    ObjectCallTypeParameterDefault { member: u32, parameter: u32 },
    ObjectCallParameter { member: u32, parameter: u32 },
    ObjectCallReturn(u32),
    ObjectConstructTypeParameterConstraint { member: u32, parameter: u32 },
    ObjectConstructTypeParameterDefault { member: u32, parameter: u32 },
    ObjectConstructParameter { member: u32, parameter: u32 },
    ObjectConstructReturn(u32),
}

/// Exact typed child path from one lowering root to a syntax node.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub(crate) struct InferSyntaxPath(Arc<[InferSyntaxPathStep]>);

impl InferSyntaxPath {
    #[must_use]
    pub(crate) fn root() -> Self {
        Self::default()
    }

    #[must_use]
    pub(crate) fn child(&self, step: InferSyntaxPathStep) -> Self {
        let mut path = Vec::with_capacity(self.0.len() + 1);
        path.extend_from_slice(&self.0);
        path.push(step);
        Self(Arc::from(path.into_boxed_slice()))
    }
}

/// One declaration introduced by a conditional's `extends` pattern.
#[derive(Debug, Clone)]
pub(crate) struct InferDeclarationSite {
    pub(crate) name: Arc<str>,
    pub(crate) path: InferSyntaxPath,
}

/// Visit every direct typed child of `expr`.
///
/// This is deliberately an exhaustive match with no wildcard. Adding a
/// `TypeExpr` variant therefore forces the binder-location and predeclaration
/// substrate to decide whether and how that variant can contain an `infer`.
pub(crate) fn for_each_type_expr_child<'a>(
    expr: &'a TypeExpr,
    mut visit: impl FnMut(InferSyntaxPathStep, &'a TypeExpr),
) {
    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Unknown(_) => {}
        TypeExpr::Parenthesized(inner) => visit(InferSyntaxPathStep::ParenthesizedInner, inner),
        TypeExpr::Rest(inner) => visit(InferSyntaxPathStep::RestInner, inner),
        TypeExpr::KeyOf(inner) => visit(InferSyntaxPathStep::KeyOfOperand, inner),
        TypeExpr::Array { element, .. } => visit(InferSyntaxPathStep::ArrayElement, element),
        TypeExpr::Union(arms) => {
            for (ordinal, arm) in arms.iter().enumerate() {
                visit(InferSyntaxPathStep::UnionArm(ordinal_u32(ordinal)), arm);
            }
        }
        TypeExpr::Intersection(arms) => {
            for (ordinal, arm) in arms.iter().enumerate() {
                visit(
                    InferSyntaxPathStep::IntersectionArm(ordinal_u32(ordinal)),
                    arm,
                );
            }
        }
        TypeExpr::Tuple { elements, .. } => {
            for (ordinal, element) in elements.iter().enumerate() {
                visit(
                    InferSyntaxPathStep::TupleElement(ordinal_u32(ordinal)),
                    &element.ty,
                );
            }
        }
        TypeExpr::Ref { type_arguments, .. } => {
            for (ordinal, argument) in type_arguments.iter().enumerate() {
                visit(
                    InferSyntaxPathStep::RefTypeArgument(ordinal_u32(ordinal)),
                    argument,
                );
            }
        }
        TypeExpr::ImportType { type_arguments, .. } => {
            for (ordinal, argument) in type_arguments.iter().enumerate() {
                visit(
                    InferSyntaxPathStep::ImportTypeArgument(ordinal_u32(ordinal)),
                    argument,
                );
            }
        }
        TypeExpr::TypeOf(value_ref) => {
            for (ordinal, argument) in value_ref.type_args.iter().enumerate() {
                visit(
                    InferSyntaxPathStep::TypeOfTypeArgument(ordinal_u32(ordinal)),
                    argument,
                );
            }
        }
        TypeExpr::IndexedAccess { object, index } => {
            visit(InferSyntaxPathStep::IndexedAccessObject, object);
            visit(InferSyntaxPathStep::IndexedAccessIndex, index);
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            visit(InferSyntaxPathStep::ConditionalCheck, check);
            visit(InferSyntaxPathStep::ConditionalExtends, extends);
            visit(InferSyntaxPathStep::ConditionalTrue, true_type);
            visit(InferSyntaxPathStep::ConditionalFalse, false_type);
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            visit(InferSyntaxPathStep::MappedSource, source);
            visit(InferSyntaxPathStep::MappedValue, value);
            if let Some(name_type) = name_type {
                visit(InferSyntaxPathStep::MappedNameType, name_type);
            }
        }
        TypeExpr::TemplateLiteral { expressions, .. } => {
            for (ordinal, expression) in expressions.iter().enumerate() {
                visit(
                    InferSyntaxPathStep::TemplateExpression(ordinal_u32(ordinal)),
                    expression,
                );
            }
        }
        TypeExpr::TypeParameter(parameter) => {
            if let Some(constraint) = &parameter.constraint {
                visit(InferSyntaxPathStep::TypeParameterConstraint, constraint);
            }
            if let Some(default) = &parameter.default {
                visit(InferSyntaxPathStep::TypeParameterDefault, default);
            }
        }
        TypeExpr::RecursiveRef {
            type_arguments,
            conditional_context,
            ..
        } => {
            for (ordinal, argument) in type_arguments.iter().enumerate() {
                visit(
                    InferSyntaxPathStep::RecursiveRefTypeArgument(ordinal_u32(ordinal)),
                    argument,
                );
            }
            for (ordinal, frame) in conditional_context.iter().enumerate() {
                let ordinal = ordinal_u32(ordinal);
                visit(
                    InferSyntaxPathStep::RecursiveConditionalCheck(ordinal),
                    &frame.check,
                );
                visit(
                    InferSyntaxPathStep::RecursiveConditionalExtends(ordinal),
                    &frame.extends,
                );
            }
        }
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
            visit_function_children(function, &mut visit)
        }
        TypeExpr::Object(object) => {
            for (member_ordinal, member) in object.properties.iter().enumerate() {
                let member_ordinal = ordinal_u32(member_ordinal);
                match member {
                    ObjectMember::Property(property) => visit(
                        InferSyntaxPathStep::ObjectProperty(member_ordinal),
                        &property.ty,
                    ),
                    ObjectMember::Spread(spread) => visit(
                        InferSyntaxPathStep::ObjectSpread(member_ordinal),
                        &spread.ty,
                    ),
                    ObjectMember::IndexSignature(signature) => {
                        visit(
                            InferSyntaxPathStep::ObjectIndexKey(member_ordinal),
                            &signature.key_type,
                        );
                        visit(
                            InferSyntaxPathStep::ObjectIndexValue(member_ordinal),
                            &signature.value_type,
                        );
                    }
                    ObjectMember::Method(method) => visit_object_function_children(
                        &method.function,
                        member_ordinal,
                        ObjectFunctionKind::Method,
                        &mut visit,
                    ),
                    ObjectMember::CallSignature(function) => visit_object_function_children(
                        function,
                        member_ordinal,
                        ObjectFunctionKind::Call,
                        &mut visit,
                    ),
                    ObjectMember::ConstructSignature(function) => visit_object_function_children(
                        function,
                        member_ordinal,
                        ObjectFunctionKind::Construct,
                        &mut visit,
                    ),
                }
            }
        }
    }
}

/// Build the exact pointer-to-path index for one lowering root.
pub(crate) fn index_type_expr_paths(
    root: &TypeExpr,
) -> std::collections::HashMap<usize, InferSyntaxPath> {
    let mut paths = std::collections::HashMap::new();
    index_subtree_paths(root, &InferSyntaxPath::root(), &mut paths);
    paths
}

/// Index `alias` at the exact authored path already assigned to `original`.
///
/// Lowerers use this only for syntax-preserving temporary wrappers, such as an
/// object method represented as a `TypeExpr::Function` carrier or a direct
/// object run split around a spread. The alias is traversed structurally from
/// the original path; no visit-order counter is involved.
pub(crate) fn index_alias_subtree(
    alias: &TypeExpr,
    original_path: &InferSyntaxPath,
    paths: &mut std::collections::HashMap<usize, InferSyntaxPath>,
) {
    index_subtree_paths(alias, original_path, paths);
}

fn index_subtree_paths(
    root: &TypeExpr,
    root_path: &InferSyntaxPath,
    paths: &mut std::collections::HashMap<usize, InferSyntaxPath>,
) {
    let mut pending = vec![(root, root_path.clone())];
    while let Some((expr, path)) = pending.pop() {
        paths.insert(expr as *const TypeExpr as usize, path.clone());
        for_each_type_expr_child(expr, |step, child| {
            pending.push((child, path.child(step)));
        });
    }
}

/// Discover exactly the declarations owned by one conditional `extends`
/// pattern. A nested conditional is a new lexical owner, so its subtree is
/// deliberately not visited by the enclosing collector.
pub(crate) fn collect_extends_infer_declarations(
    extends: &TypeExpr,
    extends_path: &InferSyntaxPath,
) -> Vec<InferDeclarationSite> {
    let mut declarations = Vec::new();
    let mut pending = vec![(extends, extends_path.clone())];
    while let Some((expr, path)) = pending.pop() {
        match expr {
            TypeExpr::Infer { name } => {
                if !declarations
                    .iter()
                    .any(|site: &InferDeclarationSite| site.name.as_ref() == name.as_str())
                {
                    declarations.push(InferDeclarationSite {
                        name: Arc::from(name.as_str()),
                        path,
                    });
                }
            }
            TypeExpr::Conditional { .. } => {}
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::Union(_)
            | TypeExpr::Intersection(_)
            | TypeExpr::Array { .. }
            | TypeExpr::Tuple { .. }
            | TypeExpr::Object(_)
            | TypeExpr::Function(_)
            | TypeExpr::ConstructorType(_)
            | TypeExpr::Ref { .. }
            | TypeExpr::TypeParameter(_)
            | TypeExpr::KeyOf(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::IndexedAccess { .. }
            | TypeExpr::Mapped { .. }
            | TypeExpr::TemplateLiteral { .. }
            | TypeExpr::Rest(_)
            | TypeExpr::Parenthesized(_)
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::SyntheticSlotBinding(_)
            | TypeExpr::ImportType { .. }
            | TypeExpr::Unknown(_) => for_each_type_expr_child(expr, |step, child| {
                pending.push((child, path.child(step)));
            }),
        }
    }
    declarations.reverse();
    declarations
}

fn visit_function_children<'a>(
    function: &'a FunctionExpr,
    visit: &mut impl FnMut(InferSyntaxPathStep, &'a TypeExpr),
) {
    for (ordinal, parameter) in function.type_parameters.iter().enumerate() {
        let ordinal = ordinal_u32(ordinal);
        if let Some(constraint) = &parameter.constraint {
            visit(
                InferSyntaxPathStep::FunctionTypeParameterConstraint(ordinal),
                constraint,
            );
        }
        if let Some(default) = &parameter.default {
            visit(
                InferSyntaxPathStep::FunctionTypeParameterDefault(ordinal),
                default,
            );
        }
    }
    for (ordinal, parameter) in function.parameters.iter().enumerate() {
        visit(
            InferSyntaxPathStep::FunctionParameter(ordinal_u32(ordinal)),
            &parameter.ty,
        );
    }
    if let Some(return_type) = &function.return_type {
        visit(InferSyntaxPathStep::FunctionReturn, return_type);
    }
}

#[derive(Clone, Copy)]
enum ObjectFunctionKind {
    Method,
    Call,
    Construct,
}

fn visit_object_function_children<'a>(
    function: &'a FunctionExpr,
    member: u32,
    kind: ObjectFunctionKind,
    visit: &mut impl FnMut(InferSyntaxPathStep, &'a TypeExpr),
) {
    for (parameter, type_parameter) in function.type_parameters.iter().enumerate() {
        let parameter = ordinal_u32(parameter);
        if let Some(constraint) = &type_parameter.constraint {
            let step = match kind {
                ObjectFunctionKind::Method => {
                    InferSyntaxPathStep::ObjectMethodTypeParameterConstraint { member, parameter }
                }
                ObjectFunctionKind::Call => {
                    InferSyntaxPathStep::ObjectCallTypeParameterConstraint { member, parameter }
                }
                ObjectFunctionKind::Construct => {
                    InferSyntaxPathStep::ObjectConstructTypeParameterConstraint {
                        member,
                        parameter,
                    }
                }
            };
            visit(step, constraint);
        }
        if let Some(default) = &type_parameter.default {
            let step = match kind {
                ObjectFunctionKind::Method => {
                    InferSyntaxPathStep::ObjectMethodTypeParameterDefault { member, parameter }
                }
                ObjectFunctionKind::Call => {
                    InferSyntaxPathStep::ObjectCallTypeParameterDefault { member, parameter }
                }
                ObjectFunctionKind::Construct => {
                    InferSyntaxPathStep::ObjectConstructTypeParameterDefault { member, parameter }
                }
            };
            visit(step, default);
        }
    }
    for (parameter, value_parameter) in function.parameters.iter().enumerate() {
        let parameter = ordinal_u32(parameter);
        let step = match kind {
            ObjectFunctionKind::Method => {
                InferSyntaxPathStep::ObjectMethodParameter { member, parameter }
            }
            ObjectFunctionKind::Call => {
                InferSyntaxPathStep::ObjectCallParameter { member, parameter }
            }
            ObjectFunctionKind::Construct => {
                InferSyntaxPathStep::ObjectConstructParameter { member, parameter }
            }
        };
        visit(step, &value_parameter.ty);
    }
    if let Some(return_type) = &function.return_type {
        let step = match kind {
            ObjectFunctionKind::Method => InferSyntaxPathStep::ObjectMethodReturn(member),
            ObjectFunctionKind::Call => InferSyntaxPathStep::ObjectCallReturn(member),
            ObjectFunctionKind::Construct => InferSyntaxPathStep::ObjectConstructReturn(member),
        };
        visit(step, return_type);
    }
}

fn ordinal_u32(ordinal: usize) -> u32 {
    u32::try_from(ordinal).expect("typed infer syntax ordinal exceeds u32")
}
