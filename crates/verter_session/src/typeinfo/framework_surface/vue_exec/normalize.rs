//! Per-surface `.vue` macro NORMALIZERS — the thin transforms that turn the
//! shared resolver's one-level macro surface into the published per-kind field
//! shapes (`AnalyzedPropField` / `AnalyzedEmitField` / `AnalyzedSlotField` /
//! `AnalyzedExposeField` / `NamedTypeMember` / index signatures / model props).
//!
//! These are NOT resolvers — they slice JSDoc spans and raise each member's
//! already-resolved value node to a `TypeExpr` through the active `ctx`. The
//! resolution itself happens once in [`super::vue_macro_dtos_with_ctx`]; this
//! module is the kind-specific projection of that single surface.

use std::sync::Arc;

use verter_semantic::analysis::type_expand::ExpandedIndexSignature;
use verter_semantic::analysis::types::{
    AnalyzedEmitField, AnalyzedPropField, AnalyzedSlotField, AnalyzedSlotFieldBinding,
};
use verter_semantic::analysis::AnalyzedMacroKind;
use verter_type_expr::{LiteralValue, TypeExpr, TypeExprScope};

use super::{
    member_jsdoc_from_spans, navigate_param_to_object_surface, raise_member_value,
    raise_realized_callable_member_value, signature_jsdoc_from_spans, slice_canonical_span,
    VueMacroSurface,
};
use crate::resolver_core::surface_projector::render_type_expr_display;
use crate::typeinfo::surface::{CanonicalSpan, TypeInfoSurfaceMember};
use crate::VerterHost;

/// Normalize a `.vue` props macro surface into the published
/// [`AnalyzedPropField`] set.
///
/// Reproduces the eager rail's `AnalyzedPropField` stream over the typeinfo
/// surface: one field per named member, carrying the surface's `optional` /
/// `readonly` / `declared_in_macro_type_arg`, the member value raised to a
/// `TypeExpr` scoped to its VALUE-NODE file (see
/// [`VueMacroSurface::member_expr_scope`]), the display `type_annotation`
/// rendered from that typed form, and JSDoc sliced from the surface spans.
/// Own-body-vs-heritage ordering + shadowing + union-common membership are
/// ALREADY resolved on the surface — this is a thin per-member transform.
///
/// `defineModel` does NOT carry an object type argument; its surface has no
/// named members and the synthesized model prop is appended from the analyzer
/// facts ([`AnalyzedMacroKind::DefineModel`]'s `prop_fields`).
#[must_use]
pub(crate) fn props_from_typeinfo_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<AnalyzedPropField> {
    // Host-level reads (graph node scope, JSDoc source slicing) go through the
    // host the active `ctx` is installed against; the view-sensitive type
    // resolution (`raise_member_value`) flows through `ctx`.
    let host = ctx.host_for_fact_tracer_install();
    // `defineModel` contributes its synthesized model prop directly from the
    // analyzer facts (the type argument is the model VALUE type).
    if macro_surface.macro_kind == AnalyzedMacroKind::DefineModel {
        return model_prop_fields(ctx, macro_surface);
    }

    macro_surface
        .surface
        .members
        .iter()
        // Publication-boundary visibility filter: the shared surface RECORDS
        // non-public class members, but Vue does NOT expose `private` /
        // `protected` class fields as props.
        .filter(|member| member.visibility.is_public())
        .map(|member| {
            let type_expr = raise_member_value(ctx, member);
            let type_expr_scope = type_expr
                .as_ref()
                .map(|_| macro_surface.member_expr_scope(host, member));
            let type_annotation = type_expr.as_ref().and_then(render_type_expr_display);
            let (description, tags) = member_jsdoc_from_spans(host, member);
            // `declared_in_macro_type_arg`: a member belongs to the macro-T own
            // body iff it is NOT heritage-reached. The terminal
            // `MacroTypeArgOwnBody` synthesis already stamps this correctly. The
            // `&& merge_role != Heritage` conjunct is REDUNDANT defense-in-depth
            // (a member can only carry `declared_in_macro_type_arg == true` if it
            // is an own-body `member_index` member).
            let declared_in_macro_type_arg = member.declared_in_macro_type_arg
                && member.origin.merge_role != crate::semantic_query::MemberMergeRole::Heritage;
            AnalyzedPropField {
                name: member.name.as_ref().to_string(),
                is_optional: member.optional,
                span: verter_span::Span::default(),
                type_annotation,
                type_expr,
                type_expr_scope,
                description,
                tags,
                resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
                declared_in_macro_type_arg,
            }
        })
        .collect()
}

