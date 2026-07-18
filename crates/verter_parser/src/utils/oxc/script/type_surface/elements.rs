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

use crate::common::Span;

use super::{
    get_type_reference_name, infer_runtime_type, resolve_type_elements_with_ctx_ref,
    ResolvedCallPayloadForm, ResolvedElements, ResolvedMemberVisibility,
    ResolvedNamedCallSignature, ResolvedProp, RuntimeType, TypeResolutionContext,
};

/// Context-aware runtime type inference.
///
/// Behaves like [`infer_runtime_type`] except that a bare reference to a
/// generic type parameter bound in the current instantiation
/// (`ctx.type_param_bindings`) resolves to the bound type's runtime type:
/// `Foo<string>` makes a member `value: T` a `String`, and `T extends
/// number` makes it a `Number`. A type-parameter DEFAULT is never bound
/// (see [`super::choose_type_param_bound`]), so an un-instantiated
/// defaulted parameter stays `Unknown` — Vue does not lower a type-param
/// default to a runtime prop constructor.
///
/// Only a genuine bound type parameter is substituted: a reference whose
/// name resolves to a local alias / interface / class keeps its ordinary
/// resolution, and generic references (`T<...>`) are left untouched.
pub(super) fn infer_runtime_type_with_ctx<'ctx, 'a: 'ctx>(
    node: &TSType<'a>,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> Vec<RuntimeType> {
    let mut visited = Vec::new();
    infer_runtime_type_with_ctx_inner(node, ctx, &mut visited)
}

fn infer_runtime_type_with_ctx_inner<'ctx, 'a: 'ctx>(
    node: &TSType<'a>,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    visited: &mut Vec<String>,
) -> Vec<RuntimeType> {
    // Peel parentheses so `(T)` substitutes like `T`.
    let mut inner = node;
    while let TSType::TSParenthesizedType(paren) = inner {
        inner = &paren.type_annotation;
    }

    if let TSType::TSTypeReference(type_ref) = inner {
        if type_ref.type_arguments.is_none() {
            let name = get_type_reference_name(&type_ref.type_name);
            let name_bytes = name.as_bytes();
            // Never shadow a local named type of the same spelling.
            if ctx.find_type_alias(name_bytes).is_none()
                && ctx.find_interface(name_bytes).is_none()
                && ctx.find_class(name_bytes).is_none()
            {
                if visited.contains(&name) {
                    // Cyclic binding (`T -> U -> T`): stop substituting and
                    // fall back to the context-free inference.
                    return infer_runtime_type(node);
                }
                if let Some(bound) = ctx.find_type_param(name_bytes) {
                    visited.push(name);
                    let resolved = infer_runtime_type_with_ctx_inner(bound, ctx, visited);
                    visited.pop();
                    return resolved;
                }
            }
        }
    }

    infer_runtime_type(node)
}

/// Resolve members from a type literal's members array.
pub(super) fn resolve_type_literal_members(
    members: &[TSSignature],
    base_offset: u32,
    result: &mut ResolvedElements,
    source: &[u8],
    from_root_body: bool,
) {
    resolve_type_literal_members_with_ctx(
        members,
        base_offset,
        result,
        source,
        from_root_body,
        None,
    );
}

