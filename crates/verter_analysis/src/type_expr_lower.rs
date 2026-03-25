//! Lower OXC `TSType` AST nodes into [`TypeExpr`].
//!
//! This module converts OXC's borrowed AST representation into our owned
//! `TypeExpr` tree. The lowering is purely syntactic — no symbol resolution
//! or evaluation happens here.
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

use crate::type_expr::{
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
                .filter(|t| !has_immediate_vue_ignore_comment(source, t.span().start))
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
            let func = FunctionExpr {
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
            };
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) fn has_immediate_vue_ignore_comment(source: &str, start: u32) -> bool {
    let start = start as usize;
    if start == 0 || start > source.len() {
        return false;
    }

    let window_start = start.saturating_sub(160);
    let prefix = source[window_start..start].trim_end();
    if let Some(comment_start) = prefix.rfind("/*") {
        let comment = &prefix[comment_start..];
        return comment.ends_with("*/") && comment.contains("@vue-ignore");
    }

    false
}

fn lower_literal(literal: &oxc_ast::ast::TSLiteral<'_>, source: &str) -> TypeExpr {
    use oxc_ast::ast::{Expression, TSLiteral};
    match literal {
        TSLiteral::StringLiteral(s) => TypeExpr::string_literal(s.value.as_str()),
        TSLiteral::NumericLiteral(n) => TypeExpr::number_literal(n.value),
        TSLiteral::BooleanLiteral(b) => TypeExpr::boolean_literal(b.value),
        TSLiteral::BigIntLiteral(b) => {
            TypeExpr::Literal(crate::type_expr::LiteralValue::BigInt(b.value.to_string()))
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
            let func = FunctionExpr {
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
            };
            Some(ObjectMember::Method(MethodSignature {
                name,
                function: func,
                optional: method.optional,
            }))
        }
        TSSignature::TSCallSignatureDeclaration(call) => {
            let func = FunctionExpr {
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
            };
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
            let func = FunctionExpr {
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
            };
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
    FunctionExpr {
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
    }
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

// ---------------------------------------------------------------------------
// Name extraction helpers
// ---------------------------------------------------------------------------

pub(crate) fn property_key_name(key: &PropertyKey<'_>) -> Option<String> {
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

// ---------------------------------------------------------------------------
// Public convenience: parse a type annotation string into TypeExpr
// ---------------------------------------------------------------------------

/// Parse a standalone TypeScript type annotation string into a `TypeExpr`.
///
/// Uses OXC to parse `type __T = <input>` and extracts the resulting type.
/// Returns `TypeExpr::Unknown` if parsing fails.
pub fn parse_type_annotation(input: &str) -> TypeExpr {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    if input.trim().is_empty() {
        return TypeExpr::Unknown {
            raw: input.to_string(),
        };
    }

    let wrapper = format!("type __T = {input}");
    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let ret = Parser::new(&allocator, &wrapper, source_type).parse();

    for stmt in &ret.program.body {
        if let oxc_ast::ast::Statement::TSTypeAliasDeclaration(alias) = stmt {
            return lower_ts_type(&alias.type_annotation, &wrapper);
        }
    }

    TypeExpr::Unknown {
        raw: input.to_string(),
    }
}