/// Normalize a `defineOptions<T>()` / `defineExpose<T>()` macro surface into the
/// neutral [`NamedTypeMember`] set — the pass-through object surface (D-s
/// options/expose are an object-member surface, NOT a prop/emit/slot normalize).
///
/// The macro surface is ALREADY the one-level object surface
/// [`VerterHost::resolve_vue_macro_surface_with_ctx`] projected from the type
/// argument through the SHARED resolver (no special-case there — only
/// `defineModel` is). This is the thin per-member normalize: one
/// [`NamedTypeMember`] per public named member carrying its name, optionality,
/// and the member value raised to a `TypeExpr` through the active `ctx`. The
/// shallow-by-default rule holds — `raise_member_value` raises the member's
/// one-level value node, it does not eagerly expand it.
#[must_use]
pub(crate) fn object_members_from_typeinfo_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<crate::typeinfo::framework_surface::results::NamedTypeMember> {
    macro_surface
        .surface
        .members
        .iter()
        // Publication-boundary visibility filter (symmetric with props): the
        // shared surface RECORDS non-public class members, but the published
        // object surface exposes only public members.
        .filter(|member| member.visibility.is_public())
        .map(
            |member| crate::typeinfo::framework_surface::results::NamedTypeMember {
                name: member.name.as_ref().to_string(),
                is_optional: member.optional,
                type_expr: raise_member_value(ctx, member),
            },
        )
        .collect()
}

/// Normalize a `defineExpose<T>()` macro surface into [`AnalyzedExposeField`]s:
/// one field per named public member, carrying the member's surface type raised
/// through the active `ctx` (overlay-aware, scoped to its value-node file like
/// props/emits) and its JSDoc sliced from the enriched typeinfo spans.
///
/// The pass-through [`NamedTypeMember`] surface ([`object_members_from_typeinfo_surface`])
/// is a REDUCED shape that drops the `type_expr_scope` and JSDoc the
/// component-meta extract layer's `AnalyzedExposeField` pairing invariant
/// (`type_expr.is_some() <=> type_expr_scope.is_some()`) requires, so expose
/// carries this richer per-member normalize alongside it. The field's `span` is
/// `None`: the surface member's spans index its DECLARATION file, not the SFC,
/// so there is no SFC-absolute key span to report; downstream,
/// `extract_exposed_from_macro` publishes the union of the SFC object-literal
/// fields (which DO carry a span) and these surface members.
#[must_use]
pub(crate) fn exposed_from_typeinfo_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<verter_semantic::analysis::types::AnalyzedExposeField> {
    let host = ctx.host_for_fact_tracer_install();
    macro_surface
        .surface
        .members
        .iter()
        .filter(|member| member.visibility.is_public())
        .map(|member| {
            let type_expr = raise_member_value(ctx, member);
            let type_expr_scope = type_expr
                .as_ref()
                .map(|_| macro_surface.member_expr_scope(host, member));
            let (description, tags) = member_jsdoc_from_spans(host, member);
            verter_semantic::analysis::types::AnalyzedExposeField {
                name: member.name.as_ref().to_string(),
                span: None,
                type_expr,
                type_expr_scope,
                description,
                tags,
            }
        })
        .collect()
}

/// Normalize a macro surface's INDEX SIGNATURES into the published
/// [`ExpandedIndexSignature`] set. A props member is `properties + index
/// signatures` and an emits object is `events + index signatures`. Kind-neutral:
/// it raises whatever index signatures the surface carries. Each signature's
/// `key_type` / `value_type` graph node is raised to a `TypeExpr` through the
/// ACTIVE `ctx` (overlay-aware); a node that does not raise is skipped (no
/// phantom signature).
pub(crate) fn index_signatures_from_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<ExpandedIndexSignature> {
    let dispatch = ctx.dispatch();
    macro_surface
        .surface
        .index_signatures
        .iter()
        .filter_map(|sig| {
            let key_type = dispatch.raise_node_to_type_expr(sig.key_type)?;
            let value_type = dispatch.raise_node_to_type_expr(sig.value_type)?;
            Some(ExpandedIndexSignature {
                key_type,
                value_type,
                readonly: sig.readonly,
            })
        })
        .collect()
}

