//! Lower OXC `TSType` AST nodes into [`TypeExpr`].
//!
//! This crate converts OXC's borrowed AST representation into the
//! owned [`TypeExpr`] tree exposed by `verter_type_expr`. The lowering
//! is purely syntactic — no symbol resolution or evaluation happens
//! here.
//!
//! The data tier (`verter_type_expr`) intentionally has no OXC
//! dependency so NAPI / WASM / JSON-only consumers can pull only that
//! crate. Producer-side callers (the analyzer, the parser's
//! cross-file external resolution path, the checker-text adapter)
//! depend on this crate to perform the lowering at OXC visit points.
//!
//! # Contract
//!
//! - Every well-formed OXC `TSType` node produces a valid `TypeExpr`.
//! - Unsupported or unrecognized syntax produces `TypeExpr::Unknown { raw }`.
//! - No partial parses — each node is fully consumed.
//! - Source text is required for `Unknown` fallback and literal extraction.

use oxc_ast::ast::{
    BindingPattern, FormalParameters, PropertyKey, TSFunctionType, TSMappedType,
    TSMappedTypeModifierOperator, TSQualifiedName, TSSignature, TSTupleElement, TSType, TSTypeName,
    TSTypeOperatorOperator, TSTypeParameterDeclaration, TSTypeQuery, TSTypeQueryExprName,
    TSTypeReference,
};
use oxc_span::GetSpan;

use std::sync::Arc;

use verter_type_expr::{
    FunctionExpr, FunctionParam, IndexSignature, MappedModifier, MethodSignature, ObjectExpr,
    ObjectMember, ObjectProperty, PrimitiveName, TupleElement, TypeExpr, TypeParam, ValueRef,
};

