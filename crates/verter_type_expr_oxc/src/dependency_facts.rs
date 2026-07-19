//! OXC-authored type dependency facts.
//!
//! This module lowers structural paths and their syntax roles directly from
//! borrowed OXC nodes. It performs no binding lookup, module resolution, or
//! declaration ownership assignment.

use std::collections::BTreeSet;

use oxc_ast::ast::*;
use verter_type_expr::facts::TypeDependencyPathFact;

/// Syntax roles emitted while walking one authored type carrier.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypeDependencyFacts {
    pub dependency_paths: BTreeSet<TypeDependencyPathFact>,
    pub structural_dependency_paths: BTreeSet<TypeDependencyPathFact>,
    pub declaration_carrier_paths: BTreeSet<TypeDependencyPathFact>,
    pub value_query_paths: BTreeSet<TypeDependencyPathFact>,
    pub value_position_paths: BTreeSet<TypeDependencyPathFact>,
    pub unsupported_value_positions: BTreeSet<UnsupportedValuePositionKind>,
}

/// Authored runtime-value positions that cannot be represented by a structural
/// dependency path.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum UnsupportedValuePositionKind {
    ClassHeritageExpression,
    ComputedSignatureKey,
    ComputedClassKey,
}

/// Collect every declaration-carrier path in one authored type expression.
#[must_use]
pub fn collect_type_dependency_paths(ts_type: &TSType<'_>) -> BTreeSet<TypeDependencyPathFact> {
    collect_type_dependency_facts(ts_type).declaration_carrier_paths
}

/// Collect syntax-role facts for one authored type expression.
#[must_use]
pub fn collect_type_dependency_facts(ts_type: &TSType<'_>) -> TypeDependencyFacts {
    let mut out = TypeDependencyFacts::default();
    collector_for(&mut out).visit_type(ts_type, StructuralDependencyContext::Root);
    out
}

/// Collect syntax-role facts for one type-alias declaration.
#[must_use]
pub fn collect_type_alias_dependency_facts(
    declaration: &TSTypeAliasDeclaration<'_>,
) -> TypeDependencyFacts {
    let mut out = TypeDependencyFacts::default();
    let mut collector = collector_for(&mut out);
    collector.visit_type(
        &declaration.type_annotation,
        StructuralDependencyContext::Root,
    );
    if let Some(parameters) = &declaration.type_parameters {
        collector.visit_type_parameters_for_carrier(parameters);
    }
    out
}

/// Collect syntax-role facts for one interface declaration.
#[must_use]
pub fn collect_interface_dependency_facts(
    declaration: &TSInterfaceDeclaration<'_>,
) -> TypeDependencyFacts {
    let mut out = TypeDependencyFacts::default();
    let mut collector = collector_for(&mut out);
    collector.visit_interface(&declaration.body.body, &declaration.extends);
    if let Some(parameters) = &declaration.type_parameters {
        collector.visit_type_parameters_for_carrier(parameters);
    }
    out
}

/// Collect syntax-role facts for one class declaration.
#[must_use]
pub fn collect_class_dependency_facts(declaration: &Class<'_>) -> TypeDependencyFacts {
    let mut out = TypeDependencyFacts::default();
    collector_for(&mut out).visit_class(declaration);
    out
}

