//! Project-scoped HTML-intrinsic surface rail (engine side).
//!
//! The host `host_manage::intrinsic_projection` callers resolve the
//! `JSX.IntrinsicElements` / `HTMLAttributes` root shape and the per-tag member
//! surface through these engine demand methods. Every node-domain decision
//! (route projection, surface composition) stays inside the query-engine sink;
//! only finished `ExpandedObjectShape` DTOs — whose member values are shallow
//! semantic SOURCES — cross back to the host. The raw `SemanticNodeId`
//! projection helpers stay subtree-confined exactly as the registry /
//! route-fixpoint siblings keep them.
//!
//! The shape builder here is a direct `NoTypeExpr` mapper over the one-level
//! [`SurfaceView`]: member sources derive by appending the member name to an
//! HONEST parent source (the root declaration's authored body slot, or an
//! already-indexed access fact), complete leaves upgrade to their closed leaf
//! fact (the `published_source_for_node` policy), and a position with no
//! honest source degrades to the typed Unknown leaf — never a fabricated
//! locator minted from a node.

use std::sync::Arc;

use verter_semantic::analysis::type_expand::{
    ExpandedCallSignature, ExpandedIndexSignature, ExpandedObjectShape, ExpandedParameter,
    ExpandedProperty,
};
use verter_type_expr::facts::{
    ClosedTypeFact, IndexedAccessFact, LeafTypeFact, NarrowTypeParam, SemanticTypeSource,
};
use verter_type_expr::locators::{
    AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, TypeBodySlot,
};
use verter_type_expr::TypeExpr;

use super::ComponentMetaQueryEngine;
use crate::project_semantic_dispatch::semantic_source::SourceRaiseContext;
use crate::project_semantic_dispatch::{node_data_for, ProjectSemanticDispatch};
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{
    ProjectionMode, ProjectionReductionContext, SemanticNodeData, SemanticNodeId,
    SurfaceProvenanceContext, SurfaceView,
};