/// Lower an OXC `TSType` node into a `TypeExpr`.
///
/// `source` is the full source text, used for extracting raw text
/// for `Unknown` fallback nodes and literal values.
pub fn lower_ts_type(ts_type: &TSType<'_>, source: &str) -> TypeExpr {
    match ts_type {
        // -- Primitive keywords --
        TSType::TSStringKeyword(_) => TypeExpr::Primitive(PrimitiveName::String),
        TSType::TSNumberKeyword(_) => TypeExpr::Primitive(PrimitiveName::Number),
        TSType::TSBooleanKeyword(_) => TypeExpr::Primitive(PrimitiveName::Boolean),
        TSType::TSSymbolKeyword(_) => TypeExpr::Primitive(PrimitiveName::Symbol),
        TSType::TSBigIntKeyword(_) => TypeExpr::Primitive(PrimitiveName::BigInt),
        TSType::TSAnyKeyword(_) => TypeExpr::Primitive(PrimitiveName::Any),
        TSType::TSUnknownKeyword(_) => TypeExpr::Primitive(PrimitiveName::Unknown),
        TSType::TSVoidKeyword(_) => TypeExpr::Primitive(PrimitiveName::Void),
        TSType::TSNeverKeyword(_) => TypeExpr::Primitive(PrimitiveName::Never),
        TSType::TSNullKeyword(_) => TypeExpr::Primitive(PrimitiveName::Null),
        TSType::TSUndefinedKeyword(_) => TypeExpr::Primitive(PrimitiveName::Undefined),
        TSType::TSObjectKeyword(_) => TypeExpr::Primitive(PrimitiveName::Object),

        // -- Literal types --
        TSType::TSLiteralType(lit) => lower_literal(&lit.literal, source),

        // -- Compound types --
        TSType::TSUnionType(union) => {
            let types: Vec<TypeExpr> = union
                .types
                .iter()
                .map(|t| lower_ts_type(t, source))
                .collect();
            TypeExpr::union(types)
        }
        TSType::TSIntersectionType(intersection) => {
            let types: Vec<TypeExpr> = intersection
                .types
                .iter()
                .map(|t| lower_ts_type(t, source))
                .collect();
            TypeExpr::intersection(types)
        }

        // -- Array --
        TSType::TSArrayType(arr) => TypeExpr::Array {
            element: Arc::new(lower_ts_type(&arr.element_type, source)),
            readonly: false,
        },

        // -- Tuple --
        TSType::TSTupleType(tuple) => {
            let elements: Vec<TupleElement> = tuple
                .element_types
                .iter()
                .map(|elem| lower_tuple_element(elem, source))
                .collect();
            TypeExpr::Tuple {
                elements: Arc::from(elements),
                readonly: false,
            }
        }

        // -- Object type literal --
        TSType::TSTypeLiteral(literal) => {
            let members = literal
                .members
                .iter()
                .filter_map(|m| lower_ts_signature(m, source))
                .collect();
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: members,
            }))
        }

        // -- Function type --
        TSType::TSFunctionType(func) => {
            TypeExpr::Function(Arc::new(lower_function_type(func, source)))
        }

        // -- Constructor type --
        TSType::TSConstructorType(ctor) => {
            let func = normalize_function_type_params(FunctionExpr {
                parameters: lower_formal_parameters(&ctor.params, source),
                return_type: Some(Arc::new(lower_ts_type(
                    &ctor.return_type.type_annotation,
                    source,
                ))),
                type_parameters: ctor
                    .type_parameters
                    .as_ref()
                    .map(|tp| lower_type_params(tp, source))
                    .unwrap_or_default(),
            });
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::ConstructSignature(func)],
            }))
        }

        // -- Type reference --
        TSType::TSTypeReference(type_ref) => lower_type_reference(type_ref, source),

        // -- Type operators (keyof, readonly, unique) --
        TSType::TSTypeOperatorType(op) => match op.operator {
            TSTypeOperatorOperator::Keyof => {
                TypeExpr::KeyOf(Arc::new(lower_ts_type(&op.type_annotation, source)))
            }
            TSTypeOperatorOperator::Readonly => {
                let inner = lower_ts_type(&op.type_annotation, source);
                match inner {
                    TypeExpr::Array { element, .. } => TypeExpr::Array {
                        element,
                        readonly: true,
                    },
                    TypeExpr::Tuple { elements, .. } => TypeExpr::Tuple {
                        elements,
                        readonly: true,
                    },
                    other => other,
                }
            }
            TSTypeOperatorOperator::Unique => lower_ts_type(&op.type_annotation, source),
        },

        // -- Indexed access: T[K] --
        TSType::TSIndexedAccessType(idx) => TypeExpr::IndexedAccess {
            object: Arc::new(lower_ts_type(&idx.object_type, source)),
            index: Arc::new(lower_ts_type(&idx.index_type, source)),
        },

        // -- Conditional type: T extends U ? A : B --
        TSType::TSConditionalType(cond) => TypeExpr::Conditional {
            check: Arc::new(lower_ts_type(&cond.check_type, source)),
            extends: Arc::new(lower_ts_type(&cond.extends_type, source)),
            true_type: Arc::new(lower_ts_type(&cond.true_type, source)),
            false_type: Arc::new(lower_ts_type(&cond.false_type, source)),
        },

        // -- Mapped type: { [K in T]: V } --
        TSType::TSMappedType(mapped) => lower_mapped_type(mapped, source),

        // -- Template literal type: `prefix${T}suffix` --
        TSType::TSTemplateLiteralType(tpl) => {
            let quasis = tpl.quasis.iter().map(|q| q.value.raw.to_string()).collect();
            let expressions: Vec<TypeExpr> =
                tpl.types.iter().map(|t| lower_ts_type(t, source)).collect();
            TypeExpr::TemplateLiteral {
                quasis,
                expressions: Arc::from(expressions),
            }
        }

        // -- Parenthesized type --
        TSType::TSParenthesizedType(paren) => {
            TypeExpr::Parenthesized(Arc::new(lower_ts_type(&paren.type_annotation, source)))
        }

        // -- typeof (type query) --
        TSType::TSTypeQuery(query) => lower_type_query(query, source),

        // -- infer T --
        TSType::TSInferType(infer) => TypeExpr::Infer {
            name: infer.type_parameter.name.to_string(),
        },

        // -- Import type: import("...").Type --
        TSType::TSImportType(import) => {
            let span = import.span;
            TypeExpr::Unknown {
                raw: span_text(source, span),
            }
        }

        // -- this type --
        TSType::TSThisType(_) => TypeExpr::named("this"),

        // -- Intrinsic keyword --
        TSType::TSIntrinsicKeyword(_) => TypeExpr::named("intrinsic"),

        // -- Catch-all --
        _ => {
            let span = ts_type.span();
            TypeExpr::Unknown {
                raw: span_text(source, span),
            }
        }
    }
}

