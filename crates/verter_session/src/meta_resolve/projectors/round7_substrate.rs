//! Block 6.i round 7 substrate primitives — codex 4th consult Q2 /
//! Q3 / Q4 building blocks for the Selected-Key Transit Realization
//! + Boundary Diagnostics architecture.
//!
//! Split out of [`super::mod`] to keep the projector entry-point
//! file under the architecture-guard line cap (`no_oversize_files`).
//! The substrate primitives are re-exported from
//! [`super`](super) so call sites continue to import from
//! `crate::meta_resolve::projectors::*` unchanged.
//!
//! Primitives in this module:
//!
//! - [`resolve_macro_payload_diagnostic_probe`] — boundary probe
//!   that restores silent-miss diagnostics under transit-shallow
//!   macro publication (codex Q2).
//! - [`PayloadSurfaceScope`] + [`resolve_payload_surface_with_scope`]
//!   — scope-gated payload surface resolver with branch-merged
//!   shallow semantics for emit-class macro object payloads (codex
//!   Q3 / Q4).
//! - [`MemberValueRole`] — disambiguates Field vs CallableSlot
//!   handling for the surface-member projection pipeline (codex Q4).
//!
//! All primitives carry `#[allow(dead_code)]` at the substrate
//! commit. The atomic 7-consumer cutover wires every callsite.

use std::sync::Arc;

use verter_semantic::analysis::component_meta::{MacroExpansionDiagnostics, MacroExpansionKind};

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    ProjectionMode, QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryKey, SurfaceMember,
};

use super::{empty_path, macro_expansion_for_cycle, macro_expansion_for_query_error};
use crate::meta_resolve::dep_signature::emit_dispatch_dep_signature_facts;
use crate::meta_resolve::diagnostic_convert::shallow_diagnostics_to_macro_expansion;

