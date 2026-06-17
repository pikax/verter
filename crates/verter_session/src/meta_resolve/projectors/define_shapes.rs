//! `define_props` / `define_emits` / `define_slots` shape publication.
//!
//! This projector owns the macro-shape level of `ExpandedComponentTypes`
//! (`define_props` / `define_emits` / `define_slots`). It is the sole author
//! of those fields — the eager macro-object materialiser is retired.
//!
//! Each shape is built from exactly TWO already-resolved, context-aware
//! sources, with NO solver fallback, NO source reparse, and NO `eval_source`
//! type-parameter collection:
//!
//! 1. the macro's normalized DTOs
//!    ([`crate::typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx`] —
//!    the SOLE props/emits/slots member authority, resolved through the active
//!    `ResolverContext` so overlay sessions read overlay content), and
//! 2. the flat `evaluated_types.props` / `evaluated_types.emits` fields that
//!    [`super::project_evaluated_types`] already projected through the shared
//!    dispatch (the authoritative per-member `TypeExpr`).
//!
//! `project_define_macro_shapes` MUST run AFTER `project_evaluated_types` (so
//! the flat fields exist to draw types from) and BEFORE
//! `resolve_slot_bindings_graph_native` (which consumes the slot shape).
//!
//! Per-kind rules:
//! - **Props**: properties only (a member is a property name on the DTO
//!   surface). A call-signature-only `defineProps<{ (): void }>()` has NO
//!   property members, so its DTO `props` is empty and the published
//!   `define_props` shape is EMPTY — no prop is yielded.
//! - **Emits**: event payload semantics are preserved from the DTO emit
//!   normalization (`AnalyzedEmitField.payload_expr`), which already applied the
//!   carrier-aware conditional path + the leading-event-name strip; the
//!   flat `evaluated_types.emits` field supplies the projected type when present.
//! - **Slots**: one slot function entry per DTO slot; the per-slot bindings are
//!   published separately by `resolve_slot_bindings_graph_native`.

use std::sync::Arc;

use verter_semantic::analysis::component_meta::MacroExpansionDiagnostics;
use verter_semantic::analysis::type_expand::{
    ExpandedComponentTypes, ExpandedMacroObjectShape, ExpandedMacroProps, ExpandedObjectShape,
    ExpandedProperty, ExpansionExecutionStatus, ExpansionResult,
};
use verter_semantic::analysis::type_solver::result::{ExecutionStatus, SolverExactness};
use verter_semantic::analysis::AnalyzedMacroKind;

use crate::resolver_core::ResolverContext;
use crate::types::FileAnalysisSnapshot;

/// Top-level driver: publish the `define_props` / `define_emits` /
/// `define_slots` shapes for every type-based macro in `snapshot`.
///
/// Runs after [`super::project_evaluated_types`]; reads the per-macro DTOs
/// through the active context and draws member types from the already-projected
/// flat fields. `diag_sink` is reserved for macro-expansion diagnostics; the
/// shape conversions below are pure (the diagnostics surface upstream at the
/// projector / DTO resolution layer).
pub(crate) fn project_define_macro_shapes(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    snapshot: &FileAnalysisSnapshot,
    evaluated_types: &mut ExpandedComponentTypes,
    _diag_sink: &mut [MacroExpansionDiagnostics],
    purpose: crate::resolver_core::ComponentMetaResolutionPurpose,
) {
    use crate::resolver_core::ComponentMetaResolutionPurpose;
    let ctx = query_engine.ctx;

    for (macro_index, mac) in snapshot.macros.iter().enumerate() {
        if !mac.is_type_based {
            continue;
        }
        // Fallthrough resolution needs only the inherited-attribute surface
        // (props/events that participate in root-attribute fallthrough). The
        // heavy `define_props` / `define_slots` SHAPE expansion is irrelevant to
        // the inheritance resolver and is skipped — slot BINDING identification
        // (load-bearing for inheritance) still runs in
        // `resolve_slot_bindings_graph_native`, which the orchestrator invokes
        // for both purposes. `define_emits` is preserved (event fallthrough).
        let skip_for_fallthrough = purpose == ComponentMetaResolutionPurpose::Fallthrough
            && matches!(
                mac.kind,
                AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::DefineSlots
            );
        if skip_for_fallthrough {
            continue;
        }
        match mac.kind {
            AnalyzedMacroKind::DefineProps => {
                if let Some(result) =
                    define_props_shape(ctx, owner_canonical, macro_index, evaluated_types)
                {
                    evaluated_types.define_props.push(ExpandedMacroProps {
                        macro_index,
                        result,
                    });
                }
            }
            AnalyzedMacroKind::DefineEmits => {
                if let Some(result) =
                    define_emits_shape(ctx, owner_canonical, macro_index, evaluated_types)
                {
                    evaluated_types.define_emits.push(ExpandedMacroObjectShape {
                        macro_index,
                        result,
                    });
                }
            }
            AnalyzedMacroKind::DefineSlots => {
                if let Some(result) = define_slots_shape(ctx, owner_canonical, macro_index) {
                    evaluated_types.define_slots.push(ExpandedMacroObjectShape {
                        macro_index,
                        result,
                    });
                }
            }
            // `WithDefaults` is never `is_type_based` (the type parameter lives
            // on the inner `defineProps`); `DefineModel` / `DefineExpose` /
            // `DefineOptions` do not contribute a `define_*` object shape.
            _ => {}
        }
    }
}

