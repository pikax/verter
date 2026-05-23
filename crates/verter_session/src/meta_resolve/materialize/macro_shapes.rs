//! Materialization core: macro-shape producers.
//!
//! domain 7 (macro-shapes portion). Owns:
//! - `produce_macro_object_shapes` + `produce_macro_object_shapes_for_purpose`
//!   (the sole authority that emits `define_props` / `define_emits` /
//!   `define_slots` object shapes for `ExpandedComponentTypes`),
//! - `MacroShapeSource` (the source-tag enum used by the projection/solver
//!   reconciliation),
//! - the macro-shape synthesis / projection / penalty helpers
//!   (`synthesize_define_props_shape_*`, `produce_one_macro_object_shape*`,
//!   `type_expr_symbolic_penalty`, `shape_symbolic_penalty`, etc.).
//!
//! Lines 1819-4102 of the pre-split `meta_resolve.rs`. The body is verbatim
//! apart from `pub(crate)` visibility escalation on the formerly-private
//! items the parent shell still calls.

use crate::host_manage::component_meta_trace_custom;
use crate::instant::Instant;
use crate::types::FileAnalysisSnapshot;

use verter_semantic::analysis::types::AnalyzedMacro;

use super::super::dispatch_helpers::{
    macro_payload_root_is_conditional_carrier,
    project_expr_class_a_shape_via_dispatch_transit_shallow,
    project_expr_class_a_via_dispatch_threaded, project_expr_class_a_via_dispatch_transit_shallow,
    project_expr_class_a_via_dispatch_transit_shallow_threaded,
    project_expr_surface_expr_with_compound_objects_transit_shallow_via_host_threaded,
    project_expr_surface_shape_via_host_threaded,
    project_prepared_type_surface_shape_via_host_threaded,
    project_type_surface_expr_via_host_threaded, project_type_surface_shape_via_host_threaded,
    project_type_surface_shape_transit_shallow_via_host_threaded,
};
// `request_host` source moved to
// `host_manage/component_meta_request_impl.rs`. Import rewritten to
// the new home.
use super::super::resolved_state::lowered_root_reaches_transitive_cycle;
use crate::host_manage::component_meta_request_impl::{
    ResolvedMacroMeta, ResolvedTypeRegistryMeta,
};