impl ComponentMetaQueryEngine<'_> {
    /// Project a root symbol's whole-surface to its [`ExpandedObjectShape`] in
    /// NODE DOMAIN.
    ///
    /// PRIMARY/FALLBACK order:
    /// - PRIMARY (root-symbol whole-surface): resolve the root NODE through the
    ///   shared dispatch surface projector, then read its one-level
    ///   `Published(Shallow)` `SurfaceView` and build the shape — the same
    ///   one-level surface composition the registry whole-surface candidate
    ///   produces. Budget-guarded so an exhausted projection budget yields no
    ///   primary surface and the Class-A fallback is tried.
    /// - FALLBACK (Class-A): re-export / namespace-qualified globals
    ///   (e.g. `JSX.IntrinsicElements`) the root-symbol path declines resolve
    ///   through the node-domain Class-A projector and its admitted-node →
    ///   object-shape rail.
    ///
    /// Member values stay SHALLOW semantic sources on the returned shape — the
    /// host raises a member source on demand through the dispatch bridge. Each
    /// member's source appends the member name to the resolved root
    /// declaration's authored body slot (an indexed-access fact); a root with
    /// no resolvable declaration yields leaf-or-degraded member sources.
    pub(crate) fn project_intrinsic_root_shape(
        &mut self,
        scope_canonical_id: &str,
        type_name: &str,
    ) -> Option<ExpandedObjectShape> {
        if let Some(shape) =
            self.project_intrinsic_root_shape_primary(scope_canonical_id, type_name)
        {
            return Some(shape);
        }
        // FALLBACK (Class-A): project the bare-named root in node domain, then
        // build the object shape from the ADMITTED route node's one-level
        // SurfaceView.
        let parent = self.intrinsic_root_parent_source(scope_canonical_id, type_name);
        let ctx = self.ctx;
        let named = TypeExpr::named(type_name);
        let node = crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
            ctx,
            Some(self),
            scope_canonical_id,
            &named,
        )?;
        let view = ctx.dispatch().resolve_typeinfo_surface_view(
            node.node(),
            ProjectionReductionContext::macro_object_surface(
                ProjectionMode::Shallow,
                SurfaceProvenanceContext::Structural,
            ),
        )?;
        Some(expanded_shape_from_surface_view(
            ctx,
            &view,
            parent.as_ref(),
        ))
    }

    /// Project an intrinsic TAG's value SOURCE (the `JSX.IntrinsicElements`
    /// member value, e.g. `HTMLAttributes & { … }`) to its
    /// [`ExpandedObjectShape`] in NODE DOMAIN, in the supplied
    /// (`NativeElements`) scope. The source raises to a graph handle through
    /// the ONE shared dispatch bridge, then the node-domain surface synthesiser
    /// composes the one-level surface off the raised node (the same
    /// `MacroObjectSurface` Shallow demand the admitted-node shape rail reads):
    /// an anonymous property-type intersection merges role-awarely (Authored
    /// arms value-INTERSECT — `number & string` — never last-arm-override), the
    /// TS-correct merge for `A & B`. `None` when the source has no live graph
    /// representation or the raised node composes no one-level object surface.
    /// The tag source itself is the HONEST parent for the produced members —
    /// each member source appends its name to it.
    pub(crate) fn project_intrinsic_tag_member_shape(
        &mut self,
        scope_canonical_id: &str,
        tag_source: &SemanticTypeSource,
    ) -> Option<ExpandedObjectShape> {
        let ctx = self.ctx;
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let raised = dispatch.raise_semantic_type_source_to_hot(
            tag_source,
            SourceRaiseContext {
                scope_canonical_id,
                context: ProjectionReductionContext::structural_transit_with_mode(
                    ProjectionMode::Navigate,
                ),
                interior_failures: None,
            },
        )?;
        let view = dispatch.resolve_typeinfo_surface_view(
            raised.node(),
            ProjectionReductionContext::macro_object_surface(
                ProjectionMode::Shallow,
                SurfaceProvenanceContext::Structural,
            ),
        )?;
        Some(expanded_shape_from_surface_view(
            ctx,
            &view,
            Some(tag_source),
        ))
    }

    /// PRIMARY arm of [`Self::project_intrinsic_root_shape`]: the root-symbol
    /// whole-surface path. `None` (deferring to the Class-A fallback) when the
    /// projection budget is exhausted, the root symbol does not resolve, or the
    /// resolved node carries no one-level object surface.
    fn project_intrinsic_root_shape_primary(
        &mut self,
        scope_canonical_id: &str,
        type_name: &str,
    ) -> Option<ExpandedObjectShape> {
        // An exhausted projection budget yields no primary surface (the Class-A
        // fallback is tried instead).
        if self.projection_op_budget_exhausted() {
            return None;
        }
        let (_surface, node) =
            self.dispatch_projected_surface_with_node(scope_canonical_id, type_name)?;
        let parent = self.intrinsic_root_parent_source(scope_canonical_id, type_name);
        let ctx = self.ctx;
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let view = dispatch.resolve_typeinfo_surface_view(
            node,
            ProjectionReductionContext::published(ProjectionMode::Shallow),
        )?;
        Some(expanded_shape_from_surface_view(
            ctx,
            &view,
            parent.as_ref(),
        ))
    }

    /// The HONEST parent source for an intrinsic ROOT surface's members: the
    /// root symbol's RESOLVED declaration as an authored body slot
    /// (`Authored(DeclBody)` — canonical + symbol from the engine's shared
    /// declaration resolver, empty path = the whole body). `None` when the
    /// symbol resolves to no declaration — the members then publish
    /// leaf-or-degraded sources, never a fabricated locator.
    fn intrinsic_root_parent_source(
        &mut self,
        scope_canonical_id: &str,
        type_name: &str,
    ) -> Option<SemanticTypeSource> {
        let declaration = self.resolve_type_declaration(scope_canonical_id, type_name);
        if declaration.canonical_source.is_empty() {
            return None;
        }
        let symbol = if declaration.resolved_name.is_empty() {
            type_name
        } else {
            declaration.resolved_name.as_str()
        };
        Some(SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(
            TypeBodySlot {
                anchor: AuthoredAnchor {
                    canonical_id: Arc::from(declaration.canonical_source.as_str()),
                    symbol: Arc::from(symbol),
                    space: LocatorSymbolSpace::Type,
                },
                path: Arc::from(Vec::new().into_boxed_slice()),
            },
        )))
    }
}