/// Build the `define_props` shape: one [`ExpandedProperty`] per DTO prop name,
/// typed from the matching already-projected `evaluated_types.props` field (the
/// shared-dispatch result) and falling back to the DTO field's own lowered
/// `type_expr` for any prop the flat projection did not surface.
///
/// Returns `None` when the macro's type-argument surface does NOT resolve at
/// all (a genuinely unresolved / missing macro — see [`macro_surface_resolves`]:
/// SFC not loaded, macro index out of range, type argument that does not project
/// to an object surface). A RESOLVED surface with no property members (a
/// call-signature-only `defineProps<{ (): void }>()`) returns `Some(empty
/// shape)` — present but with no properties — so the consumer observes "the
/// macro resolved to no props" rather than "no macro". This distinguishes
/// resolved-but-empty from unresolved/missing.
fn define_props_shape(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    macro_index: usize,
    evaluated_types: &ExpandedComponentTypes,
) -> Option<ExpansionResult<ExpandedObjectShape>> {
    if !macro_surface_resolves(
        ctx,
        owner_canonical,
        macro_index,
        AnalyzedMacroKind::DefineProps,
    ) {
        return None;
    }
    let dtos = crate::typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx(
        ctx,
        &dto_request(owner_canonical, macro_index, AnalyzedMacroKind::DefineProps),
    );

    let mut exactness = SolverExactness::ExactConcrete;
    let mut execution_status = ExecutionStatus::Completed;
    let mut diagnostics = Vec::new();
    let mut properties = Vec::with_capacity(dtos.prop_fields().len());

    for prop in dtos.prop_fields() {
        if let Some(field) = evaluated_types
            .props
            .iter()
            .find(|field| field.name == prop.name)
        {
            // Projected type from the shared dispatch (authoritative).
            exactness = exactness.merge(field.exactness);
            execution_status = merge_execution_status(execution_status, field.execution_status);
            diagnostics.extend(field.diagnostics.clone());
            properties.push(ExpandedProperty {
                name: field.name.clone(),
                ty: field.r#type.clone(),
                optional: field.optional,
                readonly: false,
                // A macro-published prop has no class accessibility origin —
                // `Public` by construction.
                visibility: verter_type_expr::MemberVisibility::Public,
                declared_in_macro_type_arg: field.declared_in_macro_type_arg,
            });
        } else {
            // The flat projection did not surface this prop — fall back to the
            // DTO's own lowered type (Typed-IR-Only: `AnalyzedPropField.type_expr`
            // is the authoritative typed form; never reparse `type_annotation`).
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
                // A macro-published prop has no class accessibility origin —
                // `Public` by construction.
                visibility: verter_type_expr::MemberVisibility::Public,
                declared_in_macro_type_arg: prop.declared_in_macro_type_arg,
            });
        }
    }

    Some(ExpansionResult {
        value: ExpandedObjectShape {
            properties,
            // A props member is `properties + index signatures`: publish the
            // DTO's index signatures (`defineProps<{ [k: string]: string }>()`)
            // so an index-signature-only props surface is not dropped.
            index_signatures: dtos.prop_index_signatures().to_vec(),
            call_signatures: Vec::new(),
        },
        exactness,
        execution_status,
        diagnostics,
    })
}

