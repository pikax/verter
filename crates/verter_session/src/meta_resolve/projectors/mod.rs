//! Per-macro projectors for component-meta extraction.
//!
//! Each projector resolves a single macro's surface members through
//! the shared dispatch primitives (`SemanticQueryKey::ResolveMacroPayload`
//! followed by an empty-path `ProjectPath` in `Shallow` mode), raises
//! members to `TypeExpr`, and produces `Vec<ExpandedField>` (or the
//! macro-specific equivalent).
//!
//! Authority chain:
//!
//! 1. `dispatch.lower_type_expr_in_scope_with_mode(file, parsed_arg, Navigate)`
//!    lowers the parsed type argument to a `SemanticNodeId` so the
//!    dispatch can resolve the macro payload.
//! 2. `dispatch.execute_read(SemanticQueryKey::ResolveMacroPayload { .. })`
//!    yields the macro payload's semantic node (the resolved type that
//!    backs the macro instance).
//! 3. `dispatch.execute_read(SemanticQueryKey::ProjectPath { base, path: [], mode: Shallow })`
//!    enumerates the payload's surface members.
//! 4. For each surface member, the projector raises the member's value
//!    node back to `TypeExpr` and classifies its exactness via
//!    `meta_resolve::exactness::classify_node`.
//!
//! All `dispatch.execute_read` calls must accumulate dep-signature via
//! `accumulate_dispatch_dep_signature` so the final-result cache can
//! revalidate on warm hits. Cycle and error branches publish a
//! `MacroExpansionDiagnostics` envelope into `diag_sink` (per §7.5
//! silent-miss prevention).
//!
//! The macro_index inside each projector identifies which macro this
//! projection corresponds to, for diagnostic correlation and for the
//! shape-merge logic in the parser-side analysis.

use std::sync::Arc;

use verter_semantic::analysis::component_meta::{MacroExpansionDiagnostics, MacroExpansionKind};
use verter_semantic::analysis::type_expand::{
    ExpandedField, ExpansionDiagnostic, ExpansionExactness, ExpansionExecutionStatus,
    ExpansionStopReason,
};
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::{AnalyzedMacro, AnalyzedMacroKind};

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{
    DeclIdentity, PathSegment, ProjectionMode, QueryResult, SemanticNodeData, SemanticNodeId,
    SemanticQueryKey, SurfaceMember,
};
use crate::types::FileAnalysisSnapshot;

use super::dep_signature::accumulate_dispatch_dep_signature;
use super::diagnostic_convert::shallow_diagnostics_to_macro_expansion;
use super::exactness::classify_node;

pub(crate) mod emits;
pub(crate) mod exposed;
pub(crate) mod model;
pub(crate) mod options;
pub(crate) mod props;
pub(crate) mod slots;

pub(crate) use emits::project_emits;
pub(crate) use exposed::project_exposed;
pub(crate) use model::project_model;
pub(crate) use options::project_options;
pub(crate) use props::project_props;
pub(crate) use slots::project_slots;

/// Merge a projector's `Vec<ExpandedField>` output into the target
/// `Vec<ExpandedField>` on `evaluated_types`.
///
/// For each projector field, looks up the target by `name`:
/// - If a matching name exists, the projector field REPLACES the
///   existing entry. The projector is the §7.1 authoritative source
///   for type-based macro surfaces — its dispatch-resolved type +
///   exactness supersedes the parser-side per-field annotation that
///   `expand_macro_types` published.
/// - If no matching name exists, the projector field is appended.
///
/// This keeps any parser-side fields the projector did NOT produce
/// (e.g., entries from prop annotations that the dispatch path did
/// not surface).
fn merge_projected_fields_by_name(
    target: &mut Vec<verter_semantic::analysis::type_expand::ExpandedField>,
    projected: Vec<verter_semantic::analysis::type_expand::ExpandedField>,
) {
    for field in projected {
        if let Some(existing) = target.iter_mut().find(|t| t.name == field.name) {
            *existing = field;
        } else {
            target.push(field);
        }
    }
}