fn collector_for(facts: &mut TypeDependencyFacts) -> TypeDependencyCollector<'_> {
    TypeDependencyCollector::new(
        &mut facts.dependency_paths,
        &mut facts.structural_dependency_paths,
        &mut facts.declaration_carrier_paths,
        &mut facts.value_query_paths,
        &mut facts.value_position_paths,
        &mut facts.unsupported_value_positions,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StructuralDependencyContext {
    Root,
    CallableParam,
    LeafProperty,
    CarrierOnly,
}

fn dependency_path_from_type_name(name: &TSTypeName<'_>) -> Option<TypeDependencyPathFact> {
    fn append(name: &TSTypeName<'_>, segments: &mut Vec<String>) -> bool {
        match name {
            TSTypeName::IdentifierReference(identifier) => {
                segments.push(identifier.name.to_string());
                true
            }
            TSTypeName::QualifiedName(qualified) => {
                if !append(&qualified.left, segments) {
                    return false;
                }
                segments.push(qualified.right.name.to_string());
                true
            }
            TSTypeName::ThisExpression(_) => false,
        }
    }
    let mut segments = Vec::new();
    append(name, &mut segments)
        .then(|| TypeDependencyPathFact::from_segments(segments))
        .flatten()
}

fn dependency_path_from_query_name(
    name: &TSTypeQueryExprName<'_>,
) -> Option<TypeDependencyPathFact> {
    match name {
        TSTypeQueryExprName::IdentifierReference(identifier) => {
            TypeDependencyPathFact::from_segments([identifier.name.to_string()])
        }
        TSTypeQueryExprName::QualifiedName(qualified) => {
            let base = dependency_path_from_type_name(&qualified.left)?;
            let mut segments = base.segments().to_vec();
            segments.push(qualified.right.name.to_string());
            TypeDependencyPathFact::from_segments(segments)
        }
        TSTypeQueryExprName::ThisExpression(_) | TSTypeQueryExprName::TSImportType(_) => None,
    }
}

fn dependency_path_from_expression(expression: &Expression<'_>) -> Option<TypeDependencyPathFact> {
    fn append(expression: &Expression<'_>, segments: &mut Vec<String>) -> bool {
        match expression {
            Expression::Identifier(identifier) => {
                segments.push(identifier.name.to_string());
                true
            }
            Expression::StaticMemberExpression(member) => {
                if !append(&member.object, segments) {
                    return false;
                }
                segments.push(member.property.name.to_string());
                true
            }
            _ => false,
        }
    }
    let mut segments = Vec::new();
    append(expression, &mut segments)
        .then(|| TypeDependencyPathFact::from_segments(segments))
        .flatten()
}

fn dependency_path_from_property_key(key: &PropertyKey<'_>) -> Option<TypeDependencyPathFact> {
    match key {
        PropertyKey::Identifier(identifier) => {
            TypeDependencyPathFact::from_segments([identifier.name.to_string()])
        }
        PropertyKey::StaticIdentifier(identifier) => {
            TypeDependencyPathFact::from_segments([identifier.name.to_string()])
        }
        PropertyKey::StaticMemberExpression(member) => {
            let base = dependency_path_from_expression(&member.object)?;
            let mut segments = base.segments().to_vec();
            segments.push(member.property.name.to_string());
            TypeDependencyPathFact::from_segments(segments)
        }
        _ => None,
    }
}

struct TypeDependencyCollector<'a> {
    full: &'a mut BTreeSet<TypeDependencyPathFact>,
    structural: &'a mut BTreeSet<TypeDependencyPathFact>,
    declaration_carrier: &'a mut BTreeSet<TypeDependencyPathFact>,
    value_queries: &'a mut BTreeSet<TypeDependencyPathFact>,
    value_positions: &'a mut BTreeSet<TypeDependencyPathFact>,
    unsupported_value_positions: &'a mut BTreeSet<UnsupportedValuePositionKind>,
}

impl<'a> TypeDependencyCollector<'a> {
    fn new(
        full: &'a mut BTreeSet<TypeDependencyPathFact>,
        structural: &'a mut BTreeSet<TypeDependencyPathFact>,
        declaration_carrier: &'a mut BTreeSet<TypeDependencyPathFact>,
        value_queries: &'a mut BTreeSet<TypeDependencyPathFact>,
        value_positions: &'a mut BTreeSet<TypeDependencyPathFact>,
        unsupported_value_positions: &'a mut BTreeSet<UnsupportedValuePositionKind>,
    ) -> Self {
        Self {
            full,
            structural,
            declaration_carrier,
            value_queries,
            value_positions,
            unsupported_value_positions,
        }
    }