/// Build the `define_emits` shape: one [`ExpandedProperty`] per DTO emit, typed
/// from the matching already-projected `evaluated_types.emits` field and
/// falling back to the DTO emit's payload-preserving `payload_expr` (the
/// carrier-aware emit normalization already applied the event-name strip).
fn define_emits_shape(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    macro_index: usize,
    evaluated_types: &ExpandedComponentTypes,
) -> Option<ExpansionResult<ExpandedObjectShape>> {
    // Unresolved emits macro → no shape (see `define_props_shape`). A resolved
    // emits surface with no events is `Some(empty)`.
    if !macro_surface_resolves(
        ctx,
        owner_canonical,
        macro_index,
        AnalyzedMacroKind::DefineEmits,
    ) {
        return None;
    }
    let dtos = crate::typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx(
        ctx,
        &dto_request(owner_canonical, macro_index, AnalyzedMacroKind::DefineEmits),
    );

    let mut exactness = SolverExactness::ExactConcrete;
    let mut execution_status = ExecutionStatus::Completed;
    let mut diagnostics = Vec::new();
    let mut properties = Vec::with_capacity(dtos.emit_fields().len());

    for emit in dtos.emit_fields() {
        if let Some(field) = evaluated_types
            .emits
            .iter()
            .find(|field| field.name == emit.name)
        {
            exactness = exactness.merge(field.exactness);
            execution_status = merge_execution_status(execution_status, field.execution_status);
            diagnostics.extend(field.diagnostics.clone());
            properties.push(ExpandedProperty {
                name: field.name.clone(),
                ty: field.r#type.clone(),
                optional: false,
                readonly: false,
                // A macro-published emit has no class accessibility origin —
                // `Public` by construction.
                visibility: verter_type_expr::MemberVisibility::Public,
                // Emit shape members do not carry own-body-vs-heritage
                // provenance (a props-axis concern); the producer type does not
                // encode it.
                declared_in_macro_type_arg: false,
            });
        } else {
            // Payload-preserving fallback (Typed-IR-Only: `payload_expr` is the
            // authoritative typed form; never reparse `payload_type`).
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
                // A macro-published emit has no class accessibility origin —
                // `Public` by construction.
                visibility: verter_type_expr::MemberVisibility::Public,
                declared_in_macro_type_arg: false,
            });
        }
    }

    Some(ExpansionResult {
        value: ExpandedObjectShape {
            properties,
            // An emits object is `events + index signatures`: publish the DTO's
            // emit index signatures (`defineEmits<{ [event: string]: [v: number]
            // }>()`) so an index-signature-only emits surface is not dropped (the
            // retired materialiser surfaced it).
            index_signatures: dtos.emit_index_signatures().to_vec(),
            call_signatures: Vec::new(),
        },
        exactness,
        execution_status,
        diagnostics,
    })
}

/// Build the `define_slots` shape: one [`ExpandedProperty`] per DTO slot, typed
/// as the slot's `(props: { ... }) => RT` function expression. Per-slot
/// bindings are published separately by `resolve_slot_bindings_graph_native`.
fn define_slots_shape(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    macro_index: usize,
) -> Option<ExpansionResult<ExpandedObjectShape>> {
    // Unresolved slots macro → no shape (see `define_props_shape`). A resolved
    // slots surface with no slot members is `Some(empty)`.
    if !macro_surface_resolves(
        ctx,
        owner_canonical,
        macro_index,
        AnalyzedMacroKind::DefineSlots,
    ) {
        return None;
    }
    let dtos = crate::typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx(
        ctx,
        &dto_request(owner_canonical, macro_index, AnalyzedMacroKind::DefineSlots),
    );

    let properties = dtos
        .slot_fields()
        .iter()
        .map(|slot| ExpandedProperty {
            name: slot.name.clone(),
            ty: slot_field_function_type_expr(slot),
            optional: !slot.is_required,
            readonly: false,
            // A macro-published slot has no class accessibility origin —
            // `Public` by construction.
            visibility: verter_type_expr::MemberVisibility::Public,
            declared_in_macro_type_arg: false,
        })
        .collect();

    Some(ExpansionResult::exact_symbolic(ExpandedObjectShape {
        properties,
        index_signatures: Vec::new(),
        call_signatures: Vec::new(),
    }))
}