/// Sole-authority producer for type-based macro object shapes.
///
/// This is the ONE place that produces `define_props`, `define_emits`, and
/// `define_slots` object shapes for `ExpandedComponentTypes`.
///
/// The production pipeline is projection-first so one phase owns object-shape
/// materialization. The solver is used only as the terminal fallback when
/// projection cannot produce a usable shape.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn produce_macro_object_shapes(
    owner_canonical: &str,
    snapshot: &FileAnalysisSnapshot,
    resolved_macros: &[ResolvedMacroMeta],
    resolved_type_registry: &[verter_semantic::analysis::component_meta::ResolvedTypeAnalysis],
    resolved_type_registry_meta: &[ResolvedTypeRegistryMeta],
    eval_source: &str,
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) {
    produce_macro_object_shapes_for_purpose(
        owner_canonical,
        snapshot,
        resolved_macros,
        resolved_type_registry,
        resolved_type_registry_meta,
        eval_source,
        evaluated_types,
        query_engine,
        crate::resolver_core::ComponentMetaResolutionPurpose::Full,
    );
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn produce_macro_object_shapes_for_purpose(
    owner_canonical: &str,
    snapshot: &FileAnalysisSnapshot,
    resolved_macros: &[ResolvedMacroMeta],
    resolved_type_registry: &[verter_semantic::analysis::component_meta::ResolvedTypeAnalysis],
    resolved_type_registry_meta: &[ResolvedTypeRegistryMeta],
    eval_source: &str,
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    purpose: crate::resolver_core::ComponentMetaResolutionPurpose,
) {
    let _loop8_timer = crate::loop5_instrumentation::TimerGuard::new(
        &crate::loop5_instrumentation::PRODUCE_MACRO_OBJECT_SHAPES_CALLS,
        &crate::loop5_instrumentation::PRODUCE_MACRO_OBJECT_SHAPES_NS,
    );
    let params =
        verter_semantic::analysis::type_eval_build::collect_define_macro_type_params(eval_source);
    let mut define_props_index = 0usize;
    let mut define_emits_index = 0usize;
    let mut define_slots_index = 0usize;
    let mut registry_hits = 0u32;
    let mut projection_hits = 0u32;
    let mut solver_fallbacks = 0u32;
    let shapes_started = Instant::now();
    let solves_before = 0u32;

    for (macro_index, mac) in snapshot.macros.iter().enumerate() {
        if !mac.is_type_based {
            continue;
        }

        if purpose == crate::resolver_core::ComponentMetaResolutionPurpose::Fallthrough {
            match mac.kind {
                verter_semantic::analysis::AnalyzedMacroKind::DefineProps => {
                    define_props_index += 1;
                    continue;
                }
                verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => {
                    define_slots_index += 1;
                    continue;
                }
                _ => {}
            }
        }

        match mac.kind {
            verter_semantic::analysis::AnalyzedMacroKind::DefineProps => {
                let define_props_lowered = params.define_props.get(define_props_index);
                let define_props_has_matching_resolved_root =
                    resolved_macros.iter().any(|resolved| {
                        resolved.macro_index == macro_index
                            && resolved.macro_kind
                                == verter_semantic::analysis::AnalyzedMacroKind::DefineProps
                            && mac
                                .type_references
                                .iter()
                                .any(|type_name| type_name == &resolved.type_name)
                    });
                let define_props_prefers_prepared_projection =
                    define_props_lowered.is_some_and(|lowered| {
                        named_ref_matches_empty_shell_registry_root(
                            owner_canonical,
                            lowered,
                            resolved_type_registry,
                            resolved_type_registry_meta,
                        )
                    });
                if define_props_prefers_prepared_projection {
                    if let Some(lowered) = define_props_lowered {
                        let item_started = Instant::now();
                        let props_projection = crate::meta_resolve::projection_demand::SurfaceProjection::whole_surface(
                            crate::meta_resolve::projection_demand::PublishedSurfaceKind::Props,
                        );
                        let (shape, source) = produce_one_macro_object_shape(
                            query_engine,
                            owner_canonical,
                            lowered,
                            has_prop_shape_surface,
                            props_projection.cursor(),
                        );
                        if source.is_projection() {
                            projection_hits += 1;
                        } else if source.is_solver() {
                            solver_fallbacks += 1;
                        }
                        if let Some(shape) = shape {
                            let count = shape.value.properties.len();
                            component_meta_trace_custom!(
                                "macro_object_shape",
                                format!(
                                    "owner={} macro_index={} kind=define_props source={} props={} took={:?}",
                                    owner_canonical, macro_index, source.label(), count,
                                    item_started.elapsed(),
                                ),
                            );
                            evaluated_types.define_props.push(
                                verter_semantic::analysis::type_expand::ExpandedMacroProps {
                                    macro_index,
                                    result: shape,
                                },
                            );
                        }
                    }
                } else if let Some(lowered) = define_props_lowered.filter(|lowered| {
                    matches!(lowered, verter_type_expr::TypeExpr::Ref { .. })
                        && !define_props_has_matching_resolved_root
                }) {
                    let item_started = Instant::now();
                    let props_projection =
                        crate::meta_resolve::projection_demand::SurfaceProjection::whole_surface(
                            crate::meta_resolve::projection_demand::PublishedSurfaceKind::Props,
                        );
                    let (shape, source) = produce_one_macro_object_shape(
                        query_engine,
                        owner_canonical,
                        lowered,
                        has_prop_shape_surface,
                        props_projection.cursor(),
                    );
                    if source.is_projection() {
                        projection_hits += 1;
                    } else if source.is_solver() {
                        solver_fallbacks += 1;
                    }
                    if let Some(shape) = shape {
                        let count = shape.value.properties.len();
                        component_meta_trace_custom!(
                            "macro_object_shape",
                            format!(
                                "owner={} macro_index={} kind=define_props source={} props={} took={:?}",
                                owner_canonical,
                                macro_index,
                                source.label(),
                                count,
                                item_started.elapsed(),
                            ),
                        );
                        evaluated_types.define_props.push(
                            verter_semantic::analysis::type_expand::ExpandedMacroProps {
                                macro_index,
                                result: shape,
                            },
                        );
                    }
                } else if let Some((shape, source)) =
                    synthesize_define_props_shape_from_known_surface_with_authority(
                        macro_index,
                        snapshot,
                        resolved_macros,
                        evaluated_types,
                        define_props_lowered,
                        true,
                    )
                {
                    projection_hits += 1;
                    let count = shape.value.properties.len();
                    component_meta_trace_custom!(
                        "macro_object_shape",
                        format!(
                            "owner={} macro_index={} kind=define_props source={} props={}",
                            owner_canonical,
                            macro_index,
                            source.label(),
                            count,
                        ),
                    );
                    evaluated_types.define_props.push(
                        verter_semantic::analysis::type_expand::ExpandedMacroProps {
                            macro_index,
                            result: shape,
                        },
                    );
                } else if !define_props_has_direct_local_root(mac)
                    && define_props_fields_fast_path_allowed(
                        mac,
                        macro_index,
                        resolved_macros,
                        params.define_props.get(define_props_index),
                    )
                {
                    if let Some((shape, source)) =
                        synthesize_define_props_shape_from_known_surface_with_authority(
                            macro_index,
                            snapshot,
                            resolved_macros,
                            evaluated_types,
                            define_props_lowered,
                            false,
                        )
                    {
                        projection_hits += 1;
                        let count = shape.value.properties.len();
                        component_meta_trace_custom!(
                            "macro_object_shape",
                            format!(
                                "owner={} macro_index={} kind=define_props source={} props={}",
                                owner_canonical,
                                macro_index,
                                source.label(),
                                count,
                            ),
                        );
                        evaluated_types.define_props.push(
                            verter_semantic::analysis::type_expand::ExpandedMacroProps {
                                macro_index,
                                result: shape,
                            },
                        );
                    } else if let Some(lowered) = params.define_props.get(define_props_index) {
                        let item_started = Instant::now();
                        let props_projection = crate::meta_resolve::projection_demand::SurfaceProjection::whole_surface(
                            crate::meta_resolve::projection_demand::PublishedSurfaceKind::Props,
                        );
                        let (shape, source) = produce_one_macro_object_shape(
                            query_engine,
                            owner_canonical,
                            lowered,
                            has_prop_shape_surface,
                            props_projection.cursor(),
                        );
                        if source.is_projection() {
                            projection_hits += 1;
                        } else if source.is_solver() {
                            solver_fallbacks += 1;
                        }
                        if let Some(shape) = shape {
                            let count = shape.value.properties.len();
                            component_meta_trace_custom!(
                                "macro_object_shape",
                                format!(
                                    "owner={} macro_index={} kind=define_props source={} props={} took={:?}",
                                    owner_canonical, macro_index, source.label(), count,
                                    item_started.elapsed(),
                                ),
                            );
                            evaluated_types.define_props.push(
                                verter_semantic::analysis::type_expand::ExpandedMacroProps {
                                    macro_index,
                                    result: shape,
                                },
                            );
                        }
                    }
                } else {
                    // Lazy compute: the rescue probe is consulted only on
                    // the registry / solver fallback branches. Branches that
                    // already succeeded above never enter this arm, so the
                    // probe is skipped on the cheap-success paths.
                    let define_props_needs_projection_rescue =
                        define_props_lowered.is_some_and(|lowered| {
                            crate::capture_token::with_active_capture(|t| {
                                t.record_counter("expr_needs_projection_rescue_calls", 1)
                            });
                            expr_needs_projection_rescue(query_engine, owner_canonical, lowered)
                        });
                    if !define_props_needs_projection_rescue {
                        if let Some((shape, source)) =
                            synthesize_define_props_shape_from_registry_root(
                                owner_canonical,
                                macro_index,
                                snapshot,
                                resolved_type_registry,
                                resolved_type_registry_meta,
                            )
                        {
                            registry_hits += 1;
                            let count = shape.value.properties.len();
                            component_meta_trace_custom!(
                                "macro_object_shape",
                                format!(
                                    "owner={} macro_index={} kind=define_props source={} props={}",
                                    owner_canonical,
                                    macro_index,
                                    source.label(),
                                    count,
                                ),
                            );
                            evaluated_types.define_props.push(
                                verter_semantic::analysis::type_expand::ExpandedMacroProps {
                                    macro_index,
                                    result: shape,
                                },
                            );
                        } else if let Some(lowered) = define_props_lowered {
                            let item_started = Instant::now();
                            let props_projection = crate::meta_resolve::projection_demand::SurfaceProjection::whole_surface(
                                crate::meta_resolve::projection_demand::PublishedSurfaceKind::Props,
                            );
                            let (shape, source) = produce_one_macro_object_shape(
                                query_engine,
                                owner_canonical,
                                lowered,
                                has_prop_shape_surface,
                                props_projection.cursor(),
                            );
                            if source.is_projection() {
                                projection_hits += 1;
                            } else if source.is_solver() {
                                solver_fallbacks += 1;
                            }
                            if let Some(shape) = shape {
                                let count = shape.value.properties.len();
                                component_meta_trace_custom!(
                                    "macro_object_shape",
                                    format!(
                                        "owner={} macro_index={} kind=define_props source={} props={} took={:?}",
                                        owner_canonical, macro_index, source.label(), count,
                                        item_started.elapsed(),
                                    ),
                                );
                                evaluated_types.define_props.push(
                                    verter_semantic::analysis::type_expand::ExpandedMacroProps {
                                        macro_index,
                                        result: shape,
                                    },
                                );
                            }
                        }
                    } else if let Some(lowered) = define_props_lowered {
                        let item_started = Instant::now();
                        let props_projection = crate::meta_resolve::projection_demand::SurfaceProjection::whole_surface(
                            crate::meta_resolve::projection_demand::PublishedSurfaceKind::Props,
                        );
                        let (shape, source) = produce_one_macro_object_shape(
                            query_engine,
                            owner_canonical,
                            lowered,
                            has_prop_shape_surface,
                            props_projection.cursor(),
                        );
                        if source.is_projection() {
                            projection_hits += 1;
                        } else if source.is_solver() {
                            solver_fallbacks += 1;
                        }
                        if let Some(shape) = shape {
                            let count = shape.value.properties.len();
                            component_meta_trace_custom!(
                                "macro_object_shape",
                                format!(
                                    "owner={} macro_index={} kind=define_props source={} props={} took={:?}",
                                    owner_canonical, macro_index, source.label(), count,
                                    item_started.elapsed(),
                                ),
                            );
                            evaluated_types.define_props.push(
                                verter_semantic::analysis::type_expand::ExpandedMacroProps {
                                    macro_index,
                                    result: shape,
                                },
                            );
                        }
                    }
                }
                define_props_index += 1;
            }
            verter_semantic::analysis::AnalyzedMacroKind::DefineEmits => {
                if evaluated_types.define_emits.iter().any(|entry| {
                    entry.macro_index == macro_index
                        && verter_semantic::analysis::type_eval_build::has_named_shape_surface(
                            &entry.result.value,
                        )
                }) {
                    projection_hits += 1;
                } else if let Some((shape, source)) =
                    synthesize_define_emits_shape_from_known_surface(
                        macro_index,
                        snapshot,
                        resolved_macros,
                        evaluated_types,
                    )
                {
                    projection_hits += 1;
                    let count = shape.value.properties.len() + shape.value.call_signatures.len();
                    component_meta_trace_custom!(
                        "macro_object_shape",
                        format!(
                            "owner={} macro_index={} kind=define_emits source={} surface={}",
                            owner_canonical,
                            macro_index,
                            source.label(),
                            count,
                        ),
                    );
                    evaluated_types.define_emits.push(
                        verter_semantic::analysis::type_expand::ExpandedMacroObjectShape {
                            macro_index,
                            result: shape,
                        },
                    );
                } else if let Some(lowered) = params.define_emits.get(define_emits_index) {
                    if let Some((shape, source)) = synthesize_macro_shape_from_registry_lowered_root(
                        lowered,
                        resolved_type_registry,
                        resolved_type_registry_meta,
                        verter_semantic::analysis::type_eval_build::has_named_shape_surface,
                    ) {
                        registry_hits += 1;
                        let count =
                            shape.value.properties.len() + shape.value.call_signatures.len();
                        component_meta_trace_custom!(
                            "macro_object_shape",
                            format!(
                                "owner={} macro_index={} kind=define_emits source={} surface={}",
                                owner_canonical,
                                macro_index,
                                source.label(),
                                count,
                            ),
                        );
                        evaluated_types.define_emits.push(
                            verter_semantic::analysis::type_expand::ExpandedMacroObjectShape {
                                macro_index,
                                result: shape,
                            },
                        );
                    } else {
                        let item_started = Instant::now();
                        let emits_projection = crate::meta_resolve::projection_demand::SurfaceProjection::whole_surface(
                            crate::meta_resolve::projection_demand::PublishedSurfaceKind::Emits,
                        );
                        let (shape, source) = produce_one_macro_object_shape(
                            query_engine,
                            owner_canonical,
                            lowered,
                            verter_semantic::analysis::type_eval_build::has_named_shape_surface,
                            emits_projection.cursor(),
                        );
                        if source.is_projection() {
                            projection_hits += 1;
                        } else if source.is_solver() {
                            solver_fallbacks += 1;
                        }
                        if let Some(shape) = shape {
                            let count =
                                shape.value.properties.len() + shape.value.call_signatures.len();
                            component_meta_trace_custom!(
                                "macro_object_shape",
                                format!(
                                    "owner={} macro_index={} kind=define_emits source={} surface={} took={:?}",
                                    owner_canonical, macro_index, source.label(), count,
                                    item_started.elapsed(),
                                ),
                            );
                            evaluated_types.define_emits.push(
                                verter_semantic::analysis::type_expand::ExpandedMacroObjectShape {
                                    macro_index,
                                    result: shape,
                                },
                            );
                        }
                    }
                }
                define_emits_index += 1;
            }
            verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => {
                let define_slots_lowered = params.define_slots.get(define_slots_index);
                let define_slots_owner_surface_incomplete = mac.slot_fields.is_empty()
                    || mac.slot_fields.iter().any(|slot| slot.bindings.is_empty());
                let define_slots_needs_projection_rescue =
                    define_slots_lowered.is_some_and(|lowered| {
                        crate::capture_token::with_active_capture(|t| {
                            t.record_counter("expr_needs_projection_rescue_calls", 1)
                        });
                        expr_needs_projection_rescue(query_engine, owner_canonical, lowered)
                    });
                if !define_slots_needs_projection_rescue {
                    if let Some((shape, source)) = synthesize_define_slots_shape_from_known_surface(
                        macro_index,
                        resolved_macros,
                    ) {
                        projection_hits += 1;
                        let count = shape.value.properties.len();
                        component_meta_trace_custom!(
                            "macro_object_shape",
                            format!(
                                "owner={} macro_index={} kind=define_slots source={} slots={}",
                                owner_canonical,
                                macro_index,
                                source.label(),
                                count,
                            ),
                        );
                        evaluated_types.define_slots.push(
                            verter_semantic::analysis::type_expand::ExpandedMacroObjectShape {
                                macro_index,
                                result: shape,
                            },
                        );
                    } else if let Some((shape, source)) = define_slots_lowered.and_then(|lowered| {
                        synthesize_macro_shape_from_registry_lowered_root(
                            lowered,
                            resolved_type_registry,
                            resolved_type_registry_meta,
                            has_shape_surface,
                        )
                    }) {
                        registry_hits += 1;
                        let count = shape.value.properties.len();
                        component_meta_trace_custom!(
                            "macro_object_shape",
                            format!(
                                "owner={} macro_index={} kind=define_slots source={} slots={}",
                                owner_canonical,
                                macro_index,
                                source.label(),
                                count,
                            ),
                        );
                        evaluated_types.define_slots.push(
                            verter_semantic::analysis::type_expand::ExpandedMacroObjectShape {
                                macro_index,
                                result: shape,
                            },
                        );
                    } else if let Some(lowered) = define_slots_lowered {
                        // Local slot_fields can preserve only the slot names while
                        // dropping the callable payload behind a helper alias.
                        // In that case the lowered-type path still owns the real
                        // defineSlots object shape.
                        if define_slots_owner_surface_incomplete {
                            let item_started = Instant::now();
                            let slots_projection = crate::meta_resolve::projection_demand::SurfaceProjection::whole_surface(
                                crate::meta_resolve::projection_demand::PublishedSurfaceKind::Slots,
                            );
                            let (shape, source) = produce_one_macro_object_shape_for_slots(
                                query_engine,
                                owner_canonical,
                                lowered,
                                slots_projection.cursor(),
                            );
                            if source.is_projection() {
                                projection_hits += 1;
                            } else if source.is_solver() {
                                solver_fallbacks += 1;
                            }
                            if let Some(shape) = shape {
                                if !shape.value.properties.is_empty() {
                                    let count = shape.value.properties.len();
                                    component_meta_trace_custom!(
                                        "macro_object_shape",
                                        format!(
                                            "owner={} macro_index={} kind=define_slots source={} slots={} took={:?}",
                                            owner_canonical, macro_index, source.label(), count,
                                            item_started.elapsed(),
                                        ),
                                    );
                                    evaluated_types.define_slots.push(
                                        verter_semantic::analysis::type_expand::ExpandedMacroObjectShape {
                                            macro_index,
                                            result: shape,
                                        },
                                    );
                                }
                            }
                        }
                    }
                } else if let Some(lowered) = define_slots_lowered {
                    let item_started = Instant::now();
                    let slots_projection =
                        crate::meta_resolve::projection_demand::SurfaceProjection::whole_surface(
                            crate::meta_resolve::projection_demand::PublishedSurfaceKind::Slots,
                        );
                    let (shape, source) = produce_one_macro_object_shape_for_slots(
                        query_engine,
                        owner_canonical,
                        lowered,
                        slots_projection.cursor(),
                    );
                    if source.is_projection() {
                        projection_hits += 1;
                    } else if source.is_solver() {
                        solver_fallbacks += 1;
                    }
                    if let Some(shape) = shape {
                        if !shape.value.properties.is_empty() {
                            let count = shape.value.properties.len();
                            component_meta_trace_custom!(
                                "macro_object_shape",
                                format!(
                                    "owner={} macro_index={} kind=define_slots source={} slots={} took={:?}",
                                    owner_canonical, macro_index, source.label(), count,
                                    item_started.elapsed(),
                                ),
                            );
                            evaluated_types.define_slots.push(
                                verter_semantic::analysis::type_expand::ExpandedMacroObjectShape {
                                    macro_index,
                                    result: shape,
                                },
                            );
                        }
                    }
                }
                define_slots_index += 1;
            }
            _ => {}
        }
    }

    let solves_after = 0u32;
    component_meta_trace_custom!(
        "produce_macro_object_shapes",
        format!(
            "owner={} define_props={} define_emits={} define_slots={} registry_hits={} projection_hits={} solver_fallbacks={} solves_delta={} took={:?}",
            owner_canonical,
            evaluated_types.define_props.len(),
            evaluated_types.define_emits.len(),
            evaluated_types.define_slots.len(),
            registry_hits,
            projection_hits,
            solver_fallbacks,
            solves_after.saturating_sub(solves_before),
            shapes_started.elapsed(),
        ),
    );
}

/// Which path produced the macro object shape.
#[derive(Clone, Copy)]
pub(crate) enum MacroShapeSource {
    Fields,
    ResolvedMacro,
    Registry,
    Projection,
    Solver,
    None,
}

impl MacroShapeSource {
    fn is_projection(self) -> bool {
        matches!(self, Self::Projection)
    }
    fn is_solver(self) -> bool {
        matches!(self, Self::Solver)
    }
    fn label(self) -> &'static str {
        match self {
            Self::Fields => "fields",
            Self::ResolvedMacro => "resolved-macro",
            Self::Registry => "registry",
            Self::Projection => "projection",
            Self::Solver => "solver",
            Self::None => "none",
        }
    }
}

