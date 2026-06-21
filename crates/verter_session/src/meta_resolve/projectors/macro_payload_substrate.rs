//! Macro-payload boundary substrate primitives for the transit-
//! shallow macro publication pipeline.
//!
//! Split out of [`super`] to keep the projector entry-point file
//! under the architecture-guard line cap (`no_oversize_files`). The
//! substrate primitives are re-exported from [`super`] so call sites
//! continue to import from `crate::meta_resolve::projectors::*`
//! unchanged.
//!
//! Primitives in this module:
//!
//! - [`resolve_macro_payload_diagnostic_probe`] — boundary probe
//!   that restores silent-miss diagnostics under transit-shallow
//!   macro publication. Re-dispatches the macro payload under
//!   publication demand on a non-value-publishing path so unresolved
//!   `DeclRef` / `Opaque` carriers surface as
//!   `MacroExpansionDiagnostics` envelopes even though the macro's
//!   published value stays shallow.
//! - [`PayloadSurfaceScope`] + [`resolve_payload_surface_with_scope`]
//!   — scope-gated payload surface resolver. Enables branch-merged
//!   shallow semantics for emit-class macro object payloads (an
//!   undecided root `Conditional` projects both branches under
//!   `Published(Shallow)` and merges their top-level Object members),
//!   while every other macro kind (props / slots / options / exposed
//!   / model) keeps the default single-dispatch behaviour.
//! - [`MemberValueRole`] — disambiguates Field vs CallableSlot
//!   handling for the surface-member projection pipeline. The
//!   CallableSlot role threads slot member values through
//!   `realize_callable_member` BEFORE caching/classifying/raising so
//!   the slot consumer's `Function`-arm match fires on the closed
//!   Function node, not on the carrier shell that the transit-shallow
//!   publication terminal preserved.
//!
//! All primitives carry `#[allow(dead_code)]` so the substrate can
//! be introduced and wired by independent commits without a clippy
//! breakage in the interim.

use std::sync::Arc;

use verter_semantic::analysis::component_meta::{MacroExpansionDiagnostics, MacroExpansionKind};
use verter_semantic::analysis::AnalyzedMacroKind;

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    ProjectionMode, QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryKey, SurfaceMember,
};

use super::{empty_path, macro_expansion_for_cycle, macro_expansion_for_query_error};
use crate::meta_resolve::dep_signature::emit_dispatch_dep_signature_facts;
use crate::meta_resolve::diagnostic_convert::shallow_diagnostics_to_macro_expansion;

/// **Macro-payload boundary diagnostic probe.**
///
/// Restores the silent-miss diagnostic contract for the transit-
/// shallow macro publication path. Under transit-shallow lowering
/// the publication boundary carrier-stops `KeyOf` / `Mapped` /
/// `DeclRef` carriers, so an unresolved-import surface can resolve
/// to a `DeclRef` shell WITHOUT firing `walker_diagnostics` — the
/// missing-decl failure that an eager `Published(Expanded)` lowering
/// would have surfaced loudly (by returning `None` from
/// `lower_type_expr_in_scope_with_mode`) silently passes through.
///
/// The probe runs a **second**, non-value-publishing dispatch under
/// publication demand on the transit-shallow payload node. The
/// probe's result is DISCARDED at the value level (the macro
/// publication still publishes the transit-shallow payload); only
/// its diagnostics, dep-signatures, and cache-suppress flag flow
/// into the consumer-visible `MacroExpansionDiagnostics` envelope.
///
/// Probe dispatch:
/// - The probe runs `ProjectPath { base: payload, [], Published(Shallow) }`
///   so the publication path tries to enumerate the payload's
///   surface members under publication demand. The `ProjectPath`
///   walker internally re-dispatches `DeclRef` → `ResolveDecl`,
///   `InstantiationRef` → `Instantiate { Published(Shallow) }`,
///   `Mapped` → `MappedType { Published(Shallow) }`, etc. — so a
///   single `ProjectPath` probe exercises the full carrier-
///   resolution surface.
/// - `walker_diagnostics`, `cache_suppress`, and the dispatch's
///   `dep_signature` translate into `MacroExpansionDiagnostics`
///   envelopes via the existing
///   `shallow_diagnostics_to_macro_expansion` /
///   `macro_expansion_for_query_error` / `macro_expansion_for_cycle`
///   helpers — identical to the diagnostic contract that eager
///   lowering used to enforce.
///
/// **Probe result MUST NOT replace the transit-shallow payload
/// value.** The probe is diagnostic-only; the macro publication's
/// downstream consumers continue reading the transit-shallow payload
/// node so the publication contract (carrier-stop / no deep member
/// breadth-enumeration) holds.
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
    crate::request_context::observe_component_meta_read_suppress(&probe_read);
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