/// Does the macro's type-argument surface RESOLVE under the active `ctx`?
///
/// The "resolved vs unresolved/missing" discriminator the `define_*_shape`
/// helpers use: `resolve_vue_macro_surface_with_ctx` returns `None` exactly for
/// the genuinely-unresolved cases (SFC not loaded, macro index out of range,
/// macro not type-based, or a type argument that does not project to an object
/// surface). A RESOLVED surface — even one with no members (a call-signature-
/// only `defineProps<{ (): void }>()`) — returns `Some(..)`, so the helper
/// publishes `Some(empty)` rather than conflating it with "no macro". Resolution
/// flows through `ctx`, and the underlying dispatch queries are memoised in the
/// shared `SemanticGraphStore`, so this shares the DTO path's reduction work.
fn macro_surface_resolves(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    macro_index: usize,
    macro_kind: AnalyzedMacroKind,
) -> bool {
    ctx.host_for_fact_tracer_install()
        .resolve_vue_macro_surface_with_ctx(
            ctx,
            &dto_request(owner_canonical, macro_index, macro_kind),
        )
        .is_some()
}

/// FullMetadata DTO request for `(owner, macro_index, kind)`.
fn dto_request(
    owner_canonical: &str,
    macro_index: usize,
    macro_kind: AnalyzedMacroKind,
) -> crate::typeinfo::types::VueMacroSurfaceRequest {
    crate::typeinfo::types::VueMacroSurfaceRequest {
        owner_canonical: Arc::from(owner_canonical),
        macro_index,
        macro_kind,
        // `root_identity` is a hint; `vue_macro_dtos_with_ctx` re-derives the
        // authoritative `whole_hash` from the ctx-resolved snapshot.
        root_identity: [0u8; 16],
        level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
    }
}

/// Construct the typed `(props: { ... }) => RT` function expression for a
/// resolved slot directly from the analyzer-populated typed sidecars
/// (`AnalyzedSlotFieldBinding.binding_expr` and `AnalyzedSlotField.return_expr`).
/// No source-text reparse. Empty-bindings slots produce `() => RT` (no `props`
/// parameter).
pub(crate) fn slot_field_function_type_expr(
    slot: &verter_semantic::analysis::AnalyzedSlotField,
) -> verter_type_expr::TypeExpr {
    use verter_type_expr::{
        FunctionExpr, FunctionParam, MemberSpans, ObjectExpr, ObjectMember, ObjectProperty,
        TypeExpr,
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
                // The analyzed binding tracks the NAME span only.
                ObjectMember::Property(ObjectProperty::with_spans_public(
                    binding.name.clone(),
                    ty,
                    false,
                    false,
                    MemberSpans::name_only(binding.span),
                ))
            })
            .collect();
        let props_object = TypeExpr::Object(Arc::new(ObjectExpr { properties }));
        // Synthetic `props` parameter wrapping the slot bindings.
        vec![FunctionParam::synthetic(
            Some("props".to_string()),
            props_object,
            false,
            false,
        )]
    };

    // Synthetic slot function wrapper `(props) => return`.
    TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
        parameters,
        Some(Arc::new(return_type)),
        Vec::new(),
    )))
}

/// Severity-ordered merge of two expansion execution statuses (the worse status
/// wins). Pure helper mirroring the retired materialiser's merge.
fn merge_execution_status(
    current: ExpansionExecutionStatus,
    next: ExpansionExecutionStatus,
) -> ExpansionExecutionStatus {
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
