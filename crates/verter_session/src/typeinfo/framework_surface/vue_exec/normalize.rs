//! Per-surface `.vue` macro NORMALIZERS — the thin transforms that turn the
//! shared resolver's one-level macro surface into the published per-kind field
//! shapes (`AnalyzedPropField` / `AnalyzedEmitField` / `AnalyzedExposeField` /
//! `NamedTypeMember` / index signatures / model props; the slot normalizer
//! lives in the sibling [`super::normalize_slots`]).
//!
//! These are NOT resolvers — they slice JSDoc spans and raise each member's
//! already-resolved value node to a `TypeExpr` through the active `ctx`. The
//! resolution itself happens once in [`super::vue_macro_dtos_with_ctx`]; this
//! module is the kind-specific projection of that single surface.

use std::sync::Arc;

use verter_semantic::analysis::type_expand::ExpandedIndexSignature;
use verter_semantic::analysis::types::{AnalyzedEmitField, AnalyzedPropField};
use verter_semantic::analysis::AnalyzedMacroKind;
use verter_type_expr::{TypeExpr, TypeExprScope};

use super::{member_jsdoc_from_spans, raise_member_value, signature_jsdoc_from_spans};
use crate::meta_resolve::callable_view::CallableNodeView;
use crate::project_semantic_dispatch::output_materialization::OutputProjector;
use crate::project_semantic_dispatch::{node_data_for, ProjectSemanticDispatch};
use crate::resolver_core::surface_projector::render_type_expr_display;
use crate::semantic_query::{
    FunctionParam, ProjectionMode, ProjectionReductionContext, SemanticNodeData, SemanticNodeId,
};
use crate::typeinfo::framework_surface::resolved_surface_access::ResolvedSurfaceAccess;
use crate::typeinfo::framework_surface::results::ResolvedEmitField;
use crate::typeinfo::surface::TypeInfoSurfaceMember;