/// Top-level driver that dispatches every type-based macro in the
/// snapshot through its per-kind projector and writes the resulting
/// fields into `evaluated_types`.
///
/// This is the §7.1 replacement for the legacy
/// `walk_component_meta_macro_shape_member_types` + per-field
/// `materialize_component_meta_field_types` enrichment pipeline. The
/// driver:
///
/// 1. For each `defineProps<T>`, calls [`project_props`] and extends
///    `evaluated_types.props` with the resulting fields.
/// 2. For each `defineEmits<T>`, calls [`project_emits`] and extends
///    `evaluated_types.emits`.
/// 3. For each `defineSlots<T>`, calls [`project_slots`]; the slot
///    fields are not directly published into `evaluated_types`
///    because the slot-shape level is consumed by
///    [`crate::meta_resolve::slot_binding_graph::resolve_slot_bindings_graph_native`]
///    via the same dispatch primitives. The diagnostic sink is the
///    only side-channel the projector contributes for slots.
/// 4. `defineModel`, `defineExpose`, `defineOptions` macros run their
///    projectors, but their downstream merge into the analysis lives
///    on the parser side; their projector results are only inspected
///    for diagnostic sink contributions.
///
/// Per §7.5 silent-miss prevention, every `Recursive` / `Error`
/// branch the projectors hit is appended to `diag_sink`, which the
/// caller merges into `analysis.macro_expansion_diagnostics`.
pub(crate) fn project_evaluated_types(
    dispatch: &ProjectSemanticDispatch<'_>,
    ctx: &dyn ResolverContext,
    file: &str,
    snapshot: &FileAnalysisSnapshot,
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) {
    let owner = build_owner_decl_identity(ctx, file);

    for (macro_index, mac) in snapshot.macros.iter().enumerate() {
        match mac.kind {
            AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults => {
                let fields = project_props(
                    dispatch,
                    ctx,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                );
                merge_projected_fields_by_name(&mut evaluated_types.props, fields);
            }
            AnalyzedMacroKind::DefineEmits => {
                let fields = project_emits(
                    dispatch,
                    ctx,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                );
                merge_projected_fields_by_name(&mut evaluated_types.emits, fields);
            }
            AnalyzedMacroKind::DefineSlots => {
                // Slot-shape projection is consumed by the
                // slot-binding-graph synthesis layer (Phase 1) which
                // shares the same dispatch primitives; running the
                // projector here populates the diagnostic stream and
                // primes the dispatch family memo.
                let _ = project_slots(
                    dispatch,
                    ctx,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                );
            }
            AnalyzedMacroKind::DefineModel => {
                // The parser-side analysis owns the
                // `synthesize_model_prop_and_event` that publishes
                // the prop+event pair. Running the projector here
                // populates the diagnostic stream and primes the
                // dispatch family memo so the parser-side merge can
                // observe the resolved type without a second
                // resolution pass.
                let _ = project_model(
                    dispatch,
                    ctx,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                );
            }
            AnalyzedMacroKind::DefineExpose => {
                let _ = project_exposed(
                    dispatch,
                    ctx,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                );
            }
            AnalyzedMacroKind::DefineOptions => {
                let _ = project_options(
                    dispatch,
                    ctx,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                );
            }
        }
    }
}

/// Identifier name used as the synthetic decl-name for a `<script setup>`
/// scope when the dispatch's `DeclIdentity` only consults the canonical
/// id + whole hash for cache keying. Mirrors the constant from
/// [`super::slot_binding_graph`] so projector code paths share the
/// same decl identity for `ResolveMacroPayload` cache keys.
pub(crate) const SFC_SCRIPT_SETUP_DECL_NAME: &str = "<sfc-script-setup>";

/// Build the owner [`DeclIdentity`] for an SFC's macro queries.
///
/// Mirrors `slot_binding_graph::build_owner_decl_identity` so the
/// dispatch keys produced by projectors collide with the existing
/// graph-native synthesis layer (path-independent caching per
/// `CLAUDE.md` Build Philosophy).
pub(crate) fn build_owner_decl_identity(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
) -> DeclIdentity {
    let whole_hash = ctx
        .shallow_file_state(owner_canonical)
        .map(|s| s.whole_hash)
        .unwrap_or_default();
    DeclIdentity {
        canonical_id: Arc::from(owner_canonical),
        whole_hash,
        decl_name: Arc::from(SFC_SCRIPT_SETUP_DECL_NAME),
    }
}

/// Convert an `Arc<[PathSegment]>` empty path constant. Cached locally
/// so each projector call reuses a shared-ref-counted empty path.
#[inline]
pub(crate) fn empty_path() -> Arc<[PathSegment]> {
    Arc::from(Vec::<PathSegment>::new().into_boxed_slice())
}

/// Read the [`SurfaceView`] members backing `node`, if `node` resolves
/// to a `SemanticNodeData::Object` shell. Empty for any other variant
/// — callers treat the empty surface as "no enumerable members".
///
/// Mirrors `slot_binding_graph::read_surface_members`.
pub(crate) fn read_surface_members(
    ctx: &dyn ResolverContext,
    surface_node: SemanticNodeId,
) -> Vec<SurfaceMember> {
    match crate::project_semantic_dispatch::node_data_for(ctx, surface_node).as_deref() {
        Some(SemanticNodeData::Object(view)) => view.members.iter().cloned().collect(),
        _ => Vec::new(),
    }
}