pub(crate) fn define_props_fields_fast_path_allowed(
    mac: &AnalyzedMacro,
    macro_index: usize,
    resolved_macros: &[ResolvedMacroMeta],
    lowered: Option<&verter_type_expr::TypeExpr>,
) -> bool {
    fn strip_parens(expr: &verter_type_expr::TypeExpr) -> &verter_type_expr::TypeExpr {
        match expr {
            verter_type_expr::TypeExpr::Parenthesized(inner) => strip_parens(inner),
            other => other,
        }
    }

    let Some(lowered) = lowered.map(strip_parens) else {
        return false;
    };

    match lowered {
        verter_type_expr::TypeExpr::Object(_) => return true,
        verter_type_expr::TypeExpr::Ref { type_arguments, .. } if type_arguments.is_empty() => {}
        _ => return false,
    }

    let mut macro_surfaces = resolved_macros.iter().filter(|resolved| {
        resolved.macro_index == macro_index
            && resolved.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps
            && !resolved.props.is_empty()
    });
    let Some(first_surface) = macro_surfaces.next() else {
        return false;
    };
    if macro_surfaces.next().is_some() {
        return false;
    }
    if !mac
        .type_references
        .iter()
        .any(|type_name| type_name == &first_surface.type_name)
    {
        return false;
    }
    if !first_surface.surface_is_authoritative {
        return false;
    }

    let Some(text) = first_surface.declaration.text.as_deref() else {
        return false;
    };
    let compact: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
    let complex_markers = [
        "extends",
        "&",
        "Omit<",
        "Pick<",
        "Partial<",
        "Required<",
        "Record<",
        "Exclude<",
        "Extract<",
        "NonNullable<",
        "Readonly<",
        "keyof",
        "typeof",
        "[",
    ];

    !complex_markers
        .iter()
        .any(|marker| compact.contains(marker))
}

pub(crate) fn define_props_has_direct_local_root(mac: &AnalyzedMacro) -> bool {
    mac.resolved_local_types
        .iter()
        .enumerate()
        .any(|(resolved_index, resolved)| {
            is_direct_local_macro_type_reference(mac, resolved_index, resolved.name.as_str())
        })
}

pub(crate) fn is_direct_local_macro_type_reference(
    mac: &AnalyzedMacro,
    resolved_index: usize,
    resolved_name: &str,
) -> bool {
    resolved_index == 0
        || mac
            .type_references
            .iter()
            .any(|type_name| type_name == resolved_name)
}

pub(crate) fn define_props_known_surface_shortcut_allowed(
    lowered: Option<&verter_type_expr::TypeExpr>,
) -> bool {
    fn strip_parens(expr: &verter_type_expr::TypeExpr) -> &verter_type_expr::TypeExpr {
        match expr {
            verter_type_expr::TypeExpr::Parenthesized(inner) => strip_parens(inner),
            other => other,
        }
    }

    match lowered.map(strip_parens) {
        Some(verter_type_expr::TypeExpr::Object(_)) => true,
        Some(verter_type_expr::TypeExpr::Ref { type_arguments, .. }) => type_arguments.is_empty(),
        _ => false,
    }
}

pub(crate) fn synthesize_define_props_shape_from_known_surface_with_authority(
    macro_index: usize,
    snapshot: &FileAnalysisSnapshot,
    resolved_macros: &[ResolvedMacroMeta],
    evaluated_types: &verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    lowered: Option<&verter_type_expr::TypeExpr>,
    require_authoritative_surface: bool,
) -> Option<(ShapeResult, MacroShapeSource)> {
    use verter_semantic::analysis::type_expand::{
        ExpandedObjectShape, ExpandedProperty, ExpansionResult,
    };
    use verter_semantic::analysis::type_solver::result::{ExecutionStatus, SolverExactness};

    if !define_props_known_surface_shortcut_allowed(lowered) {
        return None;
    }

    let mac = snapshot.macros.get(macro_index)?;
    let allow_known_surface_shortcuts = !define_props_has_direct_local_root(mac);
    let resolved_macro = if allow_known_surface_shortcuts {
        let mut macro_surfaces = resolved_macros.iter().filter(|resolved| {
            resolved.macro_index == macro_index
                && resolved.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps
                && !resolved.props.is_empty()
                && (!require_authoritative_surface || resolved.surface_is_authoritative)
        });
        let first = macro_surfaces.next();
        if macro_surfaces.next().is_none()
            && first.is_some_and(|resolved| {
                mac.type_references
                    .iter()
                    .any(|type_name| type_name == &resolved.type_name)
            })
        {
            first
        } else {
            None
        }
    } else {
        None
    };
    let expanded_fields_cover_resolved_macro = resolved_macro.is_none_or(|resolved_macro| {
        resolved_macro.props.iter().all(|prop| {
            evaluated_types
                .props
                .iter()
                .any(|field| field.name == prop.name)
        })
    });
    let use_all_expanded_props = allow_known_surface_shortcuts
        && reuse_expanded_define_props_shape(snapshot, evaluated_types)
        && expanded_fields_cover_resolved_macro;

    let mut exactness = SolverExactness::ExactConcrete;
    let mut execution_status = ExecutionStatus::Completed;
    let mut diagnostics = Vec::new();
    let mut properties = Vec::new();

    if use_all_expanded_props {
        properties.reserve(evaluated_types.props.len());
        for field in &evaluated_types.props {
            exactness = exactness.merge(field.exactness);
            execution_status =
                merge_expansion_execution_status(execution_status, field.execution_status);
            diagnostics.extend(field.diagnostics.clone());
            properties.push(ExpandedProperty {
                name: field.name.clone(),
                ty: field.r#type.clone(),
                optional: field.optional,
                readonly: false,
            });
        }
    } else if let Some(resolved_macro) = resolved_macro {
        properties.reserve(resolved_macro.props.len());
        for prop in &resolved_macro.props {
            let field = evaluated_types
                .props
                .iter()
                .find(|field| field.name == prop.name);
            if let Some(field) = field {
                exactness = exactness.merge(field.exactness);
                execution_status =
                    merge_expansion_execution_status(execution_status, field.execution_status);
                diagnostics.extend(field.diagnostics.clone());
                properties.push(ExpandedProperty {
                    name: field.name.clone(),
                    ty: field.r#type.clone(),
                    optional: field.optional,
                    readonly: false,
                });
                continue;
            }

            // Typed-IR-Only Resolver Rule: `ResolvedProp.type_expr` is
            // the authoritative typed form, lowered by the parser at
            // OXC visit time. No reparse of `type_annotation`.
            let ty = prop
                .type_expr
                .clone()
                .unwrap_or(verter_type_expr::TypeExpr::Unknown {
                    raw: "unknown".to_string(),
                });
            properties.push(ExpandedProperty {
                name: prop.name.clone(),
                ty,
                optional: prop.is_optional,
                readonly: false,
            });
        }
    } else {
        return None;
    }

    Some((
        ExpansionResult {
            value: ExpandedObjectShape {
                properties,
                index_signatures: Vec::new(),
                call_signatures: Vec::new(),
            },
            exactness,
            execution_status,
            diagnostics,
        },
        if use_all_expanded_props {
            MacroShapeSource::Fields
        } else {
            MacroShapeSource::ResolvedMacro
        },
    ))
}

pub(crate) fn synthesize_define_props_shape_from_registry_root(
    owner_canonical: &str,
    macro_index: usize,
    snapshot: &FileAnalysisSnapshot,
    resolved_type_registry: &[verter_semantic::analysis::component_meta::ResolvedTypeAnalysis],
    resolved_type_registry_meta: &[ResolvedTypeRegistryMeta],
) -> Option<(ShapeResult, MacroShapeSource)> {
    let mac = snapshot.macros.get(macro_index)?;
    let root_name = mac.resolved_local_types.first()?.name.as_str();

    let mut matches = resolved_type_registry
        .iter()
        .zip(resolved_type_registry_meta.iter())
        .filter(|(entry, meta)| {
            entry.name == root_name
                && meta.name == root_name
                && meta.declaration.canonical_source == owner_canonical
        });
    let (entry, _) = matches.next()?;
    if matches.next().is_some() {
        return None;
    }

    let shape = registry_entry_to_expanded_shape(&entry.type_expr)?;
    if !has_prop_shape_surface(&shape) {
        return None;
    }

    Some((
        verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(shape),
        MacroShapeSource::Registry,
    ))
}