/// Build the `defineModel` synthesized prop field from the analyzer facts.
/// `defineModel<T>('name', { … })` synthesizes a prop named `name`
/// (default `modelValue`) typed `T`; the analyzer already captured this as the
/// macro's single `prop_fields` entry. Re-scope the typed form to the SFC owner
/// so nested `Ref`s resolve in the SFC.
///
/// The owner SFC's `IndexedReady` is fetched through the ACTIVE `ctx`
/// (`ctx.ensure_indexed_ready_serve`), NOT the base `VerterHost`, so an overlay
/// session reads the OVERLAY `defineModel` macro facts — a `defineModel<number>`
/// edit no longer rereads the base host's `defineModel<string>` snapshot.
pub(crate) fn model_prop_fields(
    ctx: &dyn crate::resolver_core::ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<AnalyzedPropField> {
    let Some(indexed) = ctx
        .ensure_indexed_ready_serve(macro_surface.owner_canonical.as_ref())
        .map(|serve| serve.indexed)
    else {
        return Vec::new();
    };
    let Some(mac) = indexed.snapshot.macros.get(macro_surface.macro_index) else {
        return Vec::new();
    };
    mac.prop_fields
        .iter()
        .map(|field| {
            // The analyzer stamps an empty scope on the synthesized model prop;
            // re-anchor it to the SFC owner so the pairing invariant holds with
            // a real scope.
            let type_expr_scope = field
                .type_expr
                .as_ref()
                .map(|_| TypeExprScope::new(macro_surface.owner_canonical.as_ref()));
            AnalyzedPropField {
                type_expr_scope,
                ..field.clone()
            }
        })
        .collect()
}

/// Normalize a `.vue` emits macro surface into the published
/// [`AnalyzedEmitField`] set.
///
/// 1. **Call-signature emits FIRST.** Each call signature's first parameter is
///    the event name (a `String` literal, or a `Union` of `String` literals);
///    the typed `payload_expr` is the call-signature function with the leading
///    event-name parameter STRIPPED. The event name is NEVER read from `keyof`.
///    The display `payload_type` is a CONSISTENT source-span slice.
/// 2. **Property-style emits as a FALLBACK** — only when no call-signature emit
///    was found.
/// 3. **De-duplicate by event name, first-writer-wins.**
#[must_use]
pub(crate) fn emits_from_typeinfo_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<AnalyzedEmitField> {
    // View-sensitive type resolution flows through the active `ctx`
    // (`ctx.dispatch()`). Host-level reads (JSDoc source slicing, node scope)
    // use the host the `ctx` is installed against.
    let host = ctx.host_for_fact_tracer_install();
    let dispatch = ctx.dispatch();

    let mut emits: Vec<AnalyzedEmitField> = Vec::new();

    // (1) Call-signature emits.
    for sig in macro_surface.surface.call_signatures.iter() {
        // `TypeExpr` implements `Drop`, so `func` cannot be moved out of the
        // raised value; bind it and borrow the function.
        let raised = dispatch.raise_node_to_type_expr(sig.node);
        let Some(TypeExpr::Function(func)) = &raised else {
            continue;
        };
        let Some(first) = func.parameters.first() else {
            continue;
        };
        // Payload = the call signature's REMAINING parameters (after the leading
        // event-name parameter) as a TUPLE — the Vue emit payload shape. This
        // matches the eager OXC rail's `AnalyzedEmitField.payload_expr` (a
        // `TypeExpr::Tuple`). Each surviving parameter maps to a labelled tuple
        // element preserving its name / optional / rest.
        let payload_tuple = TypeExpr::Tuple {
            elements: func
                .parameters
                .iter()
                .skip(1)
                .map(|param| verter_type_expr::TupleElement {
                    label: param.name.clone(),
                    ty: param.ty.clone(),
                    optional: param.optional,
                    rest: param.rest,
                })
                .collect(),
            readonly: false,
        };
        // Scope the payload to the call signature's DECLARATION-origin file so an
        // inherited cross-file emit signature's payload `Ref`s resolve in the
        // base file. Falls back to the SFC owner.
        let payload_scope = macro_surface.signature_expr_scope(sig);
        // `payload_type` (→ `rawType`) is DISPLAY-ONLY — no consumer parses it.
        // It mirrors the payload TUPLE rendered as `[label: T, ...]`.
        let payload_type = render_type_expr_display(&payload_tuple);
        // The event's JSDoc rides on the call signature itself, sliced from the
        // signature's typeinfo JSDoc spans. A union of event-name literals on
        // ONE signature shares that signature's JSDoc across each event.
        let (description, tags) = signature_jsdoc_from_spans(host, sig);
        let mut push_event = |name: String| {
            emits.push(AnalyzedEmitField {
                name,
                span: verter_span::Span::default(),
                payload_type: payload_type.clone(),
                payload_expr: Some(payload_tuple.clone()),
                payload_expr_scope: Some(payload_scope.clone()),
                description: description.clone(),
                tags: tags.clone(),
            });
        };
        match &first.ty {
            TypeExpr::Literal(LiteralValue::String(name)) => push_event(name.clone()),
            TypeExpr::Union(types) => {
                for ty in types.iter() {
                    if let TypeExpr::Literal(LiteralValue::String(name)) = ty {
                        push_event(name.clone());
                    }
                }
            }
            _ => {}
        }
    }

    // (2) Property-style emits — fallback only when no call-signature emit fired.
    if emits.is_empty() {
        for member in macro_surface
            .surface
            .members
            .iter()
            // Public-only publication: a `private` / `protected` class member
            // recorded on the shared surface must NOT leak as a published emit.
            .filter(|member| member.visibility.is_public())
        {
            let payload_expr = raise_member_value(ctx, member);
            let payload_expr_scope = payload_expr
                .as_ref()
                .map(|_| macro_surface.member_expr_scope(host, member));
            let payload_type = payload_expr.as_ref().and_then(render_type_expr_display);
            let (description, tags) = member_jsdoc_from_spans(host, member);
            emits.push(AnalyzedEmitField {
                name: member.name.as_ref().to_string(),
                span: verter_span::Span::default(),
                payload_type,
                payload_expr,
                payload_expr_scope,
                description,
                tags,
            });
        }
    }

    // (3) De-duplicate by event name, first-writer-wins.
    let mut seen = std::collections::HashSet::new();
    emits.retain(|emit| seen.insert(emit.name.clone()));
    emits
}

/// Extract a slot callable's first-parameter type + return type from a slot
/// member value, handling an INTERSECTION or a UNION of function types.
///
/// A slot typed via an intersection of interfaces
/// (`defineSlots<SlotA & SlotB>()`) has its `default` member resolve to
/// `SlotA['default'] & SlotB['default']` — an `Intersection` of two function
/// types, NOT a single pre-merged `Function`. A slot typed as a union of
/// function aliases resolves its `default` member to a `Union` of two function
/// types. Both are slot-callable. Returns:
///
/// - `Function(f)` → `f`'s first-param type + return type directly.
/// - `Intersection(arms)` / `Union(arms)` where EVERY resolvable arm is a
///   function → the INTERSECTION of the arms' first-param types plus the
///   combined return type (intersection of returns for an intersection, union of
///   returns for a union).
/// - Anything else → `None` (the member is not a slot).
fn slot_callable_param_and_return(
    value: &TypeExpr,
) -> Option<(
    Option<TypeExpr>,
    Option<TypeExpr>,
    Option<verter_span::Span>,
)> {
    match value {
        TypeExpr::Function(func) => Some((
            func.parameters.first().map(|p| p.ty.clone()),
            func.return_type.as_ref().map(|rt| (**rt).clone()),
            // The return-type annotation span (file-relative to the slot
            // member's value-node file). Lets the caller slice the EXACT source
            // text for the display `return_type` when the typed return contains
            // an unresolved reference (`VNode` not imported).
            func.spans.return_type,
        )),
        // Intersection of slot-callable arms: param = intersection of first
        // params (required-wins merge), return = intersection of returns.
        TypeExpr::Intersection(arms) => {
            slot_callable_param_and_return_from_arms(arms, ArmCombine::Intersection)
        }
        // Union of slot-callable arms (`SlotA | SlotB`): param stays the
        // INTERSECTION of first params (a slot prop the template can rely on must
        // be present in every arm — contravariant param), but the return is the
        // UNION of the arms' return types (covariant).
        TypeExpr::Union(arms) => slot_callable_param_and_return_from_arms(arms, ArmCombine::Union),
        _ => None,
    }
}

/// How to combine the RETURN types of a multi-arm slot callable. The first
/// params are ALWAYS intersected (the bindings a template can rely on must hold
/// across every arm); only the return-type combiner differs.
#[derive(Clone, Copy)]
enum ArmCombine {
    Intersection,
    Union,
}

/// Shared multi-arm slot-callable extractor for `Intersection` / `Union` of
/// function types. Every arm MUST be a `Function` (a non-function arm makes the
/// member not slot-like → `None`). The first params are intersected; the returns
/// are combined per `combine`.
///
/// SOUNDNESS — a slot binding is guaranteed only if EVERY arm supplies a first
/// parameter. A template destructuring `<template #default="{ x }">` runs for
/// WHICHEVER arm the slot actually is, so a binding the template can rely on must
/// be present across all arms. If ANY arm is a no-param callable (`() => any`),
/// the multi-arm callable can be invoked with no slot props in that branch, so
/// there are NO guaranteed bindings — the first param is dropped to `None`. The
/// return type still combines across arms.
fn slot_callable_param_and_return_from_arms(
    arms: &[TypeExpr],
    combine: ArmCombine,
) -> Option<(
    Option<TypeExpr>,
    Option<TypeExpr>,
    Option<verter_span::Span>,
)> {
    let mut first_params: Vec<TypeExpr> = Vec::new();
    let mut returns: Vec<TypeExpr> = Vec::new();
    // A binding is guaranteed only when EVERY arm contributes a first param.
    let mut all_arms_have_first_param = true;
    for arm in arms.iter() {
        let TypeExpr::Function(func) = arm else {
            // A non-function arm means the member is not purely slot-callable.
            return None;
        };
        if let Some(p) = func.parameters.first() {
            first_params.push(p.ty.clone());
        } else {
            all_arms_have_first_param = false;
        }
        if let Some(rt) = func.return_type.as_ref() {
            returns.push((**rt).clone());
        }
    }
    if first_params.is_empty() && returns.is_empty() {
        return None;
    }
    // First params: the INTERSECTION — but ONLY when every arm supplied a first
    // param. A no-param arm guarantees nothing, so the bindings are dropped.
    let first_param = if all_arms_have_first_param {
        match first_params.len() {
            0 => None,
            1 => Some(first_params.into_iter().next().unwrap()),
            _ => Some(TypeExpr::Intersection(Arc::from(
                first_params.into_boxed_slice(),
            ))),
        }
    } else {
        None
    };
    // Returns: combine per the arm kind.
    let return_ty = match returns.len() {
        0 => None,
        1 => Some(returns.into_iter().next().unwrap()),
        _ => {
            let boxed = Arc::from(returns.into_boxed_slice());
            Some(match combine {
                ArmCombine::Intersection => TypeExpr::Intersection(boxed),
                ArmCombine::Union => TypeExpr::Union(boxed),
            })
        }
    };
    // A composed multi-arm callable has no single return-type span.
    Some((first_param, return_ty, None))
}

/// Normalize a `.vue` slots macro surface into the published
/// [`AnalyzedSlotField`] set.
///
/// Keep FUNCTION-LIKE members only (the value raises to a `TypeExpr::Function`;
/// non-function members are filtered); the slot's `bindings` come from resolving
/// the function's first-parameter type to its object surface (a literal object,
/// a `Pick<…>`, or a named alias — see [`binding_fields_from_param_ty`]); the
/// `return_expr` / `return_type` come from the function's return type. Bindings +
/// return are scoped to the slot member's VALUE-NODE file (see
/// [`VueMacroSurface::member_expr_scope`]).
#[must_use]
pub(crate) fn slots_from_typeinfo_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<AnalyzedSlotField> {
    // View-sensitive slot type resolution flows through the active `ctx`.
    // Host-level reads (JSDoc / return-type source slicing, node scope) use the
    // host the `ctx` is installed against.
    let host = ctx.host_for_fact_tracer_install();
    macro_surface
        .surface
        .members
        .iter()
        // Public-only publication: a `private` / `protected` class member must
        // NOT leak as a published slot.
        .filter(|member| member.visibility.is_public())
        .filter_map(|member| {
            // A slot member's value may be a non-`Function` carrier shell under
            // the transit-shallow macro surface — most notably a generic slot
            // alias that lowers to an `InstantiationRef` / alias carrier rather
            // than a reduced `Function`. Realize the value through the SHARED
            // callable-realization substrate so a decidable callable surfaces as
            // a `Function` BEFORE the function-like filter — otherwise the
            // generic slot is silently dropped.
            let value = raise_realized_callable_member_value(ctx, member)?;
            // A slot member is function-like: a single `Function`, or an
            // `Intersection` of functions, or a `Union` of functions.
            let (first_param, return_expr, return_span) = slot_callable_param_and_return(&value)?;
            let scope = macro_surface.member_expr_scope(host, member);
            let bindings = first_param
                .as_ref()
                .map(|param_ty| binding_fields_from_param_ty(ctx, param_ty, &scope))
                .unwrap_or_default();
            let return_expr_scope = return_expr.as_ref().map(|_| scope.clone());
            // Display `return_type`: prefer the EXACT source text sliced from the
            // return-type annotation span — this preserves a name the typed
            // return cannot surface (an unresolved imported `VNode`). Fall back
            // to rendering the typed return when there is no span. Display-only.
            let return_type = return_span
                .map(|span| CanonicalSpan::new(scope.as_str().into(), span))
                .and_then(|cspan| slice_canonical_span(host, &cspan))
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty())
                .or_else(|| return_expr.as_ref().and_then(render_type_expr_display));
            let (description, tags) = member_jsdoc_from_spans(host, member);
            Some(AnalyzedSlotField {
                name: member.name.as_ref().to_string(),
                is_required: !member.optional,
                span: verter_span::Span::default(),
                bindings,
                return_type,
                return_expr,
                return_expr_scope,
                description,
                tags,
            })
        })
        .collect()
}

