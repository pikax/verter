//! `define_props` / `define_emits` / `define_slots` shape publication.
//!
//! This projector owns the macro-shape level of `ExpandedComponentTypes`
//! (`define_props` / `define_emits` / `define_slots`). It is the sole author
//! of those fields — the eager macro-object materialiser is retired.
//!
//! Each shape is built from the macro's normalized DTOs
//! ([`crate::typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx`] —
//! the SOLE props/emits/slots member authority, resolved through the active
//! `ResolverContext` so overlay sessions read overlay content), with NO solver
//! fallback, NO source reparse, and NO `eval_source` type-parameter
//! collection. The flat `evaluated_types.props` / `evaluated_types.emits`
//! fields that [`super::project_evaluated_types`] projected contribute ONLY
//! exactness / execution-status / diagnostics METADATA — never a semantic
//! source (the normalized rows own every published `SourcePosition`).
//!
//! `project_define_macro_shapes` runs AFTER `project_evaluated_types` (so
//! the flat metadata fields exist to fold in) and BEFORE
//! `resolve_slot_bindings_graph_native` (which consumes the slot shape).
//!
//! Per-kind rules:
//! - **Props**: properties only (a member is a property name on the DTO
//!   surface), each typed from the normalized prop row's published
//!   member-value SOURCE
//!   ([`crate::typeinfo::framework_surface::results::ResolvedPropField::type_source`]
//!   — the prop-type AUTHORITY). A call-signature-only
//!   `defineProps<{ (): void }>()` has NO property members, so its DTO
//!   `props` is empty and the published `define_props` shape is EMPTY — no
//!   prop is yielded.
//! - **Emits**: the normalized emit row's published payload SOURCE
//!   ([`crate::typeinfo::framework_surface::results::ResolvedEmitField::payload_source`])
//!   is the payload AUTHORITY — the normalization already applied the
//!   carrier-aware conditional path, the leading-event-name strip, and the
//!   closed-tuple / member-path / callable-params source split.
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
        // Fallthrough resolution needs the inherited-attribute surface
        // (props/events that participate in root-attribute fallthrough). The
        // `define_props` shape is LOAD-BEARING for it: the branch prop rows
        // carry the finalized published SOURCE positions the props lane owns
        // (two children's identical closed prop types must publish the
        // identical closed source value so the output memo shares one
        // entry). The `define_slots` SHAPE expansion stays irrelevant to the
        // inheritance resolver and is skipped — slot BINDING identification
        // (load-bearing for inheritance) still runs in
        // `resolve_slot_bindings_graph_native`, which the orchestrator
        // invokes for both purposes. `define_emits` is preserved (event
        // fallthrough).
        let skip_for_fallthrough = purpose == ComponentMetaResolutionPurpose::Fallthrough
            && mac.kind == AnalyzedMacroKind::DefineSlots;
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

/// Build the `define_props` shape: one [`ExpandedProperty`] per DTO prop row,
/// typed from the normalized row's published member-value SOURCE
/// ([`crate::typeinfo::framework_surface::results::ResolvedPropField::type_source`])
/// — the prop-type AUTHORITY. The normalization already applied the
/// authored-candidate proof, the closed/ref upgrades, and the projected
/// member-path replay split; a genuine miss arrives as the typed
/// `Failed(UnrepresentableRequiredMemberValue)` position.
///
/// The flat `evaluated_types.props` field — when one matched by name —
/// contributes ONLY exactness / execution-status / diagnostics metadata,
/// NEVER the member-value source: preferring its `r#type` over the
/// normalized row shadowed the session-resolved member source (the same
/// competing-producer class the emit-authority rule closed).
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
    let dtos_read = crate::typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx(
        ctx,
        &dto_request(owner_canonical, macro_index, AnalyzedMacroKind::DefineProps),
    );
    // Fold a genuine partial macro surface into the request-result
    // completeness so the enclosing component-meta result is refused warm
    // promotion (the no-poison invariant).
    dtos_read.observe_partial();
    let dtos = dtos_read.dtos;

    let mut exactness = SolverExactness::ExactConcrete;
    let mut execution_status = ExecutionStatus::Completed;
    let mut diagnostics = Vec::new();
    let mut properties = Vec::with_capacity(dtos.prop_fields().len());

    for prop in dtos.prop_fields() {
        // Metadata-only merge from the flat evaluated field: exactness /
        // execution status / diagnostics fold in, the member-value source
        // does NOT — the normalized row's `type_source` is authoritative.
        if let Some(field) = evaluated_types
            .props
            .iter()
            .find(|field| field.name == prop.analysis.name)
        {
            exactness = exactness.merge(field.exactness);
            execution_status = merge_execution_status(execution_status, field.execution_status);
            diagnostics.extend(field.diagnostics.clone());
        }
        properties.push(ExpandedProperty {
            name: prop.analysis.name.clone(),
            ty: prop.type_source.clone(),
            optional: prop.analysis.is_optional,
            readonly: false,
            // A macro-published prop has no class accessibility origin —
            // `Public` by construction.
            visibility: verter_type_expr::MemberVisibility::Public,
            declared_in_macro_type_arg: prop.analysis.declared_in_macro_type_arg,
        });
    }

    // A props member is `properties + index signatures`: publish the
    // DTO's index signatures (`defineProps<{ [k: string]: string }>()`)
    // so an index-signature-only props surface is not dropped.
    let index_signatures = dtos.prop_index_signatures().to_vec();
    fail_shape_result_on_failed_member(
        &properties,
        &index_signatures,
        &mut exactness,
        &mut execution_status,
    );
    Some(ExpansionResult {
        value: ExpandedObjectShape {
            properties,
            index_signatures,
            call_signatures: Vec::new(),
        },
        exactness,
        execution_status,
        diagnostics,
    })
}