pub(crate) fn synthesize_macro_shape_from_registry_lowered_root(
    lowered: &verter_type_expr::TypeExpr,
    resolved_type_registry: &[verter_semantic::analysis::component_meta::ResolvedTypeAnalysis],
    resolved_type_registry_meta: &[ResolvedTypeRegistryMeta],
    shape_is_usable: impl Fn(&verter_semantic::analysis::type_expand::ExpandedObjectShape) -> bool,
) -> Option<(ShapeResult, MacroShapeSource)> {
    fn root_name(expr: &verter_type_expr::TypeExpr) -> Option<&str> {
        match expr {
            verter_type_expr::TypeExpr::Parenthesized(inner) => root_name(inner),
            verter_type_expr::TypeExpr::Ref { name, .. } => Some(name.as_ref()),
            _ => None,
        }
    }

    let root_name = root_name(lowered)?;
    let mut matches = resolved_type_registry
        .iter()
        .zip(resolved_type_registry_meta.iter())
        .filter(|(entry, meta)| entry.name == root_name && meta.name == root_name);
    let (entry, _) = matches.next()?;
    if matches.next().is_some() {
        return None;
    }

    let shape = registry_entry_to_expanded_shape(&entry.type_expr)?;
    if !shape_is_usable(&shape) {
        return None;
    }

    Some((
        verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(shape),
        MacroShapeSource::Registry,
    ))
}

pub(crate) fn named_ref_matches_empty_shell_registry_root(
    owner_canonical: &str,
    lowered: &verter_type_expr::TypeExpr,
    resolved_type_registry: &[verter_semantic::analysis::component_meta::ResolvedTypeAnalysis],
    resolved_type_registry_meta: &[ResolvedTypeRegistryMeta],
) -> bool {
    let verter_type_expr::TypeExpr::Ref {
        name,
        type_arguments,
    } = lowered
    else {
        return false;
    };
    if !type_arguments.is_empty() {
        return false;
    }

    let mut matches = resolved_type_registry
        .iter()
        .zip(resolved_type_registry_meta.iter())
        .filter(|(entry, meta)| {
            entry.name == name.as_ref()
                && meta.name == name.as_ref()
                && meta.declaration.canonical_source == owner_canonical
        });
    let Some((entry, _)) = matches.next() else {
        return false;
    };
    if matches.next().is_some() {
        return false;
    }

    registry_entry_to_expanded_shape(&entry.type_expr).is_some_and(|shape| {
        shape.properties.is_empty()
            && shape.index_signatures.is_empty()
            && shape.call_signatures.is_empty()
    })
}

pub(crate) fn reuse_expanded_define_emits_shape(
    snapshot: &FileAnalysisSnapshot,
    evaluated_types: &verter_semantic::analysis::type_expand::ExpandedComponentTypes,
) -> bool {
    !evaluated_types.emits.is_empty()
        && snapshot
            .macros
            .iter()
            .filter(|mac| {
                mac.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineEmits
                    && mac.is_type_based
            })
            .take(2)
            .count()
            == 1
}

pub(crate) fn synthesize_define_emits_shape_from_known_surface(
    macro_index: usize,
    snapshot: &FileAnalysisSnapshot,
    resolved_macros: &[ResolvedMacroMeta],
    evaluated_types: &verter_semantic::analysis::type_expand::ExpandedComponentTypes,
) -> Option<(ShapeResult, MacroShapeSource)> {
    use verter_semantic::analysis::type_expand::{
        ExpandedObjectShape, ExpandedProperty, ExpansionResult,
    };
    use verter_semantic::analysis::type_solver::result::{ExecutionStatus, SolverExactness};

    let use_all_expanded_emits = reuse_expanded_define_emits_shape(snapshot, evaluated_types);
    if use_all_expanded_emits {
        let mut exactness = SolverExactness::ExactConcrete;
        let mut execution_status = ExecutionStatus::Completed;
        let mut diagnostics = Vec::new();
        let mut properties = Vec::with_capacity(evaluated_types.emits.len());

        for emit in &evaluated_types.emits {
            exactness = exactness.merge(emit.exactness);
            execution_status =
                merge_expansion_execution_status(execution_status, emit.execution_status);
            diagnostics.extend(emit.diagnostics.clone());
            properties.push(ExpandedProperty {
                name: emit.name.clone(),
                ty: emit.r#type.clone(),
                optional: false,
                readonly: false,
            });
        }

        return Some((
            ExpansionResult {
                value: ExpandedObjectShape {
                    properties,
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
                exactness,
                execution_status,
                diagnostics,
            },
            MacroShapeSource::Fields,
        ));
    }

    let mut matching_macros = resolved_macros.iter().filter(|resolved| {
        resolved.macro_index == macro_index
            && resolved.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineEmits
            && !resolved.emits.is_empty()
    });
    let resolved_macro = matching_macros.next();
    if matching_macros.next().is_some() {
        return None;
    }
    let mut exactness = SolverExactness::ExactConcrete;
    let mut execution_status = ExecutionStatus::Completed;
    let mut diagnostics = Vec::new();
    let mut properties = Vec::new();

    if let Some(resolved_macro) = resolved_macro {
        properties.reserve(resolved_macro.emits.len());
        for emit in &resolved_macro.emits {
            let field = evaluated_types
                .emits
                .iter()
                .find(|field| field.name == emit.name);
            if let Some(field) = field {
                exactness = exactness.merge(field.exactness);
                execution_status =
                    merge_expansion_execution_status(execution_status, field.execution_status);
                diagnostics.extend(field.diagnostics.clone());
                properties.push(ExpandedProperty {
                    name: field.name.clone(),
                    ty: field.r#type.clone(),
                    optional: false,
                    readonly: false,
                });
                continue;
            }

            // Typed-IR-Only Resolver Rule: `ResolvedEmit.payload_expr`
            // is the authoritative typed form. W1.1e closed the
            // producer gap for cross-file `interface extends` emits.
            // No reparse of `payload_type`.
            let ty = emit
                .payload_expr
                .clone()
                .unwrap_or(verter_type_expr::TypeExpr::Unknown {
                    raw: "unknown".to_string(),
                });
            properties.push(ExpandedProperty {
                name: emit.name.clone(),
                ty,
                optional: false,
                readonly: false,
            });
        }
    } else {
        return None;
    }

    Some((
        ExpansionResult {
            value: ExpandedObjectShape {
                properties,
                index_signatures: Vec::new(),
                call_signatures: Vec::new(),
            },
            exactness,
            execution_status,
            diagnostics,
        },
        MacroShapeSource::ResolvedMacro,
    ))
}

pub(crate) fn synthesize_define_slots_shape_from_known_surface(
    macro_index: usize,
    resolved_macros: &[ResolvedMacroMeta],
) -> Option<(ShapeResult, MacroShapeSource)> {
    use verter_semantic::analysis::type_expand::{
        ExpandedObjectShape, ExpandedProperty, ExpansionResult,
    };

    let mut matching_macros = resolved_macros.iter().filter(|resolved| {
        resolved.macro_index == macro_index
            && resolved.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineSlots
            && !resolved.slots.is_empty()
    });
    let resolved_macro = matching_macros.next()?;
    if matching_macros.next().is_some() {
        return None;
    }

    let properties = resolved_macro
        .slots
        .iter()
        .map(|slot| ExpandedProperty {
            name: slot.name.clone(),
            ty: slot_field_function_type_expr(slot),
            optional: !slot.is_required,
            readonly: false,
        })
        .collect();

    Some((
        ExpansionResult::exact_symbolic(ExpandedObjectShape {
            properties,
            index_signatures: Vec::new(),
            call_signatures: Vec::new(),
        }),
        MacroShapeSource::ResolvedMacro,
    ))
}

/// Construct the typed `(props: { ... }) => RT` function expression for a
/// resolved slot directly from the analyzer-populated typed sidecars
/// (`AnalyzedSlotFieldBinding.binding_expr` and
/// `AnalyzedSlotField.return_expr`). No source-text reparse.
///
/// Empty-bindings slots produce `() => RT` (no `props` parameter).
/// Missing typed sources fall back to `TypeExpr::Primitive(any)` for the
/// return type and `TypeExpr::Primitive(unknown)` for each binding —
/// matching the analyzer's display-only `"any"` / `"unknown"` defaults.
pub(crate) fn slot_field_function_type_expr(
    slot: &verter_semantic::analysis::AnalyzedSlotField,
) -> verter_type_expr::TypeExpr {
    use std::sync::Arc;
    use verter_type_expr::{
        FunctionExpr, FunctionParam, ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
    };

    // W0.2 invariant: the analyzer populates AnalyzedSlotField.return_expr
    // whenever an OXC return-type TSType<'_> is in scope. A None here is a
    // producer-chain bug; panic loudly rather than silently substituting Any.
    let return_type = slot
        .return_expr
        .clone()
        .expect("AnalyzedSlotField.return_expr populated by analyzer (W0.2 invariant)");

    let parameters = if slot.bindings.is_empty() {
        Vec::new()
    } else {
        let properties = slot
            .bindings
            .iter()
            .map(|binding| {
                let ty = binding.binding_expr.clone().expect(
                    "AnalyzedSlotFieldBinding.binding_expr populated by analyzer (W0.2 invariant)",
                );
                ObjectMember::Property(ObjectProperty {
                    name: binding.name.clone(),
                    ty,
                    optional: false,
                    readonly: false,
                })
            })
            .collect();
        let props_object = TypeExpr::Object(Arc::new(ObjectExpr { properties }));
        vec![FunctionParam {
            name: Some("props".to_string()),
            ty: props_object,
            optional: false,
            rest: false,
        }]
    };

    TypeExpr::Function(Arc::new(FunctionExpr {
        parameters,
        return_type: Some(Arc::new(return_type)),
        type_parameters: Vec::new(),
    }))
}

pub(crate) fn reuse_expanded_define_props_shape(
    snapshot: &FileAnalysisSnapshot,
    evaluated_types: &verter_semantic::analysis::type_expand::ExpandedComponentTypes,
) -> bool {
    !evaluated_types.props.is_empty()
        && snapshot
            .macros
            .iter()
            .filter(|mac| mac.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
            .take(2)
            .count()
            == 1
        && !snapshot
            .macros
            .iter()
            .any(|mac| mac.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineModel)
}

pub(crate) fn merge_expansion_execution_status(
    current: verter_semantic::analysis::type_expand::ExpansionExecutionStatus,
    next: verter_semantic::analysis::type_expand::ExpansionExecutionStatus,
) -> verter_semantic::analysis::type_expand::ExpansionExecutionStatus {
    use verter_semantic::analysis::type_expand::ExpansionExecutionStatus;

    let severity = |status| match status {
        ExpansionExecutionStatus::Completed => 0u8,
        ExpansionExecutionStatus::Cancelled => 1u8,
        ExpansionExecutionStatus::Interrupted => 2u8,
        ExpansionExecutionStatus::HardStop => 3u8,
    };

    if severity(next) > severity(current) {
        next
    } else {
        current
    }
}

pub(crate) fn registry_entry_to_expanded_shape(
    expr: &verter_type_expr::TypeExpr,
) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
    use verter_semantic::analysis::type_expand::{
        ExpandedCallSignature, ExpandedIndexSignature, ExpandedObjectShape, ExpandedParameter,
        ExpandedProperty,
    };
    use verter_type_expr::{ObjectMember, PrimitiveName, TypeExpr};

    let TypeExpr::Object(object) = expr else {
        return None;
    };

    let mut properties = Vec::new();
    let mut call_signatures = Vec::new();
    let mut index_signatures = Vec::new();

    for member in &object.properties {
        match member {
            ObjectMember::Property(property) => properties.push(ExpandedProperty {
                name: property.name.clone(),
                ty: property.ty.clone(),
                optional: property.optional,
                readonly: property.readonly,
            }),
            ObjectMember::Method(method) => call_signatures.push(ExpandedCallSignature {
                parameters: method
                    .function
                    .parameters
                    .iter()
                    .map(|parameter| ExpandedParameter {
                        name: parameter.name.clone().unwrap_or_default(),
                        ty: parameter.ty.clone(),
                        optional: parameter.optional,
                        rest: parameter.rest,
                    })
                    .collect(),
                return_type: method
                    .function
                    .return_type
                    .as_ref()
                    .map(|return_type| return_type.as_ref().clone())
                    .unwrap_or(TypeExpr::Primitive(PrimitiveName::Void)),
                type_parameters: method.function.type_parameters.clone(),
            }),
            ObjectMember::CallSignature(function) | ObjectMember::ConstructSignature(function) => {
                call_signatures.push(ExpandedCallSignature {
                    parameters: function
                        .parameters
                        .iter()
                        .map(|parameter| ExpandedParameter {
                            name: parameter.name.clone().unwrap_or_default(),
                            ty: parameter.ty.clone(),
                            optional: parameter.optional,
                            rest: parameter.rest,
                        })
                        .collect(),
                    return_type: function
                        .return_type
                        .as_ref()
                        .map(|return_type| return_type.as_ref().clone())
                        .unwrap_or(TypeExpr::Primitive(PrimitiveName::Void)),
                    type_parameters: function.type_parameters.clone(),
                });
            }
            ObjectMember::IndexSignature(signature) => {
                index_signatures.push(ExpandedIndexSignature {
                    key_type: signature.key_type.clone(),
                    value_type: signature.value_type.clone(),
                    readonly: signature.readonly,
                });
            }
        }
    }

    Some(ExpandedObjectShape {
        properties,
        index_signatures,
        call_signatures,
    })
}

pub(crate) fn expanded_shape_to_type_expr(
    shape: &verter_semantic::analysis::type_expand::ExpandedObjectShape,
) -> verter_type_expr::TypeExpr {
    use verter_type_expr::{
        FunctionExpr, FunctionParam, ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
    };

    let mut properties = Vec::new();

    for property in &shape.properties {
        properties.push(ObjectMember::Property(ObjectProperty {
            name: property.name.clone(),
            ty: property.ty.clone(),
            optional: property.optional,
            readonly: property.readonly,
        }));
    }

    for signature in &shape.call_signatures {
        properties.push(ObjectMember::CallSignature(FunctionExpr {
            parameters: signature
                .parameters
                .iter()
                .map(|parameter| FunctionParam {
                    name: (!parameter.name.is_empty()).then(|| parameter.name.clone()),
                    ty: parameter.ty.clone(),
                    optional: parameter.optional,
                    rest: parameter.rest,
                })
                .collect(),
            return_type: Some(std::sync::Arc::new(signature.return_type.clone())),
            type_parameters: signature.type_parameters.clone(),
        }));
    }

    for signature in &shape.index_signatures {
        properties.push(verter_type_expr::ObjectMember::IndexSignature(
            verter_type_expr::IndexSignature {
                key_name: "key".to_string(),
                key_type: signature.key_type.clone(),
                value_type: signature.value_type.clone(),
                readonly: signature.readonly,
            },
        ));
    }

    TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties }))
}