    fn record(
        &mut self,
        fact: Option<TypeDependencyPathFact>,
        context: StructuralDependencyContext,
    ) {
        let Some(fact) = fact else {
            return;
        };
        self.declaration_carrier.insert(fact.clone());
        if context != StructuralDependencyContext::CarrierOnly {
            self.full.insert(fact.clone());
        }
        if matches!(
            context,
            StructuralDependencyContext::Root | StructuralDependencyContext::CallableParam
        ) {
            self.structural.insert(fact);
        }
    }

    fn record_expression_path(
        &mut self,
        expression: &Expression<'_>,
        context: StructuralDependencyContext,
    ) {
        let fact = dependency_path_from_expression(expression);
        let path_context = match expression {
            Expression::Identifier(_) => context,
            _ => StructuralDependencyContext::CarrierOnly,
        };
        self.record(fact, path_context);
    }

    fn visit_type_parameters_for_carrier(&mut self, parameters: &TSTypeParameterDeclaration<'_>) {
        for parameter in &parameters.params {
            if let Some(constraint) = &parameter.constraint {
                self.visit_type(constraint, StructuralDependencyContext::CarrierOnly);
            }
            if let Some(default) = &parameter.default {
                self.visit_type(default, StructuralDependencyContext::CarrierOnly);
            }
        }
    }

    fn visit_computed_key(
        &mut self,
        key: &PropertyKey<'_>,
        computed: bool,
        unsupported: UnsupportedValuePositionKind,
    ) {
        if !computed {
            return;
        }
        let path = dependency_path_from_property_key(key);
        if let Some(path) = &path {
            self.value_positions.insert(path.clone());
        } else {
            self.unsupported_value_positions.insert(unsupported);
        }
        self.record(path, StructuralDependencyContext::CarrierOnly);
    }

    fn visit_initializer_value_carrier(&mut self, initializer: Option<&Expression<'_>>) {
        let Some(path) = initializer.and_then(dependency_path_from_expression) else {
            return;
        };
        self.value_positions.insert(path.clone());
        self.record(Some(path), StructuralDependencyContext::CarrierOnly);
    }

    fn visit_parameters(
        &mut self,
        parameters: &FormalParameters<'_>,
        context: StructuralDependencyContext,
    ) {
        // Component-meta only needs callable parameter surfaces for
        // props/emits/slots. Return-only imports remain carrier-only.
        for parameter in &parameters.items {
            if let Some(annotation) = &parameter.type_annotation {
                self.visit_type(&annotation.type_annotation, context);
            }
        }
        if let Some(rest) = &parameters.rest {
            if let Some(annotation) = &rest.type_annotation {
                self.visit_type(
                    &annotation.type_annotation,
                    StructuralDependencyContext::CarrierOnly,
                );
            }
        }
    }

    fn visit_this_parameter(&mut self, parameter: Option<&TSThisParameter<'_>>) {
        if let Some(annotation) = parameter.and_then(|parameter| parameter.type_annotation.as_ref())
        {
            self.visit_type(
                &annotation.type_annotation,
                StructuralDependencyContext::CarrierOnly,
            );
        }
    }

