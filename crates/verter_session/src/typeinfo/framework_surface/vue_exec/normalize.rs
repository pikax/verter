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
    member_jsdoc_from_spans, raise_member_value, signature_jsdoc_from_spans, slice_canonical_span,
};
use crate::meta_resolve::callable_view::{ArmCombineNode, CallableNodeView};
use crate::project_semantic_dispatch::output_materialization::OutputProjector;
use crate::project_semantic_dispatch::{node_data_for, ProjectSemanticDispatch};
use crate::resolver_core::surface_projector::render_type_expr_display;
use crate::semantic_query::{
    FunctionParam, PathSegment, ProjectionMode, ProjectionReductionContext, SemanticNodeData,
    SemanticNodeId,
};
use crate::typeinfo::framework_surface::resolved_surface_access::ResolvedSurfaceAccess;
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
    resolved: &impl ResolvedSurfaceAccess,
) -> Vec<AnalyzedPropField> {
    let macro_surface = resolved.macro_surface();
    // Host-level reads (graph node scope, JSDoc source slicing) go through the
    // host the active `ctx` is installed against; the view-sensitive type
    // resolution (`raise_member_value`) flows through `ctx`.
    let host = ctx.host_for_fact_tracer_install();
    // `defineModel` contributes its synthesized model prop directly from the
    // analyzer facts (the type argument is the model VALUE type).
    if macro_surface.macro_kind == AnalyzedMacroKind::DefineModel {
        return model_prop_fields(ctx, resolved);
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
/// neutral [`NamedTypeMember`] set — the pass-through object surface
/// (options/expose are an object-member surface, NOT a prop/emit/slot normalize).
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
    resolved: &impl ResolvedSurfaceAccess,
) -> Vec<crate::typeinfo::framework_surface::results::NamedTypeMember> {
    let macro_surface = resolved.macro_surface();
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
    resolved: &impl ResolvedSurfaceAccess,
) -> Vec<verter_semantic::analysis::types::AnalyzedExposeField> {
    let macro_surface = resolved.macro_surface();
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
    resolved: &impl ResolvedSurfaceAccess,
) -> Vec<ExpandedIndexSignature> {
    let macro_surface = resolved.macro_surface();
    let dispatch = ctx.dispatch();
    // Publication sink (DTO index signatures): materialize into sealed
    // carriers and unwrap via the typeinfo output capability.
    let cap = super::TypeinfoVueSurfaceOutputCap::new(&dispatch);
    macro_surface
        .surface
        .index_signatures
        .iter()
        .filter_map(|sig| {
            let key_type = cap
                .materialize_output_type_expr(sig.key_type)?
                .into_type_expr(&cap);
            let value_type = cap
                .materialize_output_type_expr(sig.value_type)?
                .into_type_expr(&cap);
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
    resolved: &impl ResolvedSurfaceAccess,
) -> Vec<AnalyzedPropField> {
    let macro_surface = resolved.macro_surface();
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
    resolved: &impl ResolvedSurfaceAccess,
) -> Vec<AnalyzedEmitField> {
    let macro_surface = resolved.macro_surface();
    // View-sensitive type resolution flows through the active `ctx`
    // (`ctx.dispatch()`). Host-level reads (JSDoc source slicing, node scope)
    // use the host the `ctx` is installed against.
    let host = ctx.host_for_fact_tracer_install();
    let dispatch = ctx.dispatch();
    // Publication sink (DTO emit payload tuples): the event-name decide and the
    // payload param selection are made ENTIRELY in the node domain through the
    // shared `CallableNodeView`; materialization happens ONCE at the terminal
    // `materialize_payload_tuple` sink (which constructs its own mint cap from
    // `ctx`). This normalizer calls NO mint verb and holds NO cap.
    // Node-domain demand identity. `Navigate` carrier-resolves an ALIASED
    // event-name union (`type E = 'save' | 'cancel'`) so its literal names
    // surface — a shallow-`TypeExpr` decide on the first param would keep the
    // `DeclRef` carrier opaque and surface neither. The payload elements are
    // minted shallow at the sink regardless of this mode.
    let context = ProjectionReductionContext::published(ProjectionMode::Navigate);

    let mut emits: Vec<AnalyzedEmitField> = Vec::new();

    // (1) Call-signature emits — decided in the NODE domain.
    for sig in macro_surface.surface.call_signatures.iter() {
        let view = CallableNodeView::new(&dispatch, sig.node);
        // The event name(s): the realized callable's FIRST-param string literal /
        // union, carrier-resolved (fail-closed-whole). `None` (no first param, or
        // a first param carrying no string literal) contributes NO events (no
        // event name ⇒ no emit field).
        let Some(names) = view.event_names(context) else {
            continue;
        };
        // Payload = the realized signature's params AFTER the leading event-name
        // param (`[1..]`), materialized ONCE at the terminal sink. `event_names`
        // above already realized the signature (its `first_param`), so `signature`
        // here is `Some` by construction; the `else continue` is defensive.
        let Some(signature) = view.signature(context) else {
            continue;
        };
        let raw_params = signature.raw_params();
        // Materialize the payload tuple ONCE at the terminal sink, kept as an
        // `Option<TypeExpr>` so the DISPLAY renders through the by-name
        // `as_ref().and_then(render_type_expr_display)` form — the SAME shape
        // `props_from_typeinfo_surface` uses. This normalizer NEVER decides on the
        // materialized value (no direct reader call on it).
        let payload_expr = Some(materialize_payload_tuple(ctx, &raw_params[1..]));
        // Scope the payload to the call signature's DECLARATION-origin file so an
        // inherited cross-file emit signature's payload `Ref`s resolve in the base
        // file. Falls back to the SFC owner.
        let payload_scope = macro_surface.signature_expr_scope(sig);
        // `payload_type` (→ `rawType`) is DISPLAY-ONLY — no consumer parses it.
        // It mirrors the payload TUPLE rendered as `[label: T, ...]`.
        let payload_type = payload_expr.as_ref().and_then(render_type_expr_display);
        // The event's JSDoc rides on the call signature itself, sliced from the
        // signature's typeinfo JSDoc spans. A union of event-name literals on ONE
        // signature shares that signature's JSDoc across each event.
        let (description, tags) = signature_jsdoc_from_spans(host, sig);
        for name in names {
            emits.push(AnalyzedEmitField {
                name: name.to_string(),
                span: verter_span::Span::default(),
                payload_type: payload_type.clone(),
                payload_expr: payload_expr.clone(),
                payload_expr_scope: Some(payload_scope.clone()),
                description: description.clone(),
                tags: tags.clone(),
            });
        }
    }

    // (2) Property-style emits — fallback only when no call-signature emit fired.
    // The member materialization lives in the terminal `property_style_emit_fields`
    // sink so this normalizer mints nothing directly.
    if emits.is_empty() {
        emits = property_style_emit_fields(ctx, resolved);
    }

    // (3) De-duplicate by event name, first-writer-wins.
    let mut seen = std::collections::HashSet::new();
    emits.retain(|emit| seen.insert(emit.name.clone()));
    emits
}

/// Materialize a Vue emit payload tuple from NODE-DOMAIN params — a GENUINE
/// decide-free terminal one-shot sink. Each `param.ty` node is minted ONCE
/// through the sealed output capability into a labelled `TupleElement` that
/// preserves the param's name / optional / rest; the result is the payload
/// `TypeExpr::Tuple`. It makes NO decision on any materialized value (no branch /
/// match / shape-extract), takes NO `&TypeExpr` param (node ids + the active
/// `ctx`), and lives inside the Vue cap's `pub(in …::vue_exec)` mint scope. The
/// mint cap is constructed INTERNALLY from `ctx` (the `raise_member_value`
/// pattern) — a cap is a mint AUTHORITY and must not cross the boundary from the
/// non-terminal caller.
///
/// Materialization is POSITION-PRESERVING: the params are `.map`ped (never
/// `filter_map`ped), so a param whose node does not materialize keeps its tuple
/// SLOT with the opaque `Unknown` raise-miss value instead of shifting the
/// subsequent payload elements. This does not arise in practice — the realized
/// signature's param nodes ARE the callable's own declared parameter types, which
/// all materialize — so the fallback is position-safety robustness only, never a
/// fabricated meaningful element.
pub(in crate::typeinfo::framework_surface::vue_exec) fn materialize_payload_tuple(
    ctx: &dyn crate::resolver_core::ResolverContext,
    params: &[FunctionParam],
) -> TypeExpr {
    // Construct the mint cap INTERNALLY from the active `ctx` (the
    // `raise_member_value` pattern): a cap is a genuine mint AUTHORITY that must
    // not cross into a `TypeExpr`-producing sink from the non-terminal caller.
    let dispatch = ctx.dispatch();
    let cap = super::TypeinfoVueSurfaceOutputCap::new(&dispatch);
    let elements = params
        .iter()
        .map(|param| {
            // Position-preserving: mint the param's `ty` node ONCE; a node that
            // does not materialize keeps its tuple SLOT with the opaque `Unknown`
            // raise-miss value (the `output_sink::raise_node_to_sealed_carrier`
            // convention) so subsequent payload params never shift. A declared
            // param's `ty` always mints, so the fallback is robustness only.
            let ty = cap
                .materialize_output_type_expr(param.ty)
                .map(|raised| raised.into_type_expr(&cap))
                .unwrap_or_else(|| TypeExpr::Unknown { raw: String::new() });
            verter_type_expr::TupleElement {
                // Node-domain `FunctionParam.name` (`Option<Arc<str>>`) → the
                // display-facing tuple `label` (`Option<String>`).
                label: param.name.as_ref().map(|n| n.to_string()),
                ty,
                optional: param.optional,
                rest: param.rest,
            }
        })
        .collect();
    TypeExpr::Tuple {
        elements,
        readonly: false,
    }
}

/// Build the property-style Vue emit fields — the FALLBACK used when a `.vue`
/// `defineEmits<{ … }>()` object surface declares NO call signature. A GENUINE
/// decide-free terminal one-shot sink: it iterates the surface's PUBLIC members
/// (a node-domain visibility fact), mints each member value ONCE through the
/// registered `raise_member_value` sink, and builds the `AnalyzedEmitField` DTO
/// (name from the member, payload = the raised member value, scope + JSDoc from
/// the surface). It makes NO decision on any materialized `TypeExpr` (the raised
/// value is stored + display-rendered, never branched on) and takes NO
/// `&TypeExpr` param — structurally identical to the [`props_from_typeinfo_surface`]
/// member loop — so the non-terminal `emits_from_typeinfo_surface` delegates here
/// instead of minting inline.
pub(in crate::typeinfo::framework_surface::vue_exec) fn property_style_emit_fields(
    ctx: &dyn crate::resolver_core::ResolverContext,
    resolved: &impl ResolvedSurfaceAccess,
) -> Vec<AnalyzedEmitField> {
    let macro_surface = resolved.macro_surface();
    let host = ctx.host_for_fact_tracer_install();
    macro_surface
        .surface
        .members
        .iter()
        // Public-only publication: a `private` / `protected` class member
        // recorded on the shared surface must NOT leak as a published emit.
        .filter(|member| member.visibility.is_public())
        .map(|member| {
            let payload_expr = raise_member_value(ctx, member);
            let payload_expr_scope = payload_expr
                .as_ref()
                .map(|_| macro_surface.member_expr_scope(host, member));
            let payload_type = payload_expr.as_ref().and_then(render_type_expr_display);
            let (description, tags) = member_jsdoc_from_spans(host, member);
            AnalyzedEmitField {
                name: member.name.as_ref().to_string(),
                span: verter_span::Span::default(),
                payload_type,
                payload_expr,
                payload_expr_scope,
                description,
                tags,
            }
        })
        .collect()
}

/// Normalize a `.vue` slots macro surface into the published
/// [`AnalyzedSlotField`] set.
///
/// Keep FUNCTION-LIKE members only (the value raises to a `TypeExpr::Function`;
/// non-function members are filtered); the slot's `bindings` come from resolving
/// the function's first-parameter type to its object surface (a literal object,
/// a `Pick<…>`, or a named alias — see [`binding_fields_from_param_node`]); the
/// `return_expr` / `return_type` come from the function's return type. Bindings +
/// return are scoped to the slot member's VALUE-NODE file (see
/// [`VueMacroSurface::member_expr_scope`]).
#[must_use]
pub(crate) fn slots_from_typeinfo_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    resolved: &impl ResolvedSurfaceAccess,
) -> Vec<AnalyzedSlotField> {
    let macro_surface = resolved.macro_surface();
    // View-sensitive slot type resolution flows through the active `ctx`.
    // Host-level reads (JSDoc / return-type source slicing, node scope) use the
    // host the `ctx` is installed against.
    let host = ctx.host_for_fact_tracer_install();
    let dispatch = ctx.dispatch();
    // Node-domain demand identity: `Navigate` carrier-resolves an aliased /
    // generic slot callable before the function-like filter.
    let context = ProjectionReductionContext::published(ProjectionMode::Navigate);
    macro_surface
        .surface
        .members
        .iter()
        // Public-only publication: a `private` / `protected` class member must
        // NOT leak as a published slot.
        .filter(|member| member.visibility.is_public())
        .filter_map(|member| {
            // The slot callable decisions are made ENTIRELY in the node domain
            // through the shared `CallableNodeView`; the display return `TypeExpr`
            // is minted ONCE at the terminal sink. A slot member may be a
            // non-`Function` carrier shell under the transit-shallow surface (a
            // generic slot alias lowering to an `InstantiationRef` / alias
            // carrier) — the view realizes it BEFORE the function-like filter.
            let view = CallableNodeView::new(&dispatch, member.value);
            // Function-like FILTER + return-combiner selection (node-domain): a
            // member that does not realize to a callable is not a slot (dropped).
            // The realized root's top-level kind selects the return combiner — a
            // `Union` of function arms unions returns; a single `Function` or an
            // `Intersection` of function arms intersects them. (The first params
            // are ALWAYS intersected — a template binding must hold across arms.)
            let realized_root = view.realized_callable_root(context)?;
            let combine = match node_data_for(dispatch.ctx, realized_root).as_deref() {
                Some(SemanticNodeData::Union(_)) => ArmCombineNode::Union,
                _ => ArmCombineNode::Intersection,
            };
            // The across-arms first-param + return NODES (fails closed to drop
            // the slot when any arm is non-callable).
            let parts = view.slot_param_and_return_by_arm(combine, context)?;
            let scope = macro_surface.member_expr_scope(host, member);
            // Bindings from the (across-arms-intersected) first-param NODE.
            let bindings = parts
                .first_param
                .map(|first_param| binding_fields_from_param_node(ctx, first_param, &scope))
                .unwrap_or_default();
            // `return_expr`: materialize the return NODE ONCE at the terminal sink
            // (kept as `Option<TypeExpr>`; the normalizer only stores + renders it,
            // never decides on its variant).
            let return_expr = parts
                .return_type
                .map(|return_node| materialize_slot_return_node(ctx, return_node));
            let return_expr_scope = return_expr.as_ref().map(|_| scope.clone());
            // Display `return_type`: prefer the EXACT source text sliced from the
            // return-type annotation span (single-arm) — this preserves a name the
            // typed return cannot surface (an unresolved imported `VNode`). Fall
            // back to rendering the materialized return (composed multi-arm — no
            // single span). Display-only; the by-name `.and_then` render form makes
            // no decision on the materialized value.
            let return_type = parts
                .return_type_span
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

/// Materialize a Vue slot RETURN node into its display `TypeExpr` — a GENUINE
/// decide-free terminal one-shot sink (the single-node twin of
/// [`materialize_payload_tuple`]). The return `SemanticNodeId` is minted ONCE
/// through the sealed output cap; it makes NO decision on the materialized value
/// and takes NO `&TypeExpr` param (a node id + the active `ctx`). The mint cap is
/// constructed INTERNALLY from `ctx` (the `raise_member_value` pattern).
fn materialize_slot_return_node(
    ctx: &dyn crate::resolver_core::ResolverContext,
    return_node: SemanticNodeId,
) -> TypeExpr {
    let dispatch = ctx.dispatch();
    let cap = super::TypeinfoVueSurfaceOutputCap::new(&dispatch);
    // A node that does not materialize keeps the opaque `Unknown` raise-miss value
    // (the shared raise-miss convention); a realized slot's return node always
    // mints, so the fallback is robustness only.
    cap.materialize_output_type_expr(return_node)
        .map(|raised| raised.into_type_expr(&cap))
        .unwrap_or(TypeExpr::Unknown { raw: String::new() })
}

/// Reconstruct a slot's binding fields from its function's first-parameter NODE.
/// Each member of the parameter's OBJECT surface becomes one
/// [`AnalyzedSlotFieldBinding`] carrying that member's value `TypeExpr` as
/// `binding_expr`.
///
/// The first parameter is the slot-props object. It can be written several ways —
/// a literal object, a `Pick<T, 'k'>` / `Omit<…>` over a named type, an aliased
/// `Ref`, or a parenthesized form — plus the multi-arm intersected node a `Union`
/// / `Intersection` slot produces. To cover all of them WITHOUT a nominal
/// shape-sniff, the binding object is the first-param node's one-level SHALLOW
/// object surface, projected through the shared carrier-preserving
/// [`VerterHost::project_shallow_surface_from_base`]; each surface member becomes
/// a binding.
///
/// NODE-DOMAIN: the first param is a `SemanticNodeId` (the across-arms-intersected
/// slot first-param node from [`CallableNodeView::slot_param_and_return_by_arm`]),
/// NOT a materialised `TypeExpr`. The shallow projection uniformly covers a
/// literal object, a `Pick<…>`, an aliased param, AND the multi-arm intersected
/// node (which [`CallableNodeView::first_param_object_surface`] does NOT cover),
/// applying the SAME open-generic gate (`slot_param_root_is_symbolic_only`) both
/// binding paths use. A first-param node that does not project to an object
/// surface yields no bindings. Each per-member binding `TypeExpr` is minted ONCE
/// at the registered terminal [`slot_binding_field`]; this navigator holds NO
/// mint.
fn binding_fields_from_param_node(
    ctx: &dyn crate::resolver_core::ResolverContext,
    first_param: SemanticNodeId,
    scope: &TypeExprScope,
) -> Vec<AnalyzedSlotFieldBinding> {
    let dispatch = ctx.dispatch();
    let host = ctx.host_for_fact_tracer_install();
    // Open-generic gate: a symbolic-only param root (an open Conditional / mapped
    // / indexed / free `TypeParam`) must NOT be materialised into a committed
    // object surface — the SAME gate `navigate_param_to_object_surface` /
    // `first_param_object_surface` apply, keeping every binding path in agreement.
    if crate::meta_resolve::slot_binding_graph::slot_param_root_is_symbolic_only(
        &dispatch,
        first_param,
        0,
    ) {
        return Vec::new();
    }
    // Project the first-param node's one-level SHALLOW object surface (STAYS
    // Shallow / carrier-preserving — the root is NOT carrier-resolved, preserving
    // the `AppProps['avatar']` symbolic-access policy).
    let Some(surface) = host.project_shallow_surface_from_base(
        ctx,
        &dispatch,
        first_param,
        Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        ProjectionReductionContext::published(ProjectionMode::Shallow),
    ) else {
        return Vec::new();
    };
    // Shallow-by-default Pick member publication: when the slot param is a
    // `Pick<NamedRoot, K>` the picked members stay SYMBOLIC at the published
    // binding surface — the terminal sink builds each binding's value as the typed
    // indexed access `NamedRoot['member']`. The Pick source-root is read
    // NODE-DOMAIN from the first-param node (a thin structural `InstantiationRef`
    // read under the Vue Pick DTO policy — never a `"Pick<"` text sniff).
    let pick_root = pick_source_root_node(&dispatch, first_param);
    surface
        .members
        .iter()
        // Public-only publication.
        .filter(|member| member.visibility.is_public())
        .map(|member| slot_binding_field(ctx, member, pick_root, scope))
        .collect()
}

/// The Pick source-root NODE the Vue Pick DTO policy publishes each picked member
/// against. Peels the first-param node through the carrier-PRESERVING
/// [`ProjectSemanticDispatch::peel_node_for_uninstantiated_carrier_fact_demand`]
/// to reach an un-instantiated `Pick<Root, K>` `InstantiationRef`, then returns
/// its source-root arg (`args[0]`) ONLY when BOTH hold:
///
/// - **Builtin-Pick identity** — the carrier is the BUILTIN `Pick`
///   (`base.canonical_id == "__builtin__"` AND `base.decl_name == "Pick"`, two
///   args): the SAME builtin-utility identity the resolver's route extractors read
///   (`extract_route_root_identity_node` / `node_root_identity` in
///   `meta_resolve::graph_predicates`). A USERLAND `type Pick<T, K>` that shadows
///   the builtin is NOT a builtin `Pick` — its carrier base is the declaring file,
///   not `__builtin__`, so it fails here and each member mints its own concrete
///   (userland-Pick body) value instead of a symbolic access.
/// - **Nominal source root** — `args[0]` is a NOMINAL named reference (`DeclRef` /
///   `InstantiationRef` / the unresolved macro-carrier `BareRef`) — the reachable
///   breadth of an authored named-reference source. An INLINE macro-authored
///   `Pick<Source, K>` (in the `defineSlots` payload itself) lowers its source
///   root to `SemanticNodeData::BareRef` — the node-domain mirror of an authored
///   named reference — and the published binding shape must NOT depend on
///   inline-vs-named authorship, so `BareRef` is in the nominal set. A
///   STRUCTURAL source (`Pick<{ foo: string }, "foo">`) lowers `args[0]` to an
///   object/structural node — NOT nominal — so publishing a symbolic
///   `<object>['foo']` access would be bogus: return `None` and let each member
///   mint its own concrete member value.
///
/// NOTE: this predicate does NOT mirror `extract_pick_omit_route`'s arm set. In
/// that route extractor a non-nominal root returns `None` to PRESERVE the
/// carrier; here `None` means CONCRETE materialization — the OPPOSITE
/// consequence — so the arm sets are intentionally different.
///
/// A typed-IR STRUCTURAL match (node-domain `canonical_id` / `decl_name` + the
/// source-root shape) — NOT a `"Pick<"` text sniff and NOT a
/// materialise-then-decide. Any other shape (a literal object, a multi-arm
/// `Intersection` first param, a userland or non-Pick alias) returns `None`.
fn pick_source_root_node(
    dispatch: &ProjectSemanticDispatch<'_>,
    first_param: SemanticNodeId,
) -> Option<SemanticNodeId> {
    let peeled = dispatch.peel_node_for_uninstantiated_carrier_fact_demand(
        first_param,
        ProjectionReductionContext::published(ProjectionMode::Navigate),
    );
    match node_data_for(dispatch.ctx, peeled).as_deref() {
        Some(SemanticNodeData::InstantiationRef { base, args })
            if base.canonical_id.as_ref() == "__builtin__"
                && base.decl_name.as_ref() == "Pick"
                && args.len() == 2 =>
        {
            // Nominal-root restriction: publish the symbolic
            // `NamedRoot['member']` access for a nominal named-reference source
            // root — `DeclRef` / `InstantiationRef` / the unresolved
            // macro-carrier `BareRef` (an inline macro-authored source); a
            // structural source mints each member's own concrete value at the
            // sink instead. NOT `extract_pick_omit_route`'s arm set — there
            // `None` PRESERVES the carrier, here `None` means CONCRETE
            // materialization.
            let root = args[0];
            match node_data_for(dispatch.ctx, root).as_deref() {
                Some(
                    SemanticNodeData::DeclRef { .. }
                    | SemanticNodeData::InstantiationRef { .. }
                    | SemanticNodeData::BareRef(_),
                ) => Some(root),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Build ONE published slot binding for a surface member — a GENUINE decide-free
/// terminal one-shot sink. It takes NO `&TypeExpr` param (a surface member, the
/// node-domain `Option<SemanticNodeId>` Pick source-root, and the scope) and
/// makes NO decision on any materialised value:
///
/// - a `Pick` member (`pick_root == Some`) publishes the SYMBOLIC
///   `NamedRoot['member']` indexed access — the source root is minted ONCE
///   (internally) and the `IndexedAccess` is a pure syntactic display build (NOT
///   a reverse-materialisation), the shallow-by-default Pick policy;
/// - any other member mints its own value ONCE through the registered
///   [`raise_member_value`] sink.
///
/// The `pick_root` branch is a NODE-DOMAIN `Option` match, never a `TypeExpr`
/// decide; the display renders through the by-name `.and_then` form. The mint cap
/// is constructed INTERNALLY from `ctx` (the `raise_member_value` pattern).
fn slot_binding_field(
    ctx: &dyn crate::resolver_core::ResolverContext,
    member: &TypeInfoSurfaceMember,
    pick_root: Option<SemanticNodeId>,
    scope: &TypeExprScope,
) -> AnalyzedSlotFieldBinding {
    let host = ctx.host_for_fact_tracer_install();
    let (binding_expr, binding_expr_scope) = match pick_root {
        Some(root_node) => {
            // Mint the Pick source-root ONCE, then build the symbolic
            // `NamedRoot['member']` display access (a pure syntactic build).
            let dispatch = ctx.dispatch();
            let cap = super::TypeinfoVueSurfaceOutputCap::new(&dispatch);
            let named_root = cap
                .materialize_output_type_expr(root_node)
                .map(|raised| raised.into_type_expr(&cap))
                .unwrap_or(TypeExpr::Unknown { raw: String::new() });
            let symbolic = TypeExpr::IndexedAccess {
                object: Arc::new(named_root),
                index: Arc::new(TypeExpr::Literal(LiteralValue::String(
                    member.name.as_ref().to_string(),
                ))),
            };
            (Some(symbolic), Some(scope.clone()))
        }
        None => {
            let binding_expr = raise_member_value(ctx, member);
            let binding_expr_scope = binding_expr
                .as_ref()
                .map(|_| macro_member_value_scope(host, member, scope));
            (binding_expr, binding_expr_scope)
        }
    };
    let type_annotation = binding_expr.as_ref().and_then(render_type_expr_display);
    AnalyzedSlotFieldBinding {
        name: member.name.as_ref().to_string(),
        type_annotation,
        binding_expr,
        binding_expr_scope,
        span: verter_span::Span::default(),
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