type ShapeResult = verter_semantic::analysis::type_expand::ExpansionResult<
    verter_semantic::analysis::type_expand::ExpandedObjectShape,
>;

/// Returns `true` when `expr` carries a top-level surface that the
/// projector's bounded fixed-point reducer should expand further.
///
/// For a bare `Ref { name, type_arguments }` the predicate consults
/// the declaration body via the dispatch primitives and reports
/// `true` when the body has a non-object top-level surface (i.e. an
/// alias resolving to a primitive / utility wrapper / operator
/// shape) or the Ref carries type arguments and the declaration
/// body is unavailable. For any other shape the predicate returns
/// `true` when the expression itself has a non-object top-level
/// surface.
///
/// A short-circuit cycle guard
/// (`lowered_root_reaches_transitive_cycle`) returns `false` for
/// recursive aliases like `TreeNode` so the reducer cannot loop on
/// itself.
pub(crate) fn expr_needs_projection_rescue(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    expr: &verter_type_expr::TypeExpr,
) -> bool {
    use verter_type_expr::TypeExpr;

    if lowered_root_reaches_transitive_cycle(query_engine, owner_canonical, expr) {
        return false;
    }

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            let declaration = query_engine.resolve_type_declaration(owner_canonical, name);
            let scope_canonical = if declaration.canonical_source.is_empty() {
                owner_canonical
            } else {
                declaration.canonical_source.as_str()
            };
            let resolved_name = if declaration.resolved_name.is_empty() {
                name.as_ref()
            } else {
                declaration.resolved_name.as_str()
            };
            let body_needs_projection = query_engine
                .named_decl_body(scope_canonical, resolved_name)
                .is_some_and(|body| {
                    type_expr_has_non_object_top_level_surface(query_engine, scope_canonical, &body)
                });
            body_needs_projection
                || (!type_arguments.is_empty()
                    && query_engine
                        .named_decl_body(scope_canonical, resolved_name)
                        .is_none())
        }
        other => type_expr_has_non_object_top_level_surface(query_engine, owner_canonical, other),
    }
}