/// **Macro-payload boundary diagnostic probe** — Block 6.i round 7
/// codex 4th consult Q2 substrate.
///
/// The macro payload lowering migrates from `Published(Navigate)` to
/// `StructuralTransit(Navigate)` at the macro publication boundary
/// once the consumer-side cutover lands. Under transit demand the
/// publication boundary carrier-stops `KeyOf` / `Mapped` operators,
/// but an unresolved-import surface may also resolve to a `DeclRef`
/// carrier WITHOUT firing `walker_diagnostics` — silently passing
/// the missing decl through. The legacy `Published(Navigate)`
/// lowering surfaced the failure loudly
/// (`lower_type_expr_in_scope_with_mode` returned `None`); the
/// transit-shallow lowering does not.
///
/// The probe restores the silent-miss diagnostic contract by running
/// a **second**, non-value-publishing dispatch under publication
/// demand on the transit-shallow payload node. The probe's result is
/// DISCARDED at the value level (the macro publication still uses
/// the transit-shallow payload); only its diagnostics, dep-
/// signatures, and cache-suppress flag flow into the consumer-
/// visible `MacroExpansionDiagnostics` envelope.
///
/// Probe dispatch chain (codex Q2):
/// - The probe runs `ProjectPath { base: payload, [], Published(Shallow) }`
///   so the publication path tries to enumerate the payload's
///   surface members under publication demand. The `ProjectPath`
///   walker internally re-dispatches `DeclRef` → `ResolveDecl`,
///   `InstantiationRef` → `Instantiate { Published(Shallow) }`,
///   `Mapped` → `MappedType { Published(Shallow) }` etc. — so a
///   single `ProjectPath` probe exercises the full carrier-
///   resolution surface the codex spec enumerates ("DeclRef →
///   ResolveDecl; InstantiationRef → Instantiate {
///   Published(Navigate) }; payload surface candidates →
///   ProjectPath([], Published(Shallow))").
/// - `walker_diagnostics`, `cache_suppress`, and the dispatch's
///   `dep_signature` translate into `MacroExpansionDiagnostics`
///   envelopes via the existing
///   `shallow_diagnostics_to_macro_expansion` /
///   `macro_expansion_for_query_error` / `macro_expansion_for_cycle`
///   helpers — identical to the `Published(Navigate)`-era contract.
///
/// **Probe result MUST NOT replace the transit-shallow payload
/// value.** The probe is diagnostic-only; the macro publication's
/// downstream consumers continue reading the transit-shallow payload
/// node so the round-7 publication contract (carrier-stop / no
/// outputSchema-execute leak) holds.
///
/// `Opaque` carrier handling: the probe treats `Opaque(_)` payloads
/// (e.g. `Opaque(DeclPlaceholder)` for an unresolved import) as
/// failure-with-diagnostic per the existing `resolve_macro_payload`'s
/// post-lowering opaque check. Higher-level consumers consume both
/// the transit-shallow value AND the probe-emitted diagnostics
/// independently.
#[allow(dead_code)]
pub(crate) fn resolve_macro_payload_diagnostic_probe(
    dispatch: &ProjectSemanticDispatch<'_>,
    payload_node: SemanticNodeId,
    macro_index: usize,
    expansion_kind: MacroExpansionKind,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) {
    let probe_read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base: payload_node,
        path: empty_path(),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Shallow,
        ),
    });
    emit_dispatch_dep_signature_facts(dispatch.ctx, &probe_read.dep_signature);

    if !probe_read.walker_diagnostics.is_empty() {
        diag_sink.push(shallow_diagnostics_to_macro_expansion(
            &probe_read.walker_diagnostics,
            macro_index,
            expansion_kind.clone(),
            probe_read.cache_suppress,
        ));
    }

    match probe_read.value {
        QueryResult::Value(id) => {
            // Probe surface resolved. Inspect for an `Opaque(_)`
            // carrier — the transit-shallow lowering may have
            // succeeded at producing a node, but the publication-
            // demand probe walks into an opaque shell. Translate
            // the QueryError verbatim (per the existing
            // `resolve_macro_payload` opaque check pattern).
            if let Some(SemanticNodeData::Opaque(err)) =
                crate::project_semantic_dispatch::node_data_for(dispatch.ctx, id).as_deref()
            {
                diag_sink.push(macro_expansion_for_query_error(
                    macro_index,
                    expansion_kind,
                    format!("macro-payload-probe-opaque::{:?}", err),
                ));
            }
            // Otherwise: no probe-emitted diagnostic. The transit-
            // shallow payload is healthy at the publication
            // boundary.
        }
        QueryResult::Recursive(_) => {
            diag_sink.push(macro_expansion_for_cycle(
                macro_index,
                expansion_kind,
                "cyclic-macro-payload-probe".to_string(),
            ));
        }
        QueryResult::Error(e) => {
            diag_sink.push(macro_expansion_for_query_error(
                macro_index,
                expansion_kind,
                format!("macro-payload-probe-error::{:?}", e),
            ));
        }
    }
}

/// **Branch-merge primitive scope tag** — Block 6.i round 7 codex
/// 4th consult Q3 substrate.
///
/// The branch-merged shallow conditional surface scopes to macro
/// **object** publication only (emit-class macro payloads). Codex
/// Q6 top risk: the branch-merge must NOT widen unrelated symbolic
/// surfaces. This enum is the explicit gating parameter
/// [`resolve_payload_surface_with_scope`] accepts to decide whether
/// to apply the branch-merge when the payload is an undecided
/// `Conditional` shell.
///
/// - [`PayloadSurfaceScope::Default`] preserves the legacy
///   behaviour: a Conditional payload yields whatever the
///   `ProjectPath { Published(Shallow) }` walker produces directly
///   (typically a carrier shell). No branch-merge.
/// - [`PayloadSurfaceScope::EmitClassMacroObject`] enables the
///   branch-merge for emit-class macro object payloads: when the
///   payload is a `Conditional` whose check did not decide, the
///   shared resolver projects both branches under
///   `Published(Shallow)` and merges their top-level event rows
///   into a single Object surface. Other macro kinds (slots, props,
///   options, exposed, model) MUST pass `Default`.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PayloadSurfaceScope {
    /// Default scope — no branch-merge. All non-emit macro callers
    /// AND any caller outside macro publication MUST use this.
    Default,
    /// Emit-class macro object payload scope — enables branch-merge
    /// of undecided `Conditional` shells via top-level event-row
    /// union. Codex Q3 / Q4: restores
    /// `resolver_coverage_inherited_emits_branch_merged_surface`
    /// under transit-shallow macro publication without giving the
    /// inheritance reducer an `Expanded`-only escape hatch.
    EmitClassMacroObject,
}