/// Normalize a `.vue` props macro surface into the published
/// [`ResolvedPropField`] set — one row per named member, pairing the prop
/// analysis field with its session-resolved member-value SOURCE.
///
/// Reproduces the eager rail's `AnalyzedPropField` stream over the typeinfo
/// surface: one field per named member, carrying the surface's `optional` /
/// `readonly` / `declared_in_macro_type_arg`, the display `type_annotation`
/// rendered from the member value minted ONCE at this terminal sink, and JSDoc
/// sliced from the surface spans. The published field is SHALLOW; the display
/// VALUE is paired with its resolution scope (the member's VALUE-NODE file —
/// where its `Ref`s resolve, see [`VueMacroSurface::member_expr_scope`]).
/// Own-body-vs-heritage ordering + shadowing + union-common membership are
/// ALREADY resolved on the surface — this is a thin per-member transform.
///
/// Per-row member-value SOURCE (the prop-type AUTHORITY —
/// `define_props_shape` publishes it directly):
///
/// - a LOCAL AUTHORED member — one addressed by EXACTLY ONE analyzer
///   prop-field candidate with a stamped byte-precise payload locator —
///   carries that EXACT authored macro-payload position, on BOTH the
///   analysis row's `payload` locator and the `Authored(MacroPayload(..))`
///   source (mirrors [`property_style_emit_fields`]). A MERGE-CAPABLE
///   member value (a composite / object — the shapes the surface merge
///   interns for multiple same-name contributors) additionally requires the
///   candidate's authored payload to PROVABLY cover the merged member
///   ([`authored_candidate_matches_member_value`]) — a resolvable local arm
///   never masks a FAILED same-name sibling into a concrete success;
/// - every other member (inherited / substituted / merged / structural)
///   publishes the graph-native closed / shallow-ref / projected member-path
///   source ([`member_value_source`]) and keeps the honest locator-less
///   analysis row;
/// - a genuine miss carries the typed
///   `Failed(UnrepresentableRequiredMemberValue)` position — a REQUIRED
///   value-type position never degrades to a fabricated `unknown` success.
///
/// `defineModel` does NOT carry an object type argument; its surface has no
/// named members and the synthesized model prop is appended from the analyzer
/// facts ([`AnalyzedMacroKind::DefineModel`]'s `prop_fields`).
#[must_use]
pub(crate) fn props_from_typeinfo_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    resolved: &impl ResolvedSurfaceAccess,
) -> Vec<crate::typeinfo::framework_surface::results::ResolvedPropField> {
    let macro_surface = resolved.macro_surface();
    // Host-level reads (graph node scope, JSDoc source slicing) go through the
    // host the active `ctx` is installed against; the view-sensitive type
    // resolution (`raise_member_value`) flows through `ctx`.
    let host = ctx.host_for_fact_tracer_install();
    let dispatch = ctx.dispatch();
    // `defineModel` contributes its synthesized model prop directly from the
    // analyzer facts (the type argument is the model VALUE type).
    if macro_surface.macro_kind == AnalyzedMacroKind::DefineModel {
        return model_prop_fields(ctx, resolved);
    }

    // The SFC's analyzer macro facts: the byte-precise authored payload
    // positions the analyzer could address in THIS file plus the macro's
    // STAMPED type-argument locator (the authored base the projected
    // member-path route replays off). Read through the ACTIVE `ctx` so an
    // overlay session sees the overlay facts.
    let indexed = ctx
        .ensure_indexed_ready_serve(macro_surface.owner_canonical.as_ref())
        .map(|serve| serve.indexed);
    let analyzed_macro = indexed
        .as_ref()
        .and_then(|indexed| indexed.snapshot.macros.get(macro_surface.macro_index));
    let analyzer_prop_fields: &[AnalyzedPropField] = analyzed_macro
        .map(|mac| mac.prop_fields.as_slice())
        .unwrap_or(&[]);
    let type_arg_base = analyzed_macro.and_then(|mac| mac.parsed_type_argument.as_ref());

    macro_surface
        .surface
        .members
        .iter()
        // Publication-boundary visibility filter: the shared surface RECORDS
        // non-public class members, but Vue does NOT expose `private` /
        // `protected` class fields as props.
        .filter(|member| member.visibility.is_public())
        .map(|member| {
            // Display-only render of the member's one-level value, minted ONCE
            // at this terminal sink (the by-name `.and_then` form — no decision
            // on the materialized value).
            let raised = raise_member_value(ctx, member);
            let type_annotation = raised.as_ref().and_then(render_type_expr_display);
            // LOCAL authored position candidates: analyzer prop fields
            // addressing this member name WITH a stamped payload locator.
            let candidates: Vec<&AnalyzedPropField> = analyzer_prop_fields
                .iter()
                .filter(|field| field.name == member.name.as_ref() && field.payload.is_some())
                .collect();
            // A contributor locator is published for EXACTLY ONE analyzer
            // candidate — the flat member sink's authored-position parity
            // (the analyzer's by-name field payload IS the member's own
            // authored annotation). Multiple candidates (duplicate /
            // intersection same-name contributors — any single contributor
            // would misrepresent the MERGED member) keep the row
            // locator-less — the graph-native member-value ladder below
            // represents the merged member instead.
            //
            // MERGE-CAPABLE value shapes additionally require the coverage
            // proof: a member VALUE encoded as a composite / object is
            // exactly how the surface merge combines multiple same-name
            // contributors (`merge_value_nodes_recursive` — distinct values
            // intern an `Intersection`, all-object values a merged
            // `Object`, union type arguments a `Union`), so the sole
            // candidate publishes ONLY when its authored payload provably
            // covers the merged member
            // ([`authored_candidate_matches_member_value`] — the same proof
            // the property-style emit rows apply): a resolvable local arm
            // can never mask a FAILED same-name sibling into a concrete
            // success. Every other value shape (a leaf, a reference
            // carrier, a symbolic access, a function) is single-contributor
            // by construction — the merge never encodes multiple distinct
            // contributors as those shapes — so the authored-position
            // parity holds unproven, exactly as the flat member sink
            // publishes it (a symbolic surface value folds differently from
            // its Navigate-raised authored form, so an unconditional proof
            // would false-negative the single-contributor classes).
            let member_value_is_merge_capable = matches!(
                node_data_for(dispatch.ctx, member.value).as_deref(),
                Some(
                    SemanticNodeData::Intersection(_)
                        | SemanticNodeData::Union(_)
                        | SemanticNodeData::Object(_)
                )
            );
            let authored_payload = match candidates.as_slice() {
                [candidate] if !member_value_is_merge_capable => candidate.payload.clone(),
                [candidate] => candidate.payload.clone().filter(|locator| {
                    authored_candidate_matches_member_value(
                        ctx,
                        &dispatch,
                        macro_surface.owner_canonical.as_ref(),
                        locator,
                        member.value,
                    )
                }),
                _ => None,
            };
            // Value⇔scope pairing: the display value rides with the member's
            // VALUE-NODE scope (where its `Ref`s resolve — the deriving file
            // for a substituted generic inherited member); a locator-bearing
            // row is paired with the SFC owner (the file whose OXC parse
            // produced the authored payload); an unrendered locator-less
            // value carries no scope.
            let type_expr_scope = type_annotation
                .as_ref()
                .map(|_| macro_surface.member_expr_scope(host, member))
                .or_else(|| {
                    authored_payload
                        .as_ref()
                        .map(|_| TypeExprScope::new(macro_surface.owner_canonical.as_ref()))
                });
            // The published member-value SOURCE POSITION: the complete
            // CLOSED leaf / leaf-union fact when the member's value node
            // decided one (the ratified published shape — the publication
            // finalize upgrades an authored position to exactly this fact,
            // and two files' identical closed members must publish the
            // IDENTICAL source value so the output memo shares one entry);
            // else the exact authored macro-payload position for a
            // single-contributor local authored member; else the
            // graph-native shallow-ref / use-site / projected member-path
            // source. A type-based macro member's value-type position is
            // REQUIRED — with no faithful source the position is the typed
            // source-construction FAILURE, never a fabricated `unknown`
            // success.
            let type_source = dispatch
                .node_leaf_fact(member.value)
                .map(|leaf| {
                    verter_type_expr::facts::SemanticTypeSource::Closed(
                        verter_type_expr::facts::ClosedTypeFact::Leaf(leaf),
                    )
                })
                .or_else(|| {
                    dispatch.node_leaf_union_fact(member.value).map(|leaves| {
                        verter_type_expr::facts::SemanticTypeSource::Closed(
                            verter_type_expr::facts::ClosedTypeFact::LeafUnion(leaves),
                        )
                    })
                })
                .or_else(|| {
                    authored_payload.clone().map(|locator| {
                        verter_type_expr::facts::SemanticTypeSource::Authored(
                            verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(locator),
                        )
                    })
                })
                .or_else(|| member_value_source(&dispatch, member, type_arg_base))
                .map(verter_type_expr::facts::SourcePosition::Present)
                .unwrap_or(verter_type_expr::facts::SourcePosition::Failed(
                    verter_type_expr::facts::SemanticSourceFailure::UnrepresentableRequiredMemberValue,
                ));
            let (description, tags) = member_jsdoc_from_spans(host, member);
            // `declared_in_macro_type_arg`: a member belongs to the macro-T own
            // body iff it is NOT heritage-reached. The terminal
            // `MacroTypeArgOwnBody` synthesis already stamps this correctly. The
            // `&& merge_role != Heritage` conjunct is REDUNDANT defense-in-depth
            // (a member can only carry `declared_in_macro_type_arg == true` if it
            // is an own-body `member_index` member).
            let declared_in_macro_type_arg = member.declared_in_macro_type_arg
                && member.origin.merge_role != crate::semantic_query::MemberMergeRole::Heritage;
            crate::typeinfo::framework_surface::results::ResolvedPropField {
                analysis: AnalyzedPropField {
                    name: member.name.as_ref().to_string(),
                    is_optional: member.optional,
                    span: verter_span::Span::default(),
                    type_annotation,
                    payload: authored_payload,
                    type_expr_scope,
                    description,
                    tags,
                    resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                    resolution_error: None,
                    declared_in_macro_type_arg,
                },
                type_source,
            }
        })
        .collect()
}