/// Build the `define_emits` shape: one [`ExpandedProperty`] per DTO emit,
/// typed from the normalized emit row's published payload SOURCE — the
/// payload AUTHORITY. The row carries the authored macro-payload position
/// for a proven local authored property event, the closed payload tuple /
/// leaf-union for a closed-expressible payload, the projected member-path
/// or CALLABLE-PARAMS replay route for an inherited / merged / richer
/// payload (Typed-IR-Only: never reparse `payload_type`), or the typed
/// source-construction FAILURE (a realized emit's payload-tuple position is
/// REQUIRED — an unrepresentable payload fails output materialization
/// instead of rendering a fabricated `unknown` success).
///
/// The flat `evaluated_types.emits` field — when one matched by name —
/// contributes ONLY exactness / execution-status / diagnostics metadata,
/// NEVER the payload source: preferring its member-residue `r#type` over
/// the normalized row shadowed the faithful session-resolved payload (an
/// imported `save: [id: number]` published the residue instead of its real
/// closed tuple).
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
    let dtos_read = crate::typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx(
        ctx,
        &dto_request(owner_canonical, macro_index, AnalyzedMacroKind::DefineEmits),
    );
    dtos_read.observe_partial();
    let dtos = dtos_read.dtos;

    let mut exactness = SolverExactness::ExactConcrete;
    let mut execution_status = ExecutionStatus::Completed;
    let mut diagnostics = Vec::new();
    let mut properties = Vec::with_capacity(dtos.emit_fields().len());

    for emit in dtos.emit_fields() {
        // Metadata-only merge from the flat evaluated field: exactness /
        // execution status / diagnostics fold in, the payload source does
        // NOT — the normalized row's `payload_source` is authoritative.
        if let Some(field) = evaluated_types
            .emits
            .iter()
            .find(|field| field.name == emit.analysis.name)
        {
            exactness = exactness.merge(field.exactness);
            execution_status = merge_execution_status(execution_status, field.execution_status);
            diagnostics.extend(field.diagnostics.clone());
        }
        properties.push(ExpandedProperty {
            name: emit.analysis.name.clone(),
            ty: emit.payload_source.clone(),
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
    }

    // An emits object is `events + index signatures`: publish the DTO's
    // emit index signatures (`defineEmits<{ [event: string]: [v: number]
    // }>()`) so an index-signature-only emits surface is not dropped (the
    // retired materialiser surfaced it).
    let index_signatures = dtos.emit_index_signatures().to_vec();
    fail_shape_result_on_failed_member(
        &properties,
        &index_signatures,
        &mut exactness,
        &mut execution_status,
    );
    Some(ExpansionResult {
        value: ExpandedObjectShape {
            properties,
            index_signatures,
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
    let dtos_read = crate::typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx(
        ctx,
        &dto_request(owner_canonical, macro_index, AnalyzedMacroKind::DefineSlots),
    );
    dtos_read.observe_partial();
    let dtos = dtos_read.dtos;

    let properties = dtos
        .slot_fields()
        .iter()
        .map(|slot| ExpandedProperty {
            name: slot.name.clone(),
            ty: verter_type_expr::facts::SourcePosition::Present(slot_field_function_source(slot)),
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

/// The published SOURCE for a resolved slot's `(props: { ... }) => RT`
/// callable shape: the slot's authored payload position when the resolver /
/// analyzer stamped one (`AnalyzedSlotField.payload` — the demand side
/// re-raises it through the one shared dispatch), else the closed FUNCTION
/// fact shape (`SemanticTypeSource::Closed(Function)`) whose parameter /
/// return positions are typed misses recovered on demand through the
/// graph-native slot-binding walk (typed binding demand is host-raised — the
/// flat payload vocabulary cannot address the nested positions). Raising the
/// closed fact through the shared bridge interns the
/// `SemanticNodeData::Function` carrier — node synthesis is demand-driven at
/// the consuming dispatch, never eager here. No source-text reparse.
pub(crate) fn slot_field_function_source(
    slot: &verter_semantic::analysis::AnalyzedSlotField,
) -> verter_type_expr::facts::SemanticTypeSource {
    use verter_type_expr::facts::{
        ClosedTypeFact, FunctionParamFact, FunctionSignatureFact, SemanticTypeSource,
    };
    use verter_type_expr::span_origins::{
        FunctionParamSpanOrigin, FunctionSpansOrigin, SourceSynthetic,
    };

    if let Some(payload) = slot.payload.clone() {
        return SemanticTypeSource::Authored(
            verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(payload),
        );
    }

    // Synthetic `props` parameter wrapping the slot bindings; empty-bindings
    // slots produce `() => RT`.
    let parameters: Vec<FunctionParamFact> = if slot.bindings.is_empty() {
        Vec::new()
    } else {
        vec![FunctionParamFact {
            name: Some("props".to_string()),
            optional: false,
            rest: false,
            has_ts_annotation: false,
            // The synthesized props object has no single authored slot — the
            // typed miss; the bindings' typed values are host-raised through
            // the graph-native slot-binding walk.
            ty: None,
            span_origin: FunctionParamSpanOrigin {
                function: FunctionSpansOrigin::Synthetic(SourceSynthetic),
                param: verter_type_expr::span_origins::FunctionParamSelector::Positional {
                    ordinal: 0,
                },
            },
        }]
    };
    SemanticTypeSource::Closed(ClosedTypeFact::Function(FunctionSignatureFact {
        type_parameters: std::sync::Arc::from(Vec::new().into_boxed_slice()),
        parameters: std::sync::Arc::from(parameters.into_boxed_slice()),
        // The return position has no addressable authored slot on a
        // payload-less slot — the typed miss, recovered on demand.
        return_ty: None,
        has_implementation_body: false,
        spans_origin: FunctionSpansOrigin::Synthetic(SourceSynthetic),
    }))
}

/// A macro shape carrying a FAILED required member position must not be
/// labeled a complete/exact result: downgrade the shape's exactness to
/// `Incomplete`, its execution status to the deterministic `HardStop`, and
/// mark the enclosing request's completeness partial through the existing
/// suppression rail — a failed required position is never cached or reported
/// as a complete success (output materialization then fails it with the
/// typed `RequiredSourceUnavailable` error).
fn fail_shape_result_on_failed_member(
    properties: &[ExpandedProperty],
    index_signatures: &[verter_semantic::analysis::type_expand::ExpandedIndexSignature],
    exactness: &mut SolverExactness,
    execution_status: &mut ExecutionStatus,
) {
    let failed_property = properties.iter().any(|property| property.ty.is_failed());
    // A REQUIRED index-signature key/value position whose producer could
    // not construct a faithful source is the same typed failure as a failed
    // member: the shape result is non-complete and never a fabricated
    // `unknown` success.
    let failed_index = index_signatures
        .iter()
        .any(|signature| signature.key_type.is_failed() || signature.value_type.is_failed());
    if !failed_property && !failed_index {
        return;
    }
    *exactness = SolverExactness::Incomplete;
    *execution_status = merge_execution_status(*execution_status, ExecutionStatus::HardStop);
    crate::request_context::mark_request_result_partial();
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

#[cfg(test)]
#[path = "define_shapes_tests.rs"]
mod tests;