/// Build a [`MacroExpansionDiagnostics`] for a `QueryError` encountered
/// during projection. Mirrors `slot_binding_graph::macro_expansion_for_query_error`.
pub(crate) fn macro_expansion_for_query_error(
    macro_index: usize,
    macro_kind: MacroExpansionKind,
    context: String,
) -> MacroExpansionDiagnostics {
    MacroExpansionDiagnostics {
        macro_kind,
        macro_index,
        diagnostics: vec![ExpansionDiagnostic {
            reason: ExpansionStopReason::InstantiationError,
            context,
            property_name: None,
        }],
        exactness: ExpansionExactness::Incomplete,
        execution_status: ExpansionExecutionStatus::Interrupted,
    }
}

/// Build a [`MacroExpansionDiagnostics`] for a `Recursive` back-edge
/// encountered during projection. Mirrors
/// `slot_binding_graph::macro_expansion_for_cycle`. Cycles are not
/// fatal — they bound the published surface to the non-recursive arms.
pub(crate) fn macro_expansion_for_cycle(
    macro_index: usize,
    macro_kind: MacroExpansionKind,
    context: String,
) -> MacroExpansionDiagnostics {
    MacroExpansionDiagnostics {
        macro_kind,
        macro_index,
        diagnostics: vec![ExpansionDiagnostic {
            reason: ExpansionStopReason::CyclicReference,
            context,
            property_name: None,
        }],
        exactness: ExpansionExactness::Incomplete,
        execution_status: ExpansionExecutionStatus::Completed,
    }
}

/// Resolve a type-based macro's payload through `ResolveMacroPayload`.
///
/// Lowers the macro's `parsed_type_argument` to a [`SemanticNodeId`] in
/// `Navigate` mode, then dispatches `ResolveMacroPayload` and returns
/// the macro payload node on success.
///
/// On `Recursive` or `Error`, appends a diagnostic to `diag_sink` and
/// returns `None`. Dep-signature is accumulated unconditionally.
///
/// Silent-miss prevention (§7.5): when lowering itself fails (the
/// type expression cannot be lowered to a `SemanticNodeId` — e.g. an
/// unresolved import to a non-existent module), a diagnostic is
/// pushed before returning `None`.
pub(crate) fn resolve_macro_payload(
    dispatch: &ProjectSemanticDispatch<'_>,
    owner: &DeclIdentity,
    file: &str,
    macro_index: usize,
    mac: &AnalyzedMacro,
    macro_kind: AnalyzedMacroKind,
    expansion_kind: MacroExpansionKind,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) -> Option<SemanticNodeId> {
    let parsed_arg = mac.parsed_type_argument.as_ref()?;
    let type_args: Arc<[SemanticNodeId]> = match dispatch.lower_type_expr_in_scope_with_mode(
        file,
        parsed_arg,
        ProjectionMode::Navigate,
    ) {
        Some(node) => Arc::from(vec![node].into_boxed_slice()),
        None => {
            diag_sink.push(macro_expansion_for_query_error(
                macro_index,
                expansion_kind,
                format!("macro-payload-lowering-failed@{:?}", macro_kind),
            ));
            return None;
        }
    };

    let payload_read = dispatch.execute_read(SemanticQueryKey::ResolveMacroPayload {
        owner: owner.clone(),
        macro_index,
        macro_kind,
        type_args,
        mode: ProjectionMode::Navigate,
    });
    accumulate_dispatch_dep_signature(&payload_read.dep_signature);
    if !payload_read.walker_diagnostics.is_empty() {
        diag_sink.push(shallow_diagnostics_to_macro_expansion(
            &payload_read.walker_diagnostics,
            macro_index,
            expansion_kind.clone(),
            payload_read.cache_suppress,
        ));
    }

    let payload_node = match payload_read.value {
        QueryResult::Value(id) => id,
        QueryResult::Recursive(_) => {
            diag_sink.push(macro_expansion_for_cycle(
                macro_index,
                expansion_kind,
                format!("cyclic-macro-payload@{:?}", macro_kind),
            ));
            return None;
        }
        QueryResult::Error(e) => {
            diag_sink.push(macro_expansion_for_query_error(
                macro_index,
                expansion_kind,
                format!("macro-payload-error::{:?}", e),
            ));
            return None;
        }
    };

    // Silent-miss prevention (§7.5): when the dispatch returns an
    // opaque-as-value sentinel (i.e. the resolution stuck at an
    // unresolved declaration / cycle inside Navigate mode), publish
    // a diagnostic before bailing. Without this, callers see an
    // empty surface that's indistinguishable from a successful
    // empty payload.
    if let Some(SemanticNodeData::Opaque(err)) =
        crate::project_semantic_dispatch::node_data_for(dispatch.ctx, payload_node).as_deref()
    {
        diag_sink.push(macro_expansion_for_query_error(
            macro_index,
            expansion_kind,
            format!("macro-payload-opaque::{:?}", err),
        ));
        return None;
    }

    Some(payload_node)
}