/// The graph-native member-value SOURCE for a props / expose / options
/// surface member with no PROVEN single-contributor authored position:
///
/// - the complete CLOSED fact when the member's VALUE node decides one — a
///   LEAF, a LEAF-UNION, or a TUPLE whose elements are all complete closed
///   element facts;
/// - else the demand-validated structural source
///   ([`crate::meta_resolve::projectors::structural_member_value_source`] —
///   the shallow symbol-reference carrier for a resolvable reference, or the
///   projected MEMBER-PATH replay route off the macro's stamped
///   type-argument base for every remaining KNOWN structural shape);
/// - else (no authored type argument to replay off) the arg-preserving
///   authored USE-SITE body slot;
/// - else `None` — a genuine miss; the caller types the REQUIRED position's
///   source-construction failure; a partial fact is never published and a
///   locator is never fabricated.
///
/// All decisions are NODE-domain; no `TypeExpr` is materialized here.
fn member_value_source(
    dispatch: &ProjectSemanticDispatch<'_>,
    member: &TypeInfoSurfaceMember,
    type_arg_base: Option<&verter_type_expr::locators::MacroPayloadLocator>,
) -> Option<verter_type_expr::facts::SemanticTypeSource> {
    use verter_type_expr::facts::{ClosedTypeFact, SemanticTypeSource};
    if let Some(leaf) = dispatch.node_leaf_fact(member.value) {
        return Some(SemanticTypeSource::Closed(ClosedTypeFact::Leaf(leaf)));
    }
    if let Some(leaves) = dispatch.node_leaf_union_fact(member.value) {
        return Some(SemanticTypeSource::Closed(ClosedTypeFact::LeafUnion(
            leaves,
        )));
    }
    if let Some(tuple) = closed_tuple_fact(dispatch, member.value) {
        return Some(SemanticTypeSource::Closed(ClosedTypeFact::Tuple(tuple)));
    }
    // The arg-preserving authored USE-SITE body slot (the declaring
    // declaration's member-value slot, whose deref replays the authored
    // generic instantiation WITH its type arguments through the one shared
    // dispatch) is the ratified publication for an instantiation-valued
    // member — preferred over the member-path replay when recoverable.
    if let Some(slot) = crate::meta_resolve::arg_preserving_member_use_site_slot(
        dispatch,
        member.name.as_ref(),
        member.origin.canonical_file.as_deref(),
        member.value,
    ) {
        return Some(SemanticTypeSource::Authored(
            verter_type_expr::locators::AuthoredBodyLocator::DeclBody(slot),
        ));
    }
    crate::meta_resolve::projectors::structural_member_value_source(
        dispatch,
        member.value,
        member.name.as_ref(),
        type_arg_base,
    )
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
/// and the member value raised through the active `ctx` and classified INTO
/// the sealed shallow
/// [`NamedTypeMemberOutput`](crate::typeinfo::framework_surface::results::NamedTypeMemberOutput)
/// vocabulary at this publication boundary — the raised form is transient and
/// discarded here; no raw `TypeExpr` enters the DTO. The shallow-by-default
/// rule holds — `raise_member_value` raises the member's one-level value node,
/// it does not eagerly expand it.
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
                value: raise_member_value(ctx, member).map(|raised| {
                    crate::typeinfo::framework_surface::results::NamedTypeMemberOutput::classify_shallow(&raised)
                }),
            },
        )
        .collect()
}

/// Normalize a `defineExpose<T>()` macro surface into
/// [`ResolvedExposeField`](crate::typeinfo::framework_surface::results::ResolvedExposeField)
/// rows: one field per named public member, carrying its JSDoc sliced from
/// the enriched typeinfo spans plus the member VALUE's session-resolved
/// SOURCE POSITION (the exposed-type authority the extraction layer
/// publishes for type-argument-only members).
///
/// The pass-through [`NamedTypeMember`] surface ([`object_members_from_typeinfo_surface`])
/// is a REDUCED shape that drops the JSDoc the component-meta extract layer
/// publishes, so expose carries this richer per-member normalize alongside it.
/// The analysis row is SHALLOW: a resolved-surface member has no flat
/// authored macro-payload position (expose analyzer fields never stamp a
/// payload), so `payload` stays the honest `None` (paired with a `None`
/// scope); the typed member source rides `type_source` — the graph-native
/// closed / shallow-ref / projected member-path ladder
/// ([`member_value_source`]), or the typed
/// `Failed(UnrepresentableRequiredMemberValue)` for a genuine miss. The
/// field's `span` is `None`: the surface member's spans index its
/// DECLARATION file, not the SFC, so there is no SFC-absolute key span to
/// report; downstream, `extract_exposed_from_macro` publishes the union of
/// the SFC object-literal fields (which DO carry a span) and these surface
/// members.
#[must_use]
pub(crate) fn exposed_from_typeinfo_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    resolved: &impl ResolvedSurfaceAccess,
) -> Vec<crate::typeinfo::framework_surface::results::ResolvedExposeField> {
    let macro_surface = resolved.macro_surface();
    let host = ctx.host_for_fact_tracer_install();
    let dispatch = ctx.dispatch();
    // The macro's STAMPED type-argument locator — the authored base the
    // projected member-path route replays off.
    let type_arg_base = ctx
        .ensure_indexed_ready_serve(macro_surface.owner_canonical.as_ref())
        .map(|serve| serve.indexed)
        .as_ref()
        .and_then(|indexed| indexed.snapshot.macros.get(macro_surface.macro_index))
        .and_then(|mac| mac.parsed_type_argument.clone());
    macro_surface
        .surface
        .members
        .iter()
        .filter(|member| member.visibility.is_public())
        .map(|member| {
            let (description, tags) = member_jsdoc_from_spans(host, member);
            // The published member-value SOURCE POSITION: the graph-native
            // closed / shallow-ref / projected member-path source. An
            // exposed type-argument member's value-type position is
            // REQUIRED — with no faithful source the position is the typed
            // source-construction FAILURE, never a fabricated `unknown`
            // success.
            let type_source = member_value_source(&dispatch, member, type_arg_base.as_ref())
                .map(verter_type_expr::facts::SourcePosition::Present)
                .unwrap_or(verter_type_expr::facts::SourcePosition::Failed(
                    verter_type_expr::facts::SemanticSourceFailure::UnrepresentableRequiredMemberValue,
                ));
            crate::typeinfo::framework_surface::results::ResolvedExposeField {
                analysis: verter_semantic::analysis::types::AnalyzedExposeField {
                    name: member.name.as_ref().to_string(),
                    span: None,
                    payload: None,
                    type_expr_scope: None,
                    description,
                    tags,
                },
                type_source,
            }
        })
        .collect()
}