fn lower_literal(literal: &oxc_ast::ast::TSLiteral<'_>, source: &str) -> TypeExpr {
    use oxc_ast::ast::{Expression, TSLiteral};
    match literal {
        TSLiteral::StringLiteral(s) => TypeExpr::string_literal(s.value.as_str()),
        TSLiteral::NumericLiteral(n) => TypeExpr::number_literal(n.value),
        TSLiteral::BooleanLiteral(b) => TypeExpr::boolean_literal(b.value),
        TSLiteral::BigIntLiteral(b) => {
            TypeExpr::Literal(verter_type_expr::LiteralValue::BigInt(b.value.to_string()))
        }
        TSLiteral::UnaryExpression(unary) => {
            if let Expression::NumericLiteral(n) = &unary.argument {
                TypeExpr::number_literal(-n.value)
            } else {
                TypeExpr::Unknown {
                    raw: span_text(source, unary.span()),
                }
            }
        }
        TSLiteral::TemplateLiteral(tpl) => {
            if tpl.expressions.is_empty() {
                if let Some(quasi) = tpl.quasis.first() {
                    TypeExpr::string_literal(quasi.value.raw.as_str())
                } else {
                    TypeExpr::string_literal("")
                }
            } else {
                TypeExpr::Unknown {
                    raw: span_text(source, tpl.span()),
                }
            }
        } // TSLiteral is exhaustive with the variants above in OXC 0.117
    }
}

fn lower_type_reference(type_ref: &TSTypeReference<'_>, source: &str) -> TypeExpr {
    let name = match &type_ref.type_name {
        TSTypeName::IdentifierReference(id) => id.name.to_string(),
        TSTypeName::QualifiedName(qualified) => qualified_name_to_string(qualified),
        _ => {
            return TypeExpr::Unknown {
                raw: span_text(source, type_ref.span),
            };
        }
    };

    let type_arguments: Vec<TypeExpr> = type_ref
        .type_arguments
        .as_ref()
        .map(|params| {
            params
                .params
                .iter()
                .map(|p| lower_ts_type(p, source))
                .collect()
        })
        .unwrap_or_default();

    // Normalize Array<T> and ReadonlyArray<T> to array form
    if type_arguments.len() == 1 {
        if name == "Array" {
            return TypeExpr::Array {
                element: Arc::new(type_arguments.into_iter().next().unwrap()),
                readonly: false,
            };
        }
        if name == "ReadonlyArray" {
            return TypeExpr::Array {
                element: Arc::new(type_arguments.into_iter().next().unwrap()),
                readonly: true,
            };
        }
    }

    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(type_arguments),
    }
}

fn lower_type_query(query: &TSTypeQuery<'_>, source: &str) -> TypeExpr {
    let path = match &query.expr_name {
        TSTypeQueryExprName::IdentifierReference(id) => {
            vec![id.name.to_string()]
        }
        TSTypeQueryExprName::QualifiedName(qualified) => {
            let mut segments = Vec::new();
            collect_qualified_parts(qualified, &mut segments);
            segments
        }
        _ => {
            return TypeExpr::Unknown {
                raw: span_text(source, query.span),
            };
        }
    };

    TypeExpr::TypeOf(ValueRef { path })
}

