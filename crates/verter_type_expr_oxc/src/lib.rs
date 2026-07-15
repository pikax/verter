//! Lower OXC `TSType` AST nodes into [`TypeExpr`].
//!
//! This crate converts OXC's borrowed AST representation into the
//! owned [`TypeExpr`] tree exposed by `verter_type_expr`. The lowering
//! is purely syntactic — no symbol resolution or evaluation happens
//! here.
//!
//! The data tier (`verter_type_expr`) intentionally has no OXC
//! dependency so NAPI / WASM / JSON-only consumers can pull only that
//! crate. Producer-side callers (the analyzer and the parser's
//! cross-file external resolution path) depend on this crate to
//! perform the lowering at OXC visit points.
//!
//! # Contract
//!
//! - Every well-formed OXC `TSType` node produces a valid `TypeExpr`.
//! - Unsupported or unrecognized syntax produces `TypeExpr::Unknown { raw }`.
//! - No partial parses — each node is fully consumed.
//! - Source text is required for `Unknown` fallback and literal extraction.

use oxc_ast::ast::{
    BindingPattern, FormalParameters, PropertyKey, TSFunctionType, TSImportType,
    TSImportTypeQualifier, TSMappedType, TSMappedTypeModifierOperator, TSQualifiedName,
    TSSignature, TSTupleElement, TSType, TSTypeName, TSTypeOperatorOperator,
    TSTypeParameterDeclaration, TSTypeQuery, TSTypeQueryExprName, TSTypeReference, UnaryOperator,
};
use oxc_span::GetSpan;

use std::sync::Arc;