pub(crate) fn type_expr_has_non_object_top_level_surface(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    expr: &verter_type_expr::TypeExpr,
) -> bool {
    use verter_type_expr::TypeExpr;

    match expr {
        TypeExpr::TypeOf(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::KeyOf(_)
        | TypeExpr::Rest(_)
        | TypeExpr::TemplateLiteral { .. } => true,
        TypeExpr::Ref { name, .. } => {
            let declaration = query_engine.resolve_type_declaration(owner_canonical, name);
            let scope_canonical = if declaration.canonical_source.is_empty() {
                owner_canonical
            } else {
                declaration.canonical_source.as_str()
            };
            let resolved_name = if declaration.resolved_name.is_empty() {
                name.as_ref()
            } else {
                declaration.resolved_name.as_str()
            };
            query_engine
                .named_decl_body(scope_canonical, resolved_name)
                .is_some_and(|body| {
                    type_expr_has_non_object_top_level_surface(query_engine, scope_canonical, &body)
                })
        }
        TypeExpr::Parenthesized(inner) => {
            type_expr_has_non_object_top_level_surface(query_engine, owner_canonical, inner)
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            let mut saw_object = false;
            for ty in types.iter() {
                match ty {
                    TypeExpr::Parenthesized(inner) => {
                        if type_expr_has_non_object_top_level_surface(
                            query_engine,
                            owner_canonical,
                            inner.as_ref(),
                        ) {
                            return true;
                        }
                        if matches!(inner.as_ref(), TypeExpr::Object(_)) {
                            saw_object = true;
                        }
                    }
                    TypeExpr::Object(_) => saw_object = true,
                    _ => return true,
                }
            }
            !saw_object
        }
        TypeExpr::Object(_)
        | TypeExpr::Function(_)
        | TypeExpr::Array { .. }
        | TypeExpr::Tuple { .. } => false,
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => false,
    }
}

/// Produce one macro object shape.
///
/// Two strategies based on the body classification:
///
/// - **Direct Object body**: DB-backed `project_type_surface_expr` on the
///   defining file.  Solver skipped — this is the fast path for the common
///   case (imported interface with explicit members).
///
/// - **Non-Object body** (intersections, heritage, typeof, generics): solver
///   first (clean engine state → complete results), then
///   `project_expr_surface_expr` on warm caches (handles typeof member paths
///   the solver cannot resolve).  The more complete result wins.
pub(crate) fn produce_one_macro_object_shape(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    lowered: &verter_type_expr::TypeExpr,
    shape_is_usable: impl Fn(&verter_semantic::analysis::type_expand::ExpandedObjectShape) -> bool,
    cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
) -> (Option<ShapeResult>, MacroShapeSource) {
    // The threaded `cursor` carries the caller's published-surface
    // demand. Every shape-producing path finalizes through
    // [`finalize_macro_shape_through_cursor`] so the macro-shape
    // mirror publishes carrier-shallow member types (`whole_surface(kind)`
    // cursors admit every member name; each member's type body is
    // published as a `Navigate` carrier unless the consumer walked a
    // deep path).
    //
    // ── Fast path: direct Object body → DB-backed projection ──────────
    if let Some(mut projected) =
        project_named_ref_prepared_surface_shape(query_engine, owner_canonical, lowered, cursor)
    {
        finalize_macro_shape_through_cursor(cursor, &mut projected);
        return (Some(projected), MacroShapeSource::Projection);
    }

    if let verter_type_expr::TypeExpr::Ref {
        name,
        type_arguments,
    } = lowered
    {
        if type_arguments.is_empty() {
            if let Some((def_canonical, def_name)) =
                classify_named_ref_for_db_projection(query_engine, owner_canonical, name)
            {
                // Block 6.i Round 10 Commit 5 (Chain Y closure, codex
                // Q1-Y) — route fast-path is now demand-explicit. The
                // pre-Round-10 path always called
                // `project_type_surface_shape_via_host_threaded`
                // → `engine.dispatch_projected_surface`
                // → `dispatch_root_instantiated`
                // → `Instantiate(Published(Expanded))`, which
                // instantiated the root's full structural body and
                // emitted per-key `ProjectMember` edges for inherited
                // library member names on `extends Omit<…>` /
                // generic-substituted macro payloads (the diagnostic's
                // EditorDragHandle Chain Y).
                //
                // The path-precision predicate
                // `macro_payload_root_is_conditional_carrier` mirrors
                // the round-9 non-fast-path gate (see the same
                // predicate threaded into the solver block below): a
                // Conditional macro payload root retains
                // `Published(Expanded)` so the inherited-emits
                // branch-merge protocol
                // (`PayloadSurfaceScope::EmitClassMacroObject`) can
                // enumerate both branches' members; a non-Conditional
                // root (Object / Intersection / Mapped / Ref /
                // InstantiationRef) routes through the transit-shallow
                // sibling that carrier-lowers the synthesised
                // `Ref { def_name, [] }` under `Navigate` mode and
                // projects under `Published(Shallow)`.
                let payload_root_is_conditional = macro_payload_root_is_conditional_carrier(
                    query_engine,
                    owner_canonical,
                    lowered,
                );
                let shape_opt = if payload_root_is_conditional {
                    project_type_surface_shape_via_host_threaded(
                        query_engine,
                        &def_canonical,
                        &def_name,
                    )
                } else {
                    project_type_surface_shape_transit_shallow_via_host_threaded(
                        query_engine,
                        &def_canonical,
                        &def_name,
                    )
                };
                if let Some(shape) = shape_opt {
                    if shape_is_usable(&shape) {
                        let mut result =
                            verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                                shape,
                            );
                        finalize_macro_shape_through_cursor(cursor, &mut result);
                        return (Some(result), MacroShapeSource::Projection);
                    }
                }
            }
        }
    }

    // ── Non-object body: solver first, then projection on warm caches ─
    let scoped_solver_result = match lowered {
        verter_type_expr::TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => {
            let declaration = query_engine.resolve_type_declaration(owner_canonical, name.as_ref());
            let defining_canonical = if declaration.canonical_source.is_empty() {
                owner_canonical.to_string()
            } else {
                declaration.canonical_source.clone()
            };
            let defining_name = if declaration.resolved_name.is_empty() {
                name.as_ref().to_string()
            } else {
                declaration.resolved_name.clone()
            };
            // Bridge the engine method via the per-engine helper so
            // the pre-flight gate sees zero external engine-method
            // callers.
            project_type_surface_expr_via_host_threaded(
                query_engine,
                defining_canonical.as_str(),
                defining_name.as_str(),
            )
            .and_then(|solved_expr| {
                let shape =
                    verter_semantic::analysis::type_expand::type_expr_to_object_shape(&solved_expr);
                shape_is_usable(&shape).then(|| {
                    verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(shape)
                })
            })
        }
        _ => None,
    };
    // Route through dispatch's Class A surface projection and treat
    // the result as an exact-concrete `SolverResult` so
    // `solver_result_to_object_expansion` still derives the expansion.
    //
    // Non-slot macros (props / emits / exposed / options) publish each
    // property's value verbatim from the Class A surface — no
    // post-pass walks Ref-typed property bodies.
    //
    // **Path-precise lowering** (Block 6.i Round 9): the producer
    // branches on the lowered root's semantic shape:
    //
    // - **Conditional root** — the inherited-emits branch-merge
    //   protocol (see
    //   [`crate::meta_resolve::projectors::emits::project_emits`] and
    //   [`crate::meta_resolve::projectors::PayloadSurfaceScope::EmitClassMacroObject`])
    //   needs the top-level Conditional carrier visible at the
    //   publication surface so the projector pipeline can enumerate
    //   both branches' members under `Published(Shallow)`. Dispatch
    //   the existing `Published(Expanded)` helper
    //   ([`project_expr_class_a_via_dispatch_threaded`]) which
    //   produces the carrier surface the downstream branch-merge
    //   consumes.
    //
    // - **Object / Intersection / Mapped / Ref / InstantiationRef
    //   root** — the macro payload is a structural body (or a route
    //   pointing at one). The transit-shallow dispatch
    //   ([`project_expr_class_a_via_dispatch_transit_shallow_threaded`])
    //   keeps Mapped/KeyOf operators deferred at the publication
    //   boundary so inherited library member names do not enumerate
    //   into `ProjectMember` edges at the macro-publication site
    //   (Rule-5 shallow-by-default). Each property's Ref-typed body
    //   stays as a carrier the consumer re-resolves on demand.
    //
    // The path-precision predicate is the root-shape classifier
    // [`macro_payload_root_is_conditional_carrier`] in
    // `dispatch_helpers.rs`; it Navigate-lowers the payload and walks
    // pure-carrier shells (Alias, DeclRef) to find the first
    // structural root.
    let payload_root_is_conditional =
        macro_payload_root_is_conditional_carrier(query_engine, owner_canonical, lowered);
    let solver_result = scoped_solver_result.unwrap_or_else(|| {
        let projected = if payload_root_is_conditional {
            project_expr_class_a_via_dispatch_threaded(
                query_engine.ctx,
                Some(query_engine),
                owner_canonical,
                lowered,
            )
        } else {
            project_expr_class_a_via_dispatch_transit_shallow_threaded(
                query_engine.ctx,
                Some(query_engine),
                owner_canonical,
                lowered,
            )
        }
        .unwrap_or_else(|| lowered.clone());
        verter_semantic::analysis::type_expand::solver_result_to_object_expansion(
            verter_semantic::analysis::type_solver::result::SolverResult::exact_concrete(projected),
        )
    });
    let solver_count = shape_surface_count(&solver_result);
    // Path-precise rescue gate (Block 6.i Round 9). The three rescue
    // projectors below (`project_expr_surface_shape_via_host_threaded`,
    // `project_named_ref_surface_shape`,
    // `project_named_ref_imported_scope_shape`) all lower under
    // `Published(Expanded)` internally and walk the macro payload's
    // inherited library body — emitting one `ProjectMember` edge per
    // enumerated key. For non-Conditional macro payload roots the
    // transit-shallow path above is the canonical lowering (shallow-
    // by-default); rescue widening MUST NOT re-introduce the eager
    // enumeration the transit-shallow swap was written to eliminate.
    //
    // Conditional macro payload roots keep the rescue gate. The
    // `Published(Expanded)` lowering above produces the carrier shell
    // the inherited-emits branch-merge consumes; rescue widening on
    // the same Conditional-rooted surface is the round-7 + round-8
    // behaviour and must persist to satisfy the round-8 inherited-
    // emits locked-down tests under solver-empty fallback paths.
    let rescue_projection = payload_root_is_conditional
        && (solver_count == 0
            || expr_needs_projection_rescue(query_engine, owner_canonical, lowered));
    let projected = if rescue_projection {
        // Bridge the engine method via the per-engine helper so the
        // pre-flight gate sees zero external engine-method callers.
        project_expr_surface_shape_via_host_threaded(query_engine, owner_canonical, lowered)
            .and_then(|shape| {
                shape_is_usable(&shape).then(|| {
                    verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(shape)
                })
            })
    } else {
        None
    };
    let root_projected = if rescue_projection {
        project_named_ref_surface_shape(
            query_engine,
            owner_canonical,
            lowered,
            &shape_is_usable,
            cursor,
        )
    } else {
        None
    };
    let imported_scope_projected = if rescue_projection {
        project_named_ref_imported_scope_shape(
            query_engine,
            owner_canonical,
            lowered,
            &shape_is_usable,
            cursor,
        )
    } else {
        None
    };
    let projected = [projected, root_projected, imported_scope_projected]
        .into_iter()
        .flatten()
        .max_by_key(shape_surface_count);

    let (mut result, source) = match projected {
        Some(proj) if solver_count == 0 => (Some(proj), MacroShapeSource::Projection),
        Some(proj) if projection_result_beats_solver_shape(&proj, &solver_result) => {
            (Some(proj), MacroShapeSource::Projection)
        }
        _ if solver_count > 0 => (Some(solver_result), MacroShapeSource::Solver),
        _ => match projected {
            Some(proj) => (Some(proj), MacroShapeSource::Projection),
            None => (None, MacroShapeSource::None),
        },
    };
    // Finalize each published member through the cursor so the macro-
    // shape mirror publishes carrier-shallow types (member bodies stay
    // as carrier Refs at the publication boundary, no breadth-
    // enumeration of nested members).
    if let Some(shape) = result.as_mut() {
        finalize_macro_shape_through_cursor(cursor, shape);
    }
    (result, source)
}

/// Carrier-preserving per-member finalizer for a projected macro
/// object shape.
///
/// A macro projector publishes EVERY top-level member NAME admitted
/// by the cursor's published surface. This helper descends each
/// property through `cursor.descend_published_member`:
///
/// - A member the cursor does NOT admit is DROPPED from the shape (a
///   narrowed projection excludes it).
/// - An admitted member is kept; its type body is published as a
///   CARRIER unless the consumer explicitly walked a deep path. The
///   per-member carrier depth-reduction for the published
///   `evaluated_types.define_props` mirror is finalised by
///   [`crate::meta_resolve::projectors::reduce_published_field_types`]'s
///   back-sync (which runs the carrier-aware reducer in
///   request-bound context); this helper owns the breadth gate so
///   the macro-shape producer and the projector pipeline agree on
///   the published surface membership.
///
/// `descend_published_member` keeps the breadth gate identical to
/// the projector pipeline's per-member descent — when a narrowed
/// macro projection cursor (`Pick`/`Omit`) is threaded in, siblings
/// drop here exactly as they drop from `project_props`.
fn finalize_macro_shape_through_cursor(
    cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
    shape: &mut ShapeResult,
) {
    shape.value.properties.retain(|property| {
        cursor
            .descend_published_member(property.name.as_str())
            .is_some()
    });
}

pub(crate) fn project_named_ref_prepared_surface_shape(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    lowered: &verter_type_expr::TypeExpr,
    cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
) -> Option<ShapeResult> {
    // The projected macro root surface is finalized through
    // `cursor.descend_published_member` so each published member's
    // type body is a carrier (`Navigate` mode), not an eagerly-
    // expanded object surface.
    let verter_type_expr::TypeExpr::Ref {
        name,
        type_arguments,
    } = lowered
    else {
        return None;
    };
    if !named_ref_can_use_prepared_projection(query_engine, owner_canonical, name.as_ref()) {
        return None;
    }
    if !type_arguments.is_empty() {
        let declaration = query_engine.resolve_type_declaration(owner_canonical, name.as_ref());
        let scope_canonical = if declaration.canonical_source.is_empty() {
            owner_canonical
        } else {
            declaration.canonical_source.as_str()
        };
        let resolved_name = if declaration.resolved_name.is_empty() {
            name.as_ref()
        } else {
            declaration.resolved_name.as_str()
        };
        if !query_engine
            .named_decl_body(scope_canonical, resolved_name)
            .is_some_and(|body| {
                type_expr_has_non_object_top_level_surface(query_engine, scope_canonical, &body)
            })
        {
            return None;
        }
    }

    let (scope_canonical, resolved_name) =
        resolve_named_ref_prepared_projection_target(query_engine, owner_canonical, name.as_ref())?;

    // Bridge the engine method via the per-engine helper so the
    // pre-flight gate sees zero external engine-method callers.
    let mut result = project_prepared_type_surface_shape_via_host_threaded(
        query_engine,
        scope_canonical.as_str(),
        resolved_name.as_str(),
    )
    .and_then(|shape| {
        has_prop_shape_surface(&shape)
            .then(|| verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(shape))
    })?;
    finalize_macro_shape_through_cursor(cursor, &mut result);
    Some(result)
}