/// Normalize a macro surface's INDEX SIGNATURES into the published
/// [`ExpandedIndexSignature`] set. A props member is `properties + index
/// signatures` and an emits object is `events + index signatures`. Kind-neutral:
/// it publishes whatever index signatures the surface carries. Each signature's
/// `key_type` / `value_type` graph node projects to its content-free
/// [`SourcePosition`](verter_type_expr::facts::SourcePosition) — see
/// [`index_position_source`] for the faithful-vs-failed decision.
pub(crate) fn index_signatures_from_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    resolved: &impl ResolvedSurfaceAccess,
) -> Vec<ExpandedIndexSignature> {
    let macro_surface = resolved.macro_surface();
    let dispatch = ctx.dispatch();
    // The macro's STAMPED type-argument locator — the authored base the
    // projected INDEX-POSITION replay route addresses (mirrors the emit
    // callable-params base read; a parse-domain fact lookup through the
    // ACTIVE `ctx`, never a resolution).
    let type_arg_base = ctx
        .ensure_indexed_ready_serve(macro_surface.owner_canonical.as_ref())
        .map(|serve| serve.indexed)
        .as_ref()
        .and_then(|indexed| indexed.snapshot.macros.get(macro_surface.macro_index))
        .and_then(|mac| mac.parsed_type_argument.clone());
    macro_surface
        .surface
        .index_signatures
        .iter()
        .enumerate()
        .map(|(signature_ordinal, sig)| ExpandedIndexSignature {
            key_type: index_position_source(
                &dispatch,
                sig.key_type,
                type_arg_base.as_ref(),
                signature_ordinal as u32,
                verter_type_expr::facts::IndexSignaturePosition::Key,
            ),
            value_type: index_position_source(
                &dispatch,
                sig.value_type,
                type_arg_base.as_ref(),
                signature_ordinal as u32,
                verter_type_expr::facts::IndexSignaturePosition::Value,
            ),
            readonly: sig.readonly,
        })
        .collect()
}

/// The published SOURCE POSITION for one index-signature key/value position:
/// `Present` with the complete CLOSED fact when the node is one — a LEAF
/// (primitive / literal, mirroring the projector `published_source_for_node`
/// policy; a genuinely-OPEN `[key: string]` domain is the present `string`
/// leaf — semantic openness is a valid success), or a TUPLE whose element
/// values are all complete closed element facts (the emit payload-tuple
/// shape `[v: number]` of `defineEmits<{ [event: string]: [v: number] }>()`
/// — leaf and leaf-union elements are complete by themselves, so the fact
/// demands back to the typed tuple through the shared raise bridge). A
/// RICHER position (a nested object, a function, a composite, a reference)
/// demand-validates through the shared structural-fact primitive and
/// publishes the projected INDEX-POSITION replay route
/// ([`ProjectedTypeFact::IndexPosition`](verter_type_expr::facts::ProjectedTypeFact)
/// — the macro's STAMPED type-argument base + the signature's
/// declaration-order SURFACE ordinal + the key/value role, replayed through
/// the one shared dispatch on demand; the published position stays SHALLOW).
/// ONLY a genuine miss — an unresolvable residual carrier, an
/// unknown-materializing failure, or no stamped base to replay off — is the
/// typed source-construction FAILURE
/// (`UnrepresentableRequiredMemberValue`): a REQUIRED index key/value
/// position with no faithful projected source marks the result non-complete
/// instead of degrading to a fabricated `unknown` success. All decisions are
/// NODE-DOMAIN (`node_leaf_fact` / `node_leaf_fact_or_union` /
/// `node_data_for` / the structural-fact demand) — no `TypeExpr` is
/// materialized here.
fn index_position_source(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    type_arg_base: Option<&verter_type_expr::locators::MacroPayloadLocator>,
    signature_ordinal: u32,
    position: verter_type_expr::facts::IndexSignaturePosition,
) -> verter_type_expr::facts::SourcePosition {
    use verter_type_expr::facts::{
        ClosedTypeFact, ProjectedTypeFact, SemanticSourceFailure, SemanticTypeSource,
        SourcePosition,
    };
    if let Some(leaf) = dispatch.node_leaf_fact(node) {
        return SourcePosition::Present(SemanticTypeSource::Closed(ClosedTypeFact::Leaf(leaf)));
    }
    // A tuple whose element values are ALL complete closed element facts
    // (leaf or leaf-union) is complete by itself: publish the closed tuple
    // fact (label/optional/rest preserved). Any richer element falls through
    // to the projected replay route — a partial fact is never published.
    if let Some(tuple) = closed_tuple_fact(dispatch, node) {
        return SourcePosition::Present(SemanticTypeSource::Closed(ClosedTypeFact::Tuple(tuple)));
    }
    // A richer position that demand-validates to KNOWN structure publishes
    // the content-free replay address off the stamped type-argument base;
    // a genuine miss (unresolvable carrier / unknown-materializing / no
    // base) stays the typed failure. STRUCTURAL TRANSIT, not `Published`:
    // validation is a carrier-preserving classification, never consumer
    // demand — operator reduction stays deferred so no library keyspace is
    // enumerated at publication.
    if let Some(base) = type_arg_base {
        let validated = dispatch
            .demand_validated_structural_node(
                node,
                ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
            )
            .is_some();
        if validated {
            return SourcePosition::Present(SemanticTypeSource::Projected(
                ProjectedTypeFact::IndexPosition {
                    base: verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(
                        base.clone(),
                    ),
                    signature_ordinal,
                    position,
                },
            ));
        }
    }
    SourcePosition::Failed(SemanticSourceFailure::UnrepresentableRequiredMemberValue)
}