fn lower_mapped_type(mapped: &TSMappedType<'_>, source: &str) -> TypeExpr {
    // In OXC 0.117, the key is `mapped.key` (BindingIdentifier)
    let parameter = mapped.key.name.to_string();
    let source_type = lower_ts_type(&mapped.constraint, source);
    let value = mapped
        .type_annotation
        .as_ref()
        .map(|t| lower_ts_type(t, source))
        .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));

    let optional = match mapped.optional {
        Some(TSMappedTypeModifierOperator::True) => MappedModifier::Add,
        Some(TSMappedTypeModifierOperator::Plus) => MappedModifier::Add,
        Some(TSMappedTypeModifierOperator::Minus) => MappedModifier::Remove,
        None => MappedModifier::None,
    };

    let readonly = match mapped.readonly {
        Some(TSMappedTypeModifierOperator::True) => MappedModifier::Add,
        Some(TSMappedTypeModifierOperator::Plus) => MappedModifier::Add,
        Some(TSMappedTypeModifierOperator::Minus) => MappedModifier::Remove,
        None => MappedModifier::None,
    };

    let name_type = mapped
        .name_type
        .as_ref()
        .map(|n| Arc::new(lower_ts_type(n, source)));

    TypeExpr::Mapped {
        parameter,
        source: Arc::new(source_type),
        value: Arc::new(value),
        optional,
        readonly,
        name_type,
    }
}

fn lower_ts_signature(sig: &TSSignature<'_>, source: &str) -> Option<ObjectMember> {
    match sig {
        TSSignature::TSPropertySignature(prop) => {
            let name = property_key_name(&prop.key)?;
            let ty = prop
                .type_annotation
                .as_ref()
                .map(|ta| lower_ts_type(&ta.type_annotation, source))
                .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));

            Some(ObjectMember::Property(ObjectProperty {
                name,
                ty,
                optional: prop.optional,
                readonly: prop.readonly,
            }))
        }
        TSSignature::TSMethodSignature(method) => {
            let name = property_key_name(&method.key)?;
            let func = normalize_function_type_params(FunctionExpr {
                parameters: lower_formal_parameters(&method.params, source),
                return_type: method
                    .return_type
                    .as_ref()
                    .map(|rt| Arc::new(lower_ts_type(&rt.type_annotation, source))),
                type_parameters: method
                    .type_parameters
                    .as_ref()
                    .map(|tp| lower_type_params(tp, source))
                    .unwrap_or_default(),
            });
            Some(ObjectMember::Method(MethodSignature {
                name,
                function: func,
                optional: method.optional,
            }))
        }
        TSSignature::TSCallSignatureDeclaration(call) => {
            let func = normalize_function_type_params(FunctionExpr {
                parameters: lower_formal_parameters(&call.params, source),
                return_type: call
                    .return_type
                    .as_ref()
                    .map(|rt| Arc::new(lower_ts_type(&rt.type_annotation, source))),
                type_parameters: call
                    .type_parameters
                    .as_ref()
                    .map(|tp| lower_type_params(tp, source))
                    .unwrap_or_default(),
            });
            Some(ObjectMember::CallSignature(func))
        }
        TSSignature::TSIndexSignature(idx) => {
            let (key_name, key_type) = if let Some(param) = idx.parameters.first() {
                let name = param.name.to_string();
                let ty = lower_ts_type(&param.type_annotation.type_annotation, source);
                (name, ty)
            } else {
                (
                    "key".to_string(),
                    TypeExpr::Primitive(PrimitiveName::String),
                )
            };

            let value_type = lower_ts_type(&idx.type_annotation.type_annotation, source);
            Some(ObjectMember::IndexSignature(IndexSignature {
                key_name,
                key_type,
                value_type,
                readonly: idx.readonly,
            }))
        }
        TSSignature::TSConstructSignatureDeclaration(ctor) => {
            let func = normalize_function_type_params(FunctionExpr {
                parameters: lower_formal_parameters(&ctor.params, source),
                return_type: ctor
                    .return_type
                    .as_ref()
                    .map(|rt| Arc::new(lower_ts_type(&rt.type_annotation, source))),
                type_parameters: ctor
                    .type_parameters
                    .as_ref()
                    .map(|tp| lower_type_params(tp, source))
                    .unwrap_or_default(),
            });
            Some(ObjectMember::ConstructSignature(func))
        }
    }
}

