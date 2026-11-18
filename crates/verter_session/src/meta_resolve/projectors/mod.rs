//! Component-meta shape projection and shared semantic publication helpers.
//!
//! Declared macro membership, member authority, and diagnostics come from the
//! framework-surface producer. The shape projector consumes those normalized
//! rows directly. Payload resolution remains available to executor, codegen,
//! and slot-binding consumers that demand a payload rather than publication.
//! The terminal output sink owns materialization and source finalization.

use std::sync::Arc;

use verter_session_query::analysis::component_meta::{
    MacroExpansionDiagnostics, MacroExpansionKind,
};
use verter_session_query::analysis::types::{AnalyzedMacro, AnalyzedMacroKind};

use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::resolver_core::ResolverContext;
use verter_type_engine::semantic_query::{
    DeclIdentity, PathSegment, ProjectionMode, QueryResult, SemanticNodeData, SemanticNodeId,
    SemanticQueryKey,
};
use verter_type_engine::semantic_query_memo::node_roots::NodeRoot;

use super::diagnostic_convert::shallow_diagnostics_to_macro_expansion;
use verter_type_engine::meta_resolve::emit_dispatch_dep_signature_facts;

pub(crate) mod define_shapes;
pub(crate) mod output_sink;
pub(crate) mod published_reducer;
// Published-SOURCE upgrades for reduced publication nodes (leaf / leaf-union
// / member ref-identity) — pure node-domain projections consumed by the
// terminal sink.
mod published_source;
// The address-validated structural member-source projection — shared with the
// vue_exec DTO normalizers (the normalized prop/expose rows publish the same
// closed/ref/member-path ladder the member sink publishes). Terminal demand
// remains strict, while stable unresolved addresses remain valid carriers.
pub(crate) use published_source::structural_member_value_source;

// The projectors' reverse-materialization capability is DEFINED in the terminal
// `output_sink` sink module (the only module that can MINT it). It is
// re-exported here so the owner `project_semantic_dispatch::projector`
// `OutputProjector` impl and the shape projector can NAME the cap
// type at the stable `crate::meta_resolve::projectors::MetaResolveProjectorsOutputCap`
// path; the inherent `new` constructor stays scoped to `output_sink`, so naming
// the type here does NOT grant the ability to mint it (a non-sink helper that
// names the cap and calls `new` is `E0624`).
pub(crate) use output_sink::MetaResolveProjectorsOutputCap;

pub(crate) use define_shapes::project_define_macro_shapes;
pub(crate) use published_reducer::classify_node_reduction_gates;

pub(crate) use output_sink::reduce_published_field_types;

// The output-ENVELOPE builder: the terminal sink materializes the
// component-meta output type lanes and seals them (with the analysis and the
// optional narrowed resolution sidecar) into the request-local
// `crate::meta_resolve::ComponentMetaOutput` the wire converter consumes.
// Re-exported at the stable `crate::meta_resolve::projectors::*` path for the
// view-fenced host entry; envelope construction itself stays inside the sink
// (the envelope constructors require the sink-mintable capability).
pub(crate) use output_sink::build_component_meta_output;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use output_sink::OUTPUT_MATERIALIZE_FORCE_FAIL_FOR;
#[cfg(test)]
pub(crate) use output_sink::{
    LAST_OUTPUT_MATERIALIZE_CALLS, LAST_OUTPUT_MEMO_HASH_OPS, OUTPUT_MATERIALIZE_FORCE_FAIL,
    OUTPUT_PUBLICATION_FAILURE,
};

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
pub(crate) fn build_owner_decl_identity<M: verter_audit::observe::ObserveExec>(
    ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities<M>>,
    owner_canonical: &str,
    owner: verter_type_expr::TopLevelOwnerId,
) -> DeclIdentity {
    let whole_hash = ctx
        .shallow_file_state(owner_canonical)
        .map(|s| s.whole_hash)
        .unwrap_or_default();
    DeclIdentity {
        canonical_id: verter_type_expr::OwnedStr::from(owner_canonical),
        owner,
        whole_hash,
        decl_name: verter_type_expr::OwnedStr::from(SFC_SCRIPT_SETUP_DECL_NAME),
    }
}

/// Convert an `verter_type_expr::OwnedSlice<PathSegment>` empty path constant. Cached locally
/// so each projector call reuses a shared-ref-counted empty path.
#[inline]
pub(crate) fn empty_path() -> verter_type_expr::OwnedSlice<PathSegment> {
    verter_type_expr::OwnedSlice::default()
}

use crate::typeinfo::framework_surface::vue_exec::{
    macro_expansion_for_cycle, macro_expansion_for_query_error,
};