/// The complete closed TUPLE fact for a `Tuple`-shaped node whose element
/// values are ALL complete closed element facts (leaf or leaf-union) —
/// label / optionality / rest / ORDER preserved. `None` for a non-tuple
/// node, or when ANY element is richer than the closed element vocabulary:
/// the whole tuple fails closed — a partial fact is never published. Pure
/// node→fact projection (`node_data_for` + `node_leaf_fact_or_union`); no
/// reduction, no dispatch execution, no `TypeExpr` materialization. Shared
/// by the index-signature positions ([`index_position_source`]) and the
/// inherited property-style emit payloads
/// ([`inherited_emit_payload_source`]).
fn closed_tuple_fact(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Option<verter_type_expr::facts::TuplePayloadFact> {
    use verter_type_expr::facts::{TupleElementFact, TuplePayloadFact};
    let data = node_data_for(dispatch.ctx, node)?;
    let SemanticNodeData::Tuple { elements, readonly } = data.as_ref() else {
        return None;
    };
    let element_facts: Option<Vec<TupleElementFact>> = elements
        .iter()
        .map(|element| {
            dispatch
                .node_leaf_fact_or_union(element.value)
                .map(|ty| TupleElementFact {
                    label: element.label.as_ref().map(|label| label.to_string()),
                    optional: element.optional,
                    rest: element.rest,
                    ty,
                })
        })
        .collect();
    Some(TuplePayloadFact {
        readonly: *readonly,
        elements: Arc::from(element_facts?.into_boxed_slice()),
    })
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
) -> Vec<crate::typeinfo::framework_surface::results::ResolvedPropField> {
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
            // re-anchor it to the SFC owner so the pairing invariant
            // (`payload.is_some() <=> type_expr_scope.is_some()`) holds with a
            // real scope.
            let type_expr_scope = field
                .payload
                .as_ref()
                .map(|_| TypeExprScope::new(macro_surface.owner_canonical.as_ref()));
            // The synthesized model prop's SOURCE is its own authored
            // type-argument payload position (`defineModel<T>()`'s T); an
            // UNTYPED `defineModel()` has no annotation — a PROVEN
            // unannotated schema absence, never a fabricated `unknown`
            // value and never a required-position failure.
            let type_source = field
                .payload
                .clone()
                .map(|payload| {
                    verter_type_expr::facts::SourcePosition::Present(
                        verter_type_expr::facts::SemanticTypeSource::Authored(
                            verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(payload),
                        ),
                    )
                })
                .unwrap_or_else(verter_type_expr::facts::SourcePosition::unannotated);
            crate::typeinfo::framework_surface::results::ResolvedPropField {
                analysis: AnalyzedPropField {
                    type_expr_scope,
                    ..field.clone()
                },
                type_source,
            }
        })
        .collect()
}