/// Resolve the empty-path `ProjectPath` surface for a payload node.
///
/// Returns the surface node on success. On `Recursive` or `Error`,
/// appends a diagnostic to `diag_sink` and returns `None`.
pub(crate) fn resolve_payload_surface(
    dispatch: &ProjectSemanticDispatch<'_>,
    payload_node: SemanticNodeId,
    macro_index: usize,
    expansion_kind: MacroExpansionKind,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) -> Option<SemanticNodeId> {
    let surface_read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base: payload_node,
        path: empty_path(),
        mode: ProjectionMode::Shallow,
    });
    accumulate_dispatch_dep_signature(&surface_read.dep_signature);
    if !surface_read.walker_diagnostics.is_empty() {
        diag_sink.push(shallow_diagnostics_to_macro_expansion(
            &surface_read.walker_diagnostics,
            macro_index,
            expansion_kind.clone(),
            surface_read.cache_suppress,
        ));
    }
    match surface_read.value {
        QueryResult::Value(id) => Some(id),
        QueryResult::Recursive(_) => {
            diag_sink.push(macro_expansion_for_cycle(
                macro_index,
                expansion_kind,
                "cyclic-macro-payload-surface".to_string(),
            ));
            None
        }
        QueryResult::Error(e) => {
            diag_sink.push(macro_expansion_for_query_error(
                macro_index,
                expansion_kind,
                format!("macro-payload-surface-error::{:?}", e),
            ));
            None
        }
    }
}

/// Build an [`ExpandedField`] for a single surface member.
///
/// Raises the member's value node back to a [`TypeExpr`] (falling back
/// to `TypeExpr::Unknown` if raise fails — mirrors §7.2 contract) and
/// classifies its exactness through the shared
/// [`classify_node`] predicate.
///
/// `raw_type` is taken from the parser's `analyzed_prop.type_annotation`
/// when available. The caller passes `None` when no analyzed prop
/// matches the surface member's name.
///
/// Before classifying, the member's value is resolved through one
/// additional `ProjectPath { mode: Shallow }` so that `DeclRef`
/// carriers (the terminal Navigate-mode form for unparameterised
/// type aliases) collapse to their underlying primitive / object /
/// function shape. Without this hop, `defineProps<{ msg: MyStr }>`
/// where `type MyStr = string` would publish `msg` as
/// `ExactSymbolic` because the surface member's value points at the
/// `DeclRef` for `MyStr`'s declaration, not at `Primitive(String)`
/// directly.
pub(crate) fn surface_member_to_expanded_field(
    dispatch: &ProjectSemanticDispatch<'_>,
    member: &SurfaceMember,
    raw_type: Option<String>,
) -> ExpandedField {
    let resolved_value = resolve_member_value_for_classification(dispatch, member.value);
    // Type raise uses the original member.value so DeclRef carriers
    // for parameterised aliases / package-backed types stay symbolic
    // in the published TypeExpr (matches the legacy walker's
    // `materialize_component_meta_macro_shape_member_type_expr`
    // contract for symbolic-route preservation). Exactness uses the
    // resolved body so unparameterised primitive aliases collapse to
    // `ExactConcrete`.
    ExpandedField {
        name: member.name.as_ref().to_string(),
        r#type: dispatch
            .raise_node_to_type_expr(member.value)
            .unwrap_or_else(|| TypeExpr::Unknown { raw: String::new() }),
        raw_type,
        optional: member.optional,
        exactness: classify_node(dispatch, resolved_value),
        execution_status: ExpansionExecutionStatus::Completed,
        diagnostics: Vec::new(),
    }
}

/// Resolve a surface member's value to its underlying body for
/// exactness classification. For `DeclRef` carriers (e.g. an
/// unparameterised type alias `MyStr` referenced from a property
/// signature), dispatches `ProjectPath { base: value, path: [],
/// mode: Shallow }` which expands the `DeclRef` to its body. For
/// other variants the value is returned unchanged — `classify_node`
/// already alias-unwraps a single `Alias` hop.
///
/// Dep-signature is accumulated unconditionally so the final-result
/// cache observes the same revalidation surface as the projector's
/// other dispatches.
fn resolve_member_value_for_classification(
    dispatch: &ProjectSemanticDispatch<'_>,
    value: SemanticNodeId,
) -> SemanticNodeId {
    match crate::project_semantic_dispatch::node_data_for(dispatch.ctx, value).as_deref() {
        Some(SemanticNodeData::DeclRef { .. })
        | Some(SemanticNodeData::InstantiationRef { .. }) => {
            let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
                base: value,
                path: empty_path(),
                mode: ProjectionMode::Shallow,
            });
            accumulate_dispatch_dep_signature(&read.dep_signature);
            match read.value {
                QueryResult::Value(id) => id,
                _ => value,
            }
        }
        _ => value,
    }
}