/// Direct `NoTypeExpr` mapper: build an [`ExpandedObjectShape`] DTO from a
/// one-level [`SurfaceView`] plus an honest PARENT source — a pure data
/// projection over the already-resolved surface (reference resolution stays at
/// the consuming dispatch demands; this maps, it never resolves).
///
/// Per-position source policy:
/// - a member whose value node is a complete closed LEAF (primitive / literal)
///   upgrades to its closed leaf fact — the SAME policy as the projector
///   `published_source_for_node` (a leaf is the only node class whose fact is
///   complete by itself);
/// - otherwise the member source appends the member NAME to the parent: an
///   `Authored(DeclBody(slot))` parent produces
///   `Closed(IndexedAccess { object: slot, index_path: [name] })`, and an
///   existing `Closed(IndexedAccess)` parent extends its `index_path`;
/// - a position with NO honest source (no appendable parent, a non-leaf
///   signature parameter/return, an index-signature key/value) degrades to the
///   typed Unknown leaf — display-degraded, never a fabricated locator minted
///   from a node.
///
/// The projected shape preserves every member facet the `SurfaceView` carries —
/// per-member optionality (a member a union arm omits stays optional), call
/// signatures (a single-call-signature surface keeps its call signature; call
/// and construct signatures both publish as call signatures, matching the
/// object-shape extraction convention), declared index signatures, and the
/// synthetic open placeholder for a GENUINELY OPEN surface.
fn expanded_shape_from_surface_view(
    ctx: &dyn ResolverContext,
    surface: &SurfaceView,
    parent: Option<&SemanticTypeSource>,
) -> ExpandedObjectShape {
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let properties = surface
        .members
        .iter()
        .map(|member| ExpandedProperty {
            name: member.name.as_ref().to_string(),
            // Intrinsic surface members are open-position SUCCESSES: the
            // degraded Unknown leaf here is an intentionally-open position
            // (`Present(Closed(Leaf(unknown)))`), never a failure state.
            ty: verter_type_expr::facts::SourcePosition::Present(member_value_source(
                &dispatch,
                member.value,
                member.name.as_ref(),
                parent,
            )),
            optional: member.optional,
            readonly: member.readonly,
            // Carry the surface member's declared accessibility verbatim so a
            // downstream key-filtering derivation (`Pick`/`Omit` over the
            // shape) can re-apply the public-keyspace gate.
            visibility: member.visibility,
            declared_in_macro_type_arg: member.declared_in_macro_type_arg.get(),
        })
        .collect::<Vec<_>>();

    // Call and construct signatures both publish as call signatures after
    // object-shape extraction (the established round-trip convention). A
    // signature node that is not `Function`-shaped contributes nothing.
    let call_signatures = surface
        .call_signatures
        .iter()
        .chain(surface.construct_signatures.iter())
        .filter_map(|signature| expanded_call_signature_from_node(&dispatch, *signature))
        .collect::<Vec<_>>();

    // Intrinsic index-signature positions are open-position SUCCESSES like
    // the members above: the display-only intrinsic catalog PROVES the
    // openness, so the degraded Unknown leaf is a PRESENT open value —
    // never a failure state.
    let mut index_signatures =
        surface
            .index_signatures
            .iter()
            .map(|signature| ExpandedIndexSignature {
                key_type: verter_type_expr::facts::SourcePosition::Present(
                    leaf_or_degraded_source(&dispatch, signature.key_type),
                ),
                value_type: verter_type_expr::facts::SourcePosition::Present(
                    leaf_or_degraded_source(&dispatch, signature.value_type),
                ),
                readonly: signature.readonly,
            })
            .collect::<Vec<_>>();
    // Genuinely-open surface (flag set, no concrete payload) → open placeholder
    // (string keyspace, typed-degraded value).
    if surface.has_index_signature && surface.index_signatures.is_empty() {
        index_signatures.push(ExpandedIndexSignature {
            key_type: verter_type_expr::facts::SourcePosition::Present(SemanticTypeSource::Closed(
                ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
                    verter_type_expr::PrimitiveName::String,
                )),
            )),
            value_type: verter_type_expr::facts::SourcePosition::Present(unknown_leaf_source()),
            readonly: false,
        });
    }

    ExpandedObjectShape {
        properties,
        index_signatures,
        call_signatures,
    }
}