/// Reconstruct a slot's binding fields from its function's first-parameter type.
/// Each member of the parameter's OBJECT surface becomes one
/// [`AnalyzedSlotFieldBinding`] carrying that member's value `TypeExpr` as
/// `binding_expr`.
///
/// The first parameter is the slot-props object. It can be written several ways:
/// a literal object, a `Pick<T, 'k'>` over a named type, or a parenthesized
/// form. To handle all of them WITHOUT a nominal shape-sniff, the binding object
/// is obtained by RESOLVING the first-parameter type through the SHARED
/// resolver:
///
/// - A literal [`TypeExpr::Object`] is read directly (no resolution needed).
/// - Any other shape (`Pick<…>` / `Omit<…>` / a `Ref` to a named alias /
///   `Parenthesized`) is lowered and projected to its one-level object surface
///   ([`navigate_param_to_object_surface`]); each surface member becomes a
///   binding.
///
/// A first parameter that does not resolve to an object surface yields no
/// bindings.
pub(crate) fn binding_fields_from_param_ty(
    ctx: &dyn crate::resolver_core::ResolverContext,
    param_ty: &TypeExpr,
    scope: &TypeExprScope,
) -> Vec<AnalyzedSlotFieldBinding> {
    // View-sensitive navigation / raising flows through `ctx`; host-level node
    // scope reads use the host the `ctx` is installed against.
    let host = ctx.host_for_fact_tracer_install();
    // Literal object: read its properties directly (structural typed-IR match).
    if let TypeExpr::Object(obj) = param_ty {
        return obj
            .properties
            .iter()
            .filter_map(|member| match member {
                // Public-only publication.
                verter_type_expr::ObjectMember::Property(prop) if prop.visibility.is_public() => {
                    Some(AnalyzedSlotFieldBinding {
                        name: prop.name.clone(),
                        type_annotation: render_type_expr_display(&prop.ty),
                        binding_expr: Some(prop.ty.clone()),
                        binding_expr_scope: Some(scope.clone()),
                        span: verter_span::Span::default(),
                    })
                }
                _ => None,
            })
            .collect();
    }

    // Non-object first param (`Pick<…>` / alias `Ref` / `Parenthesized`):
    // navigate it through the shared resolver to its object surface.
    let Some(surface) = navigate_param_to_object_surface(ctx, scope.as_str(), param_ty) else {
        return Vec::new();
    };
    // Shallow-by-default Pick member publication: when the slot param is a
    // `Pick<NamedRoot, K>` the picked members stay SYMBOLIC at the published
    // binding surface — each binding's value is the typed indexed access
    // `NamedRoot['member']` (built from the typed param, not reparsed). Other
    // shapes keep the navigated value.
    let pick_symbolic_root = pick_named_source_root(param_ty);
    surface
        .members
        .iter()
        // Public-only publication.
        .filter(|member| member.visibility.is_public())
        .map(|member| {
            if let Some(root) = pick_symbolic_root {
                let symbolic = TypeExpr::IndexedAccess {
                    object: Arc::new(root.clone()),
                    index: Arc::new(TypeExpr::Literal(LiteralValue::String(
                        member.name.as_ref().to_string(),
                    ))),
                };
                return AnalyzedSlotFieldBinding {
                    name: member.name.as_ref().to_string(),
                    type_annotation: render_type_expr_display(&symbolic),
                    binding_expr: Some(symbolic),
                    binding_expr_scope: Some(scope.clone()),
                    span: verter_span::Span::default(),
                };
            }
            let binding_expr = raise_member_value(ctx, member);
            let binding_expr_scope = binding_expr
                .as_ref()
                .map(|_| macro_member_value_scope(host, member, scope));
            let type_annotation = binding_expr.as_ref().and_then(render_type_expr_display);
            AnalyzedSlotFieldBinding {
                name: member.name.as_ref().to_string(),
                type_annotation,
                binding_expr,
                binding_expr_scope,
                span: verter_span::Span::default(),
            }
        })
        .collect()
}