pub(crate) fn named_ref_can_use_prepared_projection(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    requested_name: &str,
) -> bool {
    let declaration = query_engine.resolve_type_declaration(owner_canonical, requested_name);
    let target_scope = if declaration.canonical_source.is_empty() {
        owner_canonical
    } else {
        declaration.canonical_source.as_str()
    };
    let resolved_name = if declaration.resolved_name.is_empty() {
        requested_name
    } else {
        declaration.resolved_name.as_str()
    };

    // Delegate the symbolic-vs-materialize decision to the shared
    // helper. Same-scope refs are always workspace-owned (they live
    // inside the owner SFC), so the helper returns `true` and this
    // predicate returns `true` (allow the prepared-surface
    // projection). Cross-file workspace-owned direct-member
    // interface/class refs flow through the same helper path —
    // canonical-reuse is shared with the field-materialise site.
    let prepared = query_engine
        .ctx()
        .prepared_type_decl(target_scope, resolved_name);
    let policy_ctx = crate::component_meta_resolution_policy::policy_helpers::PolicyContext {
        is_workspace_owned: &|canonical| query_engine.ctx.workspace_is_workspace_owned(canonical),
        is_package_backed: &|canonical| query_engine.ctx.workspace_is_package_backed(canonical),
        route_preservation_context: false,
        cycle_active_for_target: false,
        shallow_preserve_list_entry: false,
    };
    if crate::component_meta_resolution_policy::policy_helpers::imported_ref_must_materialize_canonically(
        target_scope,
        prepared.as_deref(),
        &policy_ctx,
    ) {
        return true;
    }

    // Helper said NOT must-materialize — fall back to the legacy
    // kind-based decision for cases the helper does not own
    // (package-backed Interface/Class with explicit object surfaces,
    // type aliases, the empty-canonical-source case where the
    // resolver returned no scope hint).
    if declaration.canonical_source.is_empty() {
        return true;
    }
    match declaration.kind {
        crate::resolver_core::ResolvedDeclarationKind::Class => {
            crate::resolver_core::component_meta::imported_declaration_surface_is_authoritative(
                &declaration,
            )
        }
        crate::resolver_core::ResolvedDeclarationKind::Interface
        | crate::resolver_core::ResolvedDeclarationKind::TypeAlias => true,
        crate::resolver_core::ResolvedDeclarationKind::Unknown => false,
    }
}

pub(crate) fn resolve_named_ref_prepared_projection_target(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    requested_name: &str,
) -> Option<(String, String)> {
    if query_engine
        .ctx()
        .prepared_type_decl(owner_canonical, requested_name)
        .is_some()
    {
        return Some((owner_canonical.to_string(), requested_name.to_string()));
    }

    if let Some(state) = query_engine
        .ctx()
        .route_owned_shallow_state(owner_canonical)
    {
        if state.symbol(requested_name).is_some() {
            return Some((owner_canonical.to_string(), requested_name.to_string()));
        }

        if let Some(import_target) = state.import_target(requested_name) {
            let target_canonical = if import_target.canonical_id.is_empty() {
                query_engine.ctx().resolve_route_type_edge(
                    owner_canonical,
                    import_target.source_specifier.as_str(),
                )?
            } else {
                import_target.canonical_id.clone()
            };
            let target_name = import_target.imported_name.clone();
            if let Some((routed_canonical, routed_name)) =
                query_engine.ctx().resolve_named_type_export_target_shallow(
                    target_canonical.as_str(),
                    target_name.as_str(),
                )
            {
                if query_engine
                    .ctx()
                    .prepared_type_decl(routed_canonical.as_str(), routed_name.as_str())
                    .is_some()
                {
                    return Some((routed_canonical, routed_name));
                }
            }
            if query_engine
                .ctx()
                .prepared_type_decl(target_canonical.as_str(), target_name.as_str())
                .is_some()
            {
                return Some((target_canonical, target_name));
            }
        }
    }

    let declaration = query_engine.resolve_type_declaration(owner_canonical, requested_name);
    let scope_canonical = if declaration.canonical_source.is_empty() {
        owner_canonical.to_string()
    } else {
        declaration.canonical_source
    };
    let resolved_name = if declaration.resolved_name.is_empty() {
        requested_name.to_string()
    } else {
        declaration.resolved_name
    };
    Some((scope_canonical, resolved_name))
}

pub(crate) fn project_named_ref_surface_shape(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    lowered: &verter_type_expr::TypeExpr,
    shape_is_usable: &impl Fn(&verter_semantic::analysis::type_expand::ExpandedObjectShape) -> bool,
    cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
) -> Option<ShapeResult> {
    // The projected macro root surface is finalized through
    // `cursor.descend_published_member` so each published member's
    // type body is a carrier (`Navigate` mode).
    let verter_type_expr::TypeExpr::Ref { name, .. } = lowered else {
        return None;
    };

    let declaration = query_engine.resolve_type_declaration(owner_canonical, name);
    let defining_canonical = if declaration.canonical_source.is_empty() {
        owner_canonical
    } else {
        declaration.canonical_source.as_str()
    };
    let defining_name = if declaration.resolved_name.is_empty() {
        name.as_ref()
    } else {
        declaration.resolved_name.as_str()
    };

    // Bridge the engine method via the per-engine helper so the
    // pre-flight gate sees zero external engine-method callers.
    let mut result = project_type_surface_shape_via_host_threaded(
        query_engine,
        defining_canonical,
        defining_name,
    )
    .and_then(|shape| {
        shape_is_usable(&shape)
            .then(|| verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(shape))
    })?;
    finalize_macro_shape_through_cursor(cursor, &mut result);
    Some(result)
}

pub(crate) fn project_named_ref_imported_scope_shape(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    lowered: &verter_type_expr::TypeExpr,
    shape_is_usable: &impl Fn(&verter_semantic::analysis::type_expand::ExpandedObjectShape) -> bool,
    cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
) -> Option<ShapeResult> {
    // The projected macro root surface is finalized through
    // `cursor.descend_published_member` so each published member's
    // type body is a carrier (`Navigate` mode).
    let verter_type_expr::TypeExpr::Ref { name, .. } = lowered else {
        return None;
    };

    let declaration = query_engine.resolve_type_declaration(owner_canonical, name);
    let defining_canonical = if declaration.canonical_source.is_empty() {
        owner_canonical
    } else {
        declaration.canonical_source.as_str()
    };
    if defining_canonical == owner_canonical {
        return None;
    }

    // Bridge the engine method via the per-engine helper so the
    // pre-flight gate sees zero external engine-method callers.
    let mut result =
        project_expr_surface_shape_via_host_threaded(query_engine, defining_canonical, lowered)
            .and_then(|shape| {
                shape_is_usable(&shape).then(|| {
                    verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(shape)
                })
            })?;
    finalize_macro_shape_through_cursor(cursor, &mut result);
    Some(result)
}

/// `defineSlots` macro-shape publication.
///
/// Slot Function param types stay as Ref carriers — the published
/// `Function { params: [(propsName, RefCarrier)], return }` shape
/// preserves the consumer re-resolution contract. Slot-binding
/// extraction runs through [`compute_bindings_via_graph`]
/// (`slot_binding_graph.rs`), which is graph-native and dispatches
/// its own `ProjectPath { mode: Shallow }` queries on the macro
/// payload — independent of the published shape. The parser-side
/// enumerator `slot_bindings_from_type_expr`
/// (`verter_semantic::analysis::component_meta`) handles Ref-typed
/// param surfaces by falling through to `evaluated_slot_bindings`,
/// which fills the row from the graph-native path.
pub(crate) fn produce_one_macro_object_shape_for_slots(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    lowered: &verter_type_expr::TypeExpr,
    cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
) -> (Option<ShapeResult>, MacroShapeSource) {
    // Cursor threaded for path-precision in downstream walks; most
    // callers pass `whole_surface(Slots)` so every member name is
    // admitted.
    // ── Fast path: direct Object body → DB-backed projection ──────────
    if let verter_type_expr::TypeExpr::Ref {
        name,
        type_arguments,
    } = lowered
    {
        if type_arguments.is_empty() {
            if let Some((def_canonical, def_name)) =
                classify_named_ref_for_db_projection(query_engine, owner_canonical, name)
            {
                // bridge via per-engine helper.
                if let Some(shape) = project_type_surface_shape_via_host_threaded(
                    query_engine,
                    &def_canonical,
                    &def_name,
                ) {
                    if has_shape_surface(&shape) {
                        return (
                            Some(
                                verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(shape),
                            ),
                            MacroShapeSource::Projection,
                        );
                    }
                }
            }
        }
    }

    // ── Non-object body: dispatch projection through Class A
    //    transit-shallow then fall back to the compound-objects
    //    lenient path.
    //
    // Slot macro publication lowers the payload under `Navigate` mode
    // and walks the publication terminal under `Published(Shallow)`
    // so the outer Object surface publishes while slot member values
    // (slot Function param types, etc.) stay as carrier Refs. The
    // slot-binding consumer reaches the bindings via the graph-native
    // path (see fn docstring) and the callable-realization substrate
    // (`realize_callable_member`) normalises carrier-shaped slot
    // values for the graph-native `Function`-arm match.
    //
    // Compound-shape recovery: when the strict transit-shallow Class A
    // returns `None` because a compound-shape sibling is still a
    // deferred shell (e.g. `{ explicit slots } & DynamicSlots<...>` —
    // the `DynamicSlots` arm is a Mapped that can't enumerate keys
    // when the type parameters are unresolved), fall back to the
    // lenient `project_expr_surface_expr_with_compound_objects` so the
    // explicit Object arm's properties still reach
    // [`solver_result_to_object_expansion`]. The expansion's existing
    // Intersection-merging in [`type_expr_to_expanded_shape`] then
    // collects the explicit slot members from the compound shape. The
    // compound-objects helper walks under the Expanded helper and is
    // intentionally lenient; transit-shallow Class A above is the
    // demand-driven publication path that the slot publication
    // boundary favours.
    // Block 6.i Round 10 Commit 4 (Chain Z closure, codex Q1-Z) —
    // the slot fallback's compound-objects helper migrates from the
    // pre-Round-10 Expanded path (`...via_host_threaded`) to the
    // transit-shallow sibling (`...transit_shallow_via_host_threaded`)
    // added to `dispatch_helpers.rs`. The Expanded helper lowered the
    // slot payload's TypeExpr in `Published(Expanded)` and emitted
    // 30 of the 364 captured ProjectMember leak edges on ChatMessages
    // fresh-cold (per `D:/tmp/round10-diagnostic-report.md` Chain Z).
    // The transit-shallow sibling lowers under `Navigate` mode and
    // walks the publication terminal under `Published(Shallow)` so
    // the slot payload's `Mapped<...>` body stays deferred at the
    // macro-publication boundary; the slot-binding consumer reaches
    // the bindings via the graph-native path (per the fn docstring
    // above) and the callable-realization substrate normalises
    // carrier-shaped slot values.
    let projected_body = project_expr_class_a_via_dispatch_transit_shallow(
        query_engine.ctx,
        owner_canonical,
        lowered,
    )
    .or_else(|| {
        // bridge via per-engine helper.
        project_expr_surface_expr_with_compound_objects_transit_shallow_via_host_threaded(
            query_engine,
            owner_canonical,
            lowered,
        )
    })
    .unwrap_or_else(|| lowered.clone());
    let _ = cursor; // cursor finalisation runs at the call site of `produce_one_macro_object_shape_for_slots`.
    let solver_result = verter_semantic::analysis::type_expand::solver_result_to_object_expansion(
        verter_semantic::analysis::type_solver::result::SolverResult::exact_concrete(
            projected_body,
        ),
    );
    let solver_count = shape_surface_count(&solver_result);

    let projected = project_expr_class_a_shape_via_dispatch_transit_shallow(
        query_engine.ctx,
        owner_canonical,
        lowered,
    )
    .and_then(|shape| {
        let projected_expr = expanded_shape_to_type_expr(&shape);
        // No post-pass over Ref-typed slot Function bodies — slot
        // member values stay as carriers; consumers re-resolve on
        // demand.
        registry_entry_to_expanded_shape(&projected_expr).and_then(|resolved_shape| {
            has_shape_surface(&resolved_shape).then(|| {
                verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                    resolved_shape,
                )
            })
        })
    });
    let imported_scope_projected = project_named_ref_imported_scope_shape(
        query_engine,
        owner_canonical,
        lowered,
        &has_shape_surface,
        cursor,
    )
    .and_then(|shape| {
        let projected_expr = expanded_shape_to_type_expr(&shape.value);
        // No post-pass over Ref-typed slot Function bodies — slot
        // member values stay as carriers; consumers re-resolve on
        // demand.
        registry_entry_to_expanded_shape(&projected_expr).and_then(|resolved_shape| {
            has_shape_surface(&resolved_shape).then(|| {
                verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                    resolved_shape,
                )
            })
        })
    });
    let projected = [projected, imported_scope_projected]
        .into_iter()
        .flatten()
        .max_by_key(shape_surface_count);

    match projected {
        Some(proj) if solver_count == 0 => (Some(proj), MacroShapeSource::Projection),
        Some(proj) if projection_result_beats_solver_shape(&proj, &solver_result) => {
            (Some(proj), MacroShapeSource::Projection)
        }
        _ if solver_count > 0 => (Some(solver_result), MacroShapeSource::Solver),
        _ => match projected {
            Some(proj) => (Some(proj), MacroShapeSource::Projection),
            None => (None, MacroShapeSource::None),
        },
    }
}