fn lower_tuple_element(elem: &TSTupleElement<'_>, source: &str) -> TupleElement {
    match elem {
        TSTupleElement::TSOptionalType(opt) => TupleElement {
            label: None,
            ty: lower_ts_type(&opt.type_annotation, source),
            optional: true,
            rest: false,
        },
        TSTupleElement::TSRestType(rest) => TupleElement {
            label: None,
            ty: lower_ts_type(&rest.type_annotation, source),
            optional: false,
            rest: true,
        },
        TSTupleElement::TSNamedTupleMember(named) => {
            let label = Some(named.label.name.to_string());
            // Named tuple member has its own `optional` field
            let ty = if let Some(t) = named.element_type.as_ts_type() {
                lower_ts_type(t, source)
            } else {
                TypeExpr::Primitive(PrimitiveName::Any)
            };
            TupleElement {
                label,
                ty,
                optional: named.optional,
                rest: false,
            }
        }
        _ => {
            if let Some(t) = elem.as_ts_type() {
                TupleElement {
                    label: None,
                    ty: lower_ts_type(t, source),
                    optional: false,
                    rest: false,
                }
            } else {
                TupleElement {
                    label: None,
                    ty: TypeExpr::Primitive(PrimitiveName::Any),
                    optional: false,
                    rest: false,
                }
            }
        }
    }
}

fn lower_function_type(func: &TSFunctionType<'_>, source: &str) -> FunctionExpr {
    normalize_function_type_params(FunctionExpr {
        parameters: lower_formal_parameters(&func.params, source),
        return_type: Some(Arc::new(lower_ts_type(
            &func.return_type.type_annotation,
            source,
        ))),
        type_parameters: func
            .type_parameters
            .as_ref()
            .map(|tp| lower_type_params(tp, source))
            .unwrap_or_default(),
    })
}

fn lower_formal_parameters(params: &FormalParameters<'_>, source: &str) -> Vec<FunctionParam> {
    params
        .items
        .iter()
        .map(|param| {
            let name = binding_pattern_name(&param.pattern);
            let ty = param
                .type_annotation
                .as_ref()
                .map(|ta| lower_ts_type(&ta.type_annotation, source))
                .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
            FunctionParam {
                name,
                ty,
                optional: param.optional,
                rest: false,
            }
        })
        .chain(params.rest.as_ref().map(|rest| {
            let name = binding_pattern_name(&rest.rest.argument);
            let ty = rest
                .type_annotation
                .as_ref()
                .map(|ta| lower_ts_type(&ta.type_annotation, source))
                .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
            FunctionParam {
                name,
                ty,
                optional: false,
                rest: true,
            }
        }))
        .collect()
}

fn lower_type_params(type_params: &TSTypeParameterDeclaration<'_>, source: &str) -> Vec<TypeParam> {
    type_params
        .params
        .iter()
        .map(|p| TypeParam {
            name: p.name.to_string(),
            constraint: p
                .constraint
                .as_ref()
                .map(|c| Arc::new(lower_ts_type(c, source))),
            default: p
                .default
                .as_ref()
                .map(|d| Arc::new(lower_ts_type(d, source))),
        })
        .collect()
}

fn normalize_function_type_params(mut func: FunctionExpr) -> FunctionExpr {
    if func.type_parameters.is_empty() {
        return func;
    }

    let scope = normalize_type_parameter_decls(func.type_parameters);
    func.parameters = func
        .parameters
        .into_iter()
        .map(|mut param| {
            param.ty = normalize_type_parameter_refs(&param.ty, &scope);
            param
        })
        .collect();
    func.return_type = func
        .return_type
        .map(|ret| Arc::new(normalize_type_parameter_refs(ret.as_ref(), &scope)));
    func.type_parameters = scope;
    func
}

