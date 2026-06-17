//! Type-element resolvers: type literals, mapped types, property/method
//! signatures, and the helpers that lift TypeScript signature shapes into the
//! resolver's `ResolvedElements`.
//!
//! This is the layer the type kernel hands off to once the structural body of
//! a type has been narrowed (the named-type / heritage / utility stages live
//! in `super::mod.rs`). Mapped-type key resolution, property/method emit
//! lowering, and the `extract_string_literal_keys` family for `Pick`/`Omit`
//! style heritage utilities all live here.

use oxc_ast::ast::*;
use oxc_span::GetSpan;
use std::sync::Arc;
use verter_type_expr::{FunctionExpr, FunctionParam, FunctionSpans, PrimitiveName, TypeExpr};
use verter_type_expr_oxc::lower_ts_type;

use crate::common::Span;

/// Lower an OXC `TSType<'_>` into a `TypeExpr`, decoding `source` as UTF-8.
/// `source` typically comes from `&[u8]` resolver inputs; on invalid UTF-8 we
/// fall back to an empty slice — `lower_ts_type` only consults `source` for
/// raw-text fallback / literal extraction, so an empty slice degrades to
/// `TypeExpr::Unknown { raw: String::new() }` for the affected sub-nodes.
#[inline]
pub(super) fn lower_ts_type_from_bytes(ts_type: &TSType<'_>, source: &[u8]) -> TypeExpr {
    let source_str = std::str::from_utf8(source).unwrap_or("");
    lower_ts_type(ts_type, source_str)
}

/// Lower a call signature's emit payload into a `TypeExpr::Function`.
///
/// The first parameter (`e: 'eventName'`) is the event-name selector and is
/// dropped — consumers reading `type_expr` want the payload shape, which is
/// the parameter list AFTER the event name plus any rest parameter. The
/// return type is the call signature's declared return type, defaulting to
/// `void` when none is present.
pub(super) fn lower_call_signature_payload(
    call_sig: &TSCallSignatureDeclaration<'_>,
    source: &[u8],
) -> TypeExpr {
    let source_str = std::str::from_utf8(source).unwrap_or("");
    let mut parameters: Vec<FunctionParam> = Vec::new();
    for param in call_sig.params.items.iter().skip(1) {
        let name = if let BindingPattern::BindingIdentifier(id) = &param.pattern {
            Some(id.name.to_string())
        } else {
            None
        };
        let has_ts_annotation = param.type_annotation.is_some();
        let ty = param
            .type_annotation
            .as_ref()
            .map(|ta| lower_ts_type(&ta.type_annotation, source_str))
            .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
        parameters.push(FunctionParam::with_span(
            name,
            ty,
            param.optional,
            false,
            Some(param.span.into()),
            has_ts_annotation,
        ));
    }
    if let Some(rest) = &call_sig.params.rest {
        let name = if let BindingPattern::BindingIdentifier(id) = &rest.rest.argument {
            Some(id.name.to_string())
        } else {
            None
        };
        let has_ts_annotation = rest.type_annotation.is_some();
        let ty = rest
            .type_annotation
            .as_ref()
            .map(|ta| lower_ts_type(&ta.type_annotation, source_str))
            .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
        parameters.push(FunctionParam::with_span(
            name,
            ty,
            false,
            true,
            Some(rest.span.into()),
            has_ts_annotation,
        ));
    }
    let return_type = call_sig
        .return_type
        .as_ref()
        .map(|rt| Arc::new(lower_ts_type(&rt.type_annotation, source_str)))
        .unwrap_or_else(|| Arc::new(TypeExpr::Primitive(PrimitiveName::Void)));
    let fn_spans = FunctionSpans {
        signature: Some(call_sig.span.into()),
        return_type: call_sig
            .return_type
            .as_ref()
            .map(|rt| rt.type_annotation.span().into()),
    };
    TypeExpr::Function(Arc::new(FunctionExpr::with_spans(
        parameters,
        Some(return_type),
        Vec::new(),
        fn_spans,
    )))
}

use super::{
    get_type_reference_name, infer_runtime_type, resolve_type_elements_with_ctx_ref,
    ResolvedCallPayloadForm, ResolvedElements, ResolvedMemberVisibility,
    ResolvedNamedCallSignature, ResolvedProp, RuntimeType, TypeResolutionContext,
};

