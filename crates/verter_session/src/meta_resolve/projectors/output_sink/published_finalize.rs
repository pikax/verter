//! The published-field FINALIZE pass — the whole-surface field-type reducer
//! over `ExpandedComponentTypes`, run once after the per-macro projectors and
//! the slot-binding synthesis. A CHILD module of the terminal `output_sink`
//! (inside the capability mint scope), sharing the sink-private
//! `reduce_field_value_node` node-start reducer.

use super::super::published_source::published_source_for_node;
use super::{authored_package_alias_for_carrier, reduce_field_value_node};
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::ProjectionMode;

/// Run the shared field-value reducer over every published surface in
/// `evaluated_types` so consumers see the same finalised sources the
/// per-macro projectors already publish for `props` / `emits`.
///
/// Published macro props / emits reduce under `ProjectionMode::Navigate` in
/// NODE DOMAIN: each field's content-free source raises ONCE through the
/// shared dispatch bridge, the node-domain gates + graph-native reducer run
/// off that observed node, and the published source is upgraded to the
/// complete closed leaf fact ONLY when the reduction resolved one —
/// otherwise the source stays shallow (the consumer re-raises it on demand
/// through the memos the reduction just warmed). Explicit narrowing
/// (`IndexedAccess`, finite `Pick`/`Omit`, closed/open conditionals) still
/// reduces path-precisely because those roots classify as published
/// operators downstream.
///
/// Producing-payload carriers (a field whose source is the macro
/// TYPE-ARGUMENT payload itself — the graph-raised slot-binding rows) are
/// SHORT-CIRCUITED in the `slot_bindings` / `bindings` loops: reducing the
/// whole parent payload for a single binding row would re-expand the parent
/// shell per row. The typed locator-position identity IS the carrier-skip
/// signal.
pub(crate) fn reduce_published_field_types(
    scope_canonical_id: &str,
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) {
    use verter_type_expr::facts::SemanticTypeSource;

    // One dispatch for the source raises and every per-field leaf projection.
    // `reduce_published_field_types` is a genuine publication terminal: it
    // picks the better field shape in NODE DOMAIN (`compare_node_improvement`
    // over the reduced carriers' nodes) and publishes content-free SOURCES —
    // making no decision on any materialised value.
    let dispatch = ProjectSemanticDispatch::new(query_engine.ctx);
    let transit_ctx =
        crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
            ProjectionMode::Navigate,
        );

    // A field whose source is the first-class synthetic binding carrier is a
    // session-raised row: its own value was already node-resolved at
    // publication, and a consumer deepening it routes through the
    // content-free synthetic-binding identity
    // (`ShapeCacheKey::synthetic_binding_whole_with_context`), never a
    // per-row parent-shell reduction here. The typed SOURCE-variant identity
    // is the skip signal.
    let is_synthetic_binding_carrier =
        |source: &SemanticTypeSource| matches!(source, SemanticTypeSource::SyntheticSlotBinding(_));
    // The graph publisher's shallow named-reference carrier is likewise
    // FINAL for a binding row (`message: MessageBase<T>` publishes the
    // re-resolvable `Ref` — shallow-by-default): a Navigate reduction could
    // only keep the carrier, so re-dispatching it per row is pure fan-out
    // (30 cyclic-heritage bindings re-resolving one identical ref blew the
    // audit structured-event envelope).
    let is_final_shallow_ref_carrier = |source: &SemanticTypeSource| {
        matches!(
            source,
            SemanticTypeSource::Closed(verter_type_expr::facts::ClosedTypeFact::Leaf(
                verter_type_expr::facts::LeafTypeFact::Ref(_)
            ))
        )
    };
    // The graph publisher's arg-preserving authored USE-SITE carrier
    // (`Authored(DeclBody)` — the declaring decl's member-value slot for an
    // argument-bearing named-reference binding value) is equally FINAL for a
    // binding row: a Navigate reduction could only keep the instantiation
    // carrier, so re-dispatching it per row is the same pure fan-out the
    // named-reference skip above closes. Binding rows carry `DeclBody`
    // sources ONLY from that publisher (parser-path rows are
    // `Authored(MacroPayload)` and still reduce).
    let is_publisher_use_site_carrier = |source: &SemanticTypeSource| {
        matches!(
            source,
            SemanticTypeSource::Authored(
                verter_type_expr::locators::AuthoredBodyLocator::DeclBody(_)
            )
        )
    };
    // A projected REPLAY-ROUTE source (member-path / callable-params /
    // index-position) is FINAL for every row: it is the content-free
    // consumer-demand address the member sink just minted from the SAME
    // reduction pipeline — re-raising it here would EXECUTE the replay per
    // row at publication time (a `ProjectPath` member walk per member — the
    // Rule-5 audit-footprint fan-out the `block_6i` guards pin closed). The
    // replay executes ONLY on consumer demand.
    let is_final_projected_replay = |source: &SemanticTypeSource| {
        matches!(
            source,
            SemanticTypeSource::Projected(
                verter_type_expr::facts::ProjectedTypeFact::MemberPath { .. }
                    | verter_type_expr::facts::ProjectedTypeFact::CallableParams { .. }
                    | verter_type_expr::facts::ProjectedTypeFact::IndexPosition { .. }
            )
        )
    };

    for field in evaluated_types.props.iter_mut() {
        // FLAT rows keep the leaf-only closed upgrade (their member sink
        // already applied the ref-identity upgrade at publication).
        finalize_published_prop_source(
            query_engine,
            &dispatch,
            transit_ctx,
            scope_canonical_id,
            &mut field.r#type,
            field.shallow_source.as_ref(),
            false,
        );
    }
    // The `define_props` SHAPE lane properties are the NORMALIZED macro
    // rows' sources (`define_props_shape` publishes them directly — the
    // prop-type authority) finalized HERE through the SAME per-position
    // publication finalize the flat rows run: the authored/raw normalized
    // source reduces once, upgrades to its complete closed leaf / shallow
    // ref-identity carrier when the reduction decided one, and applies the
    // shallow-by-default name preservation. The lane is mechanically
    // DERIVED from the normalized surface — never copied from the flat
    // projection (the authority inversion the emit-authority rule closed).
    for define_props in evaluated_types.define_props.iter_mut() {
        for property in define_props.result.value.properties.iter_mut() {
            // A lane row's shallow authored form IS its own source when the
            // normalized producer published the proven authored position.
            let shallow = match property.ty.present() {
                Some(SemanticTypeSource::Authored(locator)) => Some(locator.clone()),
                _ => None,
            };
            finalize_published_prop_source(
                query_engine,
                &dispatch,
                transit_ctx,
                scope_canonical_id,
                &mut property.ty,
                shallow.as_ref(),
                true,
            );
        }
    }
    // The `define_props` SHAPE lane is NOT touched here: its property sources
    // are the normalized macro rows' `type_source` positions
    // (`define_props_shape` publishes them directly — the prop-type
    // authority). Copying the finalized FLAT sources over the lane would
    // re-impose the flat projection as a competing semantic producer — the
    // authority inversion the emit-authority rule closed.
    for field in evaluated_types.emits.iter_mut() {
        let Some(current) = field.r#type.present().cloned() else {
            continue;
        };
        // A projected replay-route source is already final — never re-raised
        // at publication.
        if is_final_projected_replay(&current) {
            continue;
        }
        let Some(input) = dispatch.raise_semantic_type_source_to_hot(
            &current,
            crate::project_semantic_dispatch::semantic_source::SourceRaiseContext {
                scope_canonical_id,
                context: transit_ctx,
                interior_failures: None,
            },
        ) else {
            continue;
        };
        let carrier = reduce_field_value_node(
            query_engine,
            scope_canonical_id,
            input.node(),
            ProjectionMode::Navigate,
        );
        field.r#type = verter_type_expr::facts::SourcePosition::Present(published_source_for_node(
            &dispatch,
            carrier.node_id(),
            current,
        ));
    }
    for field in evaluated_types.slot_bindings.iter_mut() {
        // A synthetic binding carrier is a session-raised binding row — its
        // own value was already node-resolved at publication; the published
        // source stays the shallow carrier and deepening is demand-side.
        // The published shallow named-reference carrier and the
        // arg-preserving authored use-site carrier are equally final.
        let Some(current) = field.r#type.present().cloned() else {
            continue;
        };
        if is_synthetic_binding_carrier(&current)
            || is_final_shallow_ref_carrier(&current)
            || is_publisher_use_site_carrier(&current)
            || is_final_projected_replay(&current)
        {
            continue;
        }
        let Some(input) = dispatch.raise_semantic_type_source_to_hot(
            &current,
            crate::project_semantic_dispatch::semantic_source::SourceRaiseContext {
                scope_canonical_id,
                context: transit_ctx,
                interior_failures: None,
            },
        ) else {
            continue;
        };
        // Navigate (shallow-by-default): an explicit selector reduces
        // path-precisely, but a symbolic `AppProps['avatar']` through an open
        // `[k: string]: any` index signature STAYS the indexed-access carrier.
        let carrier = reduce_field_value_node(
            query_engine,
            scope_canonical_id,
            input.node(),
            ProjectionMode::Navigate,
        );
        field.r#type = verter_type_expr::facts::SourcePosition::Present(published_source_for_node(
            &dispatch,
            carrier.node_id(),
            current,
        ));
    }
    for field in evaluated_types.bindings.iter_mut() {
        let Some(current) = field.r#type.present().cloned() else {
            continue;
        };
        if is_synthetic_binding_carrier(&current)
            || is_final_shallow_ref_carrier(&current)
            || is_final_projected_replay(&current)
        {
            continue;
        }
        let Some(input) = dispatch.raise_semantic_type_source_to_hot(
            &current,
            crate::project_semantic_dispatch::semantic_source::SourceRaiseContext {
                scope_canonical_id,
                context: transit_ctx,
                interior_failures: None,
            },
        ) else {
            continue;
        };
        // Navigate (shallow-by-default), matching the props / emits /
        // slot_bindings reducers: an explicit selector reduces
        // path-precisely; a plain alias / open generic carrier stays shallow.
        let carrier = reduce_field_value_node(
            query_engine,
            scope_canonical_id,
            input.node(),
            ProjectionMode::Navigate,
        );
        field.r#type = verter_type_expr::facts::SourcePosition::Present(published_source_for_node(
            &dispatch,
            carrier.node_id(),
            current,
        ));
    }
}