/// The published SOURCE for one surface member value: the complete closed LEAF
/// fact when the node is one, else the member name appended to the honest
/// parent (an indexed-access fact), else the typed Unknown-leaf degradation.
fn member_value_source(
    dispatch: &ProjectSemanticDispatch<'_>,
    value: SemanticNodeId,
    name: &str,
    parent: Option<&SemanticTypeSource>,
) -> SemanticTypeSource {
    if let Some(leaf) = dispatch.node_leaf_fact(value) {
        return SemanticTypeSource::Closed(ClosedTypeFact::Leaf(leaf));
    }
    match parent {
        Some(SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(slot))) => {
            SemanticTypeSource::Closed(ClosedTypeFact::IndexedAccess(IndexedAccessFact {
                object: slot.clone(),
                index_path: Arc::from(vec![name.to_string()].into_boxed_slice()),
            }))
        }
        Some(SemanticTypeSource::Closed(ClosedTypeFact::IndexedAccess(fact))) => {
            let mut index_path: Vec<String> = fact.index_path.iter().cloned().collect();
            index_path.push(name.to_string());
            SemanticTypeSource::Closed(ClosedTypeFact::IndexedAccess(IndexedAccessFact {
                object: fact.object.clone(),
                index_path: Arc::from(index_path.into_boxed_slice()),
            }))
        }
        _ => unknown_leaf_source(),
    }
}

/// Build one [`ExpandedCallSignature`] from a `Function`-shaped signature node
/// — parameter/return positions publish leaf-or-degraded sources (a
/// signature-scoped position has no addressable authored slot), and the
/// signature's type parameters narrow to name+ordinal (bounds fail closed to
/// `None`, matching the [`NarrowTypeParam`] producer contract). `None` for a
/// non-`Function` signature node.
fn expanded_call_signature_from_node(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Option<ExpandedCallSignature> {
    let data = node_data_for(dispatch.ctx, node)?;
    let SemanticNodeData::Function {
        params,
        return_type,
        type_parameters,
        ..
    } = data.as_ref()
    else {
        return None;
    };
    Some(ExpandedCallSignature {
        parameters: params
            .iter()
            .map(|param| ExpandedParameter {
                name: param
                    .name
                    .as_ref()
                    .map(|name| name.as_ref().to_string())
                    .unwrap_or_default(),
                ty: leaf_or_degraded_source(dispatch, param.ty),
                optional: param.optional,
                rest: param.rest,
            })
            .collect(),
        return_type: leaf_or_degraded_source(dispatch, *return_type),
        type_parameters: Arc::from(
            type_parameters
                .iter()
                .enumerate()
                .map(|(ordinal, param)| NarrowTypeParam {
                    name: param.name.as_ref().to_string(),
                    ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
                    constraint: None,
                    default: None,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
    })
}

/// The published SOURCE for a position with no appendable parent: the complete
/// closed LEAF fact when the node is one, else the typed Unknown-leaf
/// degradation.
fn leaf_or_degraded_source(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> SemanticTypeSource {
    dispatch
        .node_leaf_fact(node)
        .map(|leaf| SemanticTypeSource::Closed(ClosedTypeFact::Leaf(leaf)))
        .unwrap_or_else(unknown_leaf_source)
}

/// The honest degraded SOURCE for a position with no typed payload — the
/// Unknown primitive leaf (display-degraded, never a fabricated reference).
fn unknown_leaf_source() -> SemanticTypeSource {
    SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
        verter_type_expr::PrimitiveName::Unknown,
    )))
}