/// Normalize a `.vue` emits macro surface into the published
/// [`ResolvedEmitField`] set — the UNION of both authored emit forms, each row
/// pairing the emit analysis field with its session-resolved payload SOURCE.
///
/// 1. **Call-signature emits FIRST.** Each call signature's first parameter is
///    the event name (a `String` literal, or a `Union` of `String` literals);
///    the display `payload_type` renders the call-signature params with the
///    leading event-name parameter STRIPPED. The event name is NEVER read from
///    `keyof`. The payload SOURCE is the closed tuple built from the SAME
///    post-event-name params in the node domain (label / optionality / rest /
///    order preserved; leaf and leaf-union element facts) when every param is
///    closed-expressible; a signature with a RICHER param (a named reference,
///    a composite, a nested object, an array/callback, an instantiated
///    generic) publishes the projected CALLABLE-PARAMS replay route instead
///    ([`ProjectedTypeFact::CallableParams`](verter_type_expr::facts::ProjectedTypeFact)
///    — the macro's STAMPED type-argument base + the signature's
///    declaration-order SURFACE ordinal + `first_param = 1`, replayed through
///    the one shared dispatch on demand). A partial fact is never published.
/// 2. **Property-style emits ALWAYS.** A property member inside a
///    `defineEmits<T>` object surface IS an emit — a mixed surface publishes
///    BOTH forms (never an either/or gate on call-signature discovery). A
///    LOCAL authored property event's payload SOURCE is its exact authored
///    macro-payload position (the analyzer-stamped locator); an INHERITED /
///    substituted member publishes the graph-native closed/use-site source
///    projected from its value node; `None` only when no faithful source
///    exists (see [`property_style_emit_fields`]).
/// 3. **De-duplicate by event name, first-writer-wins.** Call-signature emits
///    are pushed first, so a duplicate name takes CALL-SIGNATURE precedence;
///    order is deterministic (signature order, then member order).
#[must_use]
pub(crate) fn emits_from_typeinfo_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    resolved: &impl ResolvedSurfaceAccess,
) -> Vec<ResolvedEmitField> {
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

    // The macro's STAMPED type-argument locator — the authored base the
    // projected CALLABLE-PARAMS replay route addresses (mirrors the
    // property-style member-path base read; a parse-domain fact lookup
    // through the ACTIVE `ctx`, never a resolution).
    let type_arg_base = ctx
        .ensure_indexed_ready_serve(macro_surface.owner_canonical.as_ref())
        .map(|serve| serve.indexed)
        .as_ref()
        .and_then(|indexed| indexed.snapshot.macros.get(macro_surface.macro_index))
        .and_then(|mac| mac.parsed_type_argument.clone());

    let mut emits: Vec<ResolvedEmitField> = Vec::new();

    // (1) Call-signature emits — decided in the NODE domain. The enumeration
    // ordinal is the SURFACE's declaration-order call-signature sequence —
    // the exact pre-expansion index the CallableParams replay re-selects —
    // so a signature that contributes no emit row (no event-name literal)
    // still occupies its ordinal.
    for (signature_ordinal, sig) in macro_surface.surface.call_signatures.iter().enumerate() {
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
        // Materialize the payload tuple ONCE at the terminal sink for DISPLAY
        // ONLY (the by-name `render_type_expr_display` form — no decision on
        // the materialized value). The tuple is a per-event SYNTHESIS over the
        // signature's params — it has no flat authored macro-payload position,
        // so the `payload` LOCATOR stays the honest `None`; typed payload
        // demand is host-raised through the graph surface.
        let payload_tuple = materialize_payload_tuple(ctx, &raw_params[1..]);
        // `payload_type` (→ `rawType`) is DISPLAY-ONLY — no consumer parses it.
        // It mirrors the payload TUPLE rendered as `[label: T, ...]`.
        let payload_type = render_type_expr_display(&payload_tuple);
        // The payload SOURCE POSITION: the closed tuple over the SAME
        // post-event-name params, projected in the node domain through the
        // shared dispatch (leaf / leaf-union element facts, order preserved)
        // — complete by itself when every param is closed-expressible
        // (including the zero-payload empty tuple). A signature with a
        // RICHER param publishes the projected CALLABLE-PARAMS replay route
        // off the macro's stamped type-argument base: the content-free
        // `(base, surface signature ordinal, first_param = 1)` address the
        // demand side replays through the one shared dispatch — labels /
        // optionality / rest / order / nesting / generic substitutions all
        // ride the replay, so the faithful payload never degrades. The
        // realized signature's payload-tuple position is REQUIRED — with no
        // stamped type-argument base to replay off (no authored macro type
        // argument), the position is the typed source-construction FAILURE
        // (fails output materialization), never a partial fact and never a
        // fabricated `unknown` success.
        let payload_source = dispatch
            .closed_params_tuple_source(&raw_params[1..])
            .map(verter_type_expr::facts::SourcePosition::Present)
            .or_else(|| {
                type_arg_base.as_ref().map(|base| {
                    verter_type_expr::facts::SourcePosition::Present(
                        verter_type_expr::facts::SemanticTypeSource::Projected(
                            verter_type_expr::facts::ProjectedTypeFact::CallableParams {
                                base: verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(
                                    base.clone(),
                                ),
                                signature_ordinal: signature_ordinal as u32,
                                first_param: 1,
                            },
                        ),
                    )
                })
            })
            .unwrap_or(verter_type_expr::facts::SourcePosition::Failed(
                verter_type_expr::facts::SemanticSourceFailure::UnrepresentableRequiredPayload,
            ));
        // The published display VALUE is paired with its resolution scope: the
        // call signature's DECLARATION-origin file (a cross-file emit
        // interface's payload `Ref`s resolve in the base file the signature
        // was declared in, not the SFC owner). Value⇔scope pairing: a display
        // that did not render carries no scope.
        let payload_expr_scope = payload_type
            .as_ref()
            .map(|_| macro_surface.signature_expr_scope(sig));
        // The event's JSDoc rides on the call signature itself, sliced from the
        // signature's typeinfo JSDoc spans. A union of event-name literals on ONE
        // signature shares that signature's JSDoc across each event.
        let (description, tags) = signature_jsdoc_from_spans(host, sig);
        for name in names {
            emits.push(ResolvedEmitField {
                analysis: AnalyzedEmitField {
                    name: name.to_string(),
                    span: verter_span::Span::default(),
                    payload_type: payload_type.clone(),
                    payload: None,
                    payload_expr_scope: payload_expr_scope.clone(),
                    description: description.clone(),
                    tags: tags.clone(),
                },
                payload_source: payload_source.clone(),
            });
        }
    }

    // (2) Property-style emits — ALWAYS unioned in (a property member inside a
    // `defineEmits<T>` object surface IS an emit; a mixed surface publishes
    // both forms). The member materialization AND the per-row payload-source
    // decision live in the terminal `property_style_emit_fields` sink so this
    // normalizer mints nothing directly. Appended AFTER the call-signature
    // emits so the de-dup below gives duplicate names call-signature
    // precedence.
    emits.extend(property_style_emit_fields(ctx, resolved));

    // (3) De-duplicate by event name, first-writer-wins (call-signature emits
    // were pushed first, so they win duplicate names).
    let mut seen = std::collections::HashSet::new();
    emits.retain(|emit| seen.insert(emit.analysis.name.clone()));
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

/// Build the property-style Vue emit rows — the property HALF of the emit
/// UNION (`emits_from_typeinfo_surface` always appends these after the
/// call-signature emits; duplicate names resolve by first-writer-wins there).
/// A GENUINE decide-free terminal one-shot sink: it iterates the surface's
/// PUBLIC members (a node-domain visibility fact), mints each member value ONCE
/// through the registered `raise_member_value` sink for the DISPLAY
/// `payload_type`, and builds the [`ResolvedEmitField`] row (name + JSDoc from
/// the surface, the analysis payload locator, and the published payload
/// SOURCE). It makes NO decision on any materialized `TypeExpr` (the raised
/// value is display-rendered, never branched on) and takes NO `&TypeExpr`
/// param — structurally identical to the [`props_from_typeinfo_surface`]
/// member loop — so the non-terminal `emits_from_typeinfo_surface` delegates
/// here instead of minting inline.
///
/// Per-row payload SOURCE (the faithful fallback `define_emits_shape`
/// publishes when the evaluated-field match is absent):
///
/// - a LOCAL AUTHORED property event — one addressed by EXACTLY ONE analyzer
///   emit-field candidate with a stamped byte-precise payload locator, whose
///   authored payload PROVABLY raises to the same complete shape as the
///   surface member's value (the node-domain raised-shape proof,
///   [`authored_candidate_matches_member_value`]) — carries that EXACT
///   authored macro-payload position, on BOTH the analysis row's `payload`
///   locator and the `Authored(MacroPayload(..))` source. Multiple
///   candidates (duplicate / intersection same-name contributors — any
///   single contributor would misrepresent the MERGED member) or a failed /
///   unprovable equality publish NO contributor locator;
/// - every other member (inherited / substituted / merged) publishes the
///   graph-native closed fact, the projected member-path route, or the
///   use-site source projected from its VALUE node
///   ([`inherited_emit_payload_source`]) and keeps the honest locator-less
///   analysis row;
/// - `None` only when no faithful source exists (the consumer's honest
///   degraded fallback applies — never a partial or fabricated fact).
pub(in crate::typeinfo::framework_surface::vue_exec) fn property_style_emit_fields(
    ctx: &dyn crate::resolver_core::ResolverContext,
    resolved: &impl ResolvedSurfaceAccess,
) -> Vec<ResolvedEmitField> {
    let macro_surface = resolved.macro_surface();
    let host = ctx.host_for_fact_tracer_install();
    let dispatch = ctx.dispatch();
    // The SFC's analyzer macro facts: the byte-precise authored payload
    // positions the analyzer could address in THIS file (locally-declared
    // property events and local-registry-resolved bodies; cross-file members
    // never appear here) plus the macro's STAMPED type-argument locator (the
    // authored base the projected member-path route replays off). Read
    // through the ACTIVE `ctx` so an overlay session sees the overlay facts.
    let indexed = ctx
        .ensure_indexed_ready_serve(macro_surface.owner_canonical.as_ref())
        .map(|serve| serve.indexed);
    let analyzed_macro = indexed
        .as_ref()
        .and_then(|indexed| indexed.snapshot.macros.get(macro_surface.macro_index));
    let analyzer_emit_fields: &[AnalyzedEmitField] = analyzed_macro
        .map(|mac| mac.emit_fields.as_slice())
        .unwrap_or(&[]);
    let type_arg_base = analyzed_macro.and_then(|mac| mac.parsed_type_argument.as_ref());
    macro_surface
        .surface
        .members
        .iter()
        // Public-only publication: a `private` / `protected` class member
        // recorded on the shared surface must NOT leak as a published emit.
        .filter(|member| member.visibility.is_public())
        .map(|member| {
            let raised = raise_member_value(ctx, member);
            let payload_type = raised.as_ref().and_then(render_type_expr_display);
            // LOCAL authored position candidates: analyzer emit fields
            // addressing this event name WITH a stamped payload locator
            // (duplicate / intersection contributors stamp several same-name
            // fields — one per arm, in source order).
            let candidates: Vec<&AnalyzedEmitField> = analyzer_emit_fields
                .iter()
                .filter(|field| field.name == member.name.as_ref() && field.payload.is_some())
                .collect();
            // A contributor locator is published ONLY when it provably
            // denotes the resolved member: EXACTLY ONE candidate whose
            // authored payload raises to the SAME complete shape as the
            // surface member's VALUE node. Multiple candidates or a failed /
            // unprovable equality keep the row locator-less — the projected
            // member-path route below represents the MERGED member instead.
            let authored_payload = match candidates.as_slice() {
                [candidate] => candidate.payload.clone().filter(|locator| {
                    authored_candidate_matches_member_value(
                        ctx,
                        &dispatch,
                        macro_surface.owner_canonical.as_ref(),
                        locator,
                        member.value,
                    )
                }),
                _ => None,
            };
            // Value⇔scope pairing: the display value rides with the member's
            // VALUE-NODE scope (where its `Ref`s resolve); a locator-bearing
            // row is paired with the file whose OXC parse produced the
            // authored payload (the SFC owner); an unrendered locator-less
            // value carries no scope.
            let payload_expr_scope = payload_type
                .as_ref()
                .map(|_| macro_surface.member_expr_scope(host, member))
                .or_else(|| {
                    authored_payload
                        .as_ref()
                        .map(|_| TypeExprScope::new(macro_surface.owner_canonical.as_ref()))
                });
            // The published payload SOURCE POSITION: the exact authored
            // macro-payload position for a PROVEN single-contributor local
            // authored event, else the graph-native closed / projected
            // member-path / use-site source for an inherited / substituted /
            // merged member. A property event's payload position is
            // REQUIRED — with no faithful source the position is the typed
            // source-construction FAILURE, never a fabricated `unknown`
            // success.
            let payload_source = authored_payload
                .clone()
                .map(|locator| {
                    verter_type_expr::facts::SemanticTypeSource::Authored(
                        verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(locator),
                    )
                })
                .or_else(|| inherited_emit_payload_source(&dispatch, member, type_arg_base))
                .map(verter_type_expr::facts::SourcePosition::Present)
                .unwrap_or(verter_type_expr::facts::SourcePosition::Failed(
                    verter_type_expr::facts::SemanticSourceFailure::UnrepresentableRequiredPayload,
                ));
            let (description, tags) = member_jsdoc_from_spans(host, member);
            ResolvedEmitField {
                analysis: AnalyzedEmitField {
                    name: member.name.as_ref().to_string(),
                    span: verter_span::Span::default(),
                    payload_type,
                    payload: authored_payload,
                    payload_expr_scope,
                    description,
                    tags,
                },
                payload_source,
            }
        })
        .collect()
}

/// The graph-native payload SOURCE for a property-style emit member with no
/// PROVEN single-contributor authored position (inherited / substituted /
/// merged same-name members):
///
/// - the complete CLOSED fact when the member's VALUE node decides one — a
///   LEAF, a LEAF-UNION, or a TUPLE whose elements are all complete closed
///   element facts (the emit payload-tuple shape `[id: number]` of an
///   imported emits interface); closed facts are computed from the MERGED
///   value node, so they stay faithful for a merged member too;
/// - else, with a STAMPED macro type-argument base AND a merged value that
///   DEMAND-VALIDATES through the shared structural-fact primitive, the
///   projected MEMBER-PATH route
///   ([`ProjectedTypeFact::MemberPath`](verter_type_expr::facts::ProjectedTypeFact) —
///   base + event-name path, replayed through the one dispatch's EXISTING
///   `ProjectPath` query on demand): the faithful source for merged
///   same-name members, inherited referenced tuples / objects, and
///   substituted generic surfaces, which the closed vocabulary cannot
///   express and a single contributor locator would misrepresent. A merged
///   value that FAILS validation (an unresolvable contributor) publishes NO
///   replay route — the caller types the required-payload failure;
/// - else (no authored type argument to replay off) the arg-preserving
///   authored USE-SITE body slot
///   ([`crate::meta_resolve::arg_preserving_member_use_site_slot`] — the
///   declaring declaration's member-value slot, whose deref replays the
///   authored generic instantiation WITH its type arguments through the one
///   shared dispatch);
/// - else `None` — genuinely unraisable; the consumer publishes its honest
///   degraded source; a partial fact is never published and a locator is
///   never fabricated.
///
/// All decisions are NODE-domain (`node_leaf_fact` / `node_leaf_union_fact`
/// / `node_data_for` over the member's value node); no `TypeExpr` is
/// materialized here.
fn inherited_emit_payload_source(
    dispatch: &ProjectSemanticDispatch<'_>,
    member: &TypeInfoSurfaceMember,
    type_arg_base: Option<&verter_type_expr::locators::MacroPayloadLocator>,
) -> Option<verter_type_expr::facts::SemanticTypeSource> {
    use verter_type_expr::facts::{ClosedTypeFact, ProjectedTypeFact, SemanticTypeSource};
    if let Some(leaf) = dispatch.node_leaf_fact(member.value) {
        return Some(SemanticTypeSource::Closed(ClosedTypeFact::Leaf(leaf)));
    }
    if let Some(leaves) = dispatch.node_leaf_union_fact(member.value) {
        return Some(SemanticTypeSource::Closed(ClosedTypeFact::LeafUnion(
            leaves,
        )));
    }
    if let Some(tuple) = closed_tuple_fact(dispatch, member.value) {
        return Some(SemanticTypeSource::Closed(ClosedTypeFact::Tuple(tuple)));
    }
    if let Some(base) = type_arg_base {
        // The projected replay route denotes the MERGED member — publish it
        // ONLY when the merged value demand-validates through the shared
        // structural-fact primitive (the same per-root validation the
        // structural member-source projection applies). A merged value with
        // a FAILED contributor (`[id: number] & <unresolvable import>`) must
        // fail the REQUIRED payload position typed (`None` here) instead of
        // riding a replay whose demand-side reduction would drop the failed
        // arm and mask it with the resolvable sibling's tuple.
        dispatch.demand_validated_structural_node(
            member.value,
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
        )?;
        return Some(SemanticTypeSource::Projected(
            ProjectedTypeFact::MemberPath {
                base: verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(base.clone()),
                path: std::sync::Arc::from(
                    vec![member.name.as_ref().to_string()].into_boxed_slice(),
                ),
            },
        ));
    }
    crate::meta_resolve::arg_preserving_member_use_site_slot(
        dispatch,
        member.name.as_ref(),
        member.origin.canonical_file.as_deref(),
        member.value,
    )
    .map(|slot| {
        SemanticTypeSource::Authored(verter_type_expr::locators::AuthoredBodyLocator::DeclBody(
            slot,
        ))
    })
}

/// Raised-shape COVERAGE proof for a SINGLE local authored candidate (the
/// shared proof of the props and property-style emit rows): the candidate's
/// authored payload locator (absolutized to the SFC owner) raises through
/// the shared authored-locator routing under a memoized `Navigate`
/// structural transit — one member annotation, never a body expansion — and
/// must fold to the SAME interned raised shape as the surface member's
/// VALUE node
/// ([`crate::project_semantic_dispatch::raise::raised_shape_eq_nodes`]), OR
/// — for a merged COMPOSITE member value — to the same raised shape as
/// EVERY contributing arm (identical-shape same-name contributors are all
/// denoted exactly by the one authored annotation, e.g. an own-body member
/// merged with a shape-identical heritage duplicate interns
/// `Intersection([v, v'])` over two same-shape nodes).
///
/// Only a proven coverage publishes the contributor locator; an unraisable
/// candidate or an unprovable / failed equality (`None` / `Some(false)`,
/// including ANY composite arm that does not match — a FAILED contributor
/// can never be covered by its resolvable sibling) fails CLOSED — the
/// caller publishes the graph-native merged-member route instead.
/// Node-domain only; no `TypeExpr` is materialized here.
fn authored_candidate_matches_member_value(
    ctx: &dyn crate::resolver_core::ResolverContext,
    dispatch: &ProjectSemanticDispatch<'_>,
    owner_canonical: &str,
    locator: &verter_type_expr::locators::MacroPayloadLocator,
    member_value: SemanticNodeId,
) -> bool {
    let Some(raised) = dispatch.raise_authored_locator_to_hot(
        &verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(locator.clone())
            .absolutized_against(owner_canonical),
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
    ) else {
        return false;
    };
    if crate::project_semantic_dispatch::raise::raised_shape_eq_nodes(
        ctx,
        raised.node(),
        member_value,
    ) == Some(true)
    {
        return true;
    }
    // Composite coverage: the authored candidate denotes the merged member
    // when EVERY contributing arm folds to the candidate's own raised shape
    // (an agree-duplicate merge). A single non-matching arm — a different
    // shape, or an unresolvable failed contributor — fails the proof.
    match node_data_for(dispatch.ctx, member_value).as_deref() {
        Some(SemanticNodeData::Intersection(arms) | SemanticNodeData::Union(arms)) => {
            !arms.is_empty()
                && arms.iter().all(|&arm| {
                    crate::project_semantic_dispatch::raise::raised_shape_eq_nodes(
                        ctx,
                        raised.node(),
                        arm,
                    ) == Some(true)
                })
        }
        _ => false,
    }
}