/// Finalize ONE published prop SOURCE POSITION — the per-position half of
/// [`reduce_published_field_types`], shared by the FLAT `evaluated_types.props`
/// rows and the `define_props` SHAPE lane properties (the normalized-surface
/// authority): raise the present source ONCE through the shared bridge, run
/// the node-domain gates + graph-native reducer, prefer / name-preserve the
/// shallow AUTHORED form, and upgrade the published source to the complete
/// closed leaf fact (plus — for the lane, `apply_ref_identity_upgrade` — the
/// shallow declaration-identity carrier) ONLY when the reduction decided one.
/// Absent / Failed positions and projected replay-route sources pass through
/// untouched (a replay address is a final consumer-demand address — see the
/// skip rationale in [`reduce_published_field_types`]).
#[allow(clippy::too_many_arguments)]
fn finalize_published_prop_source(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    dispatch: &ProjectSemanticDispatch<'_>,
    transit_ctx: crate::semantic_query::ProjectionReductionContext,
    scope_canonical_id: &str,
    position: &mut verter_type_expr::facts::SourcePosition,
    shallow_source: Option<&verter_type_expr::locators::AuthoredBodyLocator>,
    apply_ref_identity_upgrade: bool,
) {
    use verter_type_expr::facts::SemanticTypeSource;

    let is_final_projected_replay = |source: &SemanticTypeSource| {
        matches!(
            source,
            SemanticTypeSource::Projected(
                verter_type_expr::facts::ProjectedTypeFact::MemberPath { .. }
                    | verter_type_expr::facts::ProjectedTypeFact::CallableParams { .. }
                    | verter_type_expr::facts::ProjectedTypeFact::IndexPosition { .. }
            )
        )
    };
    // Absent / Failed source POSITIONS pass through untouched: the reducer
    // refines present sources only and must never resurrect a proven absence
    // or a typed failure into a fabricated value.
    let Some(current) = position.present().cloned() else {
        return;
    };
    // A projected replay-route source is already final — never re-raised at
    // publication.
    if is_final_projected_replay(&current) {
        return;
    }
    // Raise the resolved source ONCE — the observed INPUT node for the
    // reducer AND its no-poison input fact (one lowering, no re-lower).
    let Some(input) = dispatch.raise_semantic_type_source_to_hot(
        &current,
        crate::project_semantic_dispatch::semantic_source::SourceRaiseContext {
            scope_canonical_id,
            context: transit_ctx,
            interior_failures: None,
        },
    ) else {
        // Unraisable under the live view — the source publishes shallow
        // verbatim (never a fabricated stand-in).
        return;
    };
    let mut chosen = reduce_field_value_node(
        query_engine,
        scope_canonical_id,
        input.node(),
        ProjectionMode::Navigate,
    );

    // The shallow AUTHORED form is raised EXACTLY ONCE per position (the
    // audit-footprint bound pinned by
    // `reduce_published_raises_the_shallow_form_only_in_the_props_loop`)
    // and shared by the node-compare below and the name-preservation gate
    // after it.
    let shallow_node = shallow_source
        .and_then(|shallow| dispatch.raise_authored_locator_to_hot(shallow, transit_ctx));
    if let Some(shallow) = shallow_source {
        let ctx = query_engine.ctx;
        // The shallow form is an INPUT (untainted); compare in NODE DOMAIN
        // against the reduced carrier's node — never by scoring a
        // materialised `TypeExpr`.
        let prefer_shallow = match (shallow_node.as_ref(), chosen.node_id()) {
            (Some(sn), Some(rn)) => {
                crate::meta_resolve::compare_node_improvement(ctx, sn.node(), rn)
                    || crate::meta_resolve::node_root_is_explicit_selector_operator(ctx, sn.node())
            }
            // No reduced node to compare against: prefer the shallow form
            // only when it is an explicit consumer-demand selector.
            (Some(sn), None) => {
                crate::meta_resolve::node_root_is_explicit_selector_operator(ctx, sn.node())
            }
            _ => false,
        };
        if prefer_shallow {
            if let Some(sn) = shallow_node.as_ref() {
                let shallow_reduced = reduce_field_value_node(
                    query_engine,
                    scope_canonical_id,
                    sn.node(),
                    ProjectionMode::Navigate,
                );
                if let (Some(srn), Some(rn)) = (shallow_reduced.node_id(), chosen.node_id()) {
                    if crate::meta_resolve::compare_node_improvement(query_engine.ctx, srn, rn) {
                        chosen = shallow_reduced;
                        // Adopt the shallow AUTHORED position as the
                        // published source.
                        *position = verter_type_expr::facts::SourcePosition::Present(
                            SemanticTypeSource::Authored(shallow.clone()),
                        );
                    }
                }
            }
        }
    }

    // Shallow-by-default name preservation: when the prop's AUTHORED
    // annotation is a bare declaration reference and the demanded reduction
    // did NOT decide a closed non-reference leaf (a primitive / literal or
    // an all-leaf union), the published source is the authored annotation
    // slot — the published shallow shape keeps the name AS WRITTEN (an
    // imported alias publishes `AvatarProps`, a renamed package re-export
    // publishes `RouteLocationRaw`, never the terminal declaration's
    // internal name or its eagerly-dealiased body) and consumers re-resolve
    // it through the registry on demand.
    let reduced_decided_closed_leaf = chosen.node_id().is_some_and(|node| {
        dispatch
            .node_leaf_fact(node)
            .is_some_and(|leaf| !matches!(leaf, verter_type_expr::facts::LeafTypeFact::Ref(_)))
            || dispatch.node_leaf_union_fact(node).is_some()
    });
    if !reduced_decided_closed_leaf {
        if let (Some(shallow), Some(sn)) = (shallow_source, shallow_node.as_ref()) {
            let shallow_ref_name =
                crate::resolver_core::component_meta_registry::component_meta_registry_node_ref_name(
                    query_engine.ctx,
                    sn.node(),
                );
            if let Some(shallow_ref_name) = shallow_ref_name {
                // A PACKAGE-backed carrier publishes the owner's authored
                // import-binding alias (a renamed package re-export keeps
                // the name AS WRITTEN — `RouteLocationRaw`, never the
                // package's internal terminal name) — recovered through the
                // owner's import bindings, never from raw text. An
                // OWNER-anchored annotation publishes the closed shallow
                // name fact (the bare `Ref` the consumer re-resolves in the
                // owner scope); a foreign-anchored annotation keeps the
                // authored locator (its name resolves in ITS scope).
                let anchor_is_owner = match shallow {
                    verter_type_expr::locators::AuthoredBodyLocator::DeclBody(slot) => {
                        slot.anchor.canonical_id.is_empty()
                            || slot.anchor.canonical_id.as_ref() == scope_canonical_id
                    }
                    verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(payload) => {
                        payload.anchor.canonical_id.is_empty()
                            || payload.anchor.canonical_id.as_ref() == scope_canonical_id
                    }
                    _ => false,
                };
                let published = authored_package_alias_for_carrier(
                    query_engine.ctx,
                    scope_canonical_id,
                    sn.node(),
                )
                .or_else(|| anchor_is_owner.then_some(shallow_ref_name))
                .map(|local_name| {
                    SemanticTypeSource::Closed(verter_type_expr::facts::ClosedTypeFact::Leaf(
                        verter_type_expr::facts::LeafTypeFact::Ref(local_name),
                    ))
                })
                .unwrap_or_else(|| SemanticTypeSource::Authored(shallow.clone()));
                *position = verter_type_expr::facts::SourcePosition::Present(published);
                return;
            }
        }
    }
    // The closed-leaf / leaf-union upgrade applies on both surfaces; the
    // LANE additionally publishes the shallow declaration-identity carrier
    // (`Synthesized(Ref(..))`, lossless — no type arguments) when the
    // reduction navigated to a terminal declaration reference — the SAME
    // member-sink upgrade the flat rows received at their publication sink.
    let published = if apply_ref_identity_upgrade {
        super::super::published_source::published_member_source_upgrade_for_node(
            dispatch,
            chosen.node_id(),
            false,
        )
        .unwrap_or_else(|| position.present().cloned().unwrap_or(current))
    } else {
        published_source_for_node(
            dispatch,
            chosen.node_id(),
            position.present().cloned().unwrap_or(current),
        )
    };
    *position = verter_type_expr::facts::SourcePosition::Present(published);
}