fn normalize_type_parameter_decls(type_parameters: Vec<TypeParam>) -> Vec<TypeParam> {
    let mut normalized = Vec::with_capacity(type_parameters.len());

    for param in type_parameters {
        let constraint = param
            .constraint
            .as_ref()
            .map(|expr| Arc::new(normalize_type_parameter_refs(expr.as_ref(), &normalized)));
        let default = param
            .default
            .as_ref()
            .map(|expr| Arc::new(normalize_type_parameter_refs(expr.as_ref(), &normalized)));

        normalized.push(TypeParam {
            name: param.name,
            constraint,
            default,
        });
    }

    normalized
}

fn normalize_type_parameter_refs(expr: &TypeExpr, scope: &[TypeParam]) -> TypeExpr {
    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => scope
            .iter()
            .find(|param| param.name == name.as_ref())
            .cloned()
            .map(TypeExpr::TypeParameter)
            .unwrap_or_else(|| expr.clone()),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => TypeExpr::Ref {
            name: Arc::clone(name),
            type_arguments: Arc::from(
                type_arguments
                    .iter()
                    .map(|arg| normalize_type_parameter_refs(arg, scope))
                    .collect::<Vec<_>>(),
            ),
        },
        TypeExpr::Union(types) => TypeExpr::union(
            types
                .iter()
                .map(|ty| normalize_type_parameter_refs(ty, scope))
                .collect(),
        ),
        TypeExpr::Intersection(types) => TypeExpr::intersection(
            types
                .iter()
                .map(|ty| normalize_type_parameter_refs(ty, scope))
                .collect(),
        ),
        TypeExpr::Array { element, readonly } => TypeExpr::Array {
            element: Arc::new(normalize_type_parameter_refs(element.as_ref(), scope)),
            readonly: *readonly,
        },
        TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
            elements: Arc::from(
                elements
                    .iter()
                    .map(|element| TupleElement {
                        label: element.label.clone(),
                        ty: normalize_type_parameter_refs(&element.ty, scope),
                        optional: element.optional,
                        rest: element.rest,
                    })
                    .collect::<Vec<_>>(),
            ),
            readonly: *readonly,
        },
        TypeExpr::Object(obj) => TypeExpr::Object(Arc::new(ObjectExpr {
            properties: obj
                .properties
                .iter()
                .map(|member| normalize_object_member_type_params(member, scope))
                .collect(),
        })),
        TypeExpr::Function(func) => TypeExpr::Function(Arc::new(
            normalize_nested_function_type_params(func.as_ref(), scope),
        )),
        TypeExpr::KeyOf(inner) => TypeExpr::KeyOf(Arc::new(normalize_type_parameter_refs(
            inner.as_ref(),
            scope,
        ))),
        TypeExpr::IndexedAccess { object, index } => TypeExpr::IndexedAccess {
            object: Arc::new(normalize_type_parameter_refs(object.as_ref(), scope)),
            index: Arc::new(normalize_type_parameter_refs(index.as_ref(), scope)),
        },
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => TypeExpr::Conditional {
            check: Arc::new(normalize_type_parameter_refs(check.as_ref(), scope)),
            extends: Arc::new(normalize_type_parameter_refs(extends.as_ref(), scope)),
            true_type: Arc::new(normalize_type_parameter_refs(true_type.as_ref(), scope)),
            false_type: Arc::new(normalize_type_parameter_refs(false_type.as_ref(), scope)),
        },
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => TypeExpr::Mapped {
            parameter: parameter.clone(),
            source: Arc::new(normalize_type_parameter_refs(source.as_ref(), scope)),
            value: Arc::new(normalize_type_parameter_refs(value.as_ref(), scope)),
            optional: *optional,
            readonly: *readonly,
            name_type: name_type
                .as_ref()
                .map(|expr| Arc::new(normalize_type_parameter_refs(expr.as_ref(), scope))),
        },
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => TypeExpr::TemplateLiteral {
            quasis: quasis.clone(),
            expressions: Arc::from(
                expressions
                    .iter()
                    .map(|expr| normalize_type_parameter_refs(expr, scope))
                    .collect::<Vec<_>>(),
            ),
        },
        TypeExpr::Rest(inner) => TypeExpr::Rest(Arc::new(normalize_type_parameter_refs(
            inner.as_ref(),
            scope,
        ))),
        TypeExpr::Parenthesized(inner) => TypeExpr::Parenthesized(Arc::new(
            normalize_type_parameter_refs(inner.as_ref(), scope),
        )),
        TypeExpr::TypeParameter(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::Unknown { .. } => expr.clone(),
    }
}