pub(crate) fn has_shape_surface(
    shape: &verter_semantic::analysis::type_expand::ExpandedObjectShape,
) -> bool {
    !shape.properties.is_empty()
        || !shape.index_signatures.is_empty()
        || !shape.call_signatures.is_empty()
}

pub(crate) fn type_expr_symbolic_penalty(expr: &verter_type_expr::TypeExpr) -> usize {
    use verter_type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) => 0,
        TypeExpr::Unknown { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::Infer { .. } => 2,
        TypeExpr::Ref { type_arguments, .. } => {
            1 + type_arguments
                .iter()
                .map(type_expr_symbolic_penalty)
                .sum::<usize>()
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::Parenthesized(element)
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element) => type_expr_symbolic_penalty(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .map(|element| type_expr_symbolic_penalty(&element.ty))
            .sum(),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            types.iter().map(type_expr_symbolic_penalty).sum()
        }
        TypeExpr::Object(object) => object
            .properties
            .iter()
            .map(|member| match member {
                ObjectMember::Property(property) => type_expr_symbolic_penalty(&property.ty),
                ObjectMember::IndexSignature(signature) => {
                    type_expr_symbolic_penalty(&signature.key_type)
                        + type_expr_symbolic_penalty(&signature.value_type)
                }
                ObjectMember::CallSignature(function)
                | ObjectMember::ConstructSignature(function) => {
                    function
                        .parameters
                        .iter()
                        .map(|parameter| type_expr_symbolic_penalty(&parameter.ty))
                        .sum::<usize>()
                        + function
                            .return_type
                            .as_deref()
                            .map(type_expr_symbolic_penalty)
                            .unwrap_or_default()
                }
                ObjectMember::Method(method) => {
                    method
                        .function
                        .parameters
                        .iter()
                        .map(|parameter| type_expr_symbolic_penalty(&parameter.ty))
                        .sum::<usize>()
                        + method
                            .function
                            .return_type
                            .as_deref()
                            .map(type_expr_symbolic_penalty)
                            .unwrap_or_default()
                }
            })
            .sum(),
        TypeExpr::Function(function) => {
            function
                .parameters
                .iter()
                .map(|parameter| type_expr_symbolic_penalty(&parameter.ty))
                .sum::<usize>()
                + function
                    .return_type
                    .as_deref()
                    .map(type_expr_symbolic_penalty)
                    .unwrap_or_default()
        }
        TypeExpr::IndexedAccess { object, index } => {
            2 + type_expr_symbolic_penalty(object) + type_expr_symbolic_penalty(index)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            2 + type_expr_symbolic_penalty(check)
                + type_expr_symbolic_penalty(extends)
                + type_expr_symbolic_penalty(true_type)
                + type_expr_symbolic_penalty(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            2 + type_expr_symbolic_penalty(source)
                + type_expr_symbolic_penalty(value)
                + name_type
                    .as_deref()
                    .map(type_expr_symbolic_penalty)
                    .unwrap_or_default()
        }
        TypeExpr::TemplateLiteral { expressions, .. } => {
            1 + expressions
                .iter()
                .map(type_expr_symbolic_penalty)
                .sum::<usize>()
        }
        TypeExpr::TypeOf(_) => 2,
    }
}

pub(crate) fn shape_symbolic_penalty(
    shape: &verter_semantic::analysis::type_expand::ExpandedObjectShape,
) -> usize {
    shape
        .properties
        .iter()
        .map(|property| type_expr_symbolic_penalty(&property.ty))
        .sum::<usize>()
        + shape
            .index_signatures
            .iter()
            .map(|signature| {
                type_expr_symbolic_penalty(&signature.key_type)
                    + type_expr_symbolic_penalty(&signature.value_type)
            })
            .sum::<usize>()
        + shape
            .call_signatures
            .iter()
            .map(|signature| {
                signature
                    .parameters
                    .iter()
                    .map(|parameter| type_expr_symbolic_penalty(&parameter.ty))
                    .sum::<usize>()
                    + type_expr_symbolic_penalty(&signature.return_type)
            })
            .sum::<usize>()
}

pub(crate) fn projection_result_beats_solver_shape(
    projected: &ShapeResult,
    solver: &ShapeResult,
) -> bool {
    let projected_count = shape_surface_count(projected);
    let solver_count = shape_surface_count(solver);
    projected_count > solver_count
        || (projected_count == solver_count
            && shape_symbolic_penalty(&projected.value) < shape_symbolic_penalty(&solver.value))
}

pub(crate) fn has_prop_shape_surface(
    shape: &verter_semantic::analysis::type_expand::ExpandedObjectShape,
) -> bool {
    !shape.properties.is_empty() || !shape.index_signatures.is_empty()
}

pub(crate) fn shape_surface_count(result: &ShapeResult) -> usize {
    result.value.properties.len()
        + result.value.index_signatures.len()
        + result.value.call_signatures.len()
}

/// Classify whether a zero-arg named ref can use DB-backed projection.
///
/// Returns `Some((defining_canonical, defining_name))` when the body is a
/// direct Object and `project_type_surface_expr` on the defining file is the
/// correct fast path.  Returns `None` for bodies that need the solver (typeof,
/// intersections, heritage Refs, etc.).
pub(crate) fn classify_named_ref_for_db_projection(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    name: &str,
) -> Option<(String, String)> {
    let declaration = query_engine.resolve_type_declaration(owner_canonical, name);
    let defining_canonical = declaration.canonical_source.clone();
    let defining_name = declaration.resolved_name.clone();
    if !declaration.canonical_source.is_empty()
        && declaration.canonical_source != owner_canonical
        && !crate::resolver_core::component_meta::imported_declaration_surface_is_authoritative(
            &declaration,
        )
    {
        return None;
    }
    let safe = match declaration.kind {
        crate::resolver_core::ResolvedDeclarationKind::Interface
        | crate::resolver_core::ResolvedDeclarationKind::Class => query_engine
            .named_decl_body(&defining_canonical, &defining_name)
            .is_some(),
        crate::resolver_core::ResolvedDeclarationKind::TypeAlias
        | crate::resolver_core::ResolvedDeclarationKind::Unknown => query_engine
            .named_decl_body(&defining_canonical, &defining_name)
            .is_some_and(|body| matches!(body, verter_type_expr::TypeExpr::Object(_),)),
    };
    safe.then_some((defining_canonical, defining_name))
}

/// Collect every type-reference name mentioned inside `expr` (including names
/// reachable through object members, unions/intersections, indexed access,
/// tuples, arrays, parenthesized and function type nodes).
///
/// Used to decide which registry-referenced names are already "seeded" by
/// published entries and therefore must keep their own registry publication
/// instead of being inlined as indexed-access paths.
pub(crate) fn collect_type_expr_ref_names(
    expr: &verter_type_expr::TypeExpr,
    out: &mut rustc_hash::FxHashSet<String>,
) {
    use verter_type_expr::{ObjectMember, TypeExpr};
    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
            ..
        } => {
            out.insert(name.to_string());
            for arg in type_arguments.iter() {
                collect_type_expr_ref_names(arg, out);
            }
        }
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => collect_type_expr_ref_names(&prop.ty, out),
                    ObjectMember::IndexSignature(sig) => {
                        collect_type_expr_ref_names(&sig.key_type, out);
                        collect_type_expr_ref_names(&sig.value_type, out);
                    }
                    ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                        for param in &func.parameters {
                            collect_type_expr_ref_names(&param.ty, out);
                        }
                        if let Some(ret) = &func.return_type {
                            collect_type_expr_ref_names(ret, out);
                        }
                    }
                    ObjectMember::Method(method) => {
                        for param in &method.function.parameters {
                            collect_type_expr_ref_names(&param.ty, out);
                        }
                        if let Some(ret) = &method.function.return_type {
                            collect_type_expr_ref_names(ret, out);
                        }
                    }
                }
            }
        }
        TypeExpr::Array { element, .. } => collect_type_expr_ref_names(element, out),
        TypeExpr::Tuple { elements, .. } => {
            for el in elements.iter() {
                collect_type_expr_ref_names(&el.ty, out);
            }
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for ty in types.iter() {
                collect_type_expr_ref_names(ty, out);
            }
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_type_expr_ref_names(object, out);
            collect_type_expr_ref_names(index, out);
        }
        TypeExpr::Parenthesized(inner) => collect_type_expr_ref_names(inner, out),
        TypeExpr::Function(func) => {
            for param in &func.parameters {
                collect_type_expr_ref_names(&param.ty, out);
            }
            if let Some(ret) = &func.return_type {
                collect_type_expr_ref_names(ret, out);
            }
        }
        _ => {}
    }
}