/// **Scope-gated payload surface resolver** — Block 6.i round 7
/// codex 4th consult Q3 / Q4 substrate. Sibling of
/// [`super::resolve_payload_surface`] that consults
/// [`PayloadSurfaceScope`] to decide whether to apply branch-merged
/// shallow semantics when the payload is an undecided `Conditional`.
///
/// For [`PayloadSurfaceScope::Default`] the behaviour is identical
/// to [`super::resolve_payload_surface`] — single `ProjectPath {
/// Published(Shallow) }` dispatch over the payload.
///
/// For [`PayloadSurfaceScope::EmitClassMacroObject`] the resolver
/// peeks the payload's node data BEFORE the surface dispatch: when
/// the payload is a [`SemanticNodeData::Conditional`] whose check
/// did not decide (the dispatch returned the Conditional verbatim),
/// the resolver projects BOTH `true_branch_ref` and
/// `false_branch_ref` under `Published(Shallow)` and merges their
/// top-level Object members into a single `SurfaceView`. The merge
/// takes the union of member names — events that appear in either
/// branch surface on the inherited `accepted_events` set.
///
/// Codex Q6 risk scoping: the branch-merge runs ONLY when the
/// caller explicitly passes
/// [`PayloadSurfaceScope::EmitClassMacroObject`]. Non-emit macros
/// (slots / props / options / exposed / model) pass
/// [`PayloadSurfaceScope::Default`] so the round-7 substrate does
/// not widen unrelated symbolic surfaces.
///
/// **Diagnostic propagation**: the wrapper inherits the dep-
/// signature, `walker_diagnostics`, and `cache_suppress` translation
/// from the underlying dispatches — both the original payload
/// `ProjectPath` AND each branch's `ProjectPath` fan their facts
/// into the active fact tracer via `emit_dispatch_dep_signature_facts`.
/// A branch dispatch returning `Recursive` / `Error` falls back to
/// using ONLY the other branch's surface (better partial coverage
/// than dropping the whole inherited emit set); a hard failure on
/// both branches publishes a single
/// `macro-payload-surface-branch-merge-error` diagnostic.
#[allow(dead_code)]
pub(crate) fn resolve_payload_surface_with_scope(
    dispatch: &ProjectSemanticDispatch<'_>,
    payload_node: SemanticNodeId,
    macro_index: usize,
    expansion_kind: MacroExpansionKind,
    scope: PayloadSurfaceScope,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) -> Option<SemanticNodeId> {
    if matches!(scope, PayloadSurfaceScope::Default) {
        return super::resolve_payload_surface(
            dispatch,
            payload_node,
            macro_index,
            expansion_kind,
            diag_sink,
        );
    }

    // Emit-class macro object scope. Peek the payload's node data
    // to detect an undecided `Conditional` shell BEFORE dispatching
    // the outer `ProjectPath`. The pre-dispatch peek is non-invasive
    // (it reads already-interned `SemanticNodeData` without re-
    // dispatch); it does NOT emit any dep-signature on its own.
    let payload_is_conditional = matches!(
        crate::project_semantic_dispatch::node_data_for(dispatch.ctx, payload_node).as_deref(),
        Some(SemanticNodeData::Conditional { .. })
    );

    if !payload_is_conditional {
        // Not a Conditional payload — branch-merge is inapplicable.
        // Fall through to the Default path.
        return super::resolve_payload_surface(
            dispatch,
            payload_node,
            macro_index,
            expansion_kind,
            diag_sink,
        );
    }

    // Conditional payload under emit-class scope. Project both
    // branches under `Published(Shallow)` and merge their top-level
    // Object members.
    let (true_branch, false_branch) = match crate::project_semantic_dispatch::node_data_for(
        dispatch.ctx,
        payload_node,
    )
    .as_deref()
    {
        Some(SemanticNodeData::Conditional {
            true_branch_ref,
            false_branch_ref,
            ..
        }) => (*true_branch_ref, *false_branch_ref),
        _ => {
            // Unreachable per the peek above, but fall through
            // safely.
            return super::resolve_payload_surface(
                dispatch,
                payload_node,
                macro_index,
                expansion_kind,
                diag_sink,
            );
        }
    };

    let mut project_branch = |branch_node: SemanticNodeId| -> Option<SemanticNodeId> {
        let branch_read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
            base: branch_node,
            path: empty_path(),
            context: crate::semantic_query::ProjectionReductionContext::published(
                ProjectionMode::Shallow,
            ),
        });
        emit_dispatch_dep_signature_facts(dispatch.ctx, &branch_read.dep_signature);
        if !branch_read.walker_diagnostics.is_empty() {
            diag_sink.push(shallow_diagnostics_to_macro_expansion(
                &branch_read.walker_diagnostics,
                macro_index,
                expansion_kind.clone(),
                branch_read.cache_suppress,
            ));
        }
        match branch_read.value {
            QueryResult::Value(id) => Some(id),
            _ => None,
        }
    };

    let true_surface = project_branch(true_branch);
    let false_surface = project_branch(false_branch);

    let read_members = |surface: SemanticNodeId| -> Option<Arc<[SurfaceMember]>> {
        match crate::project_semantic_dispatch::node_data_for(dispatch.ctx, surface).as_deref() {
            Some(SemanticNodeData::Object(view)) => Some(Arc::clone(&view.members)),
            _ => None,
        }
    };

    let true_members = true_surface.and_then(read_members);
    let false_members = false_surface.and_then(read_members);

    match (true_members, false_members) {
        (Some(t), Some(f)) => {
            // Merge — union by member name. Members from the true
            // branch take precedence on collision (TS conditional
            // semantics: when `Mode extends 'editor'` is true, the
            // EditorEmits row is the canonical one; the false-
            // branch row only surfaces when its name is unique to
            // that branch). Inherited `accepted_events` is the set
            // union; identical event names across branches dedup
            // naturally.
            let mut merged: Vec<SurfaceMember> = Vec::with_capacity(t.len() + f.len());
            let mut seen_names: rustc_hash::FxHashSet<Arc<str>> = rustc_hash::FxHashSet::default();
            for member in t.iter().chain(f.iter()) {
                if seen_names.insert(Arc::clone(&member.name)) {
                    merged.push(member.clone());
                }
            }
            let view = crate::semantic_query::SurfaceView {
                members: Arc::from(merged.into_boxed_slice()),
                call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                index_signatures: Arc::from(
                    Vec::<crate::semantic_query::IndexSignature>::new().into_boxed_slice(),
                ),
                keyspace: None,
                has_index_signature: false,
            };
            Some(
                dispatch
                    .ctx
                    .project_type_store()
                    .semantic_graph()
                    .intern_node(SemanticNodeData::Object(view)),
            )
        }
        // Partial coverage — better than dropping the inherited set
        // entirely. The available branch surfaces alone.
        (Some(_), None) => true_surface,
        (None, Some(_)) => false_surface,
        (None, None) => {
            diag_sink.push(macro_expansion_for_query_error(
                macro_index,
                expansion_kind,
                "macro-payload-surface-branch-merge-error::both-branches-unresolved".to_string(),
            ));
            None
        }
    }
}