/// **Branch-merge primitive scope tag.**
///
/// The branch-merged shallow conditional surface is scoped to macro
/// **object** publication only (emit-class macro payloads). The
/// branch-merge must NOT widen unrelated symbolic surfaces, so this
/// enum is the explicit gating parameter
/// [`resolve_payload_surface_with_scope`] accepts to decide whether
/// to apply the branch-merge when the payload is an undecided
/// `Conditional` shell.
///
/// - [`PayloadSurfaceScope::Default`] preserves the default
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
    /// union. Preserves the inherited-emits branch-merged surface
    /// under transit-shallow macro publication without giving the
    /// inheritance reducer an `Expanded`-only escape hatch.
    EmitClassMacroObject,
}

/// **Scope-gated payload surface resolver.**
///
/// Sibling of [`super::resolve_payload_surface`] that consults
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
/// Risk scoping: the branch-merge runs ONLY when the caller
/// explicitly passes [`PayloadSurfaceScope::EmitClassMacroObject`].
/// Non-emit macros (slots / props / options / exposed / model) pass
/// [`PayloadSurfaceScope::Default`] so this resolver does not widen
/// unrelated symbolic surfaces.
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
///
/// Resolve an emit macro payload node to its underlying `Conditional`
/// root, walking through `DeclRef` carriers for named conditional
/// aliases.
///
/// `resolve_macro_payload` lowers in `Navigate` mode, so
/// `defineEmits<ConditionalEmits>()` (where
/// `type ConditionalEmits = Mode extends X ? Y : Z`) surfaces as a
/// terminal `DeclRef` carrier rather than the `Conditional` node the
/// branch-merge needs. This helper reaches the alias body and recurses
/// through chained aliases until it finds a `Conditional` (returns
/// `Some`) or a non-`Conditional`/non-carrier node (returns `None`).
///
/// **Termination is by visited-node identity, not by a depth bound.**
/// `visited` accumulates every `SemanticNodeId` the walk has entered;
/// the first re-entry of a node is a cycle (`type A = B; type B = A`
/// used as an emit payload, or any longer alias loop) and returns
/// `None` immediately — a cyclic emit alias has no conditional root, so
/// `None` is the correct degraded answer. Because the walk terminates on
/// identity, a legitimate alias chain of arbitrary length still reaches
/// its terminal `Conditional` (each distinct hop is a fresh node). This
/// mirrors the `PathWalker::visited_nodes` rail in
/// [`crate::project_semantic_dispatch::walk`]: the set grows only on
/// genuine re-entry, so a linear chain costs O(n) inserts.
///
/// `depth` / [`EMIT_CARRIER_WALK_FUSE`] is a pure pathological fuse, NOT
/// the cyclic-termination mechanism — it caps a degenerate graph that
/// somehow produces a fresh node on every hop without ever cycling. It
/// is set far above any real alias-chain length so it never truncates a
/// legitimate chain.
///
/// The branch-merge only fires for the undecided-conditional
/// emit-inheritance carve-out, so a `None` here correctly routes the
/// (decided / object / union / cyclic) payload to the default
/// single-dispatch surface.
pub(crate) fn resolve_emit_payload_to_conditional_root(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    depth: u16,
    visited: &mut rustc_hash::FxHashSet<SemanticNodeId>,
) -> Option<SemanticNodeId> {
    // Pure pathological fuse — see [`EMIT_CARRIER_WALK_FUSE`]. The cycle
    // detection below is the real termination authority; this guards only
    // a degenerate non-cyclic graph that never re-enters a node.
    if depth as usize > EMIT_CARRIER_WALK_FUSE {
        return None;
    }
    // Cycle detection by node identity. The first re-entry of `node` is a
    // genuine alias loop (caught on the FIRST repeat, regardless of the
    // fuse) — return the degraded `None` for a cyclic emit alias.
    if !visited.insert(node) {
        return None;
    }
    match crate::project_semantic_dispatch::node_data_for(dispatch.ctx, node).as_deref() {
        // Already the Conditional — done.
        Some(SemanticNodeData::Conditional { .. }) => Some(node),
        // Navigate-mode carrier for a named alias. Resolve the alias's
        // body and recurse. `ResolveDecl` on a navigate `DeclRef`
        // returns an `Opaque(DeclPlaceholder)` deferral (the body is NOT
        // materialised by `ResolveDecl`), so reach the body by lowering
        // the prepared declaration's body `TypeExpr` directly — the same
        // mechanism the structural `named_decl_body` walker used.
        Some(SemanticNodeData::DeclRef { identity }) => {
            lower_decl_body_to_node(dispatch, &identity.canonical_id, &identity.decl_name).and_then(
                |resolved| {
                    resolve_emit_payload_to_conditional_root(dispatch, resolved, depth + 1, visited)
                },
            )
        }
        // A `DeclPlaceholder` deferral (e.g. surfaced by an upstream
        // `ResolveDecl`). Reach its body the same way.
        Some(SemanticNodeData::Opaque(crate::semantic_query::QueryError::DeclPlaceholder {
            canonical_id,
            name,
            ..
        })) => lower_decl_body_to_node(dispatch, canonical_id, name).and_then(|resolved| {
            resolve_emit_payload_to_conditional_root(dispatch, resolved, depth + 1, visited)
        }),
        _ => None,
    }
}