    fn visit_index_parameters(&mut self, parameters: &[TSIndexSignatureName<'_>]) {
        for parameter in parameters {
            self.visit_type(
                &parameter.type_annotation.type_annotation,
                StructuralDependencyContext::CarrierOnly,
            );
        }
    }

    fn visit_signatures(&mut self, members: &[TSSignature<'_>], carrier_only: bool) {
        let leaf_context = if carrier_only {
            StructuralDependencyContext::CarrierOnly
        } else {
            StructuralDependencyContext::LeafProperty
        };
        let callable_context = if carrier_only {
            StructuralDependencyContext::CarrierOnly
        } else {
            StructuralDependencyContext::CallableParam
        };
        for member in members {
            match member {
                TSSignature::TSPropertySignature(property) => {
                    self.visit_computed_key(
                        &property.key,
                        property.computed,
                        UnsupportedValuePositionKind::ComputedSignatureKey,
                    );
                    if let Some(annotation) = &property.type_annotation {
                        self.visit_type(&annotation.type_annotation, leaf_context);
                    }
                }
                TSSignature::TSMethodSignature(method) => {
                    self.visit_computed_key(
                        &method.key,
                        method.computed,
                        UnsupportedValuePositionKind::ComputedSignatureKey,
                    );
                    self.visit_this_parameter(method.this_param.as_deref());
                    self.visit_parameters(&method.params, callable_context);
                    if let Some(parameters) = &method.type_parameters {
                        self.visit_type_parameters_for_carrier(parameters);
                    }
                    if let Some(return_type) = &method.return_type {
                        self.visit_type(
                            &return_type.type_annotation,
                            StructuralDependencyContext::CarrierOnly,
                        );
                    }
                }
                TSSignature::TSCallSignatureDeclaration(call) => {
                    self.visit_this_parameter(call.this_param.as_deref());
                    self.visit_parameters(&call.params, callable_context);
                    if let Some(parameters) = &call.type_parameters {
                        self.visit_type_parameters_for_carrier(parameters);
                    }
                    if let Some(return_type) = &call.return_type {
                        self.visit_type(
                            &return_type.type_annotation,
                            StructuralDependencyContext::CarrierOnly,
                        );
                    }
                }
                TSSignature::TSIndexSignature(index) => {
                    self.visit_index_parameters(&index.parameters);
                    self.visit_type(&index.type_annotation.type_annotation, leaf_context)
                }
                TSSignature::TSConstructSignatureDeclaration(constructor) => {
                    self.visit_parameters(
                        &constructor.params,
                        StructuralDependencyContext::CarrierOnly,
                    );
                    if let Some(parameters) = &constructor.type_parameters {
                        self.visit_type_parameters_for_carrier(parameters);
                    }
                    if let Some(return_type) = &constructor.return_type {
                        self.visit_type(
                            &return_type.type_annotation,
                            StructuralDependencyContext::CarrierOnly,
                        );
                    }
                }
            }
        }
    }

    fn visit_type(&mut self, ts_type: &TSType<'_>, context: StructuralDependencyContext) {
        match ts_type {
            TSType::TSTypeReference(reference) => {
                self.record(
                    dependency_path_from_type_name(&reference.type_name),
                    context,
                );
                if let Some(arguments) = &reference.type_arguments {
                    for argument in &arguments.params {
                        self.visit_type(argument, context);
                    }
                }
            }
            TSType::TSUnionType(union) => {
                for member in &union.types {
                    self.visit_type(member, context);
                }
            }
            TSType::TSIntersectionType(intersection) => {
                for member in &intersection.types {
                    self.visit_type(member, context);
                }
            }
            TSType::TSTypeLiteral(literal) => self.visit_signatures(
                &literal.members,
                context == StructuralDependencyContext::CarrierOnly,
            ),
            TSType::TSArrayType(array) => self.visit_type(&array.element_type, context),
            TSType::TSTupleType(tuple) => {
                for element in &tuple.element_types {
                    let nested = match element {
                        TSTupleElement::TSOptionalType(optional) => Some(&optional.type_annotation),
                        TSTupleElement::TSRestType(rest) => Some(&rest.type_annotation),
                        TSTupleElement::TSNamedTupleMember(named) => {
                            named.element_type.as_ts_type()
                        }
                        _ => element.as_ts_type(),
                    };
                    if let Some(nested) = nested {
                        self.visit_type(nested, context);
                    }
                }
            }
            TSType::TSConditionalType(conditional) => {
                for nested in [
                    &conditional.check_type,
                    &conditional.extends_type,
                    &conditional.true_type,
                    &conditional.false_type,
                ] {
                    self.visit_type(nested, context);
                }
            }
            TSType::TSMappedType(mapped) => {
                self.visit_type(&mapped.constraint, context);
                if let Some(name_type) = &mapped.name_type {
                    self.visit_type(name_type, StructuralDependencyContext::CarrierOnly);
                }
                if let Some(annotation) = &mapped.type_annotation {
                    self.visit_type(annotation, context);
                }
            }
            TSType::TSIndexedAccessType(indexed) => {
                let indexed_context = match context {
                    StructuralDependencyContext::CarrierOnly => {
                        StructuralDependencyContext::CarrierOnly
                    }
                    _ => StructuralDependencyContext::Root,
                };
                self.visit_type(&indexed.object_type, indexed_context);
                self.visit_type(&indexed.index_type, indexed_context);
            }
            TSType::TSTypeOperatorType(operator) => {
                self.visit_type(&operator.type_annotation, context);
            }
            TSType::TSParenthesizedType(parenthesized) => {
                self.visit_type(&parenthesized.type_annotation, context);
            }
            TSType::TSTemplateLiteralType(template) => {
                for nested in &template.types {
                    self.visit_type(nested, context);
                }
            }
            TSType::TSFunctionType(function) => {
                let parameter_context = match context {
                    StructuralDependencyContext::CarrierOnly => {
                        StructuralDependencyContext::CarrierOnly
                    }
                    StructuralDependencyContext::LeafProperty => {
                        StructuralDependencyContext::LeafProperty
                    }
                    _ => StructuralDependencyContext::CallableParam,
                };
                self.visit_this_parameter(function.this_param.as_deref());
                self.visit_parameters(&function.params, parameter_context);
                if let Some(parameters) = &function.type_parameters {
                    self.visit_type_parameters_for_carrier(parameters);
                }
                self.visit_type(
                    &function.return_type.type_annotation,
                    StructuralDependencyContext::CarrierOnly,
                );
            }
            TSType::TSConstructorType(constructor) => {
                let parameter_context = match context {
                    StructuralDependencyContext::CarrierOnly => {
                        StructuralDependencyContext::CarrierOnly
                    }
                    StructuralDependencyContext::LeafProperty => {
                        StructuralDependencyContext::LeafProperty
                    }
                    _ => StructuralDependencyContext::CallableParam,
                };
                self.visit_parameters(&constructor.params, parameter_context);
                if let Some(parameters) = &constructor.type_parameters {
                    self.visit_type_parameters_for_carrier(parameters);
                }
                self.visit_type(
                    &constructor.return_type.type_annotation,
                    StructuralDependencyContext::CarrierOnly,
                );
            }
            TSType::TSTypeQuery(query) => {
                let query_context = match &query.expr_name {
                    TSTypeQueryExprName::IdentifierReference(_) => context,
                    _ => StructuralDependencyContext::CarrierOnly,
                };
                let query_path = dependency_path_from_query_name(&query.expr_name);
                if let Some(path) = &query_path {
                    self.value_queries.insert(path.clone());
                }
                self.record(query_path, query_context);
                if let TSTypeQueryExprName::TSImportType(import) = &query.expr_name {
                    if let Some(arguments) = &import.type_arguments {
                        for argument in &arguments.params {
                            self.visit_type(argument, StructuralDependencyContext::CarrierOnly);
                        }
                    }
                }
                if let Some(arguments) = &query.type_arguments {
                    for argument in &arguments.params {
                        self.visit_type(argument, StructuralDependencyContext::CarrierOnly);
                    }
                }
            }
            TSType::TSTypePredicate(predicate) => {
                if let Some(annotation) = &predicate.type_annotation {
                    self.visit_type(
                        &annotation.type_annotation,
                        StructuralDependencyContext::CarrierOnly,
                    );
                }
            }
            TSType::TSInferType(infer) => {
                if let Some(constraint) = &infer.type_parameter.constraint {
                    self.visit_type(constraint, StructuralDependencyContext::CarrierOnly);
                }
                if let Some(default) = &infer.type_parameter.default {
                    self.visit_type(default, StructuralDependencyContext::CarrierOnly);
                }
            }
            TSType::TSImportType(import) => {
                if let Some(arguments) = &import.type_arguments {
                    for argument in &arguments.params {
                        self.visit_type(argument, StructuralDependencyContext::CarrierOnly);
                    }
                }
            }
            TSType::JSDocNullableType(nullable) => {
                self.visit_type(
                    &nullable.type_annotation,
                    StructuralDependencyContext::CarrierOnly,
                );
            }
            TSType::JSDocNonNullableType(non_nullable) => {
                self.visit_type(
                    &non_nullable.type_annotation,
                    StructuralDependencyContext::CarrierOnly,
                );
            }
            _ => {}
        }
    }

    fn visit_interface(
        &mut self,
        members: &[TSSignature<'_>],
        heritage: &[TSInterfaceHeritage<'_>],
    ) {
        for base in heritage {
            self.record_expression_path(&base.expression, StructuralDependencyContext::Root);
            if let Some(arguments) = &base.type_arguments {
                for argument in &arguments.params {
                    self.visit_type(argument, StructuralDependencyContext::Root);
                }
            }
        }
        self.visit_signatures(members, false);
    }

    fn visit_class(&mut self, class: &Class<'_>) {
        if let Some(parameters) = &class.type_parameters {
            self.visit_type_parameters_for_carrier(parameters);
        }
        if let Some(base) = &class.super_class {
            let value_path = dependency_path_from_expression(base);
            if let Some(path) = &value_path {
                self.value_positions.insert(path.clone());
            } else {
                self.unsupported_value_positions
                    .insert(UnsupportedValuePositionKind::ClassHeritageExpression);
            }
            let path_context = match base {
                Expression::Identifier(_) => StructuralDependencyContext::Root,
                _ => StructuralDependencyContext::CarrierOnly,
            };
            self.record(value_path, path_context);
            if let Some(arguments) = &class.super_type_arguments {
                for argument in &arguments.params {
                    self.visit_type(argument, StructuralDependencyContext::Root);
                }
            }
        }
        for clause in &class.implements {
            self.record(
                dependency_path_from_type_name(&clause.expression),
                StructuralDependencyContext::Root,
            );
            if let Some(arguments) = &clause.type_arguments {
                for argument in &arguments.params {
                    self.visit_type(argument, StructuralDependencyContext::Root);
                }
            }
        }
        for member in &class.body.body {
            match member {
                ClassElement::PropertyDefinition(property) => {
                    self.visit_computed_key(
                        &property.key,
                        property.computed,
                        UnsupportedValuePositionKind::ComputedClassKey,
                    );
                    self.visit_initializer_value_carrier(property.value.as_ref());
                    if let Some(annotation) = &property.type_annotation {
                        self.visit_type(
                            &annotation.type_annotation,
                            StructuralDependencyContext::LeafProperty,
                        );
                    }
                }
                ClassElement::MethodDefinition(method) => {
                    self.visit_computed_key(
                        &method.key,
                        method.computed,
                        UnsupportedValuePositionKind::ComputedClassKey,
                    );
                    self.visit_this_parameter(method.value.this_param.as_deref());
                    self.visit_parameters(
                        &method.value.params,
                        StructuralDependencyContext::CallableParam,
                    );
                    if let Some(parameters) = &method.value.type_parameters {
                        self.visit_type_parameters_for_carrier(parameters);
                    }
                    if let Some(return_type) = &method.value.return_type {
                        self.visit_type(
                            &return_type.type_annotation,
                            StructuralDependencyContext::CarrierOnly,
                        );
                    }
                }
                ClassElement::AccessorProperty(property) => {
                    self.visit_computed_key(
                        &property.key,
                        property.computed,
                        UnsupportedValuePositionKind::ComputedClassKey,
                    );
                    self.visit_initializer_value_carrier(property.value.as_ref());
                    if let Some(annotation) = &property.type_annotation {
                        self.visit_type(
                            &annotation.type_annotation,
                            StructuralDependencyContext::LeafProperty,
                        );
                    }
                }
                ClassElement::TSIndexSignature(index) => {
                    self.visit_index_parameters(&index.parameters);
                    self.visit_type(
                        &index.type_annotation.type_annotation,
                        StructuralDependencyContext::LeafProperty,
                    );
                }
                _ => {}
            }
        }
    }
}