use verter_type_expr::{
    FunctionExpr, FunctionParam, FunctionSpans, IndexSignature, IndexSignatureSpans,
    MappedModifier, MemberSpans, MethodSignature, ObjectExpr, ObjectMember, ObjectProperty,
    PrimitiveName, TupleElement, TypeExpr, TypeParam, ValueRef,
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
        //
        // The arms collect straight into the `Arc<[TypeExpr]>` payload via
        // the exact-size factories — one allocation, no intermediate `Vec`.
        TSType::TSUnionType(union) => {
            TypeExpr::union_from_exact_iter(union.types.iter().map(|t| lower_ts_type(t, source)))
        }
        TSType::TSIntersectionType(intersection) => TypeExpr::intersection_from_exact_iter(
            intersection.types.iter().map(|t| lower_ts_type(t, source)),
        ),

        // -- Array --
        TSType::TSArrayType(arr) => TypeExpr::Array {
            element: Arc::new(lower_ts_type(&arr.element_type, source)),
            readonly: false,
        },

        // -- Tuple --
        TSType::TSTupleType(tuple) => TypeExpr::Tuple {
            // Exact-size collect straight into the `Arc<[TupleElement]>`
            // payload — one allocation, no intermediate `Vec`.
            elements: tuple
                .element_types
                .iter()
                .map(|elem| lower_tuple_element(elem, source))
                .collect(),
            readonly: false,
        },

        // -- Object type literal --
        TSType::TSTypeLiteral(literal) => {
            // `filter_map` erases the exact size hint, so pre-size to the
            // AST member count (tight upper bound: only unnameable members
            // drop) instead of growing by doubling.
            let mut members = Vec::with_capacity(literal.members.len());
            members.extend(
                literal
                    .members
                    .iter()
                    .filter_map(|m| lower_ts_signature(m, source)),
            );
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: members,
            }))
        }

        // -- Function type --
        TSType::TSFunctionType(func) => {
            TypeExpr::Function(Arc::new(lower_function_type(func, source)))
        }

        // -- Constructor type: `new (x: T) => R`.
        //
        // Lowered to the dedicated `TypeExpr::ConstructorType` variant rather
        // than `Object { ConstructSignature }`. A bare constructor *type* and a
        // type-literal `{ new (): R }` are otherwise structurally identical after
        // lowering, but Vue's runtime-constructor inference maps the former to
        // `Function` and the latter to `Object` (legacy `infer_runtime_type`:
        // `TSConstructorType` -> Function at infer.rs:61; `TSTypeLiteral` ->
        // Object at infer.rs:55). Keeping them apart at the producer lets the
        // shared reducer reproduce that distinction. The carried `FunctionExpr`
        // is identical to the construct-signature form, so any consumer wanting
        // construct semantics walks the inner function exactly as before.
        TSType::TSConstructorType(ctor) => {
            let func = normalize_function_type_params(FunctionExpr::with_spans(
                lower_formal_parameters(&ctor.params, source),
                Some(Arc::new(lower_ts_type(
                    &ctor.return_type.type_annotation,
                    source,
                ))),
                ctor.type_parameters
                    .as_ref()
                    .map(|tp| lower_type_params(tp, source))
                    .unwrap_or_default(),
                FunctionSpans {
                    signature: Some(ctor.span.into()),
                    return_type: Some(ctor.return_type.type_annotation.span().into()),
                },
            ));
            TypeExpr::ConstructorType(Arc::new(func))
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
                // `TypeExpr` implements `Drop` (iterative deep-drop), so we
                // cannot move `element` / `elements` out of `inner` by
                // value. Clone the (cheap, refcounted) `Arc` child instead
                // and let the old `inner` drop normally.
                match &inner {
                    TypeExpr::Array { element, .. } => TypeExpr::Array {
                        element: Arc::clone(element),
                        readonly: true,
                    },
                    TypeExpr::Tuple { elements, .. } => TypeExpr::Tuple {
                        elements: Arc::clone(elements),
                        readonly: true,
                    },
                    _ => inner,
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
        TSType::TSTemplateLiteralType(tpl) => TypeExpr::TemplateLiteral {
            quasis: tpl.quasis.iter().map(|q| q.value.raw.to_string()).collect(),
            // Exact-size collect straight into the `Arc<[TypeExpr]>`
            // payload — one allocation, no intermediate `Vec`.
            expressions: tpl.types.iter().map(|t| lower_ts_type(t, source)).collect(),
        },

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

        // -- Import type in TYPE position: `import("./m")` /
        //    `import("./m").Member`. Lowered to the typed-IR `ImportType`
        //    carrier (NOT the raw-text `Unknown` fallback) — the shared
        //    dispatch resolves the module + TYPE-export member cross-file.
        TSType::TSImportType(import) => lower_import_type(import, source, false),

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
        TSLiteral::UnaryExpression(unary) => match &unary.argument {
            // `-1` / `+1` — a signed numeric literal type. The sign is the
            // wrapping `UnaryExpression`'s operator, NOT a property of the inner
            // literal, so it must be applied operator-aware: a `UnaryNegation`
            // negates the magnitude while a `UnaryPlus` preserves it. Blindly
            // negating would turn `+1` into the wrong literal `-1`. Any other
            // unary operator (`~`, `!`, …) is not a valid literal-type sign and
            // falls through to the `Unknown` raw-text fallback below.
            Expression::NumericLiteral(n) => match unary.operator {
                UnaryOperator::UnaryNegation => TypeExpr::number_literal(-n.value),
                UnaryOperator::UnaryPlus => TypeExpr::number_literal(n.value),
                _ => TypeExpr::Unknown {
                    raw: span_text(source, unary.span()),
                },
            },
            // `-1n` / `+1n` — a signed bigint literal type. `BigIntLiteral.value`
            // is the base-10 magnitude with NO sign (the sign lives on this
            // wrapping `UnaryExpression`), so the signed form is reconstructed
            // here operator-aware, mirroring the positive `TSLiteral::BigIntLiteral`
            // arm. A `UnaryNegation` prepends `-`; a `UnaryPlus` keeps the bare
            // magnitude (prepending `-` unconditionally would turn `+1n` into the
            // wrong literal `-1n`). Lowering it to a `BigInt` literal (rather than
            // the `Unknown` fallback) keeps it a first-class bigint literal so the
            // Vue runtime-constructor reducer maps it to `Number` exactly like a
            // positive bigint literal — matching legacy `infer_runtime_type`'s
            // unary-bigint -> Number rule (`type_surface/infer.rs`).
            Expression::BigIntLiteral(b) => match unary.operator {
                UnaryOperator::UnaryNegation => TypeExpr::Literal(
                    verter_type_expr::LiteralValue::BigInt(format!("-{}", b.value)),
                ),
                UnaryOperator::UnaryPlus => {
                    TypeExpr::Literal(verter_type_expr::LiteralValue::BigInt(b.value.to_string()))
                }
                _ => TypeExpr::Unknown {
                    raw: span_text(source, unary.span()),
                },
            },
            _ => TypeExpr::Unknown {
                raw: span_text(source, unary.span()),
            },
        },
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

    let params: &[TSType<'_>] = type_ref
        .type_arguments
        .as_ref()
        .map_or(&[], |args| &args.params);

    // Normalize Array<T> and ReadonlyArray<T> to array form. Checked on the
    // raw argument slice BEFORE lowering so the single element lowers
    // straight into the `Array` node.
    if params.len() == 1 && (name == "Array" || name == "ReadonlyArray") {
        return TypeExpr::Array {
            element: Arc::new(lower_ts_type(&params[0], source)),
            readonly: name == "ReadonlyArray",
        };
    }

    if params.is_empty() {
        // Shared empty type-argument slice — no per-call allocation.
        return TypeExpr::named(name);
    }

    TypeExpr::Ref {
        name: Arc::from(name),
        // Exact-size collect straight into the `Arc<[TypeExpr]>` payload —
        // one allocation, no intermediate `Vec`.
        type_arguments: params.iter().map(|p| lower_ts_type(p, source)).collect(),
    }
}

/// Lower a `TSImportType` (`import("./m")` / `import("./m").A.B` /
/// `typeof import("./m")`) into the typed-IR [`TypeExpr::ImportType`]
/// carrier. `typeof_query` is `true` when the node sits under a
/// `typeof` query (the module's VALUE-export namespace) and `false`
/// for a bare `import(...)` in type position (the TYPE-export space).
/// The qualifier and instantiation type-arguments are captured so the
/// node is FULLY consumed — no raw-text reparsing downstream.
fn lower_import_type(import: &TSImportType<'_>, source: &str, typeof_query: bool) -> TypeExpr {
    let specifier: Arc<str> = Arc::from(import.source.value.as_str());
    let mut qualifier: Vec<Arc<str>> = Vec::new();
    if let Some(q) = &import.qualifier {
        collect_import_qualifier_parts(q, &mut qualifier);
    }
    // Exact-size collect straight into the `Arc<[TypeExpr]>` payload; the
    // no-argument case reuses the shared empty slice (no per-call
    // allocation).
    let type_arguments: Arc<[TypeExpr]> = import
        .type_arguments
        .as_ref()
        .filter(|params| !params.params.is_empty())
        .map(|params| {
            params
                .params
                .iter()
                .map(|p| lower_ts_type(p, source))
                .collect()
        })
        .unwrap_or_else(verter_type_expr::empty_type_args);
    TypeExpr::ImportType {
        specifier,
        qualifier: Arc::from(qualifier),
        typeof_query,
        type_arguments,
    }
}

/// Flatten an import-type qualifier (`.a.b.c` in `import("./m").a.b.c`)
/// into ordered segments `["a", "b", "c"]`.
fn collect_import_qualifier_parts(q: &TSImportTypeQualifier<'_>, parts: &mut Vec<Arc<str>>) {
    match q {
        TSImportTypeQualifier::Identifier(id) => parts.push(Arc::from(id.name.as_str())),
        TSImportTypeQualifier::QualifiedName(qualified) => {
            collect_import_qualifier_parts(&qualified.left, parts);
            parts.push(Arc::from(qualified.right.name.as_str()));
        }
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
        // `typeof import("./m")` / `typeof import("./m").member`: the type
        // query targets a dynamic-import VALUE namespace. Lower to the
        // typed-IR `ImportType` carrier (`typeof_query == true`) — the
        // shared dispatch resolves the module's value exports cross-file.
        TSTypeQueryExprName::TSImportType(import) => {
            return lower_import_type(import, source, true);
        }
        _ => {
            return TypeExpr::Unknown {
                raw: span_text(source, query.span),
            };
        }
    };

    // Instantiation expression under typeof: `typeof C.make<string>` carries
    // its type arguments on the query node. They select the generic
    // instantiation of the referenced value — lower them structurally.
    let type_args: Vec<TypeExpr> = query
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

    TypeExpr::TypeOf(ValueRef { path, type_args })
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

            let spans = MemberSpans {
                declaration: Some(prop.span.into()),
                name: Some(prop.key.span().into()),
                type_annotation: prop
                    .type_annotation
                    .as_ref()
                    .map(|ta| ta.type_annotation.span().into()),
            };
            Some(ObjectMember::Property(ObjectProperty::with_spans_public(
                name,
                ty,
                prop.optional,
                prop.readonly,
                spans,
            )))
        }
        TSSignature::TSMethodSignature(method) => {
            let name = property_key_name(&method.key)?;
            let func = normalize_function_type_params(FunctionExpr::with_spans(
                lower_formal_parameters(&method.params, source),
                method
                    .return_type
                    .as_ref()
                    .map(|rt| Arc::new(lower_ts_type(&rt.type_annotation, source))),
                method
                    .type_parameters
                    .as_ref()
                    .map(|tp| lower_type_params(tp, source))
                    .unwrap_or_default(),
                FunctionSpans {
                    signature: Some(method.span.into()),
                    return_type: method
                        .return_type
                        .as_ref()
                        .map(|rt| rt.type_annotation.span().into()),
                },
            ));
            let spans = MemberSpans {
                declaration: Some(method.span.into()),
                name: Some(method.key.span().into()),
                type_annotation: None,
            };
            Some(ObjectMember::Method(MethodSignature::with_spans_public(
                name,
                func,
                method.optional,
                spans,
            )))
        }
        TSSignature::TSCallSignatureDeclaration(call) => {
            let func = normalize_function_type_params(FunctionExpr::with_spans(
                lower_formal_parameters(&call.params, source),
                call.return_type
                    .as_ref()
                    .map(|rt| Arc::new(lower_ts_type(&rt.type_annotation, source))),
                call.type_parameters
                    .as_ref()
                    .map(|tp| lower_type_params(tp, source))
                    .unwrap_or_default(),
                FunctionSpans {
                    signature: Some(call.span.into()),
                    return_type: call
                        .return_type
                        .as_ref()
                        .map(|rt| rt.type_annotation.span().into()),
                },
            ));
            Some(ObjectMember::CallSignature(func))
        }
        TSSignature::TSIndexSignature(idx) => {
            let (key_name, key_type, key_span) = if let Some(param) = idx.parameters.first() {
                let name = param.name.to_string();
                let ty = lower_ts_type(&param.type_annotation.type_annotation, source);
                (name, ty, Some(param.span.into()))
            } else {
                (
                    "key".to_string(),
                    TypeExpr::Primitive(PrimitiveName::String),
                    None,
                )
            };

            let value_type = lower_ts_type(&idx.type_annotation.type_annotation, source);
            let spans = IndexSignatureSpans {
                declaration: Some(idx.span.into()),
                key: key_span,
                value: Some(idx.type_annotation.type_annotation.span().into()),
            };
            Some(ObjectMember::IndexSignature(IndexSignature::with_spans(
                key_name,
                key_type,
                value_type,
                idx.readonly,
                spans,
            )))
        }
        TSSignature::TSConstructSignatureDeclaration(ctor) => {
            let func = normalize_function_type_params(FunctionExpr::with_spans(
                lower_formal_parameters(&ctor.params, source),
                ctor.return_type
                    .as_ref()
                    .map(|rt| Arc::new(lower_ts_type(&rt.type_annotation, source))),
                ctor.type_parameters
                    .as_ref()
                    .map(|tp| lower_type_params(tp, source))
                    .unwrap_or_default(),
                FunctionSpans {
                    signature: Some(ctor.span.into()),
                    return_type: ctor
                        .return_type
                        .as_ref()
                        .map(|rt| rt.type_annotation.span().into()),
                },
            ));
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
    normalize_function_type_params(FunctionExpr::with_spans(
        lower_formal_parameters(&func.params, source),
        Some(Arc::new(lower_ts_type(
            &func.return_type.type_annotation,
            source,
        ))),
        func.type_parameters
            .as_ref()
            .map(|tp| lower_type_params(tp, source))
            .unwrap_or_default(),
        FunctionSpans {
            signature: Some(func.span.into()),
            return_type: Some(func.return_type.type_annotation.span().into()),
        },
    ))
}

fn lower_formal_parameters(params: &FormalParameters<'_>, source: &str) -> Vec<FunctionParam> {
    params
        .items
        .iter()
        .map(|param| {
            let name = binding_pattern_name(&param.pattern);
            // OXC structural fact: did this parameter carry an explicit TS
            // annotation? (An explicit `: any` lowers to `Primitive(Any)` like a
            // missing annotation, so the lowered `ty` cannot distinguish them.)
            let has_ts_annotation = param.type_annotation.is_some();
            let ty = param
                .type_annotation
                .as_ref()
                .map(|ta| lower_ts_type(&ta.type_annotation, source))
                .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
            FunctionParam::with_span(
                name,
                ty,
                param.optional,
                false,
                Some(param.span().into()),
                has_ts_annotation,
            )
        })
        .chain(params.rest.as_ref().map(|rest| {
            let name = binding_pattern_name(&rest.rest.argument);
            let has_ts_annotation = rest.type_annotation.is_some();
            let ty = rest
                .type_annotation
                .as_ref()
                .map(|ta| lower_ts_type(&ta.type_annotation, source))
                .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
            FunctionParam::with_span(
                name,
                ty,
                false,
                true,
                Some(rest.span().into()),
                has_ts_annotation,
            )
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
        // `import("m").Gen<T>` — only the instantiation type-arguments can
        // reference the enclosing generic scope; specifier / qualifier /
        // typeof_query are leaves. Normalise the arguments exactly as `Ref`.
        TypeExpr::ImportType {
            specifier,
            qualifier,
            typeof_query,
            type_arguments,
        } => TypeExpr::ImportType {
            specifier: Arc::clone(specifier),
            qualifier: Arc::clone(qualifier),
            typeof_query: *typeof_query,
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
        // A constructor type carries the same `FunctionExpr` payload as a
        // function type — its parameters / return may reference the enclosing
        // generic scope (e.g. `<T>(...) => new () => T[]`), so it normalises
        // identically; only the variant tag differs.
        TypeExpr::ConstructorType(func) => TypeExpr::ConstructorType(Arc::new(
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
        // Synthetic slot-binding carrier is a TERMINAL leaf. The carrier
        // is shallow-by-construction and is never resolved as a type
        // alias via the type registry — return it unchanged.
        TypeExpr::TypeParameter(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Unknown { .. } => expr.clone(),
    }
}

fn normalize_object_member_type_params(member: &ObjectMember, scope: &[TypeParam]) -> ObjectMember {
    match member {
        // Reconstruction of an EXISTING member (only the type-parameter refs in
        // its value are rewritten): preserve the member's declared accessibility
        // via `with_visibility`. `with_spans` would default it to Public,
        // dropping a non-public class member's visibility when its generic
        // instance shape is normalized.
        ObjectMember::Property(prop) => ObjectMember::Property(ObjectProperty::with_visibility(
            prop.name.clone(),
            normalize_type_parameter_refs(&prop.ty, scope),
            prop.optional,
            prop.readonly,
            prop.visibility,
            prop.spans,
        )),
        ObjectMember::IndexSignature(sig) => {
            ObjectMember::IndexSignature(IndexSignature::with_spans(
                sig.key_name.clone(),
                normalize_type_parameter_refs(&sig.key_type, scope),
                normalize_type_parameter_refs(&sig.value_type, scope),
                sig.readonly,
                sig.spans,
            ))
        }
        ObjectMember::CallSignature(func) => {
            ObjectMember::CallSignature(normalize_nested_function_type_params(func, scope))
        }
        ObjectMember::ConstructSignature(func) => {
            ObjectMember::ConstructSignature(normalize_nested_function_type_params(func, scope))
        }
        ObjectMember::Method(method) => ObjectMember::Method(MethodSignature::with_visibility(
            method.name.clone(),
            normalize_nested_function_type_params(&method.function, scope),
            method.optional,
            method.visibility,
            method.spans,
        )),
    }
}

fn normalize_nested_function_type_params(func: &FunctionExpr, scope: &[TypeParam]) -> FunctionExpr {
    let mut combined_scope = scope.to_vec();
    let nested_scope = normalize_type_parameter_decls(func.type_parameters.clone());
    combined_scope.extend(nested_scope.clone());

    FunctionExpr::with_spans(
        func.parameters
            .iter()
            .map(|param| {
                FunctionParam::with_span(
                    param.name.clone(),
                    normalize_type_parameter_refs(&param.ty, &combined_scope),
                    param.optional,
                    param.rest,
                    param.span,
                    param.has_ts_annotation,
                )
            })
            .collect(),
        func.return_type
            .as_ref()
            .map(|ret| Arc::new(normalize_type_parameter_refs(ret.as_ref(), &combined_scope))),
        nested_scope,
        func.spans,
    )
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

#[cfg(test)]
mod synthetic_carrier_tests {
    //! S1 discrimination tests for the `TypeExpr::SyntheticSlotBinding`
    //! variant's traversal through OXC normalisation helpers.
    //!
    //! Contract: synthetic carrier is a TERMINAL leaf — the
    //! type-parameter normalisation walk must return it unchanged
    //! (cheap Arc clone; pointer equality holds).
    use super::*;
    use verter_type_expr::{SyntheticCarrierKey, SyntheticCarrierSurfaceKind, TypeExpr};

    fn make_carrier() -> TypeExpr {
        TypeExpr::synthetic_slot_binding(SyntheticCarrierKey {
            scope_canonical_id: Arc::from("/abs/Foo.vue"),
            surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
            slot_name: Some(Arc::from("default")),
            binding_name: Arc::from("controls"),
            value_node: 42,
        })
    }

    #[test]
    fn synthetic_carrier_oxc_normalize_terminal() {
        let carrier = make_carrier();
        let normalised = normalize_type_parameter_refs(&carrier, &[]);
        // Structural equality holds.
        assert_eq!(carrier, normalised);
        // The walk took the terminal branch — the returned Arc is the
        // SAME Arc as the input (cheap clone), not a freshly minted one.
        if let (
            TypeExpr::SyntheticSlotBinding(input_key),
            TypeExpr::SyntheticSlotBinding(out_key),
        ) = (&carrier, &normalised)
        {
            assert!(
                Arc::ptr_eq(input_key, out_key),
                "synthetic carrier traversed through normalize_type_parameter_refs must reuse the input Arc"
            );
        } else {
            panic!(
                "expected SyntheticSlotBinding on both sides; got input={:?} output={:?}",
                carrier, normalised
            );
        }
    }

    /// `normalize_object_member_type_params` (reached via
    /// `normalize_type_parameter_refs` over a `TypeExpr::Object`) rebuilds each
    /// member to rewrite its type-parameter refs. That rebuild MUST preserve the
    /// member's declared accessibility — it is a reconstruction of an existing
    /// member, not a fresh mint.
    ///
    /// Discriminating: against the tree where the rebuild uses `with_spans`, the
    /// normalized `protected`/`private` members come back `Public` and the
    /// assertions FAIL.
    #[test]
    fn normalize_object_member_preserves_member_visibility() {
        use verter_type_expr::{
            FunctionExpr, MemberSpans, MemberVisibility, MethodSignature, ObjectExpr, ObjectMember,
            ObjectProperty, PrimitiveName,
        };

        // A member value referencing a type parameter `T` forces the normalize
        // walk to actually rebuild the member (rewrites the ref).
        let t_ref = TypeExpr::Ref {
            name: Arc::from("T"),
            type_arguments: Arc::from(Vec::new()),
        };
        let object = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty::with_visibility(
                    "prot".to_string(),
                    t_ref.clone(),
                    false,
                    false,
                    MemberVisibility::Protected,
                    MemberSpans::default(),
                )),
                ObjectMember::Method(MethodSignature::with_visibility(
                    "priv".to_string(),
                    FunctionExpr::synthetic(Vec::new(), Some(Arc::new(t_ref.clone())), Vec::new()),
                    false,
                    MemberVisibility::Private,
                    MemberSpans::default(),
                )),
                ObjectMember::Property(ObjectProperty::with_visibility(
                    "pub_field".to_string(),
                    TypeExpr::Primitive(PrimitiveName::String),
                    false,
                    false,
                    MemberVisibility::Public,
                    MemberSpans::default(),
                )),
            ],
        }));

        let scope = vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }];
        let normalized = normalize_type_parameter_refs(&object, &scope);
        let TypeExpr::Object(obj) = &normalized else {
            panic!("expected object, got {normalized:?}");
        };

        let prot = obj
            .properties
            .iter()
            .find_map(|m| match m {
                ObjectMember::Property(p) if p.name == "prot" => Some(p.visibility),
                _ => None,
            })
            .expect("`prot` property must survive normalization");
        assert_eq!(
            prot,
            MemberVisibility::Protected,
            "a protected property must keep its visibility through normalize",
        );

        let priv_method = obj
            .properties
            .iter()
            .find_map(|m| match m {
                ObjectMember::Method(m) if m.name == "priv" => Some(m.visibility),
                _ => None,
            })
            .expect("`priv` method must survive normalization");
        assert_eq!(
            priv_method,
            MemberVisibility::Private,
            "a private method must keep its visibility through normalize",
        );

        let pub_field = obj
            .properties
            .iter()
            .find_map(|m| match m {
                ObjectMember::Property(p) if p.name == "pub_field" => Some(p.visibility),
                _ => None,
            })
            .expect("`pub_field` property must survive normalization");
        assert_eq!(pub_field, MemberVisibility::Public);
    }
}