/// Pathological-graph fuse for [`resolve_emit_payload_to_conditional_root`].
///
/// The carrier walk terminates on visited-node identity (a cycle is
/// caught on the first re-entry). This fuse only bounds a degenerate
/// graph that produces a fresh node on every hop without ever cycling —
/// it is set far above any plausible real alias-chain depth so it never
/// truncates a legitimate chain.
pub(crate) const EMIT_CARRIER_WALK_FUSE: usize = 1024;

/// Resolve a named declaration's body to a graph node in `Navigate` mode
/// (terminal carriers preserved) through the shared hot accessor. Returns
/// the resolved body node — for a conditional alias body
/// (`type X = A extends B ? C : D`) this is the `Conditional` node whose
/// branch refs the emit branch-merge enumerates. The accessor drives the
/// shared `Instantiate` query (the single resolution engine), so the node
/// stays graph-native and is never materialised back to a `TypeExpr`.
/// `args` is empty — a bare named-decl demand carries no explicit type
/// arguments.
fn lower_decl_body_to_node(
    dispatch: &ProjectSemanticDispatch<'_>,
    canonical_id: &str,
    name: &str,
) -> Option<SemanticNodeId> {
    let handle = dispatch.decl_body_hot_ref(
        canonical_id,
        name,
        Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        crate::semantic_query::ProjectionReductionContext::published(ProjectionMode::Navigate),
    )?;
    Some(handle.node())
}

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
            // Emit-class payloads are structural (props-axis bit is
            // always false for emits). Routed through the
            // single-source-of-truth helper (which maps DefineEmits →
            // Structural) rather than hardcoding the value.
            super::macro_payload_surface_provenance(AnalyzedMacroKind::DefineEmits),
            diag_sink,
        );
    }

    // Emit-class macro object scope. Resolve the payload to its
    // underlying `Conditional` root BEFORE dispatching the outer
    // `ProjectPath`. The payload node may be the `Conditional` directly,
    // OR a `DeclRef` carrier for a NAMED alias whose body is the
    // `Conditional` (`defineEmits<ConditionalEmits>()` where
    // `type ConditionalEmits = Mode extends X ? Y : Z`).
    // `resolve_macro_payload` lowers in `Navigate` mode, so a named
    // conditional alias surfaces as a terminal `DeclRef` here, not the
    // `Conditional`. The carrier walk follows DeclRef/DeclPlaceholder
    // carriers to the conditional root, terminating on visited-node
    // identity (a cyclic alias loop returns `None`). The walk is
    // non-invasive (it reads already-interned `SemanticNodeData` and
    // lowers prepared bodies in Navigate); the branch dispatches below
    // emit the dep-signature.
    let mut carrier_visited: rustc_hash::FxHashSet<SemanticNodeId> =
        rustc_hash::FxHashSet::default();
    let conditional_node =
        resolve_emit_payload_to_conditional_root(dispatch, payload_node, 0, &mut carrier_visited);

    let Some(conditional_node) = conditional_node else {
        // No Conditional reachable — branch-merge is inapplicable.
        // Fall through to the Default path.
        return super::resolve_payload_surface(
            dispatch,
            payload_node,
            macro_index,
            expansion_kind,
            // Emit-class payloads are structural (props-axis bit is
            // always false for emits). Routed through the
            // single-source-of-truth helper (which maps DefineEmits →
            // Structural) rather than hardcoding the value.
            super::macro_payload_surface_provenance(AnalyzedMacroKind::DefineEmits),
            diag_sink,
        );
    };

    // Conditional payload under emit-class scope. Project both
    // branches under `Published(Shallow)` and merge their top-level
    // Object members.
    let (true_branch, false_branch) =
        match crate::project_semantic_dispatch::node_data_for(dispatch.ctx, conditional_node)
            .as_deref()
        {
            Some(SemanticNodeData::Conditional {
                true_branch_ref,
                false_branch_ref,
                ..
            }) => (*true_branch_ref, *false_branch_ref),
            _ => {
                // Unreachable per the resolution above, but fall through
                // safely.
                return super::resolve_payload_surface(
                    dispatch,
                    payload_node,
                    macro_index,
                    expansion_kind,
                    // Emit-class payloads are structural (props-axis bit is
                    // always false for emits).
                    crate::semantic_query::SurfaceProvenanceContext::Structural,
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
        crate::request_context::observe_component_meta_read_suppress(&branch_read);
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
            // branch take precedence on collision for the VALUE (TS
            // conditional semantics: when `Mode extends 'editor'` is
            // true, the EditorEmits row is the canonical one; the
            // false-branch row only surfaces when its name is unique
            // to that branch). Inherited `accepted_events` is the set
            // union; identical event names across branches dedup
            // naturally.
            //
            // VISIBILITY is NOT first-branch-wins: a name present in
            // both branches folds its accessibility to the MOST
            // RESTRICTIVE across both via the shared merge rule (an
            // open conditional `T extends U ? {public x} : {private x}`
            // can resolve to either branch, so `x` is only safely
            // public when public in BOTH). A copy that kept the
            // true-branch visibility would leak a private false-branch
            // member as public.
            let mut merged: Vec<SurfaceMember> = Vec::with_capacity(t.len() + f.len());
            let mut name_to_index: rustc_hash::FxHashMap<Arc<str>, usize> =
                rustc_hash::FxHashMap::default();
            for member in t.iter().chain(f.iter()) {
                match name_to_index.get(&member.name) {
                    Some(&existing_idx) => {
                        // Already represented (true-branch row wins for the
                        // value/optional/readonly fields); fold in this
                        // contributor's visibility to the most restrictive.
                        let folded = merged[existing_idx]
                            .visibility
                            .most_restrictive(member.visibility);
                        merged[existing_idx].visibility = folded;
                    }
                    None => {
                        name_to_index.insert(Arc::clone(&member.name), merged.len());
                        merged.push(member.clone());
                    }
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

/// **Member-value role tag.**
///
/// Disambiguates how the `surface_member_to_expanded_field` /
/// `member_shape_peek_or_compute` /
/// `resolve_member_value_for_classification` pipeline handles a
/// macro surface member's value. Two roles:
///
/// - [`MemberValueRole::Field`] — the default "field shape" path
///   (props / emits / options / exposed / model). The pipeline
///   raises/classifies the member's value verbatim.
/// - [`MemberValueRole::CallableSlot`] — slot members. Contract:
///   the pipeline calls `realize_callable_member` on the member's
///   value FIRST, then caches / classifies / raises the
///   **realized** function node (not the original carrier). Cache
///   keys use the realized node so the cache shape matches what
///   downstream consumers see, and the slot-binding consumer's
///   `Function`-arm match fires on the closed Function. An eager
///   `Published(Expanded)` lowering would close the Conditional /
///   carrier chain during publication; under transit-shallow the
///   value stays carrier-shaped, so this realization step is what
///   restores the Function node the slot consumer expects.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberValueRole {
    /// Field shape role. Props / emits / options / exposed / model
    /// — the pipeline raises/classifies the member's value
    /// verbatim.
    Field,
    /// Callable-slot role. Slot members — the pipeline realizes the
    /// member's value through
    /// [`crate::meta_resolve::dispatch_helpers::realize_callable_member`]
    /// (carrier-shell normalization + conditional re-dispatch) FIRST,
    /// then caches/classifies/raises the realized function node.
    CallableSlot,
}