/// Like [`resolve_type_literal_members`], but with an optional local
/// [`TypeResolutionContext`]. The context extends the named-tuple emit
/// shorthand (`change: [id: number]`) to member VALUE types that are
/// indexed accesses resolving to a tuple through local declarations —
/// `escapeKeydown: LayerEmits['escapeKeydown']`, the reka-ui /
/// oku-primitives `DismissableLayer` emit-forwarding pattern.
///
/// Classification is AST-shape driven (alias / interface lookups through
/// the context walking to a `TSTupleType` node) — never display-text
/// driven. With `ctx = None`, or when the indexed access does not resolve
/// to a tuple, behavior is identical to [`resolve_type_literal_members`]:
/// the member stays a plain prop.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn resolve_type_literal_members_with_ctx<'ctx, 'a: 'ctx>(
    members: &[TSSignature],
    base_offset: u32,
    result: &mut ResolvedElements,
    source: &[u8],
    from_root_body: bool,
    ctx: Option<&TypeResolutionContext<'ctx, 'a>>,
) {
    // A tuple-shaped member VALUE (`change: [id: number]`) or an
    // indexed-access-to-tuple member VALUE (`escapeKeydown:
    // LayerEmits['escapeKeydown']`) is the Vue emit shorthand ONLY when the
    // type feeds an emits surface. On a props surface the same member is a
    // genuine prop, so the reclassification is suppressed (F22). When the
    // surface is unknown (`ctx = None`, or no surface set) the legacy
    // reclassifying behavior is preserved.
    let reclassify_tuple_as_emit = !ctx.is_some_and(|ctx| ctx.is_props_surface());

    for member in members {
        match member {
            TSSignature::TSPropertySignature(prop) => {
                let emit = if reclassify_tuple_as_emit {
                    // Direct tuple shorthand (`change: [id: number]`) first,
                    // then the local-context indexed-access-to-tuple form.
                    resolve_property_as_emit(prop, base_offset, source).or_else(|| {
                        ctx.and_then(|ctx| resolve_property_as_emit_via_ctx(prop, base_offset, ctx))
                    })
                } else {
                    None
                };
                if let Some(emit) = emit {
                    result.call_signatures.push(emit);
                } else if let Some(resolved) =
                    resolve_property_signature(prop, base_offset, source, from_root_body, ctx)
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

/// Named-tuple emit shorthand through the local type context: the property
/// VALUE is an indexed access (`Emits['key']`, optionally parenthesized)
/// whose target member resolves to a tuple type.
///
/// Scoped to indexed accesses on purpose: a bare alias reference to a tuple
/// (`coords: TupleAlias`) keeps its pre-existing plain-prop classification,
/// matching the source-only path, so props surfaces are not re-classified.
fn resolve_property_as_emit_via_ctx<'ctx, 'a: 'ctx>(
    prop: &TSPropertySignature<'_>,
    base_offset: u32,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> Option<ResolvedNamedCallSignature> {
    let ann = prop.type_annotation.as_ref()?;
    let mut value = &ann.type_annotation;
    while let TSType::TSParenthesizedType(paren) = value {
        value = &paren.type_annotation;
    }
    let TSType::TSIndexedAccessType(_) = value else {
        return None;
    };
    let name = get_property_key_name(&prop.key)?;
    let key_span = get_property_key_span(&prop.key, base_offset)?;
    let mut visited = Vec::new();
    let tuple_text = ctx_resolved_tuple_text(value, ctx, &mut visited)?;
    Some(ResolvedNamedCallSignature {
        span: Span {
            start: prop.span.start + base_offset,
            end: prop.span.end + base_offset,
        },
        name,
        name_span: Some(key_span),
        signature: ResolvedCallPayloadForm::Tuple { tuple_text },
        map_local: true,
        span_is_absolute: base_offset != 0,
    })
}

/// Resolve a type through the local context to a TUPLE's source text.
///
/// Follows, AST-shape driven: parenthesized types (transparent); bare
/// (argument-less) type-alias references (`visited` guards name cycles);
/// indexed access with a string-literal index whose object resolves to a
/// type literal, an interface body, or an alias chain to either.
///
/// Returns `None` for everything else — including companion (host-external)
/// surfaces, which carry no AST to slice a tuple from. Callers treat `None`
/// as "not an emit": the member stays a plain prop.
fn ctx_resolved_tuple_text<'ctx, 'a: 'ctx>(
    ty: &TSType<'_>,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    visited: &mut Vec<String>,
) -> Option<String> {
    match ty {
        TSType::TSTupleType(tuple) => {
            slice_source_span(ctx.source, tuple.span.start, tuple.span.end)
        }
        TSType::TSParenthesizedType(paren) => {
            ctx_resolved_tuple_text(&paren.type_annotation, ctx, visited)
        }
        TSType::TSTypeReference(type_ref) if type_ref.type_arguments.is_none() => {
            let name = get_type_reference_name(&type_ref.type_name);
            if visited.contains(&name) {
                return None;
            }
            visited.push(name.clone());
            let result = ctx
                .find_type_alias(name.as_bytes())
                .and_then(|(aliased, _)| ctx_resolved_tuple_text(aliased, ctx, visited));
            visited.pop();
            result
        }
        TSType::TSIndexedAccessType(indexed) => {
            let TSType::TSLiteralType(lit) = &indexed.index_type else {
                return None;
            };
            let TSLiteral::StringLiteral(key) = &lit.literal else {
                return None;
            };
            ctx_indexed_member_tuple_text(&indexed.object_type, key.value.as_str(), ctx, visited)
        }
        _ => None,
    }
}

/// Resolve `Object['key']`: find `key`'s property signature on the object
/// type (type literal, interface body, or an alias chain to either) and
/// resolve its VALUE type to a tuple's source text.
fn ctx_indexed_member_tuple_text<'ctx, 'a: 'ctx>(
    object: &TSType<'_>,
    key: &str,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    visited: &mut Vec<String>,
) -> Option<String> {
    match object {
        TSType::TSTypeLiteral(lit) => member_value_tuple_text(&lit.members, key, ctx, visited),
        TSType::TSParenthesizedType(paren) => {
            ctx_indexed_member_tuple_text(&paren.type_annotation, key, ctx, visited)
        }
        TSType::TSTypeReference(type_ref) if type_ref.type_arguments.is_none() => {
            let name = get_type_reference_name(&type_ref.type_name);
            if visited.contains(&name) {
                return None;
            }
            visited.push(name.clone());
            let result = if let Some((aliased, _)) = ctx.find_type_alias(name.as_bytes()) {
                ctx_indexed_member_tuple_text(aliased, key, ctx, visited)
            } else if let Some((members, _, _, _)) = ctx.find_interface(name.as_bytes()) {
                member_value_tuple_text(members, key, ctx, visited)
            } else {
                None
            };
            visited.pop();
            result
        }
        _ => None,
    }
}

/// Find `key` among property signatures and resolve its value to a tuple's
/// source text.
fn member_value_tuple_text<'ctx, 'a: 'ctx>(
    members: &[TSSignature<'_>],
    key: &str,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    visited: &mut Vec<String>,
) -> Option<String> {
    members.iter().find_map(|member| {
        let TSSignature::TSPropertySignature(prop) = member else {
            return None;
        };
        if get_property_key_name(&prop.key).as_deref() != Some(key) {
            return None;
        }
        let ann = prop.type_annotation.as_ref()?;
        ctx_resolved_tuple_text(&ann.type_annotation, ctx, visited)
    })
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
        .map(|ann| infer_runtime_type_with_ctx(ann, ctx))
        .unwrap_or_else(|| vec![RuntimeType::Unknown]);
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
pub(super) fn resolve_property_signature<'ctx, 'a: 'ctx>(
    prop: &TSPropertySignature<'a>,
    base_offset: u32,
    source: &[u8],
    from_root_body: bool,
    ctx: Option<&TypeResolutionContext<'ctx, 'a>>,
) -> Option<ResolvedProp> {
    let key = get_property_key_span(&prop.key, base_offset)?;
    let optional = prop.optional;

    let types = prop
        .type_annotation
        .as_ref()
        .map(|ann| match ctx {
            Some(ctx) => infer_runtime_type_with_ctx(&ann.type_annotation, ctx),
            None => infer_runtime_type(&ann.type_annotation),
        })
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
        declared_in_macro_type_arg: from_root_body,
    })
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