fn normalize_object_member_type_params(member: &ObjectMember, scope: &[TypeParam]) -> ObjectMember {
    match member {
        ObjectMember::Property(prop) => ObjectMember::Property(ObjectProperty {
            name: prop.name.clone(),
            ty: normalize_type_parameter_refs(&prop.ty, scope),
            optional: prop.optional,
            readonly: prop.readonly,
        }),
        ObjectMember::IndexSignature(sig) => ObjectMember::IndexSignature(IndexSignature {
            key_name: sig.key_name.clone(),
            key_type: normalize_type_parameter_refs(&sig.key_type, scope),
            value_type: normalize_type_parameter_refs(&sig.value_type, scope),
            readonly: sig.readonly,
        }),
        ObjectMember::CallSignature(func) => {
            ObjectMember::CallSignature(normalize_nested_function_type_params(func, scope))
        }
        ObjectMember::ConstructSignature(func) => {
            ObjectMember::ConstructSignature(normalize_nested_function_type_params(func, scope))
        }
        ObjectMember::Method(method) => ObjectMember::Method(MethodSignature {
            name: method.name.clone(),
            function: normalize_nested_function_type_params(&method.function, scope),
            optional: method.optional,
        }),
    }
}

fn normalize_nested_function_type_params(func: &FunctionExpr, scope: &[TypeParam]) -> FunctionExpr {
    let mut combined_scope = scope.to_vec();
    let nested_scope = normalize_type_parameter_decls(func.type_parameters.clone());
    combined_scope.extend(nested_scope.clone());

    FunctionExpr {
        parameters: func
            .parameters
            .iter()
            .map(|param| FunctionParam {
                name: param.name.clone(),
                ty: normalize_type_parameter_refs(&param.ty, &combined_scope),
                optional: param.optional,
                rest: param.rest,
            })
            .collect(),
        return_type: func
            .return_type
            .as_ref()
            .map(|ret| Arc::new(normalize_type_parameter_refs(ret.as_ref(), &combined_scope))),
        type_parameters: nested_scope,
    }
}

// ---------------------------------------------------------------------------
// Name extraction helpers
// ---------------------------------------------------------------------------

pub fn property_key_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        PropertyKey::NumericLiteral(n) => Some(n.value.to_string()),
        _ => None,
    }
}

fn binding_pattern_name(pattern: &BindingPattern<'_>) -> Option<String> {
    // In OXC 0.117, BindingPattern is directly the enum
    if let BindingPattern::BindingIdentifier(id) = pattern {
        Some(id.name.to_string())
    } else {
        None
    }
}

fn qualified_name_to_string(qualified: &TSQualifiedName<'_>) -> String {
    let mut parts = Vec::new();
    collect_qualified_parts(qualified, &mut parts);
    parts.join(".")
}

fn collect_qualified_parts(qualified: &TSQualifiedName<'_>, parts: &mut Vec<String>) {
    match &qualified.left {
        TSTypeName::IdentifierReference(id) => {
            parts.push(id.name.to_string());
        }
        TSTypeName::QualifiedName(inner) => {
            collect_qualified_parts(inner, parts);
        }
        _ => {}
    }
    parts.push(qualified.right.name.to_string());
}

fn span_text(source: &str, span: oxc_span::Span) -> String {
    let start = span.start as usize;
    let end = span.end as usize;
    if end <= source.len() {
        source[start..end].to_string()
    } else {
        String::new()
    }
}