/// Resolve a type-based macro's payload through `ResolveMacroPayload`.
///
/// Reads the macro arg's ONE mode-neutral mirror handle
/// (`verter_type_engine::structural_carrier_producer::macro_type_arg_hot_ref`) — the producer lowered
/// the `parsed_type_argument` once — then RE-ENTERS the shared dispatch with
/// `ResolveMacroPayload` (Navigate) for the terminal demand over that carrier
/// handle and returns the macro payload node on success. This is a different
/// DEMAND on the same producer, NOT a second lowering of the macro arg.
///
/// On `Recursive` or `Error`, appends a diagnostic to `diag_sink` and
/// returns `None`. Dep-signature is accumulated unconditionally.
///
/// Silent-miss prevention (§7.5): when lowering itself fails (the
/// type expression cannot be lowered to a `SemanticNodeId` — e.g. an
/// unresolved import to a non-existent module), a diagnostic is
/// pushed before returning `None`.
pub(crate) fn resolve_macro_payload<M: verter_audit::observe::ObserveExec>(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities<M>>,
    owner: &DeclIdentity,
    file: &str,
    macro_index: usize,
    mac: &AnalyzedMacro,
    macro_kind: AnalyzedMacroKind,
    expansion_kind: MacroExpansionKind,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) -> Option<NodeRoot> {
    let _scope = dispatch.enter_construction_scope();
    let _ = mac.parsed_type_argument.as_ref()?;
    // The macro type-argument graph node is the ONE mode-neutral mirror
    // handle (its macro-T own-body provenance is baked at production time —
    // `defineProps` / `withDefaults` own-body members carry
    // `declared_in_macro_type_arg = true`). The terminal demand below
    // (`ResolveMacroPayload`, Navigate) re-enters the ONE shared dispatch
    // from the carrier handle — a different DEMAND on the same producer, NOT
    // a second lowering of the macro arg.
    let type_args: Arc<[SemanticNodeId]> = match dispatch.macro_type_arg_hot_ref(file, macro_index)
    {
        Some(product) => Arc::from(vec![product.hot.node()].into_boxed_slice()),
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
        owner: dispatch.type_slot_for(
            owner.canonical_id.clone(),
            owner.owner,
            owner.decl_name.clone(),
        ),
        macro_index,
        macro_kind,
        type_args,
        context: dispatch.macro_payload_context_for(&owner.canonical_id, ProjectionMode::Navigate),
    });
    verter_type_engine::request_context::observe_component_meta_read_suppress(&payload_read);
    emit_dispatch_dep_signature_facts(dispatch, payload_read.dep_signature());
    if !payload_read.walker_diagnostics().is_empty() {
        diag_sink.push(shallow_diagnostics_to_macro_expansion(
            payload_read.walker_diagnostics(),
            macro_index,
            expansion_kind.clone(),
            payload_read.cache_suppress(),
            &|diagnostic| dispatch.checker_message(diagnostic),
        ));
    }

    let payload_node = match payload_read.value() {
        QueryResult::Value(id) => *id,
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
        dispatch.graph().read_in_scope(payload_node).as_deref()
    {
        diag_sink.push(macro_expansion_for_query_error(
            macro_index,
            expansion_kind,
            format!("macro-payload-opaque::{:?}", err),
        ));
        return None;
    }

    // Silent-miss compensation for the carrier-preserving publication
    // contract. Under `Navigate` lowering an unresolved import either
    // collapses to an empty `Object` surface downstream or rides a
    // `DeclRef` carrier whose declaration does not exist (the import
    // mapping is a shallow fact; declaration existence is a resolution
    // fact). Both shapes are indistinguishable from a legitimately
    // empty / carrier payload without a structural probe. Probe the
    // macro's `parsed_type_argument` at `Navigate` and discriminate
    // STRUCTURALLY — typed-IR inspection only, no Expanded lowering of
    // the full type argument:
    //   - a probe node that is an `Opaque` Miss-class sentinel is
    //     unresolved directly;
    //   - a probe node that is a root `DeclRef` carrier resolves ONE
    //     hop through `ResolveDecl`; a Miss-class outcome means the
    //     name routed to a file that does not declare it.
    // Legitimate empty macros (`defineProps<{}>()`) probe to a
    // non-`Opaque` Object; carrier-stopped payloads (an open mapped
    // surface) are neither empty-Object nor root-`DeclRef` shapes and
    // never reach the probe.
    let payload_data = dispatch.graph().read_in_scope(payload_node);
    let payload_is_empty_surface = matches!(
        payload_data.as_deref(),
        Some(SemanticNodeData::Object(view))
            if view.closed().is_empty()
    );
    let payload_is_decl_ref_carrier = matches!(
        payload_data.as_deref(),
        Some(SemanticNodeData::DeclRef { .. })
    );
    // An unresolved-reference carrier (`BareRef` / `ImportType` / `TypeOf`)
    // surviving as the macro payload is the carrier-preserving counterpart of
    // the `DeclRef` case: under the query-free macro hot mirror a single-arg
    // props payload (`defineProps<MissingImport>()`) returns the macro arg's
    // structural carrier verbatim, so a payload whose declaration does not
    // exist arrives here as a `BareRef`/`ImportType`/`TypeOf` carrier rather
    // than a pre-resolved `DeclRef`. The structural probe below resolves it ONE
    // Navigate hop through the shared dispatch (which records the value-root
    // `ImportRoute` / `FileWholeHash` facts) and discriminates a genuine miss.
    let payload_is_unresolved_ref_carrier = matches!(
        payload_data.as_deref(),
        Some(data) if data.bare_ref_head().is_some()
            || data.import_type_head().is_some()
            || data.typeof_head().is_some()
    );
    drop(payload_data);
    if (payload_is_empty_surface
        || payload_is_decl_ref_carrier
        || payload_is_unresolved_ref_carrier)
        && mac.parsed_type_argument.is_some()
    {
        {
            // Probe the SAME mirror handle (a different DEMAND on the one
            // producer — never a second lowering). Resolve its carrier head
            // ONE Navigate hop through the shared dispatch so the structural
            // root carrier becomes the resolved `DeclRef` / `Opaque(Miss)`
            // the discrimination below inspects.
            let probe_node = dispatch.macro_type_arg_hot_ref(file, macro_index)
            .map(|product| {
                let probe_read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
                    base: product.hot.node(),
                    path: empty_path(),
                    context:
                        verter_type_engine::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                            ProjectionMode::Navigate,
                        ),
                });
                emit_dispatch_dep_signature_facts(dispatch,  probe_read.dep_signature());
                if !probe_read.walker_diagnostics().is_empty() {
                    diag_sink.push(shallow_diagnostics_to_macro_expansion(
                        probe_read.walker_diagnostics(),
                        macro_index,
                        expansion_kind.clone(),
                        probe_read.cache_suppress(),
                        &|diagnostic| dispatch.checker_message(diagnostic),
                    ));
                }
                match *probe_read.value() {
                    QueryResult::Value(id) => id,
                    QueryResult::Recursive(id) => id,
                    QueryResult::Error(_) => product.hot.node(),
                }
            });
            if let Some(probe_node) = probe_node {
                let unresolved: Option<String> = match dispatch
                    .graph()
                    .read_in_scope(probe_node)
                    .as_deref()
                {
                    Some(SemanticNodeData::Opaque(err)) => Some(format!("{err:?}")),
                    Some(SemanticNodeData::DeclRef { identity }) => {
                        let read = dispatch.execute_read(SemanticQueryKey::ResolveDecl(
                                verter_type_engine::semantic_query::ResolveDeclKey {
                                    scope: verter_type_engine::semantic_query::ScopeId {
                                        canonical_id: identity.canonical_id.clone(),
                                        owner: identity.owner,
                                        local_scope: None,
                                        binder_scope_id:
                                            verter_type_engine::semantic_query::BinderScopeId::file_scope(
                                                identity.owner,
                                            ),
                                    },
                                    name: identity.decl_name.clone(),
                                },
                            ));
                        emit_dispatch_dep_signature_facts(dispatch, read.dep_signature());
                        if !read.walker_diagnostics().is_empty() {
                            diag_sink.push(shallow_diagnostics_to_macro_expansion(
                                read.walker_diagnostics(),
                                macro_index,
                                expansion_kind.clone(),
                                read.cache_suppress(),
                                &|diagnostic| dispatch.checker_message(diagnostic),
                            ));
                        }
                        match *read.value() {
                            QueryResult::Value(anchor) => {
                                match dispatch.graph().read_in_scope(anchor).as_deref() {
                                    Some(SemanticNodeData::Opaque(
                                        err @ verter_type_engine::semantic_query::QueryError::Miss,
                                    )) => Some(format!("{err:?}")),
                                    _ => None,
                                }
                            }
                            QueryResult::Error(ref err) => Some(format!("{err:?}")),
                            QueryResult::Recursive(_) => None,
                        }
                    }
                    // An unresolved-reference carrier SURVIVING the
                    // Navigate identity retry is a genuine unresolved
                    // route/declaration: a resolvable head would have
                    // become its `DeclRef` / `InstantiationRef` identity
                    // carrier under the Navigate probe. The preserved
                    // `BareRef` / `ImportType` / `TypeOf` shape is the
                    // carrier-preserving counterpart of the pre-carrier
                    // `Opaque(Miss)` terminal — same silent-miss
                    // contract, one diagnostic.
                    Some(data)
                        if data.bare_ref_head().is_some()
                            || data.import_type_head().is_some()
                            || data.typeof_head().is_some() =>
                    {
                        Some("unresolved reference carrier".to_string())
                    }
                    _ => None,
                };
                if let Some(err) = unresolved {
                    diag_sink.push(macro_expansion_for_query_error(
                        macro_index,
                        expansion_kind,
                        format!("macro-payload-decl-unresolved::{err}"),
                    ));
                    return None;
                }
            }
        }
    }

    dispatch.graph().root_in_scope(payload_node).ok()
}