/// Resolve members from a type literal's members array.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn resolve_type_literal_members(
    members: &[TSSignature],
    base_offset: u32,
    result: &mut ResolvedElements,
    source: &[u8],
    from_root_body: bool,
) {
    for member in members {
        match member {
            TSSignature::TSPropertySignature(prop) => {
                // Check if this is a shorthand emit: { change: [id: number] }
                // Properties with tuple/array type values are treated as emits
                if let Some(emit) = resolve_property_as_emit(prop, base_offset, source) {
                    result.call_signatures.push(emit);
                } else if let Some(resolved) =
                    resolve_property_signature(prop, base_offset, source, from_root_body)
                {
                    result.props.push(resolved);
                }
            }
            TSSignature::TSMethodSignature(method) => {
                if let Some(resolved) =
                    resolve_method_signature(method, base_offset, source, from_root_body)
                {
                    result.props.push(resolved);
                }
            }
            TSSignature::TSCallSignatureDeclaration(call_sig) => {
                result.has_call_signature = true;
                // Extract emit from call signature: (e: 'change', id: number): void
                if let Some(emit) = resolve_call_signature_as_emit(call_sig, base_offset, source) {
                    result.call_signatures.push(emit);
                }
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedMappedKey {
    name: String,
    key: Span,
    optional: bool,
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn resolve_mapped_type_with_ctx<'ctx, 'a: 'ctx>(
    mapped: &'ctx TSMappedType<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    from_root_body: bool,
) {
    // Renamed mapped keys (`as ...`) need a dedicated key-evaluation path.
    // Until that exists, only materialize direct finite key sets.
    if mapped.name_type.is_some() {
        return;
    }

    let keys = resolve_mapped_type_keys_with_ctx(&mapped.constraint, ctx);
    if keys.is_empty() {
        return;
    }

    let span = Span {
        start: mapped.span.start + base_offset,
        end: mapped.span.end + base_offset,
    };
    let type_span = mapped.type_annotation.as_ref().map(|ann| Span {
        start: ann.span().start + base_offset,
        end: ann.span().end + base_offset,
    });
    let type_text = mapped
        .type_annotation
        .as_ref()
        .and_then(|ann| span_text(ctx.source, ann.span().into()));
    let types = mapped
        .type_annotation
        .as_ref()
        .map(|ann| infer_runtime_type(ann))
        .unwrap_or_else(|| vec![RuntimeType::Unknown]);
    let type_expr = mapped
        .type_annotation
        .as_ref()
        .map(|ann| lower_ts_type_from_bytes(ann, ctx.source));
    let optional_override = mapped_optional_override(mapped.optional);

    for key in keys {
        result.props.push(ResolvedProp {
            span,
            key: Span {
                start: key.key.start + base_offset,
                end: key.key.end + base_offset,
            },
            key_name: Some(key.name),
            optional: optional_override.unwrap_or(key.optional),
            types: types.clone(),
            visibility: ResolvedMemberVisibility::Public,
            type_span,
            type_text: type_text.clone(),
            map_local: true,
            span_is_absolute: base_offset != 0,
            type_expr: type_expr.clone(),
            type_expr_scope: None,
            // Mapped-type members are own-body members of the mapped
            // construction currently being resolved — they reflect the
            // caller's macro-T own-body / heritage context unchanged.
            declared_in_macro_type_arg: from_root_body,
        });
    }

    result.dedup_props();
}

pub(super) fn mapped_optional_override(
    modifier: Option<TSMappedTypeModifierOperator>,
) -> Option<bool> {
    match modifier {
        Some(TSMappedTypeModifierOperator::True | TSMappedTypeModifierOperator::Plus) => Some(true),
        Some(TSMappedTypeModifierOperator::Minus) => Some(false),
        None => None,
    }
}

pub(super) fn resolve_mapped_type_keys_with_ctx<'ctx, 'a: 'ctx>(
    constraint: &'ctx TSType<'a>,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> Vec<ResolvedMappedKey> {
    match constraint {
        TSType::TSTypeOperatorType(op) if matches!(op.operator, TSTypeOperatorOperator::Keyof) => {
            // `keyof X` enumerates X's member NAMES; the resulting
            // ResolvedElements is consumed for `key.key_name` only and
            // its props are discarded. The `from_root_body` value
            // therefore does not affect mapped-key output — but we
            // pass `false` for structural correctness: a `keyof`
            // operand is NOT the macro T's own body.
            let resolved = resolve_type_elements_with_ctx_ref(&op.type_annotation, 0, ctx, false);
            resolved
                .props
                .into_iter()
                .filter_map(|prop| {
                    let name = prop
                        .key_name
                        .clone()
                        .or_else(|| span_text(ctx.source, prop.key))?;
                    Some(ResolvedMappedKey {
                        name,
                        key: prop.key,
                        optional: prop.optional,
                    })
                })
                .collect()
        }
        TSType::TSLiteralType(literal) => resolve_mapped_string_literal_key(literal),
        TSType::TSUnionType(union) => union
            .types
            .iter()
            .flat_map(|ty| resolve_mapped_type_keys_with_ctx(ty, ctx))
            .collect(),
        TSType::TSParenthesizedType(paren) => {
            resolve_mapped_type_keys_with_ctx(&paren.type_annotation, ctx)
        }
        TSType::TSTypeReference(type_ref) => {
            let name = get_type_reference_name(&type_ref.type_name);
            if let Some((aliased_type, _)) = ctx.find_type_alias(name.as_bytes()) {
                resolve_mapped_type_keys_with_ctx(aliased_type, ctx)
            } else {
                extract_string_literal_keys_with_ctx(constraint, Some(ctx))
                    .into_iter()
                    .map(|name| ResolvedMappedKey {
                        name,
                        key: Span::new(0, 0),
                        optional: false,
                    })
                    .collect()
            }
        }
        _ => extract_string_literal_keys_with_ctx(constraint, Some(ctx))
            .into_iter()
            .map(|name| ResolvedMappedKey {
                name,
                key: Span::new(0, 0),
                optional: false,
            })
            .collect(),
    }
}

pub(super) fn resolve_mapped_string_literal_key(
    literal: &TSLiteralType<'_>,
) -> Vec<ResolvedMappedKey> {
    match &literal.literal {
        TSLiteral::StringLiteral(value) => vec![ResolvedMappedKey {
            name: value.value.to_string(),
            key: Span::from(value.span),
            optional: false,
        }],
        _ => Vec::new(),
    }
}

/// Try to resolve a property signature as an emit (shorthand style).
/// Shorthand style: `{ change: [id: number] }` or `{ update: [] }`
pub(super) fn resolve_property_as_emit(
    prop: &TSPropertySignature,
    base_offset: u32,
    source: &[u8],
) -> Option<ResolvedNamedCallSignature> {
    // Get the property key as the event name
    let name = get_property_key_name(&prop.key)?;
    let key_span = get_property_key_span(&prop.key, base_offset)?;

    // Check if the type is a tuple type - this indicates emit shorthand
    // Note: Only TSTupleType (e.g., `[id: number]`) is emit shorthand.
    // TSArrayType (e.g., `string[]`) is a regular array prop type.
    if let Some(ann) = &prop.type_annotation {
        if let TSType::TSTupleType(_) = &ann.type_annotation {
            let tuple_text = slice_source_span(
                source,
                ann.type_annotation.span().start,
                ann.type_annotation.span().end,
            )?;
            let type_expr = lower_ts_type_from_bytes(&ann.type_annotation, source);
            return Some(ResolvedNamedCallSignature {
                span: Span {
                    start: prop.span.start + base_offset,
                    end: prop.span.end + base_offset,
                },
                name,
                name_span: Some(key_span),
                signature: ResolvedCallPayloadForm::Tuple { tuple_text },
                map_local: true,
                span_is_absolute: base_offset != 0,
                type_expr: Some(type_expr),
                type_expr_scope: None,
            });
        }
    }

    None
}

/// Resolve a call signature as an emit event.
/// Call signature style: `(e: 'change', id: number): void`
/// The event name is extracted from the first parameter's type if it's a string literal.
pub(super) fn resolve_call_signature_as_emit(
    call_sig: &TSCallSignatureDeclaration,
    base_offset: u32,
    source: &[u8],
) -> Option<ResolvedNamedCallSignature> {
    // Get the first parameter - should be like `e: 'eventName'`
    let first_param = call_sig.params.items.first()?;

    // The type annotation is on the FormalParameter, not the pattern
    let type_ann = first_param.type_annotation.as_ref()?;

    // Extract event name from string literal type
    if let TSType::TSLiteralType(lit) = &type_ann.type_annotation {
        if let TSLiteral::StringLiteral(s) = &lit.literal {
            let mut params_text = String::new();
            for param in call_sig.params.items.iter().skip(1) {
                if !params_text.is_empty() {
                    params_text.push_str(", ");
                }
                params_text.push_str(&slice_source_span(
                    source,
                    param.span().start,
                    param.span().end,
                )?);
            }
            if let Some(rest) = &call_sig.params.rest {
                if !params_text.is_empty() {
                    params_text.push_str(", ");
                }
                params_text.push_str(&slice_source_span(source, rest.span.start, rest.span.end)?);
            }
            // Lower the call signature into a `TypeExpr::Function` whose
            // `parameters` are the post-event-name params (the actual emit
            // payload). The first param (e: 'eventName') is the event-name
            // selector; consumers reading `type_expr` want the payload shape.
            let type_expr = lower_call_signature_payload(call_sig, source);
            return Some(ResolvedNamedCallSignature {
                span: Span {
                    start: call_sig.span.start + base_offset,
                    end: call_sig.span.end + base_offset,
                },
                name: s.value.to_string(),
                name_span: Some(Span {
                    start: s.span.start + base_offset,
                    end: s.span.end + base_offset,
                }),
                signature: ResolvedCallPayloadForm::Call { params_text },
                map_local: true,
                span_is_absolute: base_offset != 0,
                type_expr: Some(type_expr),
                type_expr_scope: None,
            });
        }
    }

    None
}

/// Get the name of a property key as a string.
pub(super) fn get_property_key_name(key: &PropertyKey) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        PropertyKey::NumericLiteral(n) => n.raw.as_ref().map(|r| r.to_string()),
        _ => None,
    }
}

/// Extract string literal keys from a type argument (supports single literal and unions).
/// Used for `Omit<T, 'a' | 'b'>` and `Pick<T, 'a' | 'b'>`.
pub(super) fn extract_string_literal_keys(ty: &TSType) -> Vec<String> {
    extract_string_literal_keys_with_ctx(ty, None)
}

/// Extract string literal keys from a type, optionally following type alias references
/// when a context is available. This is critical for `Omit<T, KeysAlias | 'literal'>`
/// where `KeysAlias` is a type alias expanding to a union of string literals.
pub(super) fn extract_string_literal_keys_with_ctx<'ctx, 'a: 'ctx>(
    ty: &TSType<'a>,
    ctx: Option<&TypeResolutionContext<'ctx, 'a>>,
) -> Vec<String> {
    let mut visited = Vec::new();
    extract_string_literal_keys_inner(ty, ctx, &mut visited)
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn extract_string_literal_keys_inner<'ctx, 'a: 'ctx>(
    ty: &TSType<'a>,
    ctx: Option<&TypeResolutionContext<'ctx, 'a>>,
    visited: &mut Vec<String>,
) -> Vec<String> {
    match ty {
        TSType::TSLiteralType(lit) => {
            if let TSLiteral::StringLiteral(s) = &lit.literal {
                vec![s.value.to_string()]
            } else {
                vec![]
            }
        }
        TSType::TSUnionType(union) => union
            .types
            .iter()
            .flat_map(|t| extract_string_literal_keys_inner(t, ctx, visited))
            .collect(),
        TSType::TSParenthesizedType(paren) => {
            extract_string_literal_keys_inner(&paren.type_annotation, ctx, visited)
        }
        TSType::TSTypeReference(type_ref) if ctx.is_some() => {
            let ctx = ctx.unwrap();
            let name = type_ref.type_name.to_string();
            // Recursion guard: prevent infinite loops on circular type aliases
            if visited.contains(&name) {
                return vec![];
            }
            visited.push(name.clone());
            let name_bytes = name.as_bytes();
            // Follow local type aliases to extract their string literal keys
            let result = if let Some((aliased_type, _)) = ctx.find_type_alias(name_bytes) {
                extract_string_literal_keys_inner(aliased_type, Some(ctx), visited)
            } else {
                vec![]
            };
            visited.pop();
            result
        }
        _ => vec![],
    }
}

pub(super) fn slice_source_span(source: &[u8], start: u32, end: u32) -> Option<String> {
    let start = start as usize;
    let end = end as usize;
    if end > source.len() || start > end {
        return None;
    }
    std::str::from_utf8(&source[start..end])
        .ok()
        .map(|s| s.trim().to_string())
}

pub(super) fn has_immediate_vue_ignore_comment(source: &[u8], start: u32) -> bool {
    let start = start as usize;
    if start == 0 || start > source.len() {
        return false;
    }

    let window_start = start.saturating_sub(160);
    let prefix = match std::str::from_utf8(&source[window_start..start]) {
        Ok(text) => text.trim_end(),
        Err(_) => return false,
    };

    if let Some(comment_start) = prefix.rfind("/*") {
        let comment = &prefix[comment_start..];
        return comment.ends_with("*/") && comment.contains("@vue-ignore");
    }

    false
}

/// Resolve a property signature to a ResolvedProp.
///
/// `from_root_body` is stamped onto the produced `ResolvedProp` as the
/// `declared_in_macro_type_arg` fact. See [`resolve_type_literal_members`]
/// for the propagation contract.
pub(super) fn resolve_property_signature(
    prop: &TSPropertySignature,
    base_offset: u32,
    source: &[u8],
    from_root_body: bool,
) -> Option<ResolvedProp> {
    let key = get_property_key_span(&prop.key, base_offset)?;
    let optional = prop.optional;

    let types = prop
        .type_annotation
        .as_ref()
        .map(|ann| infer_runtime_type(&ann.type_annotation))
        .unwrap_or_else(|| vec![RuntimeType::Unknown]);

    // Full span from the property signature, adjusted by base_offset
    let span = Span {
        start: prop.span.start + base_offset,
        end: prop.span.end + base_offset,
    };

    let type_span = prop.type_annotation.as_ref().map(|ann| Span {
        start: ann.type_annotation.span().start + base_offset,
        end: ann.type_annotation.span().end + base_offset,
    });
    let type_text = prop
        .type_annotation
        .as_ref()
        .and_then(|ann| span_text(source, ann.type_annotation.span().into()));
    let type_expr = prop
        .type_annotation
        .as_ref()
        .map(|ann| lower_ts_type_from_bytes(&ann.type_annotation, source));

    Some(ResolvedProp {
        span,
        key,
        key_name: get_property_key_name(&prop.key),
        optional,
        types,
        visibility: ResolvedMemberVisibility::Public,
        type_span,
        type_text,
        map_local: true,
        span_is_absolute: base_offset != 0,
        type_expr,
        type_expr_scope: None,
        declared_in_macro_type_arg: from_root_body,
    })
}

/// Resolve a method signature to a ResolvedProp (methods are function-typed properties).
///
/// `from_root_body` is stamped onto the produced `ResolvedProp` as the
/// `declared_in_macro_type_arg` fact.
pub(super) fn resolve_method_signature(
    method: &TSMethodSignature,
    base_offset: u32,
    source: &[u8],
    from_root_body: bool,
) -> Option<ResolvedProp> {
    let key = get_property_key_span(&method.key, base_offset)?;
    let optional = method.optional;

    // Full span from the method signature, adjusted by base_offset
    let span = Span {
        start: method.span.start + base_offset,
        end: method.span.end + base_offset,
    };

    let type_expr = lower_method_signature(method, source);
    Some(ResolvedProp {
        span,
        key,
        key_name: get_property_key_name(&method.key),
        optional,
        types: vec![RuntimeType::Function],
        visibility: ResolvedMemberVisibility::Public,
        type_span: None,
        type_text: callable_signature_text(
            source,
            &method.params.items,
            method
                .return_type
                .as_ref()
                .map(|return_type| &return_type.type_annotation),
        ),
        map_local: true,
        span_is_absolute: base_offset != 0,
        type_expr: Some(type_expr),
        type_expr_scope: None,
        declared_in_macro_type_arg: from_root_body,
    })
}

/// Lower a method signature into a `TypeExpr::Function` representing the
/// method's call shape: `(...params) => return_type`. Used for property
/// surfaces declared as method signatures (`onClick(): void`).
pub(super) fn lower_method_signature(method: &TSMethodSignature<'_>, source: &[u8]) -> TypeExpr {
    let source_str = std::str::from_utf8(source).unwrap_or("");
    lower_function_shape(
        &method.params,
        method.return_type.as_deref().map(|rt| &rt.type_annotation),
        source_str,
    )
}

/// Lower a class method's shape (used by `resolve_class_method_definition`
/// for class declarations: `class C { onClick(): void {} }`).
pub(super) fn lower_method_signature_for_class(
    method: &MethodDefinition<'_>,
    source: &[u8],
) -> TypeExpr {
    let source_str = std::str::from_utf8(source).unwrap_or("");
    lower_function_shape(
        &method.value.params,
        method
            .value
            .return_type
            .as_deref()
            .map(|rt| &rt.type_annotation),
        source_str,
    )
}

/// Build a `TypeExpr::Function` from a `FormalParameters` and an optional
/// return-type annotation. Shared by emit call signatures, type-literal
/// method signatures, and class method definitions.
fn lower_function_shape(
    params: &FormalParameters<'_>,
    return_type: Option<&TSType<'_>>,
    source_str: &str,
) -> TypeExpr {
    let mut parameters: Vec<FunctionParam> = Vec::new();
    for param in params.items.iter() {
        let name = if let BindingPattern::BindingIdentifier(id) = &param.pattern {
            Some(id.name.to_string())
        } else {
            None
        };
        let has_ts_annotation = param.type_annotation.is_some();
        let ty = param
            .type_annotation
            .as_ref()
            .map(|ta| lower_ts_type(&ta.type_annotation, source_str))
            .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
        parameters.push(FunctionParam::with_span(
            name,
            ty,
            param.optional,
            false,
            Some(param.span.into()),
            has_ts_annotation,
        ));
    }
    if let Some(rest) = &params.rest {
        let name = if let BindingPattern::BindingIdentifier(id) = &rest.rest.argument {
            Some(id.name.to_string())
        } else {
            None
        };
        let has_ts_annotation = rest.type_annotation.is_some();
        let ty = rest
            .type_annotation
            .as_ref()
            .map(|ta| lower_ts_type(&ta.type_annotation, source_str))
            .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
        parameters.push(FunctionParam::with_span(
            name,
            ty,
            false,
            true,
            Some(rest.span.into()),
            has_ts_annotation,
        ));
    }
    let return_type_expr = return_type
        .map(|rt| Arc::new(lower_ts_type(rt, source_str)))
        .unwrap_or_else(|| Arc::new(TypeExpr::Primitive(PrimitiveName::Void)));
    // No enclosing signature node is available here (parameters and return
    // type arrive separately), so `signature` stays `None`; the return-type
    // span is recovered from the supplied return-type node.
    let fn_spans = FunctionSpans {
        signature: None,
        return_type: return_type.map(|rt| rt.span().into()),
    };
    TypeExpr::Function(Arc::new(FunctionExpr::with_spans(
        parameters,
        Some(return_type_expr),
        Vec::new(),
        fn_spans,
    )))
}

pub(super) fn callable_signature_text<'a>(
    source: &[u8],
    params: &[FormalParameter<'a>],
    return_type: Option<&TSType<'a>>,
) -> Option<String> {
    let params = params
        .iter()
        .map(|param| {
            let name = span_text(source, param.pattern.span().into()).unwrap_or("_".to_string());
            let mut rendered = name.trim().to_string();
            if let Some(type_annotation) = &param.type_annotation {
                if let Some(type_text) =
                    span_text(source, type_annotation.type_annotation.span().into())
                {
                    rendered.push_str(": ");
                    rendered.push_str(type_text.trim());
                }
            }
            rendered
        })
        .collect::<Vec<_>>();
    let return_type = return_type
        .and_then(|return_type| span_text(source, return_type.span().into()))
        .unwrap_or_else(|| "void".to_string());
    Some(format!("({}) => {}", params.join(", "), return_type.trim()))
}

pub(super) fn span_text(source: &[u8], span: Span) -> Option<String> {
    let start = span.start as usize;
    let end = span.end as usize;
    if start >= end || end > source.len() {
        return None;
    }
    std::str::from_utf8(&source[start..end])
        .ok()
        .map(ToString::to_string)
}

/// Extract the span of a property key.
pub(super) fn get_property_key_span(key: &PropertyKey, base_offset: u32) -> Option<Span> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(Span {
            start: id.span.start + base_offset,
            end: id.span.end + base_offset,
        }),
        PropertyKey::StringLiteral(s) => Some(Span {
            start: s.span.start + base_offset,
            end: s.span.end + base_offset,
        }),
        PropertyKey::NumericLiteral(n) => Some(Span {
            start: n.span.start + base_offset,
            end: n.span.end + base_offset,
        }),
        // Computed keys are not supported
        _ => None,
    }
}