/// When `param_ty` is structurally `Pick<NamedRoot, K>` (modulo `Parenthesized`
/// wrappers) with `NamedRoot` a nominal [`TypeExpr::Ref`], return that
/// source-root `Ref` so a slot binding can publish each picked member as the
/// symbolic `NamedRoot['member']` indexed access. This is a STRUCTURAL match on
/// the typed IR — no type-text sniffing, no reparse. Any other shape returns
/// `None`.
fn pick_named_source_root(param_ty: &TypeExpr) -> Option<&TypeExpr> {
    match param_ty {
        TypeExpr::Parenthesized(inner) => pick_named_source_root(inner),
        TypeExpr::Ref {
            name,
            type_arguments,
        } if name.as_ref() == "Pick" && type_arguments.len() == 2 => {
            let mut source = &type_arguments[0];
            while let TypeExpr::Parenthesized(inner) = source {
                source = inner;
            }
            matches!(source, TypeExpr::Ref { .. }).then_some(source)
        }
        _ => None,
    }
}

/// The [`TypeExprScope`] a navigated binding member's `binding_expr` binds to —
/// its value-node scope (matching [`VueMacroSurface::member_expr_scope`]),
/// falling back to the slot's scope when the member's value node is
/// structural / scope-less.
fn macro_member_value_scope(
    host: &VerterHost,
    member: &TypeInfoSurfaceMember,
    fallback: &TypeExprScope,
) -> TypeExprScope {
    host.project_type_store()
        .semantic_graph()
        .node_scope(member.value)
        .and_then(|scope| scope.canonical_file())
        .map(|canonical| TypeExprScope::new(canonical.as_ref()))
        .or_else(|| {
            member
                .origin
                .canonical_file
                .as_ref()
                .map(|canonical| TypeExprScope::new(canonical.as_ref()))
        })
        .unwrap_or_else(|| fallback.clone())
}