/// **Member-value role tag** — Block 6.i round 7 codex 4th consult
/// Q4 substrate.
///
/// Disambiguates how the `surface_member_to_expanded_field` /
/// `member_shape_peek_or_compute` /
/// `resolve_member_value_for_classification` pipeline handles a
/// macro surface member's value. Two roles:
///
/// - [`MemberValueRole::Field`] — the legacy "field shape" path
///   (props / emits / options / exposed / model). The pipeline
///   raises/classifies the member's value verbatim (unchanged
///   pre-cutover behaviour).
/// - [`MemberValueRole::CallableSlot`] — slot members. Contract:
///   the pipeline calls `realize_callable_member` on the member's
///   value FIRST, then caches / classifies / raises the
///   **realized** function node (not the original carrier). Cache
///   keys use the realized node so the cache shape matches what
///   downstream consumers see, and the slot-binding consumer's
///   `Function`-arm match fires on the closed Function. Pre-cutover
///   the macro publication's Expanded lowering closed the
///   Conditional during publication; under transit-shallow it
///   stays carrier-shaped and consumers must realize.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberValueRole {
    /// Field shape role. Props / emits / options / exposed / model
    /// — the pipeline raises/classifies the member's value
    /// verbatim.
    Field,
    /// Callable-slot role. Slot members — the pipeline realizes the
    /// member's value through `realize_callable_member` (carrier-
    /// shell normalization + conditional re-dispatch) FIRST, then
    /// caches/classifies/raises the realized function node.
    CallableSlot,
}
