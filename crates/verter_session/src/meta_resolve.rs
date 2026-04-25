//! Shared materialization and resolved-meta owner for component-meta.
//!
//! This module owns:
//! - mode selection (`ProjectionMode::Identity` vs `ProjectionMode::Expanded`)
//! - materialized resolved outputs (`ResolvedComponentMetaState`)
//! - mode-aware caching
//! - JSDoc attachment and typed-tag resolution
//!
//! It calls into `host_resolve.rs` for declaration traversal â€” it does NOT
//! replace or duplicate the shared traversal substrate.
//!
//! # Architecture
//!
//! ```text
//! caller â†’ resolve_component_meta(canonical, mode)
//!            â†“
//!        meta_resolve.rs  (orchestration, materialization, caching)
//!            â†“
//!        host_resolve.rs  (declaration graph traversal, shared cache)
//! ```

use crate::host_manage::{
    component_meta_debug, component_meta_debug_enabled, component_meta_trace_custom,
};
use crate::resolver_core::{
    run_component_meta_request, ComponentMetaEvalOutputs, ComponentMetaRequestHost, RequestSource,
    SingleflightRole,
};
use crate::types::{FileAnalysisSnapshot, Hash16, ProjectionMode};
use crate::VerterHost;
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use verter_semantic::analysis::types::AnalyzedMacro;

pub(crate) const STORE_VIEW_STABILITY_MAX_ATTEMPTS: usize = 3;

fn next_component_meta_audit_request_id() -> u64 {
    static NEXT_REQUEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT_REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn trace_request_source(source: RequestSource) -> &'static str {
    match source {
        RequestSource::Cache => "cache",
        RequestSource::Flight {
            role: SingleflightRole::Leader,
            ..
        } => "flight:leader",
        RequestSource::Flight {
            role: SingleflightRole::Follower,
            ..
        } => "flight:follower",
        RequestSource::Fallback => "fallback",
    }
}

fn request_source_performed_compute(source: RequestSource) -> bool {
    matches!(
        source,
        RequestSource::Flight {
            role: SingleflightRole::Leader,
            ..
        } | RequestSource::Fallback,
    )
}

fn should_skip_imported_registry_seed_refresh(
    owner_canonical: &str,
    declaration: &ResolvedTypeDeclaration,
    existing_expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    crate::resolver_core::component_meta::imported_registry_seed_can_skip_refresh(
        owner_canonical,
        declaration,
        existing_expr,
    )
}

#[derive(Debug, Clone)]
pub struct CapturedComponentMetaInputs {
    whole_hash: Hash16,
    snapshot: FileAnalysisSnapshot,
    owner_eval_source: Option<String>,
    direct_dependency_candidates: std::collections::BTreeSet<String>,
    audit_capture_inputs_ms: f64,
    audit_store_read_ms: f64,
    audit_direct_import_proof_ms: f64,
}

impl ComponentMetaRequestHost for VerterHost {
    type View = crate::resolver_store::HostStoreView;
    type Mode = ProjectionMode;
    type Resolution = ResolvedComponentMetaState;
    type CapturedInputs = CapturedComponentMetaInputs;

    fn cache_key(
        &self,
        canonical: &str,
        mode: Self::Mode,
    ) -> crate::resolver_core::ResolutionNodeKey {
        resolved_meta_cache_key(canonical, mode)
    }

    fn snapshot_store_view(&self) -> Self::View {
        self.resolver_store_view()
    }

    fn view_mutation_epoch(&self, store_view: &Self::View) -> u64 {
        store_view.mutation_epoch()
    }

    fn current_store_view_epoch(&self) -> u64 {
        VerterHost::current_store_view_epoch(self)
    }

    fn capture_component_meta_inputs(
        &self,
        canonical: &str,
        _view: &Self::View,
    ) -> Option<Self::CapturedInputs> {
        let audit_enabled = self.config.audit_enabled;
        let capture_started = audit_enabled.then(Instant::now);
        let store_read_started = audit_enabled.then(Instant::now);
        component_meta_trace_custom!(
            "capture_component_meta_inputs",
            format!("owner={} store_view=true", canonical),
        );
        let snapshot = self.get_raw_analysis_snapshot(canonical)?;
        component_meta_trace_custom!(
            "capture_component_meta_snapshot",
            format!(
                "owner={} imports={} macros={} bindings={} has_template={}",
                canonical,
                snapshot.imports.len(),
                snapshot.macros.len(),
                snapshot.bindings.len(),
                snapshot.template.is_some(),
            ),
        );
        let facts = self.ensure_indexed_ready(canonical)?;
        let whole_hash = facts.whole_hash;
        let store_read_ms = store_read_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        component_meta_trace_custom!(
            "capture_component_meta_eval_state",
            format!(
                "owner={} source_len={} has_cached_parse={} whole_hash={whole_hash:?}",
                canonical,
                facts.raw_source.len(),
                facts.cached_parse.is_some(),
            ),
        );
        let owner_eval_source =
            VerterHost::build_eval_script_source(&facts.raw_source, facts.cached_parse.as_deref());
        let direct_import_started = audit_enabled.then(Instant::now);
        let direct_dependency_candidates =
            self.cache_dependency_candidates_from_snapshot(canonical, &snapshot);
        let direct_import_proof_ms = direct_import_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        let capture_inputs_ms = capture_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        component_meta_trace_custom!(
            "capture_component_meta_inputs_result",
            format!(
                "owner={} owner_eval_source_len={} dependency_candidates={}",
                canonical,
                owner_eval_source.len(),
                direct_dependency_candidates.len(),
            ),
        );
        Some(CapturedComponentMetaInputs {
            whole_hash,
            snapshot,
            owner_eval_source: Some(owner_eval_source),
            direct_dependency_candidates,
            audit_capture_inputs_ms: capture_inputs_ms,
            audit_store_read_ms: store_read_ms,
            audit_direct_import_proof_ms: direct_import_proof_ms,
        })
    }

    fn try_get_cached_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        _store_view: &Self::View,
    ) -> Option<Self::Resolution> {
        component_meta_trace_custom!(
            "try_get_cached_component_meta",
            format!("owner={} mode={mode:?}", canonical),
        );
        let result = self.try_get_cached_resolved_meta(canonical, mode);
        component_meta_trace_custom!(
            "try_get_cached_component_meta_result",
            format!("owner={} mode={mode:?} hit={}", canonical, result.is_some()),
        );
        result
    }

    fn compute_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        captured: Option<&Self::CapturedInputs>,
        _store_view: Option<&Self::View>,
    ) -> Option<Self::Resolution> {
        if let Some(captured) = captured {
            return self.compute_component_meta_state_from_captured(canonical, mode, captured);
        }

        let whole_hash = self
            .current_or_read_whole_hash(canonical)
            .unwrap_or_default();
        self.compute_component_meta_state(canonical, mode, whole_hash)
    }

    fn store_component_meta_result(
        &self,
        canonical: &str,
        mode: Self::Mode,
        result: &Self::Resolution,
    ) {
        self.store_cached_resolved_meta(canonical, mode, result, &result.fact_versions);
    }
}

// ---------------------------------------------------------------------------
// SessionRequestHost — session-scoped ComponentMetaRequestHost (Path C C14)
// ---------------------------------------------------------------------------

/// Session-scoped request host that routes reads through the session
/// runtime and writes to the session-scoped resolved-meta cache.
///
/// Replaces `impl ComponentMetaRequestHost for VerterHost` for all
/// session-scoped callers. The generic executor at
/// `component_meta_request.rs` calls these methods on the trait object,
/// so every axis is session-aware end to end.
pub struct SessionRequestHost<'a> {
    pub(crate) runtime: &'a crate::session_runtime::SessionRuntime,
}

impl<'a> ComponentMetaRequestHost for SessionRequestHost<'a> {
    type View = crate::resolver_store::HostStoreView;
    type Mode = ProjectionMode;
    type Resolution = ResolvedComponentMetaState;
    type CapturedInputs = CapturedComponentMetaInputs;

    fn cache_key(
        &self,
        canonical: &str,
        mode: Self::Mode,
    ) -> crate::resolver_core::ResolutionNodeKey {
        resolved_meta_cache_key(canonical, mode)
    }

    fn snapshot_store_view(&self) -> Self::View {
        let view = self.runtime.current_view();
        crate::resolver_store::HostStoreView::from_session(&view, self.runtime.host())
    }

    fn view_mutation_epoch(&self, store_view: &Self::View) -> u64 {
        store_view.mutation_epoch()
    }

    fn current_store_view_epoch(&self) -> u64 {
        self.runtime.current_store_view_epoch()
    }

    fn capture_component_meta_inputs(
        &self,
        canonical: &str,
        _view: &Self::View,
    ) -> Option<Self::CapturedInputs> {
        let host = self.runtime.host();
        let audit_enabled = host.config.audit_enabled;
        let capture_started = audit_enabled.then(Instant::now);
        let store_read_started = audit_enabled.then(Instant::now);
        component_meta_trace_custom!(
            "session_capture_component_meta_inputs",
            format!("owner={} session={}", canonical, self.runtime.session_id()),
        );
        let snapshot = host.get_raw_analysis_snapshot(canonical)?;
        let facts = host.ensure_indexed_ready(canonical)?;
        let whole_hash = facts.whole_hash;
        let store_read_ms = store_read_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        let owner_eval_source =
            VerterHost::build_eval_script_source(&facts.raw_source, facts.cached_parse.as_deref());
        let direct_import_started = audit_enabled.then(Instant::now);
        let direct_dependency_candidates =
            host.cache_dependency_candidates_from_snapshot(canonical, &snapshot);
        let direct_import_proof_ms = direct_import_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        let capture_inputs_ms = capture_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        Some(CapturedComponentMetaInputs {
            whole_hash,
            snapshot,
            owner_eval_source: Some(owner_eval_source),
            direct_dependency_candidates,
            audit_capture_inputs_ms: capture_inputs_ms,
            audit_store_read_ms: store_read_ms,
            audit_direct_import_proof_ms: direct_import_proof_ms,
        })
    }

    fn try_get_cached_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        _store_view: &Self::View,
    ) -> Option<Self::Resolution> {
        self.runtime.try_get_cached_resolved_meta(canonical, mode)
    }

    fn compute_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        captured: Option<&Self::CapturedInputs>,
        _store_view: Option<&Self::View>,
    ) -> Option<Self::Resolution> {
        let host = self.runtime.host();
        if let Some(captured) = captured {
            return host.compute_component_meta_state_from_captured(canonical, mode, captured);
        }
        let whole_hash = host
            .current_or_read_whole_hash(canonical)
            .unwrap_or_default();
        host.compute_component_meta_state(canonical, mode, whole_hash)
    }

    fn store_component_meta_result(
        &self,
        canonical: &str,
        mode: Self::Mode,
        result: &Self::Resolution,
    ) {
        self.runtime.store_resolved_meta(canonical, mode, result);
    }
}

/// Native declaration kind for the resolved pre-expansion type.
pub type ResolvedDeclarationKind = crate::resolver_core::ResolvedDeclarationKind;

/// Native pre-expansion declaration metadata retained by the shared resolver.
pub type ResolvedTypeDeclaration = crate::resolver_core::ResolvedTypeDeclaration;
pub type ResolvedTypeRegistryMeta = crate::resolver_core::ResolvedTypeRegistryMeta;
pub type ResolvedMacroMeta = crate::resolver_core::ResolvedMacroMeta;
pub type ResolvedNativeProp = crate::resolver_core::ResolvedNativeProp;
pub type ResolvedJsdocBlock = crate::resolver_core::ResolvedJsdocBlock;
pub type ResolvedJsdocTag = crate::resolver_core::ResolvedJsdocTag;

/// Host-owned sidecar result for component-meta / analysis enrichment.
///
/// Raw snapshot remains raw â€” resolved imported metadata lives in this sidecar.
/// `Expanded` mode carries materialized surfaces; `Type` mode carries
/// identity/location only.
#[derive(Debug, Clone)]
pub struct ResolvedComponentMetaComputeAudit {
    pub timings: crate::component_meta_audit::RustTimingAudit,
    pub solver: crate::component_meta_audit::RustSolverAudit,
}

#[derive(Debug, Clone)]
pub struct ResolvedComponentMetaState {
    /// The raw analysis snapshot (never mutated for enrichment).
    pub snapshot: FileAnalysisSnapshot,
    /// Which mode was used to produce this state.
    pub mode: ProjectionMode,
    /// Content hash of the owner file at resolution time.
    pub whole_hash: Hash16,
    /// Resolved macro metadata from cross-file traversal.
    pub resolved_macros: Vec<ResolvedMacroMeta>,
    /// Resolved type registry entries (populated in `Expanded` mode).
    pub resolved_type_registry:
        Vec<verter_semantic::analysis::component_meta::ResolvedTypeAnalysis>,
    /// Native declaration metadata for each resolved type-registry entry.
    pub resolved_type_registry_meta: Vec<ResolvedTypeRegistryMeta>,
    /// Expanded types (populated in `Expanded` mode only).
    pub evaluated_types: Option<verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
    /// Semantic fact versions consumed while producing this resolved state.
    pub fact_versions: Vec<crate::resolver_core::FactVersionRef>,
    /// Non-semantic compute audit captured only when native audit is enabled.
    pub compute_audit: Option<ResolvedComponentMetaComputeAudit>,
    /// Origin subgraph for semantic results. Populated in `Expanded` mode
    /// by walking the `SemanticGraphStore` after dispatch resolution.
    pub origin_graph: Option<verter_protocol::types::OriginGraphDto>,
    /// Request identifier stamped by the host at the entry of
    /// `get_component_meta_with_resolution`. Non-zero. Consumers (the
    /// `AuditedRequest` harness and NAPI/WASM/LSP wrappers) use this
    /// to retrieve the matching `RustAuditRecord` via
    /// `VerterHost::take_audit_record(resolution.request_id)`.
    ///
    /// Zero is reserved for "not populated" — emitted by internal
    /// tests / FFI fixtures that predate the Commit 3 wiring.
    pub request_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegistryMaterialization {
    Full,
    SkipAppend,
}

fn collect_expanded_slot_binding_param_types<'a>(
    ty: &'a verter_semantic::analysis::type_expr::TypeExpr,
    out: &mut Vec<&'a verter_semantic::analysis::type_expr::TypeExpr>,
) {
    match ty {
        verter_semantic::analysis::type_expr::TypeExpr::Parenthesized(inner) => {
            collect_expanded_slot_binding_param_types(inner, out);
        }
        verter_semantic::analysis::type_expr::TypeExpr::Intersection(types)
        | verter_semantic::analysis::type_expr::TypeExpr::Union(types) => {
            for inner in types.iter() {
                collect_expanded_slot_binding_param_types(inner, out);
            }
        }
        verter_semantic::analysis::type_expr::TypeExpr::Function(func) => {
            if let Some(first) = func.parameters.first() {
                out.push(&first.ty);
            }
        }
        // Path C C11-residual-A: deferred Conditional whose extends has
        // `infer X` in a Function position represents a TS conditional
        // that the dispatch couldn't decide (typically due to an
        // in-flight sentinel during the upstream evaluation context).
        // For slot-binding extraction we use the conventional
        // TS-truthy semantics: walk the true_type as the slot-shape
        // contributor. The infer bindings extracted from the check's
        // matching Function position are folded into the true_type via
        // `decide_typeexpr_conditional_with_function_extends`, which
        // the caller (`enrich_missing_slot_bindings`) invokes before
        // collection.
        verter_semantic::analysis::type_expr::TypeExpr::Conditional { true_type, .. } => {
            collect_expanded_slot_binding_param_types(true_type, out);
        }
        verter_semantic::analysis::type_expr::TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match member {
                    verter_semantic::analysis::type_expr::ObjectMember::CallSignature(function)
                    | verter_semantic::analysis::type_expr::ObjectMember::ConstructSignature(
                        function,
                    ) => {
                        if let Some(first) = function.parameters.first() {
                            out.push(&first.ty);
                        }
                    }
                    verter_semantic::analysis::type_expr::ObjectMember::Method(method) => {
                        if let Some(first) = method.function.parameters.first() {
                            out.push(&first.ty);
                        }
                    }
                    verter_semantic::analysis::type_expr::ObjectMember::Property(_)
                    | verter_semantic::analysis::type_expr::ObjectMember::IndexSignature(_) => {}
                }
            }
        }
        _ => {}
    }
}

/// Path C C11-residual-B: shallow substitution for owner-local generic
/// alias refs at the registry-publish boundary. When a registry entry's
/// raw body is `Ref { name, [args..] }` and the alias is declared in the
/// SAME canonical scope as the registry consumer, look up the alias's
/// prepared body. If the body is an Object, substitute the type
/// arguments into its members and return the substituted Object. The
/// substituted Object preserves owner-local helper Refs (e.g.,
/// `ComponentVariants<T>` stays as `Ref { name: "ComponentVariants", ..}`)
/// rather than recursively expanding them — the registry consumer can
/// follow the helper Refs through the registry.
///
/// Returns `None` when the raw body is not a Ref, the alias is
/// cross-file, the alias has no prepared body, or the body is not an
/// Object.
fn component_meta_owner_local_shallow_substituted_alias_body(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    raw_body: Option<&verter_semantic::analysis::type_expr::TypeExpr>,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    use verter_semantic::analysis::type_expr::TypeExpr;
    let TypeExpr::Ref {
        name,
        type_arguments,
    } = raw_body?
    else {
        return None;
    };
    if type_arguments.is_empty() {
        return None;
    }
    let declaration = query_engine.resolve_type_declaration(scope_canonical_id, name);
    let target_canonical = if declaration.canonical_source.is_empty() {
        scope_canonical_id.to_string()
    } else {
        declaration.canonical_source.clone()
    };
    if target_canonical != scope_canonical_id {
        // Cross-file alias — let the imported_generic_alias_root path
        // handle it via materialisation + per-member refinement.
        return None;
    }
    let resolved_name = if declaration.resolved_name.is_empty() {
        name.as_ref().to_string()
    } else {
        declaration.resolved_name.clone()
    };
    let prepared = query_engine.prepared_type_decl(&target_canonical, &resolved_name)?;
    if prepared.type_parameters.len() < type_arguments.len() {
        return None;
    }
    let mut substitutions: rustc_hash::FxHashMap<String, TypeExpr> =
        rustc_hash::FxHashMap::default();
    for (index, param) in prepared.type_parameters.iter().enumerate() {
        let arg = type_arguments
            .get(index)
            .or(param.default.as_deref())
            .cloned();
        if let Some(arg) = arg {
            substitutions.insert(param.name.clone(), arg);
        }
        // Partial substitution still useful when later params have no
        // arg and no default — leave them unsubstituted in the body.
    }
    let body = &prepared.body;
    let TypeExpr::Object(_) = body else {
        return None;
    };
    Some(component_meta_substitute_typeexpr(body, &substitutions))
}

/// Recursive TypeExpr substitution walker. Walks every variant and
/// delegates leaf replacement to `try_replace`: return `Some(expr)` to
/// replace, `None` to recurse structurally.
fn walk_substitute_typeexpr(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    try_replace: &impl Fn(
        &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> Option<verter_semantic::analysis::type_expr::TypeExpr>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use verter_semantic::analysis::type_expr::{
        FunctionExpr, FunctionParam, IndexSignature, MethodSignature, ObjectExpr, ObjectMember,
        ObjectProperty, TupleElement, TypeExpr,
    };
    if let Some(replaced) = try_replace(expr) {
        return replaced;
    }
    let recurse = |e: &TypeExpr| -> TypeExpr { walk_substitute_typeexpr(e, try_replace) };
    let recurse_fn = |f: &FunctionExpr| -> FunctionExpr {
        FunctionExpr {
            parameters: f
                .parameters
                .iter()
                .map(|fp| FunctionParam {
                    name: fp.name.clone(),
                    ty: recurse(&fp.ty),
                    optional: fp.optional,
                    rest: fp.rest,
                })
                .collect(),
            return_type: f
                .return_type
                .as_ref()
                .map(|rt| std::sync::Arc::new(recurse(rt))),
            type_parameters: f.type_parameters.clone(),
        }
    };
    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => TypeExpr::Ref {
            name: name.clone(),
            type_arguments: std::sync::Arc::from(
                type_arguments.iter().map(&recurse).collect::<Vec<_>>(),
            ),
        },
        TypeExpr::Parenthesized(inner) => {
            TypeExpr::Parenthesized(std::sync::Arc::new(recurse(inner)))
        }
        TypeExpr::Union(parts) => TypeExpr::Union(std::sync::Arc::from(
            parts.iter().map(&recurse).collect::<Vec<_>>(),
        )),
        TypeExpr::Intersection(parts) => TypeExpr::Intersection(std::sync::Arc::from(
            parts.iter().map(&recurse).collect::<Vec<_>>(),
        )),
        TypeExpr::Array { element, readonly } => TypeExpr::Array {
            element: std::sync::Arc::new(recurse(element)),
            readonly: *readonly,
        },
        TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
            elements: std::sync::Arc::from(
                elements
                    .iter()
                    .map(|element| TupleElement {
                        label: element.label.clone(),
                        ty: recurse(&element.ty),
                        optional: element.optional,
                        rest: element.rest,
                    })
                    .collect::<Vec<_>>(),
            ),
            readonly: *readonly,
        },
        TypeExpr::Object(obj) => TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
            properties: obj
                .properties
                .iter()
                .map(|member| match member {
                    ObjectMember::Property(p) => ObjectMember::Property(ObjectProperty {
                        name: p.name.clone(),
                        ty: recurse(&p.ty),
                        optional: p.optional,
                        readonly: p.readonly,
                    }),
                    ObjectMember::Method(m) => ObjectMember::Method(MethodSignature {
                        name: m.name.clone(),
                        function: recurse_fn(&m.function),
                        optional: m.optional,
                    }),
                    ObjectMember::CallSignature(f) => ObjectMember::CallSignature(recurse_fn(f)),
                    ObjectMember::ConstructSignature(f) => {
                        ObjectMember::ConstructSignature(recurse_fn(f))
                    }
                    ObjectMember::IndexSignature(sig) => {
                        ObjectMember::IndexSignature(IndexSignature {
                            key_name: sig.key_name.clone(),
                            key_type: recurse(&sig.key_type),
                            value_type: recurse(&sig.value_type),
                            readonly: sig.readonly,
                        })
                    }
                })
                .collect(),
        })),
        TypeExpr::Function(func) => TypeExpr::Function(std::sync::Arc::new(recurse_fn(func))),
        _ => expr.clone(),
    }
}

fn component_meta_substitute_typeexpr(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    substitutions: &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use verter_semantic::analysis::type_expr::TypeExpr;
    walk_substitute_typeexpr(expr, &|e| match e {
        TypeExpr::TypeParameter(param) => substitutions.get(&param.name).cloned(),
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => substitutions.get(name.as_ref()).cloned(),
        _ => None,
    })
}

/// TypeExpr-level conditional decision (Path C C11-residual-A workaround).
///
/// When the dispatch fails to decide a `T extends (props: infer P) => any
/// ? F<P, ...> : ...` pattern at evaluation time (typically because a
/// same-path sentinel suppressed the cross-file `T[K]` evaluation), the
/// resulting `slot.ty` is left as a deferred `TypeExpr::Conditional`.
/// This helper applies the same nested-Function-Infer reduction that
/// `build_conditional`'s C11a path performs, but at the `TypeExpr`
/// level so slot-binding extraction can proceed without re-running the
/// dispatch.
///
/// Returns:
/// - `Some(decided_true_type_with_infer_substituted)` when the
///   conditional has a concrete Function check, a Function extends with
///   at least one `infer X` position, and the corresponding check
///   parameter types can be bound to those infer names.
/// - `None` when the conditional cannot be decided at this layer (no
///   infer-bearing Function extends, no Function check, or empty
///   bindings).
fn decide_typeexpr_conditional_with_function_extends(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    use verter_semantic::analysis::type_expr::TypeExpr;
    let TypeExpr::Conditional {
        check,
        extends,
        true_type,
        ..
    } = expr
    else {
        return None;
    };
    let TypeExpr::Function(check_fn) = check.as_ref() else {
        return None;
    };
    let TypeExpr::Function(extends_fn) = extends.as_ref() else {
        return None;
    };
    let mut bindings: rustc_hash::FxHashMap<String, TypeExpr> = rustc_hash::FxHashMap::default();
    for (e_param, c_param) in extends_fn.parameters.iter().zip(check_fn.parameters.iter()) {
        if let TypeExpr::Infer { name } = &e_param.ty {
            bindings.insert(name.clone(), c_param.ty.clone());
        }
    }
    if let (Some(TypeExpr::Infer { name }), Some(check_ret)) = (
        extends_fn.return_type.as_deref(),
        check_fn.return_type.as_deref(),
    ) {
        bindings.insert(name.clone(), check_ret.clone());
    }
    if bindings.is_empty() {
        return None;
    }
    Some(substitute_infer_in_typeexpr(true_type, &bindings))
}

fn substitute_infer_in_typeexpr(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    bindings: &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use verter_semantic::analysis::type_expr::TypeExpr;
    walk_substitute_typeexpr(expr, &|e| match e {
        TypeExpr::Infer { name } => bindings.get(name).cloned(),
        // Replace `semanticMiss` sentinel with the unique bound infer
        // when there is exactly one — recovers an inferred prop whose
        // SemanticNode-level position was lost during dispatch.
        TypeExpr::Unknown { raw }
            if raw == crate::resolver_core::component_meta_query_engine::SEMANTIC_MISS
                && bindings.len() == 1 =>
        {
            bindings.values().next().cloned()
        }
        _ => None,
    })
}

fn collect_expanded_slot_bindings_from_object_type(
    ty: &verter_semantic::analysis::type_expr::TypeExpr,
    seen: &mut rustc_hash::FxHashSet<String>,
    out: &mut Vec<(String, verter_semantic::analysis::type_expr::TypeExpr, bool)>,
) {
    match ty {
        verter_semantic::analysis::type_expr::TypeExpr::Parenthesized(inner) => {
            collect_expanded_slot_bindings_from_object_type(inner, seen, out);
        }
        verter_semantic::analysis::type_expr::TypeExpr::Intersection(types)
        | verter_semantic::analysis::type_expr::TypeExpr::Union(types) => {
            for inner in types.iter() {
                collect_expanded_slot_bindings_from_object_type(inner, seen, out);
            }
        }
        verter_semantic::analysis::type_expr::TypeExpr::Object(obj) => {
            for member in &obj.properties {
                let verter_semantic::analysis::type_expr::ObjectMember::Property(prop) = member
                else {
                    continue;
                };
                if !seen.insert(prop.name.clone()) {
                    continue;
                }
                out.push((prop.name.clone(), prop.ty.clone(), prop.optional));
            }
        }
        _ => {}
    }
}

fn enrich_missing_slot_bindings(
    resolved_macros: &[ResolvedMacroMeta],
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
) {
    let mut seen_names: rustc_hash::FxHashSet<String> = evaluated_types
        .slot_bindings
        .iter()
        .map(|binding| binding.name.clone())
        .collect();

    for entry in &evaluated_types.define_slots {
        for slot in &entry.result.value.properties {
            // Path C C11-residual-A: normalize a deferred Conditional
            // slot.ty by performing TS truthy-branch reduction at the
            // TypeExpr level before extracting binding params. The
            // dispatch may have left a deferred Conditional in the
            // slot value when an upstream sentinel suppressed
            // evaluation; recovering the true_type here lets the
            // caller's `collect_expanded_slot_bindings_from_object_type`
            // reach into the Function param and surface the binding
            // names.
            let normalized_ty;
            let slot_ty_for_collect = if let Some(decided) =
                decide_typeexpr_conditional_with_function_extends(&slot.ty)
            {
                normalized_ty = decided;
                &normalized_ty
            } else {
                &slot.ty
            };
            let mut binding_param_types = Vec::new();
            collect_expanded_slot_binding_param_types(
                slot_ty_for_collect,
                &mut binding_param_types,
            );
            if binding_param_types.is_empty() {
                continue;
            }

            let mut seen_bindings = rustc_hash::FxHashSet::default();
            let mut bindings = Vec::new();
            for binding_param_ty in binding_param_types {
                collect_expanded_slot_bindings_from_object_type(
                    binding_param_ty,
                    &mut seen_bindings,
                    &mut bindings,
                );
            }

            for (binding_name, binding_type, optional) in bindings {
                let field_name = format!("{}.{}", slot.name, binding_name);
                if !seen_names.insert(field_name.clone()) {
                    continue;
                }
                evaluated_types.slot_bindings.push(
                    verter_semantic::analysis::type_expand::ExpandedField {
                        name: field_name,
                        r#type: binding_type,
                        raw_type: None,
                        optional,
                        exactness: entry.result.exactness,
                        execution_status: entry.result.execution_status,
                        diagnostics: Vec::new(),
                    },
                );
            }
        }
    }

    for resolved in resolved_macros.iter().filter(|resolved| {
        resolved.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineSlots
    }) {
        for slot in &resolved.slots {
            for binding in &slot.bindings {
                let field_name = format!("{}.{}", slot.name, binding.name);
                if !seen_names.insert(field_name.clone()) {
                    continue;
                }
                let raw_type = binding.type_annotation.clone();
                let parsed_type = raw_type
                    .as_deref()
                    .map(verter_semantic::analysis::type_expr_lower::parse_type_annotation)
                    .unwrap_or_else(|| verter_semantic::analysis::type_expr::TypeExpr::Unknown {
                        raw: "unknown".to_string(),
                    });
                evaluated_types
                    .slot_bindings
                    .push(verter_semantic::analysis::type_expand::ExpandedField {
                    name: field_name,
                    r#type: parsed_type,
                    raw_type,
                    optional: false,
                    exactness:
                        verter_semantic::analysis::type_expand::ExpansionExactness::ExactConcrete,
                    execution_status:
                        verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
                    diagnostics: Vec::new(),
                });
            }
        }
    }
}

fn materialize_component_meta_type_expr_until_stable_in_scope(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    let mut current = expr.clone();
    for _ in 0..4 {
        let materialized = materialize_component_meta_member_surface_expr(
            &current,
            scope_canonical_id,
            query_engine,
            true,
        );
        let next = query_engine
            .solve_expr_type_expr(scope_canonical_id, &materialized)
            .map(|solved| {
                materialize_component_meta_member_surface_expr(
                    &solved,
                    scope_canonical_id,
                    query_engine,
                    true,
                )
            })
            .unwrap_or(materialized);
        if next == current {
            return next;
        }
        current = next;
    }
    current
}

fn imported_component_meta_materialization_scope(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    owner_canonical: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> Option<String> {
    let route_root_name = component_meta_registry_public_utility_route(expr)
        .or_else(|| component_meta_registry_public_indexed_access_route(expr))
        .map(|(root_name, _)| root_name);
    let root_name = match expr {
        verter_semantic::analysis::type_expr::TypeExpr::Ref { name, .. } => name.as_ref(),
        _ => route_root_name.as_deref()?,
    };

    let declaration = query_engine.resolve_type_declaration(owner_canonical, root_name);
    let declaration_scope = if declaration.canonical_source.is_empty() {
        owner_canonical.to_string()
    } else {
        declaration.canonical_source.clone()
    };
    let declaration_name = if declaration.resolved_name.is_empty() {
        root_name.to_string()
    } else {
        declaration.resolved_name.clone()
    };
    let (final_scope, _) = query_engine
        .resolve_final_prepared_type_target(declaration_scope.as_str(), declaration_name.as_str());

    (!final_scope.is_empty() && final_scope != owner_canonical).then_some(final_scope)
}

/// Returns `true` when `expr` is (or routes through) a named declaration whose
/// own body reaches itself through one or more sibling declarations — a
/// transitively recursive generic helper (e.g. `DotPathKeys` reached via
/// `GetItemKeys`, or `NestedItem` reached via `Extract<NestedItem<T>, U>`).
///
/// Rescuing such a Ref in the declaration's scope restarts the solver in a
/// scope with full local visibility to every sibling helper, which can exhaust
/// the structural-expansion budgets during a single projection call even
/// though the owner-scope result is already a good surface.
pub(crate) fn expr_has_transitively_recursive_generic_root(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    let route_root_name = component_meta_registry_public_utility_route(expr)
        .or_else(|| component_meta_registry_public_indexed_access_route(expr))
        .map(|(root_name, _)| root_name);
    let root_name = match expr {
        verter_semantic::analysis::type_expr::TypeExpr::Ref {
            name,
            type_arguments,
        } if !type_arguments.is_empty() => name.to_string(),
        _ => match route_root_name {
            Some(name) => name,
            None => return false,
        },
    };
    let declaration = query_engine.resolve_type_declaration(owner_canonical, root_name.as_str());
    let scope_canonical = if declaration.canonical_source.is_empty() {
        owner_canonical.to_string()
    } else {
        declaration.canonical_source.clone()
    };
    let resolved_name = if declaration.resolved_name.is_empty() {
        root_name.clone()
    } else {
        declaration.resolved_name.clone()
    };

    named_decl_body_reaches_cycle(
        query_engine,
        scope_canonical.as_str(),
        resolved_name.as_str(),
    )
}

fn named_decl_body_reaches_cycle(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical: &str,
    root_name: &str,
) -> bool {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    fn has_complex_cycle_guard_surface(expr: &TypeExpr) -> bool {
        match expr {
            TypeExpr::Parenthesized(inner) => has_complex_cycle_guard_surface(inner),
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                types.iter().any(has_complex_cycle_guard_surface)
                    || types.iter().any(|ty| !matches!(ty, TypeExpr::Object(_)))
            }
            TypeExpr::Ref { .. }
            | TypeExpr::IndexedAccess { .. }
            | TypeExpr::Conditional { .. }
            | TypeExpr::Mapped { .. }
            | TypeExpr::KeyOf(_)
            | TypeExpr::Rest(_)
            | TypeExpr::TemplateLiteral { .. }
            | TypeExpr::TypeOf(_) => true,
            TypeExpr::Object(_)
            | TypeExpr::Function(_)
            | TypeExpr::Array { .. }
            | TypeExpr::Tuple { .. }
            | TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::Unknown { .. }
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::TypeParameter(_)
            | TypeExpr::Infer { .. } => false,
        }
    }

    fn collect_ref_names(expr: &TypeExpr, out: &mut rustc_hash::FxHashMap<String, bool>) {
        let mut stack = vec![expr];

        while let Some(current) = stack.pop() {
            match current {
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } => {
                    out.entry(name.to_string())
                        .and_modify(|has_args| *has_args |= !type_arguments.is_empty())
                        .or_insert(!type_arguments.is_empty());
                    for argument in type_arguments.iter().rev() {
                        stack.push(argument);
                    }
                }
                TypeExpr::Parenthesized(inner)
                | TypeExpr::Array { element: inner, .. }
                | TypeExpr::KeyOf(inner)
                | TypeExpr::Rest(inner) => stack.push(inner),
                TypeExpr::Tuple { elements, .. } => {
                    for element in elements.iter().rev() {
                        stack.push(&element.ty);
                    }
                }
                TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                    for ty in types.iter().rev() {
                        stack.push(ty);
                    }
                }
                TypeExpr::Object(object) => {
                    for member in object.properties.iter().rev() {
                        match member {
                            ObjectMember::Property(property) => stack.push(&property.ty),
                            ObjectMember::IndexSignature(signature) => {
                                stack.push(&signature.value_type);
                                stack.push(&signature.key_type);
                            }
                            ObjectMember::CallSignature(function)
                            | ObjectMember::ConstructSignature(function) => {
                                if let Some(return_type) = function.return_type.as_ref() {
                                    stack.push(return_type);
                                }
                                for param in function.parameters.iter().rev() {
                                    stack.push(&param.ty);
                                }
                            }
                            ObjectMember::Method(method) => {
                                if let Some(return_type) = method.function.return_type.as_ref() {
                                    stack.push(return_type);
                                }
                                for param in method.function.parameters.iter().rev() {
                                    stack.push(&param.ty);
                                }
                            }
                        }
                    }
                }
                TypeExpr::Function(function) => {
                    if let Some(return_type) = function.return_type.as_ref() {
                        stack.push(return_type);
                    }
                    for param in function.parameters.iter().rev() {
                        stack.push(&param.ty);
                    }
                }
                TypeExpr::IndexedAccess { object, index } => {
                    stack.push(index);
                    stack.push(object);
                }
                TypeExpr::Conditional {
                    check,
                    extends,
                    true_type,
                    false_type,
                } => {
                    stack.push(false_type);
                    stack.push(true_type);
                    stack.push(extends);
                    stack.push(check);
                }
                TypeExpr::Mapped {
                    source,
                    name_type,
                    value,
                    ..
                } => {
                    stack.push(value);
                    if let Some(name_type) = name_type.as_ref() {
                        stack.push(name_type);
                    }
                    stack.push(source);
                }
                TypeExpr::TemplateLiteral { expressions, .. } => {
                    for expression in expressions.iter().rev() {
                        stack.push(expression);
                    }
                }
                _ => {}
            }
        }
    }

    // BFS through declaration bodies reachable from `root_name`. Any
    // declaration body that references itself (directly or transitively via a
    // sibling helper) marks the whole reachable graph as cyclic from
    // `root_name`'s point of view. This is the "expensive-to-solve" signal.
    let mut visited: rustc_hash::FxHashSet<(String, String)> = rustc_hash::FxHashSet::default();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back((scope_canonical.to_string(), root_name.to_string(), false));
    visited.insert((scope_canonical.to_string(), root_name.to_string()));
    // Bounded traversal: if the reachable decl graph is larger than this we
    // treat it as "complex enough that imported-scope rescue is not worth the
    // risk" rather than keep walking.
    let mut remaining_steps: u32 = 64;

    while let Some((scope, name, path_has_complex_signal)) = queue.pop_front() {
        if remaining_steps == 0 {
            return path_has_complex_signal;
        }
        remaining_steps -= 1;

        let Some(body) = query_engine.named_decl_body(scope.as_str(), name.as_str()) else {
            continue;
        };
        let mut refs = rustc_hash::FxHashMap::default();
        collect_ref_names(&body, &mut refs);
        let body_has_complex_signal =
            path_has_complex_signal || has_complex_cycle_guard_surface(&body);
        for (ref_name, ref_has_type_args) in refs {
            let cycle_has_complex_signal = body_has_complex_signal || ref_has_type_args;
            // Direct self-reference at this decl: the body talks about
            // itself through a complex helper surface, which is the
            // canonical recursive-helper pattern (DotPathKeys, NestedItem,
            // IsPlainObject, ...). Plain object self-member routes like
            // `Props['to']` are intentionally excluded.
            if ref_name == name && cycle_has_complex_signal {
                return true;
            }
            let declaration =
                query_engine.resolve_type_declaration(scope.as_str(), ref_name.as_str());
            let next_scope = if declaration.canonical_source.is_empty() {
                scope.clone()
            } else {
                declaration.canonical_source.clone()
            };
            let next_name = if declaration.resolved_name.is_empty() {
                ref_name
            } else {
                declaration.resolved_name
            };
            let key = (next_scope.clone(), next_name.clone());
            if visited.insert(key) {
                queue.push_back((next_scope, next_name, cycle_has_complex_signal));
            }
        }
    }
    false
}

thread_local! {
    /// Step 6.6.A dep-signature accumulator.
    ///
    /// `materialize_component_meta_type_expr_until_stable_full` populates
    /// this thread-local with each dispatch round-trip's
    /// `DepSignature`; `compute_component_meta_state_inner` reads + clears
    /// it before publish and merges the accumulated facts into
    /// `ResolvedComponentMetaState.fact_versions` (D31). The thread-local
    /// is request-scoped — the compute entry point clears it; if any
    /// recursive materialize call accumulates without a matching read,
    /// the next request's compute clears it before populating fresh
    /// facts.
    ///
    /// **Why thread-local, not host-owned cache:** the accumulator is
    /// transient per-request channel state, not a reusable cache. It
    /// crosses caller boundaries (deep materialize stacks), but the
    /// completion-fence design already uses thread-locals for the same
    /// reason. CLAUDE.md "host-owned cache principle" applies to
    /// reusable semantic caches, not request-scoped instrumentation
    /// accumulators.
    static DISPATCH_DEP_SIGNATURE_ACCUMULATOR: std::cell::RefCell<
        Vec<crate::resolver_core::FactVersionRef>,
    > = const { std::cell::RefCell::new(Vec::new()) };
}

/// Reset the per-request dep-signature accumulator. Called at the
/// entry of `compute_component_meta_state_inner` so each request
/// starts with a clean slate.
pub(crate) fn reset_dispatch_dep_signature_accumulator() {
    DISPATCH_DEP_SIGNATURE_ACCUMULATOR.with(|cell| cell.borrow_mut().clear());
}

/// Drain the per-request dep-signature accumulator. Called at publish
/// time in `compute_component_meta_state_inner` so accumulated facts
/// merge into `ResolvedComponentMetaState.fact_versions` (Step 6.6.A).
pub(crate) fn drain_dispatch_dep_signature_accumulator() -> Vec<crate::resolver_core::FactVersionRef>
{
    DISPATCH_DEP_SIGNATURE_ACCUMULATOR.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}

/// Convert dispatch's `DepSignature` (canonical-id + DepVersion pairs)
/// into session-layer `FactVersionRef` entries and merge them into the
/// thread-local accumulator. Deduplicates against entries already in
/// the accumulator on the way in (linear scan; the accumulator is
/// short for a typical request).
fn accumulate_dispatch_dep_signature(sig: &crate::semantic_query::DepSignature) {
    use crate::resolver_core::FactVersionRef;
    use crate::semantic_query::DepVersion;

    DISPATCH_DEP_SIGNATURE_ACCUMULATOR.with(|cell| {
        let mut accumulator = cell.borrow_mut();
        for (canonical, version) in sig.iter() {
            let canonical_id = canonical.as_ref().to_string();
            let fact = match version {
                DepVersion::WholeHash(hash) => FactVersionRef::FileWholeHash {
                    canonical_id,
                    hash: *hash,
                },
                DepVersion::RouteGeneration(_) | DepVersion::ProjectGeneration(_) => {
                    // Route / project generation are coarse-grained
                    // counters; they don't map to a content-hash and
                    // would force the warm-cache validator to invalidate
                    // on unrelated activity. Skip them at the merge
                    // point — `HostFenceValidator`'s revalidation
                    // covers the project-generation lifecycle directly.
                    continue;
                }
            };
            if !accumulator.iter().any(|existing| existing == &fact) {
                accumulator.push(fact);
            }
        }
    });
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn materialize_component_meta_type_expr_until_stable(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    mode: crate::semantic_query::ProjectionMode,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    materialize_component_meta_type_expr_until_stable_full(
        expr,
        scope_canonical_id,
        mode,
        query_engine,
    )
    .type_expr
}

/// Materialize a `TypeExpr` and return both the result and the
/// producing `SemanticNodeId` + accumulated dep_signature
/// ([`MaterializedTypeExpr`]; D31 / D32). Sidecar-capture call sites
/// (Step 9 surface-id propagation) read `.node_id`; the session merges
/// `.dep_signature` into `ResolvedComponentMetaState.fact_versions`
/// before publish (Step 6.6.A).
///
/// The main entry [`materialize_component_meta_type_expr_until_stable`]
/// remains for callers that need only the `TypeExpr` shell — it
/// delegates here and discards `node_id` / `dep_signature`.
///
/// **Body (Step 6.3):** legacy owner-vs-imported scope reconciliation
/// drives the user-visible `type_expr`; a dispatch round-trip
/// (`shallow_lower_type_expr` → `raise_and_reduce`) runs alongside on
/// the SAME expr to supply the `node_id` for sidecar capture and the
/// `dep_signature` for fence merge — no public API duplication. The
/// dispatch result's `type_expr` is informational only at this
/// landing; the legacy path stays primary because it captures the
/// generic-substitution machinery in `materialize_in_scope`'s
/// fixed-point descent + `solve_expr_type_expr` that the dispatch
/// `Instantiate` substitution does not yet replicate for every
/// fixture (notably `defineProps<Props<T>>()` over a script-setup
/// generic with constrained body parameters). When dispatch's
/// substitution handling closes that gap, the legacy path's reads
/// become subsumed and the body collapses to the thin
/// `dispatch.raise_and_reduce(lowered, mode)` form documented in plan
/// §3 Step 6.3.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn materialize_component_meta_type_expr_until_stable_full(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    mode: crate::semantic_query::ProjectionMode,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> crate::project_semantic_dispatch::raise::MaterializedTypeExpr {
    use crate::project_semantic_dispatch::raise::MaterializedTypeExpr;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::NodeScopeId;
    use rustc_hash::FxHashMap;
    use std::sync::Arc;

    // Step 6.2 / D22: count every entry into whole-expression
    // materialization. Memo hits + cold builds both increment so the
    // FAIL-FIRST test discriminates the call-ordering contract at the
    // *entry* boundary, not the build closure.
    #[cfg(test)]
    MTL_CALL_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    // §4.5 items 2-5: per-request memo keyed on `(scope, candidate, mode)`.
    let memo_key = (
        scope_canonical_id.to_string(),
        expr.clone(),
        matches!(mode, crate::semantic_query::ProjectionMode::Navigate),
    );
    if let Some(cached) = query_engine.materialize_memo.get(&memo_key) {
        return cached.clone();
    }

    // Legacy owner-vs-imported reconciliation drives the user-visible
    // type_expr (preserves substitution behaviour for script-setup
    // generics + Pick / Omit utility expansion the dispatch path's
    // Instantiate substitution doesn't yet replicate verbatim).
    let owner_materialized = materialize_component_meta_type_expr_until_stable_in_scope(
        expr,
        scope_canonical_id,
        query_engine,
    );
    let mut chosen_type_expr = if !type_expr_needs_projection_rescue(
        query_engine,
        scope_canonical_id,
        &owner_materialized,
    ) {
        owner_materialized.clone()
    } else if let Some(imported_scope_canonical_id) =
        imported_component_meta_materialization_scope(expr, scope_canonical_id, query_engine)
    {
        if owner_materialized != *expr
            && expr_has_transitively_recursive_generic_root(query_engine, scope_canonical_id, expr)
        {
            owner_materialized.clone()
        } else {
            let imported_materialized = materialize_component_meta_type_expr_until_stable_in_scope(
                expr,
                imported_scope_canonical_id.as_str(),
                query_engine,
            );
            if imported_materialized != owner_materialized
                && component_meta_type_expr_improves(&imported_materialized, &owner_materialized)
                && (!type_expr_needs_projection_rescue(
                    query_engine,
                    imported_scope_canonical_id.as_str(),
                    &imported_materialized,
                ) || type_expr_needs_projection_rescue(
                    query_engine,
                    scope_canonical_id,
                    &owner_materialized,
                ))
            {
                imported_materialized
            } else {
                owner_materialized.clone()
            }
        }
    } else {
        owner_materialized.clone()
    };

    // Run dispatch alongside the legacy result to harvest:
    //   - `node_id`: the producing SemanticNodeId for sidecar capture
    //     (Step 9 surface-id propagation).
    //   - `dep_signature`: accumulated fence facts from every dispatch
    //     subquery so `ResolvedComponentMetaState.fact_versions`
    //     captures the dependency graph correctly (Step 6.6.A fence
    //     merge).
    //
    // Dispatch's `type_expr` is informational here — `chosen_type_expr`
    // already carries the user-visible result. Mode is threaded so the
    // dispatch path itself observes Navigate vs. Expanded for the
    // accompanying carrier emission, even though we use the legacy
    // body content.
    let scope_payload = query_engine.scope_payload_for_scope(scope_canonical_id);
    let host = query_engine.host();
    let dispatch = ProjectSemanticDispatch::new(host);
    let env: FxHashMap<String, crate::semantic_query::SemanticNodeId> = FxHashMap::default();
    let whole_hash = host
        .shallow_file_state(scope_canonical_id)
        .map(|state| state.whole_hash)
        .unwrap_or_default();
    let scope = NodeScopeId::File {
        canonical_id: Arc::from(scope_canonical_id),
        whole_hash,
        local_scope: None,
    };
    let name_resolution = rustc_hash::FxHashMap::default();
    let mut substitutions: Vec<(Arc<str>, crate::semantic_query::SemanticNodeId)> = Vec::new();
    let lowered = dispatch.shallow_lower_type_expr(
        expr,
        &env,
        &scope,
        &name_resolution,
        scope_payload.as_deref(),
        &mut substitutions,
        mode,
    );
    let dispatch_materialized = dispatch.raise_and_reduce(lowered, mode);

    // In Navigate mode the dispatch result IS the user-visible answer:
    // lazy carriers are the entire point of Navigate, and the legacy
    // path doesn't preserve them. So Navigate callers get the dispatch
    // type_expr; Expanded callers get the legacy reconciliation.
    if matches!(mode, crate::semantic_query::ProjectionMode::Navigate) {
        chosen_type_expr = dispatch_materialized.type_expr.clone();
    }

    // Step 6.6.A: accumulate dispatch's dep_signature into the
    // per-request thread-local so compute_component_meta_state_inner
    // can merge the facts into ResolvedComponentMetaState.fact_versions
    // before publish. Each materialize call contributes its own
    // dispatch-side fact set; the accumulator deduplicates.
    accumulate_dispatch_dep_signature(&dispatch_materialized.dep_signature);

    let materialized = MaterializedTypeExpr {
        node_id: dispatch_materialized.node_id,
        type_expr: chosen_type_expr,
        dep_signature: dispatch_materialized.dep_signature,
    };

    query_engine
        .materialize_memo
        .insert(memo_key, materialized.clone());
    materialized
}

fn type_expr_has_package_backed_root(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

    match expr {
        TypeExpr::Ref { name, .. } => {
            component_meta_ref_resolves_to_package(scope_canonical_id, name.as_ref(), query_engine)
        }
        TypeExpr::IndexedAccess { object, .. }
        | TypeExpr::Array {
            element: object, ..
        }
        | TypeExpr::KeyOf(object)
        | TypeExpr::Rest(object)
        | TypeExpr::Parenthesized(object) => {
            type_expr_has_package_backed_root(object, scope_canonical_id, query_engine)
        }
        TypeExpr::Tuple { elements, .. } => elements.iter().any(|element| {
            type_expr_has_package_backed_root(&element.ty, scope_canonical_id, query_engine)
        }),
        _ => false,
    }
}

fn type_expr_has_package_backed_object_like_root(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    fn root_name(expr: &verter_semantic::analysis::type_expr::TypeExpr) -> Option<String> {
        use verter_semantic::analysis::type_expr::TypeExpr;

        match expr {
            TypeExpr::Parenthesized(inner) => root_name(inner),
            TypeExpr::IndexedAccess { object, .. } => root_name(object),
            TypeExpr::Ref {
                name,
                type_arguments,
            } if matches!(name.as_ref(), "Pick" | "Omit") && type_arguments.len() == 2 => {
                crate::resolver_core::component_meta_registry::component_meta_registry_ref_name(
                    &type_arguments[0],
                )
                .map(str::to_string)
            }
            TypeExpr::Ref { name, .. } => Some(name.to_string()),
            _ => None,
        }
    }

    let Some(root_name) = root_name(expr) else {
        return false;
    };

    let declaration = query_engine.resolve_type_declaration(scope_canonical_id, root_name.as_str());
    let declaration_scope = if declaration.canonical_source.is_empty() {
        scope_canonical_id.to_string()
    } else {
        declaration.canonical_source.clone()
    };
    if !declaration_scope.contains("/node_modules/") {
        return false;
    }

    if matches!(
        declaration.kind,
        crate::resolver_core::ResolvedDeclarationKind::Interface
            | crate::resolver_core::ResolvedDeclarationKind::Class,
    ) {
        return true;
    }

    let declaration_name = if declaration.resolved_name.is_empty() {
        root_name.clone()
    } else {
        declaration.resolved_name
    };
    let (target_scope, target_name) = query_engine
        .resolve_final_prepared_type_target(declaration_scope.as_str(), declaration_name.as_str());
    query_engine
        .named_decl_body(target_scope.as_str(), target_name.as_str())
        .is_some_and(|body| {
            crate::resolver_core::component_meta_registry::component_meta_registry_has_explicit_object_surface(&body)
        })
}

fn type_expr_needs_member_route_materialization(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            type_arguments.is_empty()
                && !component_meta_ref_resolves_to_package(
                    scope_canonical_id,
                    name.as_ref(),
                    query_engine,
                )
        }
        TypeExpr::TypeOf(_) | TypeExpr::IndexedAccess { .. } | TypeExpr::TypeParameter(_) => {
            !expr_has_transitively_recursive_generic_root(query_engine, scope_canonical_id, expr)
                && !type_expr_has_package_backed_root(expr, scope_canonical_id, query_engine)
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => {
            type_expr_needs_member_route_materialization(element, scope_canonical_id, query_engine)
        }
        TypeExpr::Tuple { elements, .. } => elements.iter().any(|element| {
            type_expr_needs_member_route_materialization(
                &element.ty,
                scope_canonical_id,
                query_engine,
            )
        }),
        _ => false,
    }
}

fn type_expr_is_slots_member_route(expr: &verter_semantic::analysis::type_expr::TypeExpr) -> bool {
    use verter_semantic::analysis::type_expr::{LiteralValue, TypeExpr};

    match expr {
        TypeExpr::IndexedAccess { index, .. } => matches!(
            index.as_ref(),
            TypeExpr::Literal(LiteralValue::String(name)) if name.as_str() == "slots"
        ),
        TypeExpr::Parenthesized(inner) => type_expr_is_slots_member_route(inner),
        _ => false,
    }
}

pub(crate) fn materialize_member_route_from_alias_body_in_owner_scope(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    use crate::resolver_core::component_meta_registry::raw_member_path_leaf;

    fn materialized_member_path_leaf(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        path: &[String],
        scope_canonical_id: &str,
        engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
        let mut leaf = expr.clone();
        for member_name in path {
            if let Some(next) = raw_member_path_leaf(&leaf, std::slice::from_ref(member_name)) {
                leaf = next;
                continue;
            }

            let materialize_scope =
                imported_component_meta_materialization_scope(&leaf, scope_canonical_id, engine)
                    .unwrap_or_else(|| scope_canonical_id.to_string());
            let materialized = materialize_component_meta_member_surface_expr(
                &leaf,
                materialize_scope.as_str(),
                engine,
                true,
            );
            if materialized == leaf {
                return None;
            }
            leaf = raw_member_path_leaf(&materialized, std::slice::from_ref(member_name))?;
        }
        Some(leaf)
    }

    let (root_name, route) = component_meta_registry_public_utility_route(expr)
        .or_else(|| component_meta_registry_public_indexed_access_route(expr))?;
    let declaration = engine.resolve_type_declaration(scope_canonical_id, root_name.as_str());
    let declaration_scope = if declaration.canonical_source.is_empty() {
        scope_canonical_id.to_string()
    } else {
        declaration.canonical_source
    };
    let declaration_name = if declaration.resolved_name.is_empty() {
        root_name.clone()
    } else {
        declaration.resolved_name
    };
    let body = engine.named_decl_body(declaration_scope.as_str(), declaration_name.as_str())?;
    // D-Cutover §5.8: `project_type_surface_expr` now routes the full
    // projection through dispatch (`dispatch_projected_surface` →
    // `dispatch_root_instantiated` → Instantiate). The pre-§5.8
    // `solve_scoped` fallback was a duplicate solver path and is
    // retired with the solver subsystem.
    let materialized_body = engine
        .project_type_surface_expr(scope_canonical_id, root_name.as_str())
        .or_else(|| {
            let materialized = materialize_component_meta_type_expr_until_stable(
                &body,
                scope_canonical_id,
                crate::semantic_query::ProjectionMode::Expanded,
                engine,
            );
            (materialized != body).then_some(materialized)
        })?;

    let leaf = match route {
        crate::resolver_core::RouteDemand::Whole => materialized_body,
        crate::resolver_core::RouteDemand::MemberPath(path) if path.is_empty() => materialized_body,
        crate::resolver_core::RouteDemand::MemberPath(path) => {
            materialized_member_path_leaf(&materialized_body, &path, scope_canonical_id, engine)?
        }
        crate::resolver_core::RouteDemand::Pick(ref members)
        | crate::resolver_core::RouteDemand::Omit(ref members) => {
            query_engine::project_route_surface(
                engine,
                scope_canonical_id,
                &materialized_body,
                &route,
                members,
            )?
        }
    };

    let materialized_leaf = materialize_component_meta_type_expr_until_stable(
        &leaf,
        scope_canonical_id,
        crate::semantic_query::ProjectionMode::Expanded,
        engine,
    );
    Some(if materialized_leaf != leaf {
        materialized_leaf
    } else {
        leaf
    })
}

mod query_engine {
    use verter_semantic::analysis::type_expr::{
        ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
    };

    pub(crate) fn project_route_surface(
        _engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
        _scope_canonical_id: &str,
        materialized_body: &TypeExpr,
        route: &crate::resolver_core::RouteDemand,
        members: &[String],
    ) -> Option<TypeExpr> {
        match route {
            crate::resolver_core::RouteDemand::Pick(_) => {
                let TypeExpr::Object(object) = materialized_body else {
                    return None;
                };
                let members: rustc_hash::FxHashSet<_> =
                    members.iter().map(String::as_str).collect();
                Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                    properties: object
                        .properties
                        .iter()
                        .filter(|member| match member {
                            ObjectMember::Property(property) => {
                                members.contains(property.name.as_str())
                            }
                            ObjectMember::Method(method) => members.contains(method.name.as_str()),
                            _ => false,
                        })
                        .cloned()
                        .collect(),
                })))
            }
            crate::resolver_core::RouteDemand::Omit(_) => {
                let TypeExpr::Object(object) = materialized_body else {
                    return None;
                };
                let members: rustc_hash::FxHashSet<_> =
                    members.iter().map(String::as_str).collect();
                Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                    properties: object
                        .properties
                        .iter()
                        .filter(|member| match member {
                            ObjectMember::Property(property) => {
                                !members.contains(property.name.as_str())
                            }
                            ObjectMember::Method(method) => !members.contains(method.name.as_str()),
                            _ => true,
                        })
                        .cloned()
                        .collect(),
                })))
            }
            crate::resolver_core::RouteDemand::Whole => Some(materialized_body.clone()),
            crate::resolver_core::RouteDemand::MemberPath(path) => {
                let leaf = super::component_meta_registry_raw_member_path_surface(
                    materialized_body,
                    path,
                )?;
                Some(path.iter().rfold(leaf, |child, member| {
                    TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                        properties: vec![ObjectMember::Property(ObjectProperty {
                            name: member.clone(),
                            ty: child,
                            optional: true,
                            readonly: false,
                        })],
                    }))
                }))
            }
        }
    }
}

fn parsed_field_raw_type(
    field: &verter_semantic::analysis::type_expand::ExpandedField,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    field
        .raw_type
        .as_deref()
        .map(verter_semantic::analysis::type_expr_lower::parse_type_annotation)
        .filter(|expr| !expr.is_unknown())
}

fn interface_body_has_members_needing_materialization(
    body: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};
    fn member_type_needs_materialization(ty: &TypeExpr) -> bool {
        match ty {
            TypeExpr::IndexedAccess { .. }
            | TypeExpr::Mapped { .. }
            | TypeExpr::Conditional { .. } => true,
            TypeExpr::Parenthesized(inner) => member_type_needs_materialization(inner),
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                types.iter().any(member_type_needs_materialization)
            }
            _ => false,
        }
    }
    match body {
        TypeExpr::Object(obj) => obj.properties.iter().any(|member| match member {
            ObjectMember::Property(prop) => member_type_needs_materialization(&prop.ty),
            _ => false,
        }),
        TypeExpr::Intersection(types) => types
            .iter()
            .any(interface_body_has_members_needing_materialization),
        _ => false,
    }
}

fn top_level_imported_ref_can_stay_symbolic(
    scope_canonical_id: &str,
    name: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    let declaration = query_engine.resolve_type_declaration(scope_canonical_id, name);
    let declaration_scope = if declaration.canonical_source.is_empty() {
        scope_canonical_id.to_string()
    } else {
        declaration.canonical_source.clone()
    };
    let declaration_name = if declaration.resolved_name.is_empty() {
        name.to_string()
    } else {
        declaration.resolved_name.clone()
    };
    let (target_scope, target_name) = query_engine
        .resolve_final_prepared_type_target(declaration_scope.as_str(), declaration_name.as_str());
    let target_declaration = query_engine
        .resolve_direct_prepared_type_declaration_metadata(
            target_scope.as_str(),
            target_name.as_str(),
        )
        .unwrap_or(declaration);

    if matches!(
        target_declaration.kind,
        crate::resolver_core::ResolvedDeclarationKind::Interface
            | crate::resolver_core::ResolvedDeclarationKind::Class,
    ) {
        // Interfaces with members that need materialization (IndexedAccess,
        // Mapped types) should not stay symbolic — the consumer needs the
        // concrete member shapes.
        let body_needs_materialization = query_engine
            .named_decl_body(target_scope.as_str(), target_name.as_str())
            .is_some_and(|body| interface_body_has_members_needing_materialization(&body));
        if !body_needs_materialization {
            return true;
        }
    }

    query_engine
        .named_decl_body(target_scope.as_str(), target_name.as_str())
        .is_some_and(|body| {
            component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                &body,
                target_scope.as_str(),
                query_engine,
            )
        })
}

fn field_should_preserve_shallow_symbolic_raw_type(
    scope_canonical_id: &str,
    field: &verter_semantic::analysis::type_expand::ExpandedField,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    let Some(raw) = parsed_field_raw_type(field) else {
        return false;
    };

    match &raw {
        verter_semantic::analysis::type_expr::TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => top_level_imported_ref_can_stay_symbolic(
            scope_canonical_id,
            name.as_ref(),
            query_engine,
        ),
        _ if component_meta_registry_public_utility_route(&raw).is_some() => {
            type_expr_has_package_backed_object_like_root(&raw, scope_canonical_id, query_engine)
        }
        verter_semantic::analysis::type_expr::TypeExpr::IndexedAccess { .. } => {
            type_expr_has_package_backed_object_like_root(&raw, scope_canonical_id, query_engine)
        }
        verter_semantic::analysis::type_expr::TypeExpr::TypeOf(_)
        | verter_semantic::analysis::type_expr::TypeExpr::TypeParameter(_) => false,
        _ => {
            query_engine.should_preserve_shallow_field_expr(scope_canonical_id, &raw)
                && !type_expr_needs_member_route_materialization(
                    &raw,
                    scope_canonical_id,
                    query_engine,
                )
        }
    }
}

fn define_props_member_can_stay_symbolic_without_rescue(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner)
        | TypeExpr::Array { element: inner, .. }
        | TypeExpr::KeyOf(inner)
        | TypeExpr::Rest(inner) => define_props_member_can_stay_symbolic_without_rescue(
            inner,
            scope_canonical_id,
            query_engine,
        ),
        TypeExpr::Tuple { elements, .. } => elements.iter().all(|element| {
            define_props_member_can_stay_symbolic_without_rescue(
                &element.ty,
                scope_canonical_id,
                query_engine,
            )
        }),
        TypeExpr::Union(types) => types.iter().all(|ty| {
            define_props_member_can_stay_symbolic_without_rescue(
                ty,
                scope_canonical_id,
                query_engine,
            )
        }),
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => {
            let declaration = query_engine.resolve_type_declaration(scope_canonical_id, name);
            let declaration_scope = if declaration.canonical_source.is_empty() {
                scope_canonical_id
            } else {
                declaration.canonical_source.as_str()
            };
            let resolved_name = if declaration.resolved_name.is_empty() {
                name.as_ref()
            } else {
                declaration.resolved_name.as_str()
            };
            query_engine
                .named_decl_body(declaration_scope, resolved_name)
                .is_none()
                || (declaration_scope != scope_canonical_id
                    && top_level_imported_ref_can_stay_symbolic(
                        scope_canonical_id,
                        name.as_ref(),
                        query_engine,
                    ))
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::Function(_) => true,
        _ => false,
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn materialize_component_meta_field_types(
    scope_canonical_id: &str,
    snapshot: &FileAnalysisSnapshot,
    eval_source: &str,
    resolved_macros: &[ResolvedMacroMeta],
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) {
    fn rescue_field(
        scope_canonical_id: &str,
        field: &mut verter_semantic::analysis::type_expand::ExpandedField,
        query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) {
        if !type_expr_needs_projection_rescue(query_engine, scope_canonical_id, &field.r#type) {
            return;
        }

        let materialize_scope_canonical_id = imported_component_meta_materialization_scope(
            &field.r#type,
            scope_canonical_id,
            query_engine,
        )
        .or_else(|| {
            parsed_field_raw_type(field).as_ref().and_then(|raw| {
                imported_component_meta_materialization_scope(raw, scope_canonical_id, query_engine)
            })
        })
        .unwrap_or_else(|| scope_canonical_id.to_string());
        let rescued = materialize_component_meta_type_expr_until_stable(
            &field.r#type,
            materialize_scope_canonical_id.as_str(),
            crate::semantic_query::ProjectionMode::Expanded,
            query_engine,
        );
        if rescued != field.r#type {
            field.r#type = rescued;
        }
    }

    fn route_leaf_beats_wrapper_object(
        candidate: &verter_semantic::analysis::type_expr::TypeExpr,
        current: &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> bool {
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        let TypeExpr::Object(object) = current else {
            return false;
        };
        let [ObjectMember::Property(property)] = object.properties.as_slice() else {
            return false;
        };
        property.ty == *candidate
    }

    fn type_expr_contains_named_recursive_ref(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        target_name: &str,
    ) -> bool {
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        match expr {
            TypeExpr::RecursiveRef { name, .. } => name.as_ref() == target_name,
            TypeExpr::Parenthesized(inner)
            | TypeExpr::Array { element: inner, .. }
            | TypeExpr::KeyOf(inner)
            | TypeExpr::Rest(inner) => type_expr_contains_named_recursive_ref(inner, target_name),
            TypeExpr::Tuple { elements, .. } => elements
                .iter()
                .any(|element| type_expr_contains_named_recursive_ref(&element.ty, target_name)),
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
                .iter()
                .any(|ty| type_expr_contains_named_recursive_ref(ty, target_name)),
            TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                ObjectMember::Property(property) => {
                    type_expr_contains_named_recursive_ref(&property.ty, target_name)
                }
                ObjectMember::Method(method) => {
                    method.function.parameters.iter().any(|parameter| {
                        type_expr_contains_named_recursive_ref(&parameter.ty, target_name)
                    }) || method
                        .function
                        .return_type
                        .as_deref()
                        .is_some_and(|return_type| {
                            type_expr_contains_named_recursive_ref(return_type, target_name)
                        })
                }
                ObjectMember::IndexSignature(signature) => {
                    type_expr_contains_named_recursive_ref(&signature.key_type, target_name)
                        || type_expr_contains_named_recursive_ref(
                            &signature.value_type,
                            target_name,
                        )
                }
                ObjectMember::CallSignature(function)
                | ObjectMember::ConstructSignature(function) => {
                    function.parameters.iter().any(|parameter| {
                        type_expr_contains_named_recursive_ref(&parameter.ty, target_name)
                    }) || function.return_type.as_deref().is_some_and(|return_type| {
                        type_expr_contains_named_recursive_ref(return_type, target_name)
                    })
                }
            }),
            TypeExpr::Function(function) => {
                function.parameters.iter().any(|parameter| {
                    type_expr_contains_named_recursive_ref(&parameter.ty, target_name)
                }) || function.return_type.as_deref().is_some_and(|return_type| {
                    type_expr_contains_named_recursive_ref(return_type, target_name)
                })
            }
            TypeExpr::IndexedAccess { object, index } => {
                type_expr_contains_named_recursive_ref(object, target_name)
                    || type_expr_contains_named_recursive_ref(index, target_name)
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                type_expr_contains_named_recursive_ref(check, target_name)
                    || type_expr_contains_named_recursive_ref(extends, target_name)
                    || type_expr_contains_named_recursive_ref(true_type, target_name)
                    || type_expr_contains_named_recursive_ref(false_type, target_name)
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                type_expr_contains_named_recursive_ref(source, target_name)
                    || type_expr_contains_named_recursive_ref(value, target_name)
                    || name_type.as_deref().is_some_and(|name_type| {
                        type_expr_contains_named_recursive_ref(name_type, target_name)
                    })
            }
            TypeExpr::TemplateLiteral { expressions, .. } => expressions
                .iter()
                .any(|expr| type_expr_contains_named_recursive_ref(expr, target_name)),
            TypeExpr::Ref { type_arguments, .. } => type_arguments
                .iter()
                .any(|arg| type_expr_contains_named_recursive_ref(arg, target_name)),
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::Unknown { .. } => false,
        }
    }

    fn expand_named_recursive_refs_one_layer(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        target_name: &str,
        replacement: &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> verter_semantic::analysis::type_expr::TypeExpr {
        use std::sync::Arc;
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        match expr {
            TypeExpr::RecursiveRef { name, .. } if name.as_ref() == target_name => {
                replacement.clone()
            }
            TypeExpr::Parenthesized(inner) => TypeExpr::Parenthesized(Arc::new(
                expand_named_recursive_refs_one_layer(inner, target_name, replacement),
            )),
            TypeExpr::Array { element, readonly } => TypeExpr::Array {
                element: Arc::new(expand_named_recursive_refs_one_layer(
                    element,
                    target_name,
                    replacement,
                )),
                readonly: *readonly,
            },
            TypeExpr::KeyOf(inner) => TypeExpr::KeyOf(Arc::new(
                expand_named_recursive_refs_one_layer(inner, target_name, replacement),
            )),
            TypeExpr::Rest(inner) => TypeExpr::Rest(Arc::new(
                expand_named_recursive_refs_one_layer(inner, target_name, replacement),
            )),
            TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
                elements: elements
                    .iter()
                    .map(
                        |element| verter_semantic::analysis::type_expr::TupleElement {
                            label: element.label.clone(),
                            ty: expand_named_recursive_refs_one_layer(
                                &element.ty,
                                target_name,
                                replacement,
                            ),
                            optional: element.optional,
                            rest: element.rest,
                        },
                    )
                    .collect(),
                readonly: *readonly,
            },
            TypeExpr::Union(types) => TypeExpr::Union(
                types
                    .iter()
                    .map(|ty| expand_named_recursive_refs_one_layer(ty, target_name, replacement))
                    .collect(),
            ),
            TypeExpr::Intersection(types) => TypeExpr::Intersection(
                types
                    .iter()
                    .map(|ty| expand_named_recursive_refs_one_layer(ty, target_name, replacement))
                    .collect(),
            ),
            TypeExpr::Object(object) => {
                let mut next = object.as_ref().clone();
                for member in &mut next.properties {
                    match member {
                        ObjectMember::Property(property) => {
                            property.ty = expand_named_recursive_refs_one_layer(
                                &property.ty,
                                target_name,
                                replacement,
                            );
                        }
                        ObjectMember::Method(method) => {
                            for parameter in &mut method.function.parameters {
                                parameter.ty = expand_named_recursive_refs_one_layer(
                                    &parameter.ty,
                                    target_name,
                                    replacement,
                                );
                            }
                            if let Some(return_type) = method.function.return_type.as_mut() {
                                *return_type = Arc::new(expand_named_recursive_refs_one_layer(
                                    return_type,
                                    target_name,
                                    replacement,
                                ));
                            }
                        }
                        ObjectMember::IndexSignature(signature) => {
                            signature.key_type = expand_named_recursive_refs_one_layer(
                                &signature.key_type,
                                target_name,
                                replacement,
                            );
                            signature.value_type = expand_named_recursive_refs_one_layer(
                                &signature.value_type,
                                target_name,
                                replacement,
                            );
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            for parameter in &mut function.parameters {
                                parameter.ty = expand_named_recursive_refs_one_layer(
                                    &parameter.ty,
                                    target_name,
                                    replacement,
                                );
                            }
                            if let Some(return_type) = function.return_type.as_mut() {
                                *return_type = Arc::new(expand_named_recursive_refs_one_layer(
                                    return_type,
                                    target_name,
                                    replacement,
                                ));
                            }
                        }
                    }
                }
                TypeExpr::Object(Arc::new(next))
            }
            TypeExpr::Function(function) => {
                let mut next = function.as_ref().clone();
                for parameter in &mut next.parameters {
                    parameter.ty = expand_named_recursive_refs_one_layer(
                        &parameter.ty,
                        target_name,
                        replacement,
                    );
                }
                if let Some(return_type) = next.return_type.as_mut() {
                    *return_type = Arc::new(expand_named_recursive_refs_one_layer(
                        return_type,
                        target_name,
                        replacement,
                    ));
                }
                TypeExpr::Function(Arc::new(next))
            }
            TypeExpr::IndexedAccess { object, index } => TypeExpr::IndexedAccess {
                object: Arc::new(expand_named_recursive_refs_one_layer(
                    object,
                    target_name,
                    replacement,
                )),
                index: Arc::new(expand_named_recursive_refs_one_layer(
                    index,
                    target_name,
                    replacement,
                )),
            },
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => TypeExpr::Conditional {
                check: Arc::new(expand_named_recursive_refs_one_layer(
                    check,
                    target_name,
                    replacement,
                )),
                extends: Arc::new(expand_named_recursive_refs_one_layer(
                    extends,
                    target_name,
                    replacement,
                )),
                true_type: Arc::new(expand_named_recursive_refs_one_layer(
                    true_type,
                    target_name,
                    replacement,
                )),
                false_type: Arc::new(expand_named_recursive_refs_one_layer(
                    false_type,
                    target_name,
                    replacement,
                )),
            },
            TypeExpr::Mapped {
                parameter,
                source,
                optional,
                readonly,
                name_type,
                value,
            } => TypeExpr::Mapped {
                parameter: parameter.clone(),
                source: Arc::new(expand_named_recursive_refs_one_layer(
                    source,
                    target_name,
                    replacement,
                )),
                optional: *optional,
                readonly: *readonly,
                name_type: name_type.as_ref().map(|name_type| {
                    Arc::new(expand_named_recursive_refs_one_layer(
                        name_type,
                        target_name,
                        replacement,
                    ))
                }),
                value: Arc::new(expand_named_recursive_refs_one_layer(
                    value,
                    target_name,
                    replacement,
                )),
            },
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => TypeExpr::TemplateLiteral {
                quasis: quasis.clone(),
                expressions: expressions
                    .iter()
                    .map(|expr| {
                        expand_named_recursive_refs_one_layer(expr, target_name, replacement)
                    })
                    .collect(),
            },
            TypeExpr::RecursiveRef { .. }
            | TypeExpr::Ref { .. }
            | TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::Unknown { .. } => expr.clone(),
        }
    }

    fn rewrite_named_self_refs_to_recursive_ref(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        target_name: &str,
    ) -> verter_semantic::analysis::type_expr::TypeExpr {
        use std::sync::Arc;
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        match expr {
            TypeExpr::Ref {
                name,
                type_arguments,
            } if name.as_ref() == target_name && type_arguments.is_empty() => {
                TypeExpr::recursive_ref(target_name, Vec::new())
            }
            TypeExpr::Parenthesized(inner) => TypeExpr::Parenthesized(Arc::new(
                rewrite_named_self_refs_to_recursive_ref(inner, target_name),
            )),
            TypeExpr::Array { element, readonly } => TypeExpr::Array {
                element: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    element,
                    target_name,
                )),
                readonly: *readonly,
            },
            TypeExpr::KeyOf(inner) => TypeExpr::KeyOf(Arc::new(
                rewrite_named_self_refs_to_recursive_ref(inner, target_name),
            )),
            TypeExpr::Rest(inner) => TypeExpr::Rest(Arc::new(
                rewrite_named_self_refs_to_recursive_ref(inner, target_name),
            )),
            TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
                elements: elements
                    .iter()
                    .map(
                        |element| verter_semantic::analysis::type_expr::TupleElement {
                            label: element.label.clone(),
                            ty: rewrite_named_self_refs_to_recursive_ref(&element.ty, target_name),
                            optional: element.optional,
                            rest: element.rest,
                        },
                    )
                    .collect(),
                readonly: *readonly,
            },
            TypeExpr::Union(types) => TypeExpr::Union(
                types
                    .iter()
                    .map(|ty| rewrite_named_self_refs_to_recursive_ref(ty, target_name))
                    .collect(),
            ),
            TypeExpr::Intersection(types) => TypeExpr::Intersection(
                types
                    .iter()
                    .map(|ty| rewrite_named_self_refs_to_recursive_ref(ty, target_name))
                    .collect(),
            ),
            TypeExpr::Object(object) => {
                let mut next = object.as_ref().clone();
                for member in &mut next.properties {
                    match member {
                        ObjectMember::Property(property) => {
                            property.ty =
                                rewrite_named_self_refs_to_recursive_ref(&property.ty, target_name);
                        }
                        ObjectMember::Method(method) => {
                            for parameter in &mut method.function.parameters {
                                parameter.ty = rewrite_named_self_refs_to_recursive_ref(
                                    &parameter.ty,
                                    target_name,
                                );
                            }
                            if let Some(return_type) = method.function.return_type.as_mut() {
                                *return_type = Arc::new(rewrite_named_self_refs_to_recursive_ref(
                                    return_type,
                                    target_name,
                                ));
                            }
                        }
                        ObjectMember::IndexSignature(signature) => {
                            signature.key_type = rewrite_named_self_refs_to_recursive_ref(
                                &signature.key_type,
                                target_name,
                            );
                            signature.value_type = rewrite_named_self_refs_to_recursive_ref(
                                &signature.value_type,
                                target_name,
                            );
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            for parameter in &mut function.parameters {
                                parameter.ty = rewrite_named_self_refs_to_recursive_ref(
                                    &parameter.ty,
                                    target_name,
                                );
                            }
                            if let Some(return_type) = function.return_type.as_mut() {
                                *return_type = Arc::new(rewrite_named_self_refs_to_recursive_ref(
                                    return_type,
                                    target_name,
                                ));
                            }
                        }
                    }
                }
                TypeExpr::Object(Arc::new(next))
            }
            TypeExpr::Function(function) => {
                let mut next = function.as_ref().clone();
                for parameter in &mut next.parameters {
                    parameter.ty =
                        rewrite_named_self_refs_to_recursive_ref(&parameter.ty, target_name);
                }
                if let Some(return_type) = next.return_type.as_mut() {
                    *return_type = Arc::new(rewrite_named_self_refs_to_recursive_ref(
                        return_type,
                        target_name,
                    ));
                }
                TypeExpr::Function(Arc::new(next))
            }
            TypeExpr::IndexedAccess { object, index } => TypeExpr::IndexedAccess {
                object: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    object,
                    target_name,
                )),
                index: Arc::new(rewrite_named_self_refs_to_recursive_ref(index, target_name)),
            },
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => TypeExpr::Conditional {
                check: Arc::new(rewrite_named_self_refs_to_recursive_ref(check, target_name)),
                extends: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    extends,
                    target_name,
                )),
                true_type: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    true_type,
                    target_name,
                )),
                false_type: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    false_type,
                    target_name,
                )),
            },
            TypeExpr::Mapped {
                parameter,
                source,
                value,
                optional,
                readonly,
                name_type,
            } => TypeExpr::Mapped {
                parameter: parameter.clone(),
                source: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    source,
                    target_name,
                )),
                value: Arc::new(rewrite_named_self_refs_to_recursive_ref(value, target_name)),
                optional: *optional,
                readonly: *readonly,
                name_type: name_type.as_ref().map(|name_type| {
                    Arc::new(rewrite_named_self_refs_to_recursive_ref(
                        name_type,
                        target_name,
                    ))
                }),
            },
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => TypeExpr::TemplateLiteral {
                quasis: quasis.clone(),
                expressions: expressions
                    .iter()
                    .map(|expr| rewrite_named_self_refs_to_recursive_ref(expr, target_name))
                    .collect(),
            },
            TypeExpr::Ref { .. }
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::Unknown { .. } => expr.clone(),
        }
    }

    fn indexed_access_alias_body_transport(
        scope_canonical_id: &str,
        raw: &verter_semantic::analysis::type_expr::TypeExpr,
        query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
        let (root_symbol, route) = component_meta_registry_public_indexed_access_route(raw)?;
        let crate::resolver_core::RouteDemand::MemberPath(path) = route else {
            return None;
        };
        let [member_name] = path.as_slice() else {
            return None;
        };

        let member_ty = query_engine.prepared_member_raw_type(
            scope_canonical_id,
            root_symbol.as_str(),
            member_name,
        )?;
        let verter_semantic::analysis::type_expr::TypeExpr::Ref {
            name,
            type_arguments,
        } = member_ty
        else {
            return None;
        };
        if !type_arguments.is_empty() {
            return None;
        }

        let declaration = query_engine.resolve_type_declaration(scope_canonical_id, name.as_ref());
        let declaration_scope = if declaration.canonical_source.is_empty() {
            scope_canonical_id.to_string()
        } else {
            declaration.canonical_source
        };
        let declaration_name = if declaration.resolved_name.is_empty() {
            name.as_ref().to_string()
        } else {
            declaration.resolved_name
        };
        let (target_scope, target_name) = query_engine.resolve_final_prepared_type_target(
            declaration_scope.as_str(),
            declaration_name.as_str(),
        );
        let body = query_engine.named_decl_body(target_scope.as_str(), target_name.as_str())?;
        let replacement = rewrite_named_self_refs_to_recursive_ref(&body, target_name.as_str());
        matches!(
            replacement,
            verter_semantic::analysis::type_expr::TypeExpr::Union(_)
                | verter_semantic::analysis::type_expr::TypeExpr::Primitive(_),
        )
        .then_some(replacement)
    }

    let params =
        verter_semantic::analysis::type_eval_build::collect_define_macro_type_params(eval_source);
    let mut prop_member_routes = rustc_hash::FxHashMap::<
        String,
        Vec<verter_semantic::analysis::type_expr::TypeExpr>,
    >::default();
    let mut slot_binding_scope_hints = rustc_hash::FxHashMap::<String, Vec<String>>::default();
    let mut define_props_index = 0usize;
    for (macro_index, mac) in snapshot.macros.iter().enumerate() {
        if !mac.is_type_based {
            continue;
        }

        match mac.kind {
            verter_semantic::analysis::AnalyzedMacroKind::DefineProps => {
                if let Some(lowered) = params.define_props.get(define_props_index) {
                    let mut prop_names = rustc_hash::FxHashSet::<String>::default();
                    prop_names.extend(mac.prop_fields.iter().map(|field| field.name.clone()));
                    for resolved in resolved_macros.iter().filter(|resolved| {
                        resolved.macro_index == macro_index
                            && resolved.macro_kind
                                == verter_semantic::analysis::AnalyzedMacroKind::DefineProps
                    }) {
                        prop_names.extend(resolved.props.iter().map(|field| field.name.clone()));
                    }
                    for prop_name in prop_names {
                        prop_member_routes
                            .entry(prop_name)
                            .or_default()
                            .push(lowered.clone());
                    }
                }
                define_props_index += 1;
            }
            verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => {
                for resolved in resolved_macros.iter().filter(|resolved| {
                    resolved.macro_index == macro_index
                        && resolved.macro_kind
                            == verter_semantic::analysis::AnalyzedMacroKind::DefineSlots
                }) {
                    let declaration_scope = resolved.declaration.canonical_source.as_str();
                    if declaration_scope.is_empty() {
                        continue;
                    }
                    for slot in &resolved.slots {
                        for binding in &slot.bindings {
                            let entry = slot_binding_scope_hints
                                .entry(format!("{}.{}", slot.name, binding.name))
                                .or_default();
                            if !entry.iter().any(|scope| scope == declaration_scope) {
                                entry.push(declaration_scope.to_string());
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    for field in &mut evaluated_types.props {
        let preserve_raw = field_should_preserve_shallow_symbolic_raw_type(
            scope_canonical_id,
            field,
            query_engine,
        );
        if crate::host_manage::component_meta_debug_enabled() {
            crate::host_manage::component_meta_debug(format!(
                "FIELD_MATERIALIZE owner={} field={} raw={:?} current={:?} preserve_raw={}",
                scope_canonical_id, field.name, field.raw_type, field.r#type, preserve_raw,
            ));
        }
        if preserve_raw {
            continue;
        }
        if let Some(candidate) = evaluated_types
            .define_props
            .iter()
            .flat_map(|define_props| define_props.result.value.properties.iter())
            .find(|property| property.name == field.name)
            .map(|property| property.ty.clone())
        {
            if component_meta_type_expr_improves(&candidate, &field.r#type)
                && !type_expr_needs_projection_rescue(query_engine, scope_canonical_id, &candidate)
            {
                field.r#type = candidate;
            }
        }
        rescue_field(scope_canonical_id, field, query_engine);
        let raw_needs_member_route = parsed_field_raw_type(field).as_ref().is_some_and(|raw| {
            type_expr_needs_member_route_materialization(raw, scope_canonical_id, query_engine)
                || component_meta_registry_public_utility_route(raw).is_some()
        });
        let raw_is_unpreserved_top_level_ref =
            parsed_field_raw_type(field).as_ref().is_some_and(|raw| {
                matches!(
                    raw,
                    verter_semantic::analysis::type_expr::TypeExpr::Ref { type_arguments, .. }
                        if type_arguments.is_empty()
                )
            });
        if crate::host_manage::component_meta_debug_enabled() {
            crate::host_manage::component_meta_debug(format!(
                "FIELD_MATERIALIZE_POST_RESCUE owner={} field={} current={:?} raw_needs_member_route={} raw_is_unpreserved_top_level_ref={} current_needs_member_route={}",
                scope_canonical_id,
                field.name,
                field.r#type,
                raw_needs_member_route,
                raw_is_unpreserved_top_level_ref,
                type_expr_needs_member_route_materialization(
                    &field.r#type,
                    scope_canonical_id,
                    query_engine,
                ),
            ));
        }
        if !(raw_needs_member_route
            || raw_is_unpreserved_top_level_ref
            || type_expr_needs_member_route_materialization(
                &field.r#type,
                scope_canonical_id,
                query_engine,
            ))
        {
            continue;
        }

        if let Some(routes) = prop_member_routes.get(&field.name).cloned() {
            for lowered in routes {
                let rescued = materialize_component_meta_macro_shape_member_type_expr(
                    &lowered,
                    field.name.as_str(),
                    &field.r#type,
                    scope_canonical_id,
                    query_engine,
                );
                if component_meta_type_expr_improves(&rescued, &field.r#type) {
                    field.r#type = rescued;
                }
            }
        }
        let materialize_scope_canonical_id = imported_component_meta_materialization_scope(
            &field.r#type,
            scope_canonical_id,
            query_engine,
        )
        .or_else(|| {
            parsed_field_raw_type(field).as_ref().and_then(|raw| {
                imported_component_meta_materialization_scope(raw, scope_canonical_id, query_engine)
            })
        })
        .unwrap_or_else(|| scope_canonical_id.to_string());
        let raw_route_root_is_package_backed =
            parsed_field_raw_type(field).as_ref().is_some_and(|raw| {
                type_expr_has_package_backed_object_like_root(raw, scope_canonical_id, query_engine)
            });
        if raw_needs_member_route && !raw_route_root_is_package_backed {
            let mut owner_alias_route_rescued = false;
            if let Some(owner_alias_route_surface) =
                parsed_field_raw_type(field).as_ref().and_then(|raw| {
                    materialize_member_route_from_alias_body_in_owner_scope(
                        raw,
                        scope_canonical_id,
                        query_engine,
                    )
                })
            {
                let owner_alias_route_surface = materialize_component_meta_member_surface_expr(
                    &owner_alias_route_surface,
                    materialize_scope_canonical_id.as_str(),
                    query_engine,
                    false,
                );
                if component_meta_type_expr_improves(&owner_alias_route_surface, &field.r#type)
                    || route_leaf_beats_wrapper_object(&owner_alias_route_surface, &field.r#type)
                {
                    field.r#type = owner_alias_route_surface;
                    owner_alias_route_rescued = true;
                }
            }
            if !owner_alias_route_rescued {
                let routed_surface = materialize_component_meta_member_surface_expr(
                    &field.r#type,
                    materialize_scope_canonical_id.as_str(),
                    query_engine,
                    true,
                );
                if component_meta_type_expr_improves(&routed_surface, &field.r#type) {
                    field.r#type = routed_surface;
                }
                if let Some(raw_route_surface) =
                    parsed_field_raw_type(field).as_ref().and_then(|raw| {
                        query_engine
                            .project_expr_surface_expr(materialize_scope_canonical_id.as_str(), raw)
                    })
                {
                    let raw_route_surface = materialize_component_meta_member_surface_expr(
                        &raw_route_surface,
                        materialize_scope_canonical_id.as_str(),
                        query_engine,
                        true,
                    );
                    if component_meta_type_expr_improves(&raw_route_surface, &field.r#type)
                        || route_leaf_beats_wrapper_object(&raw_route_surface, &field.r#type)
                    {
                        field.r#type = raw_route_surface;
                    }
                }
                if let Some(projected_route_surface) = query_engine.project_expr_surface_expr(
                    materialize_scope_canonical_id.as_str(),
                    &field.r#type,
                ) {
                    let projected_route_surface = materialize_component_meta_member_surface_expr(
                        &projected_route_surface,
                        materialize_scope_canonical_id.as_str(),
                        query_engine,
                        false,
                    );
                    if component_meta_type_expr_improves(&projected_route_surface, &field.r#type)
                        || route_leaf_beats_wrapper_object(&projected_route_surface, &field.r#type)
                    {
                        field.r#type = projected_route_surface;
                    }
                }
            }
        }
        let rescued = match &field.r#type {
            verter_semantic::analysis::type_expr::TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => {
                let declaration = query_engine.resolve_type_declaration(
                    materialize_scope_canonical_id.as_str(),
                    name.as_ref(),
                );
                let declaration_scope = if declaration.canonical_source.is_empty() {
                    materialize_scope_canonical_id.clone()
                } else {
                    declaration.canonical_source.clone()
                };
                let declaration_name = if declaration.resolved_name.is_empty() {
                    name.as_ref().to_string()
                } else {
                    declaration.resolved_name.clone()
                };
                let (target_scope, target_name) = query_engine.resolve_final_prepared_type_target(
                    declaration_scope.as_str(),
                    declaration_name.as_str(),
                );
                let rescued = query_engine
                    .named_decl_body(target_scope.as_str(), target_name.as_str())
                    .map(|body| {
                        rewrite_named_self_refs_to_recursive_ref(&body, target_name.as_str())
                    })
                    .or_else(|| {
                        // D-Cutover §5.8: route through dispatch's
                        // surface projection; the pre-§5.8
                        // `solve_scoped` solver fallback is retired.
                        query_engine
                            .project_type_surface_expr(target_scope.as_str(), target_name.as_str())
                    })
                    .unwrap_or_else(|| {
                        materialize_component_meta_type_expr_until_stable(
                            &field.r#type,
                            materialize_scope_canonical_id.as_str(),
                            crate::semantic_query::ProjectionMode::Expanded,
                            query_engine,
                        )
                    });
                if crate::host_manage::component_meta_debug_enabled() {
                    crate::host_manage::component_meta_debug(format!(
                        "FIELD_MATERIALIZE_REF owner={} field={} current_ref={} materialize_scope={} target_scope={} target_name={} rescued={:?}",
                        scope_canonical_id,
                        field.name,
                        name,
                        materialize_scope_canonical_id,
                        target_scope,
                        target_name,
                        rescued,
                    ));
                }
                rescued
            }
            _ => materialize_component_meta_type_expr_until_stable(
                &field.r#type,
                materialize_scope_canonical_id.as_str(),
                crate::semantic_query::ProjectionMode::Expanded,
                query_engine,
            ),
        };
        if component_meta_type_expr_improves(&rescued, &field.r#type) {
            field.r#type = rescued;
        }
        if let Some(verter_semantic::analysis::type_expr::TypeExpr::Ref {
            name,
            type_arguments,
        }) = parsed_field_raw_type(field)
        {
            if type_arguments.is_empty() {
                let declaration =
                    query_engine.resolve_type_declaration(scope_canonical_id, name.as_ref());
                let declaration_scope = if declaration.canonical_source.is_empty() {
                    scope_canonical_id.to_string()
                } else {
                    declaration.canonical_source
                };
                let declaration_name = if declaration.resolved_name.is_empty() {
                    name.as_ref().to_string()
                } else {
                    declaration.resolved_name
                };
                let (target_scope, target_name) = query_engine.resolve_final_prepared_type_target(
                    declaration_scope.as_str(),
                    declaration_name.as_str(),
                );
                if let Some(body) =
                    query_engine.named_decl_body(target_scope.as_str(), target_name.as_str())
                {
                    let replacement =
                        rewrite_named_self_refs_to_recursive_ref(&body, target_name.as_str());
                    if crate::host_manage::component_meta_debug_enabled() {
                        crate::host_manage::component_meta_debug(format!(
                            "FIELD_RAW_REF_BODY owner={} field={} target_scope={} target_name={} body={:?} replacement={:?}",
                            scope_canonical_id,
                            field.name,
                            target_scope,
                            target_name,
                            body,
                            replacement,
                        ));
                    }
                    if matches!(
                        replacement,
                        verter_semantic::analysis::type_expr::TypeExpr::Union(_)
                            | verter_semantic::analysis::type_expr::TypeExpr::Primitive(_),
                    ) && !matches!(
                        field.r#type,
                        verter_semantic::analysis::type_expr::TypeExpr::Union(_)
                            | verter_semantic::analysis::type_expr::TypeExpr::Primitive(_),
                    ) {
                        field.r#type = replacement;
                        continue;
                    }
                    // When the field type is a bare Ref to the target type,
                    // apply the body replacement directly (initial expansion).
                    if matches!(
                        &field.r#type,
                        verter_semantic::analysis::type_expr::TypeExpr::Ref {
                            name: ref_name,
                            type_arguments,
                        } if type_arguments.is_empty()
                            && ref_name.as_ref() == target_name.as_str()
                    ) {
                        field.r#type = replacement;
                        continue;
                    }
                    if type_expr_contains_named_recursive_ref(&field.r#type, target_name.as_str()) {
                        let expanded = expand_named_recursive_refs_one_layer(
                            &field.r#type,
                            target_name.as_str(),
                            &replacement,
                        );
                        if expanded != field.r#type {
                            field.r#type = expanded;
                        }
                    } else {
                        // The define_props macro shape projection may have
                        // expanded the top-level type one layer (producing an
                        // Object body) while leaving nested self-references
                        // as bare `Ref{target}` instead of `RecursiveRef`.
                        // Rewrite those bare self-refs to `RecursiveRef` and
                        // expand one more layer so the transport carries the
                        // same two-level shape the RecursiveRef path would
                        // produce.
                        let with_recursive_refs = rewrite_named_self_refs_to_recursive_ref(
                            &field.r#type,
                            target_name.as_str(),
                        );
                        if with_recursive_refs != field.r#type {
                            let expanded = expand_named_recursive_refs_one_layer(
                                &with_recursive_refs,
                                target_name.as_str(),
                                &replacement,
                            );
                            if expanded != field.r#type {
                                field.r#type = expanded;
                            }
                        }
                    }
                }
            }
        }
        if let Some(raw) = parsed_field_raw_type(field).as_ref() {
            if let Some(replacement) =
                indexed_access_alias_body_transport(scope_canonical_id, raw, query_engine)
            {
                if !matches!(
                    field.r#type,
                    verter_semantic::analysis::type_expr::TypeExpr::Union(_)
                        | verter_semantic::analysis::type_expr::TypeExpr::Primitive(_),
                ) {
                    field.r#type = replacement;
                }
            }
        }
        if crate::host_manage::component_meta_debug_enabled() {
            crate::host_manage::component_meta_debug(format!(
                "FIELD_MATERIALIZE_FINAL owner={} field={} final={:?}",
                scope_canonical_id, field.name, field.r#type,
            ));
        }
    }
    let finalized_prop_types = evaluated_types
        .props
        .iter()
        .map(|field| (field.name.clone(), field.r#type.clone()))
        .collect::<rustc_hash::FxHashMap<_, _>>();
    for define_props in &mut evaluated_types.define_props {
        for property in &mut define_props.result.value.properties {
            if let Some(finalized) = finalized_prop_types.get(&property.name) {
                property.ty = finalized.clone();
            }
        }
        if crate::host_manage::component_meta_debug_enabled() {
            crate::host_manage::component_meta_debug(format!(
                "FIELD_DEFINE_PROPS_SYNC owner={} props={:?}",
                scope_canonical_id,
                define_props
                    .result
                    .value
                    .properties
                    .iter()
                    .map(|property| (property.name.clone(), property.ty.clone()))
                    .collect::<Vec<_>>(),
            ));
        }
    }
    for field in &mut evaluated_types.emits {
        rescue_field(scope_canonical_id, field, query_engine);
    }
    for field in &mut evaluated_types.slot_bindings {
        rescue_field(scope_canonical_id, field, query_engine);
        let Some(scope_hints) = slot_binding_scope_hints.get(&field.name) else {
            continue;
        };
        for scope_hint in scope_hints {
            let parsed_raw = field
                .raw_type
                .as_deref()
                .map(verter_semantic::analysis::type_expr_lower::parse_type_annotation);
            for candidate in [
                Some(field.r#type.clone()),
                parsed_raw.clone(),
                parsed_raw
                    .as_ref()
                    .and_then(|raw| query_engine.project_expr_surface_expr(scope_hint, raw)),
                query_engine.project_expr_surface_expr(scope_hint, &field.r#type),
            ]
            .into_iter()
            .flatten()
            {
                let rescued = materialize_component_meta_type_expr_until_stable(
                    &candidate,
                    scope_hint,
                    crate::semantic_query::ProjectionMode::Expanded,
                    query_engine,
                );
                if component_meta_type_expr_improves(&rescued, &field.r#type) {
                    field.r#type = rescued;
                }
                let surface = materialize_component_meta_member_surface_expr(
                    &candidate,
                    scope_hint,
                    query_engine,
                    false,
                );
                if component_meta_type_expr_improves(&surface, &field.r#type) {
                    field.r#type = surface;
                }
            }
        }
    }
    for field in &mut evaluated_types.bindings {
        rescue_field(scope_canonical_id, field, query_engine);
    }
}

/// Sole-authority producer for type-based macro object shapes.
///
/// This is the ONE place that produces `define_props`, `define_emits`, and
/// `define_slots` object shapes for `ExpandedComponentTypes`.
///
/// The production pipeline is projection-first so one phase owns object-shape
/// materialization. The solver is used only as the terminal fallback when
/// projection cannot produce a usable shape.
#[cfg_attr(not(test), allow(dead_code))]
fn produce_macro_object_shapes(
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
fn produce_macro_object_shapes_for_purpose(
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
                let define_props_needs_projection_rescue =
                    define_props_lowered.is_some_and(|lowered| {
                        type_expr_needs_projection_rescue(query_engine, owner_canonical, lowered)
                    });
                if define_props_prefers_prepared_projection {
                    if let Some(lowered) = define_props_lowered {
                        let item_started = Instant::now();
                        let (shape, source) = produce_one_macro_object_shape(
                            query_engine,
                            owner_canonical,
                            lowered,
                            has_prop_shape_surface,
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
                    matches!(
                        lowered,
                        verter_semantic::analysis::type_expr::TypeExpr::Ref { .. }
                    ) && !define_props_has_matching_resolved_root
                }) {
                    let item_started = Instant::now();
                    let (shape, source) = produce_one_macro_object_shape(
                        query_engine,
                        owner_canonical,
                        lowered,
                        has_prop_shape_surface,
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
                        let (shape, source) = produce_one_macro_object_shape(
                            query_engine,
                            owner_canonical,
                            lowered,
                            has_prop_shape_surface,
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
                } else if !define_props_needs_projection_rescue {
                    if let Some((shape, source)) = synthesize_define_props_shape_from_registry_root(
                        owner_canonical,
                        macro_index,
                        snapshot,
                        resolved_type_registry,
                        resolved_type_registry_meta,
                    ) {
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
                        let (shape, source) = produce_one_macro_object_shape(
                            query_engine,
                            owner_canonical,
                            lowered,
                            has_prop_shape_surface,
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
                    let (shape, source) = produce_one_macro_object_shape(
                        query_engine,
                        owner_canonical,
                        lowered,
                        has_prop_shape_surface,
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
                        let (shape, source) = produce_one_macro_object_shape(
                            query_engine,
                            owner_canonical,
                            lowered,
                            verter_semantic::analysis::type_eval_build::has_named_shape_surface,
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
                        type_expr_needs_projection_rescue(query_engine, owner_canonical, lowered)
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
                            let (shape, source) = produce_one_macro_object_shape_for_slots(
                                query_engine,
                                owner_canonical,
                                lowered,
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
                    let (shape, source) = produce_one_macro_object_shape_for_slots(
                        query_engine,
                        owner_canonical,
                        lowered,
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
enum MacroShapeSource {
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

fn define_props_fields_fast_path_allowed(
    mac: &AnalyzedMacro,
    macro_index: usize,
    resolved_macros: &[ResolvedMacroMeta],
    lowered: Option<&verter_semantic::analysis::type_expr::TypeExpr>,
) -> bool {
    fn strip_parens(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> &verter_semantic::analysis::type_expr::TypeExpr {
        match expr {
            verter_semantic::analysis::type_expr::TypeExpr::Parenthesized(inner) => {
                strip_parens(inner)
            }
            other => other,
        }
    }

    let Some(lowered) = lowered.map(strip_parens) else {
        return false;
    };

    match lowered {
        verter_semantic::analysis::type_expr::TypeExpr::Object(_) => return true,
        verter_semantic::analysis::type_expr::TypeExpr::Ref { type_arguments, .. }
            if type_arguments.is_empty() => {}
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

fn define_props_has_direct_local_root(mac: &AnalyzedMacro) -> bool {
    mac.resolved_local_types
        .iter()
        .enumerate()
        .any(|(resolved_index, resolved)| {
            is_direct_local_macro_type_reference(mac, resolved_index, resolved.name.as_str())
        })
}

fn is_direct_local_macro_type_reference(
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

fn define_props_known_surface_shortcut_allowed(
    lowered: Option<&verter_semantic::analysis::type_expr::TypeExpr>,
) -> bool {
    fn strip_parens(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> &verter_semantic::analysis::type_expr::TypeExpr {
        match expr {
            verter_semantic::analysis::type_expr::TypeExpr::Parenthesized(inner) => {
                strip_parens(inner)
            }
            other => other,
        }
    }

    match lowered.map(strip_parens) {
        Some(verter_semantic::analysis::type_expr::TypeExpr::Object(_)) => true,
        Some(verter_semantic::analysis::type_expr::TypeExpr::Ref { type_arguments, .. }) => {
            type_arguments.is_empty()
        }
        _ => false,
    }
}

fn synthesize_define_props_shape_from_known_surface_with_authority(
    macro_index: usize,
    snapshot: &FileAnalysisSnapshot,
    resolved_macros: &[ResolvedMacroMeta],
    evaluated_types: &verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    lowered: Option<&verter_semantic::analysis::type_expr::TypeExpr>,
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

            let ty = prop
                .type_annotation
                .as_deref()
                .map(verter_semantic::analysis::type_expr_lower::parse_type_annotation)
                .unwrap_or_else(|| verter_semantic::analysis::type_expr::TypeExpr::Unknown {
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

fn synthesize_define_props_shape_from_registry_root(
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

fn synthesize_macro_shape_from_registry_lowered_root(
    lowered: &verter_semantic::analysis::type_expr::TypeExpr,
    resolved_type_registry: &[verter_semantic::analysis::component_meta::ResolvedTypeAnalysis],
    resolved_type_registry_meta: &[ResolvedTypeRegistryMeta],
    shape_is_usable: impl Fn(&verter_semantic::analysis::type_expand::ExpandedObjectShape) -> bool,
) -> Option<(ShapeResult, MacroShapeSource)> {
    fn root_name(expr: &verter_semantic::analysis::type_expr::TypeExpr) -> Option<&str> {
        match expr {
            verter_semantic::analysis::type_expr::TypeExpr::Parenthesized(inner) => {
                root_name(inner)
            }
            verter_semantic::analysis::type_expr::TypeExpr::Ref { name, .. } => Some(name.as_ref()),
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

fn named_ref_matches_empty_shell_registry_root(
    owner_canonical: &str,
    lowered: &verter_semantic::analysis::type_expr::TypeExpr,
    resolved_type_registry: &[verter_semantic::analysis::component_meta::ResolvedTypeAnalysis],
    resolved_type_registry_meta: &[ResolvedTypeRegistryMeta],
) -> bool {
    let verter_semantic::analysis::type_expr::TypeExpr::Ref {
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

fn reuse_expanded_define_emits_shape(
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

fn synthesize_define_emits_shape_from_known_surface(
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

            let ty = emit
                .payload_type
                .as_deref()
                .map(verter_semantic::analysis::type_expr_lower::parse_type_annotation)
                .unwrap_or_else(|| verter_semantic::analysis::type_expr::TypeExpr::Unknown {
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

fn synthesize_define_slots_shape_from_known_surface(
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

fn slot_field_function_type_expr(
    slot: &verter_semantic::analysis::AnalyzedSlotField,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    let return_type = slot.return_type.as_deref().unwrap_or("any");
    let signature = if slot.bindings.is_empty() {
        format!("() => {return_type}")
    } else {
        let bindings = slot
            .bindings
            .iter()
            .map(|binding| {
                format!(
                    "{}: {}",
                    binding.name,
                    binding.type_annotation.as_deref().unwrap_or("unknown")
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("(props: {{ {bindings} }}) => {return_type}")
    };

    verter_semantic::analysis::type_expr_lower::parse_type_annotation(&signature)
}

fn reuse_expanded_define_props_shape(
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

fn merge_expansion_execution_status(
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

fn registry_entry_to_expanded_shape(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
    use verter_semantic::analysis::type_expand::{
        ExpandedCallSignature, ExpandedIndexSignature, ExpandedObjectShape, ExpandedParameter,
        ExpandedProperty,
    };
    use verter_semantic::analysis::type_expr::{ObjectMember, PrimitiveName, TypeExpr};

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

fn expanded_shape_to_type_expr(
    shape: &verter_semantic::analysis::type_expand::ExpandedObjectShape,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use verter_semantic::analysis::type_expr::{
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
        properties.push(
            verter_semantic::analysis::type_expr::ObjectMember::IndexSignature(
                verter_semantic::analysis::type_expr::IndexSignature {
                    key_name: "key".to_string(),
                    key_type: signature.key_type.clone(),
                    value_type: signature.value_type.clone(),
                    readonly: signature.readonly,
                },
            ),
        );
    }

    TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties }))
}

type ShapeResult = verter_semantic::analysis::type_expand::ExpansionResult<
    verter_semantic::analysis::type_expand::ExpandedObjectShape,
>;

fn type_expr_needs_projection_rescue(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

    if expr_has_transitively_recursive_generic_root(query_engine, owner_canonical, expr) {
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
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

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
fn produce_one_macro_object_shape(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    lowered: &verter_semantic::analysis::type_expr::TypeExpr,
    shape_is_usable: impl Fn(&verter_semantic::analysis::type_expand::ExpandedObjectShape) -> bool,
) -> (Option<ShapeResult>, MacroShapeSource) {
    // ── Fast path: direct Object body → DB-backed projection ──────────
    if let Some(projected) =
        project_named_ref_prepared_surface_shape(query_engine, owner_canonical, lowered)
    {
        return (Some(projected), MacroShapeSource::Projection);
    }

    if let verter_semantic::analysis::type_expr::TypeExpr::Ref {
        name,
        type_arguments,
    } = lowered
    {
        if type_arguments.is_empty() {
            if let Some((def_canonical, def_name)) =
                classify_named_ref_for_db_projection(query_engine, owner_canonical, name)
            {
                if let Some(shape) =
                    query_engine.project_type_surface_shape(&def_canonical, &def_name)
                {
                    if shape_is_usable(&shape) {
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

    // ── Non-object body: solver first, then projection on warm caches ─
    let scoped_solver_result = match lowered {
        verter_semantic::analysis::type_expr::TypeExpr::Ref {
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
            // D-Cutover §5.8: route through dispatch's surface
            // projection — the pre-§5.8 `solve_scoped` solver path
            // is retired.
            query_engine
                .project_type_surface_expr(defining_canonical.as_str(), defining_name.as_str())
                .and_then(|solved_expr| {
                    let shape = verter_semantic::analysis::type_expand::type_expr_to_object_shape(
                        &solved_expr,
                    );
                    shape_is_usable(&shape).then(|| {
                        verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                            shape,
                        )
                    })
                })
        }
        _ => None,
    };
    // D-Cutover §5.8: the retired solver's `owner_engine.solve` is gone.
    // Route through dispatch's surface projection + the dispatch-backed
    // `deep_resolve_slot_function_refs` on CMQE (replacement for the
    // retired solver-backed pass), treating the result as an
    // exact-concrete SolverResult so `solver_result_to_object_expansion`
    // still derives the expansion.
    let solver_result = scoped_solver_result.unwrap_or_else(|| {
        // D-Cutover §5.8: dispatch's `project_expr_surface_expr` is the
        // sole solve path. Empty-path `ProjectPath` with mode Expanded
        // now expands terminal DeclAnchors via `Instantiate(anchor, [])`
        // so non-generic aliases (including namespace-qualified
        // `Types.Props` → `Props`) emit their body surface here.
        let projected = query_engine
            .project_expr_surface_expr(owner_canonical, lowered)
            .unwrap_or_else(|| lowered.clone());
        let deeply_resolved =
            query_engine.deep_resolve_slot_function_refs(owner_canonical, &projected);
        verter_semantic::analysis::type_expand::solver_result_to_object_expansion(
            verter_semantic::analysis::type_solver::result::SolverResult::exact_concrete(
                deeply_resolved,
            ),
        )
    });
    let solver_count = shape_surface_count(&solver_result);
    let rescue_projection = solver_count == 0
        || type_expr_needs_projection_rescue(query_engine, owner_canonical, lowered);
    let projected = if rescue_projection {
        query_engine
            .project_expr_surface_shape(owner_canonical, lowered)
            .and_then(|shape| {
                shape_is_usable(&shape).then(|| {
                    verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(shape)
                })
            })
    } else {
        None
    };
    let root_projected = if rescue_projection {
        project_named_ref_surface_shape(query_engine, owner_canonical, lowered, &shape_is_usable)
    } else {
        None
    };
    let imported_scope_projected = if rescue_projection {
        project_named_ref_imported_scope_shape(
            query_engine,
            owner_canonical,
            lowered,
            &shape_is_usable,
        )
    } else {
        None
    };
    let projected = [projected, root_projected, imported_scope_projected]
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

fn project_named_ref_prepared_surface_shape(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    lowered: &verter_semantic::analysis::type_expr::TypeExpr,
) -> Option<ShapeResult> {
    let verter_semantic::analysis::type_expr::TypeExpr::Ref {
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

    query_engine
        .project_prepared_type_surface_shape(scope_canonical.as_str(), resolved_name.as_str())
        .and_then(|shape| {
            has_prop_shape_surface(&shape).then(|| {
                verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(shape)
            })
        })
}

fn named_ref_can_use_prepared_projection(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    requested_name: &str,
) -> bool {
    let declaration = query_engine.resolve_type_declaration(owner_canonical, requested_name);
    if declaration.canonical_source.is_empty() || declaration.canonical_source == owner_canonical {
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

fn resolve_named_ref_prepared_projection_target(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    requested_name: &str,
) -> Option<(String, String)> {
    if query_engine
        .host()
        .prepared_type_decl(owner_canonical, requested_name)
        .is_some()
    {
        return Some((owner_canonical.to_string(), requested_name.to_string()));
    }

    if let Some(state) = query_engine
        .host()
        .route_owned_shallow_state(owner_canonical)
    {
        if state.symbol(requested_name).is_some() {
            return Some((owner_canonical.to_string(), requested_name.to_string()));
        }

        if let Some(import_target) = state.import_target(requested_name) {
            let target_canonical = if import_target.canonical_id.is_empty() {
                query_engine.host().resolve_route_type_edge(
                    owner_canonical,
                    import_target.source_specifier.as_str(),
                )?
            } else {
                import_target.canonical_id.clone()
            };
            let target_name = import_target.imported_name.clone();
            if let Some((routed_canonical, routed_name)) = query_engine
                .host()
                .resolve_named_type_export_target_shallow(
                    target_canonical.as_str(),
                    target_name.as_str(),
                )
            {
                if query_engine
                    .host()
                    .prepared_type_decl(routed_canonical.as_str(), routed_name.as_str())
                    .is_some()
                {
                    return Some((routed_canonical, routed_name));
                }
            }
            if query_engine
                .host()
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

fn project_named_ref_surface_shape(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    lowered: &verter_semantic::analysis::type_expr::TypeExpr,
    shape_is_usable: &impl Fn(&verter_semantic::analysis::type_expand::ExpandedObjectShape) -> bool,
) -> Option<ShapeResult> {
    let verter_semantic::analysis::type_expr::TypeExpr::Ref { name, .. } = lowered else {
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

    query_engine
        .project_type_surface_shape(defining_canonical, defining_name)
        .and_then(|shape| {
            shape_is_usable(&shape).then(|| {
                verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(shape)
            })
        })
}

fn project_named_ref_imported_scope_shape(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    lowered: &verter_semantic::analysis::type_expr::TypeExpr,
    shape_is_usable: &impl Fn(&verter_semantic::analysis::type_expand::ExpandedObjectShape) -> bool,
) -> Option<ShapeResult> {
    let verter_semantic::analysis::type_expr::TypeExpr::Ref { name, .. } = lowered else {
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

    query_engine
        .project_expr_surface_shape(defining_canonical, lowered)
        .and_then(|shape| {
            shape_is_usable(&shape).then(|| {
                verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(shape)
            })
        })
}

/// Like `produce_one_macro_object_shape` but applies `deep_resolve_slot_function_refs`
/// on the solver path for defineSlots.
fn produce_one_macro_object_shape_for_slots(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    lowered: &verter_semantic::analysis::type_expr::TypeExpr,
) -> (Option<ShapeResult>, MacroShapeSource) {
    // ── Fast path: direct Object body → DB-backed projection ──────────
    if let verter_semantic::analysis::type_expr::TypeExpr::Ref {
        name,
        type_arguments,
    } = lowered
    {
        if type_arguments.is_empty() {
            if let Some((def_canonical, def_name)) =
                classify_named_ref_for_db_projection(query_engine, owner_canonical, name)
            {
                if let Some(shape) =
                    query_engine.project_type_surface_shape(&def_canonical, &def_name)
                {
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

    // ── Non-object body: dispatch projection first, then projection on warm caches ─
    // D-Cutover §5.8: dispatch's `project_expr_surface_expr` replaces
    // `owner_engine.solve`; `CMQE::deep_resolve_slot_function_refs`
    // replaces the retired `type_eval_build::deep_resolve_slot_function_refs`
    // pass. Both route through the shared dispatch memo so caches stay
    // path-independent.
    //
    // Path C C11-residual-A: when the strict `project_expr_surface_expr`
    // returns `None` because a compound-shape sibling is still a
    // deferred shell (e.g. `{ explicit slots } & DynamicSlots<...>` —
    // the `DynamicSlots` arm is a Mapped that can't enumerate keys
    // when the type parameters are unresolved), fall back to the
    // lenient `project_expr_surface_expr_with_compound_objects` so the
    // explicit Object arm's properties still reach
    // `solver_result_to_object_expansion`. The expansion's existing
    // Intersection-merging in [`type_expr_to_expanded_shape`] then
    // collects the explicit slot members from the compound shape.
    let projected_body = query_engine
        .project_expr_surface_expr(owner_canonical, lowered)
        .or_else(|| {
            query_engine.project_expr_surface_expr_with_compound_objects(owner_canonical, lowered)
        })
        .unwrap_or_else(|| lowered.clone());
    let deeply_resolved =
        query_engine.deep_resolve_slot_function_refs(owner_canonical, &projected_body);
    let solver_result = verter_semantic::analysis::type_expand::solver_result_to_object_expansion(
        verter_semantic::analysis::type_solver::result::SolverResult::exact_concrete(
            deeply_resolved,
        ),
    );
    let solver_count = shape_surface_count(&solver_result);

    let projected = query_engine
        .project_expr_surface_shape(owner_canonical, lowered)
        .and_then(|shape| {
            let projected_expr = expanded_shape_to_type_expr(&shape);
            let resolved_expr =
                query_engine.deep_resolve_slot_function_refs(owner_canonical, &projected_expr);
            registry_entry_to_expanded_shape(&resolved_expr).and_then(|resolved_shape| {
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
    )
    .and_then(|shape| {
        let projected_expr = expanded_shape_to_type_expr(&shape.value);
        let resolved_expr =
            query_engine.deep_resolve_slot_function_refs(owner_canonical, &projected_expr);
        registry_entry_to_expanded_shape(&resolved_expr).and_then(|resolved_shape| {
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

fn has_shape_surface(shape: &verter_semantic::analysis::type_expand::ExpandedObjectShape) -> bool {
    !shape.properties.is_empty()
        || !shape.index_signatures.is_empty()
        || !shape.call_signatures.is_empty()
}

fn type_expr_symbolic_penalty(expr: &verter_semantic::analysis::type_expr::TypeExpr) -> usize {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

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

fn shape_symbolic_penalty(
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

fn projection_result_beats_solver_shape(projected: &ShapeResult, solver: &ShapeResult) -> bool {
    let projected_count = shape_surface_count(projected);
    let solver_count = shape_surface_count(solver);
    projected_count > solver_count
        || (projected_count == solver_count
            && shape_symbolic_penalty(&projected.value) < shape_symbolic_penalty(&solver.value))
}

fn has_prop_shape_surface(
    shape: &verter_semantic::analysis::type_expand::ExpandedObjectShape,
) -> bool {
    !shape.properties.is_empty() || !shape.index_signatures.is_empty()
}

fn shape_surface_count(result: &ShapeResult) -> usize {
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
fn classify_named_ref_for_db_projection(
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
            .is_some_and(|body| {
                matches!(
                    body,
                    verter_semantic::analysis::type_expr::TypeExpr::Object(_),
                )
            }),
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
fn collect_type_expr_ref_names(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    out: &mut rustc_hash::FxHashSet<String>,
) {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};
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

impl VerterHost {
    /// Single host-backed resolver API for cross-file component-meta enrichment.
    ///
    /// This is the ONLY entry point for cross-file component-meta resolution.
    /// Mode is chosen explicitly by callers â€” never inferred.
    ///
    /// - `Type`: resolves symbol identity, canonical location, and attached JSDoc
    ///   without materializing expanded shapes.
    /// - `Expanded`: resolves the same way, then materializes props/emits/slots,
    ///   populates the type registry, and computes evaluated types.
    pub fn resolve_component_meta(
        &self,
        canonical_or_alias: &str,
        mode: ProjectionMode,
    ) -> Option<ResolvedComponentMetaState> {
        self.resolve_component_meta_with_view(canonical_or_alias, mode)
    }

    fn resolve_component_meta_with_view(
        &self,
        canonical_or_alias: &str,
        mode: ProjectionMode,
    ) -> Option<ResolvedComponentMetaState> {
        let started = component_meta_debug_enabled().then(Instant::now);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let audit = self.config.audit_enabled.then(|| {
            // Prefer the request_id stamped by
            // `get_component_meta_with_resolution` (via the installed
            // `RequestContext`). Falls back to the global static only
            // when no context is installed — e.g. direct callers of
            // `resolve_component_meta` outside the audited-request
            // path. Without this link `take_audit_record` would look
            // up the outer id while the record is stored under the
            // inner id, and every `AuditedRequest::resolve` would
            // fail with `AuditRecordMissing`.
            let request_id = crate::request_context::current_request_context()
                .map(|ctx| ctx.request_id)
                .unwrap_or_else(next_component_meta_audit_request_id);
            let (host_cache_before_bytes, workspace_before_bytes) =
                self.component_meta_audit_memory_bytes();
            (
                request_id,
                crate::component_meta_audit::begin_request_audit(request_id),
                crate::component_meta_audit::AuditBuilder::new(request_id, canonical.clone()),
                host_cache_before_bytes,
                workspace_before_bytes,
            )
        });
        component_meta_trace_custom!(
            "resolve_component_meta",
            format!("owner={} mode={mode:?}", canonical),
        );
        let result = run_component_meta_request(
            self,
            self.resolver_runtime().component_meta.singleflight(),
            &canonical,
            mode,
            None,
            STORE_VIEW_STABILITY_MAX_ATTEMPTS,
        );

        if matches!(result.source, RequestSource::Cache) {
            self.provenance
                .resolver_node_cache_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if !(matches!(result.source, RequestSource::Cache) && result.attempts == 1) {
            self.provenance
                .resolver_node_cache_misses
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if let RequestSource::Flight { role, forked_lane } = result.source {
            if role == SingleflightRole::Follower {
                self.provenance
                    .resolver_singleflight_coalesced
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if forked_lane {
                self.provenance
                    .resolver_cross_view_lane_forks
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }

        if let Some(started) = started {
            match result.source {
                RequestSource::Cache => component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} cached attempt={} took {:?}",
                    canonical,
                    mode,
                    result.attempts.saturating_sub(1),
                    started.elapsed(),
                )),
                RequestSource::Flight { role, .. } => component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} role={:?} stable attempt={} total took {:?}",
                    canonical,
                    mode,
                    role,
                    result.attempts.saturating_sub(1),
                    started.elapsed(),
                )),
                RequestSource::Fallback => component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} retries_exhausted total took {:?}",
                    canonical,
                    mode,
                    started.elapsed(),
                )),
            }
        }

        if let Some(resolved) = result.value.as_ref() {
            component_meta_trace_custom!(
                "resolve_component_meta_result",
                format!(
                    "owner={} mode={mode:?} source={} attempts={} macros={} resolved_types={} has_evaluated_types={} fact_versions={}",
                    canonical,
                    trace_request_source(result.source),
                    result.attempts,
                    resolved.resolved_macros.len(),
                    resolved.resolved_type_registry.len(),
                    resolved.evaluated_types.is_some(),
                    resolved.fact_versions.len(),
                ),
            );
        }

        if let Some((
            _request_id,
            request_audit_guard,
            mut audit_builder,
            host_cache_before_bytes,
            workspace_before_bytes,
        )) = audit
        {
            audit_builder.record_store(self.component_meta_audit_store_snapshot(None));
            let (host_cache_after_bytes, workspace_after_bytes) =
                self.component_meta_audit_memory_bytes();
            audit_builder.record_memory_snapshots(
                host_cache_before_bytes,
                host_cache_after_bytes,
                workspace_before_bytes,
                workspace_after_bytes,
            );
            if request_source_performed_compute(result.source) {
                if let Some(compute_audit) = result
                    .value
                    .as_ref()
                    .and_then(|resolved| resolved.compute_audit.as_ref())
                {
                    let mut timings = compute_audit.timings.clone();
                    timings.imported_root_proof_ms =
                        request_audit_guard.snapshot().imported_root_proof_ms;
                    audit_builder.record_timings(timings);
                    audit_builder.record_solver(compute_audit.solver.clone());
                }
            }
            // Mine the semantic footprint when the active request is
            // capturing. Plan §3 Commit 4: drains the per-request
            // accumulator and feeds the result through the deterministic
            // miner before the builder finalises. Without this step,
            // `RustAuditRecord.footprint` would always be `None` even
            // for footprint-enabled requests.
            if let Some(ctx) = crate::request_context::current_request_context() {
                if ctx.footprint_capture {
                    if let Some(acc) = ctx.audit_accumulator.as_ref() {
                        let state = acc.drain();
                        let footprint = crate::component_meta_audit::mine_footprint(
                            self.project_type_store().semantic_graph(),
                            state,
                            &ctx,
                            self.config.max_derivation_edges,
                        );
                        audit_builder.record_footprint(footprint);
                    }
                }
            }
            let record = audit_builder.finish();
            crate::component_meta_audit::emit_audit_trace(&record);
            // Publish into the host's bounded audit-record store so
            // `take_audit_record(resolution.request_id)` can drain it.
            // Plan §2.5 — without this line the store stays empty and
            // every `AuditedRequest::resolve` surfaces
            // `AuditRecordMissing`.
            self.publish_audit_record(record);
        }

        result.value
    }

    pub(crate) fn compute_component_meta_state(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        whole_hash: Hash16,
    ) -> Option<ResolvedComponentMetaState> {
        self.compute_component_meta_state_inner(
            canonical,
            mode,
            whole_hash,
            None,
            crate::resolver_core::ComponentMetaResolutionPurpose::Full,
            RegistryMaterialization::Full,
        )
    }

    fn compute_component_meta_state_from_captured(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        captured: &CapturedComponentMetaInputs,
    ) -> Option<ResolvedComponentMetaState> {
        self.compute_component_meta_state_inner(
            canonical,
            mode,
            captured.whole_hash,
            Some(captured),
            crate::resolver_core::ComponentMetaResolutionPurpose::Full,
            RegistryMaterialization::Full,
        )
    }

    pub(crate) fn compute_component_meta_state_for_fallthrough(
        &self,
        canonical: &str,
        whole_hash: Hash16,
    ) -> Option<ResolvedComponentMetaState> {
        self.compute_component_meta_state_inner(
            canonical,
            ProjectionMode::Expanded,
            whole_hash,
            None,
            crate::resolver_core::ComponentMetaResolutionPurpose::Fallthrough,
            RegistryMaterialization::SkipAppend,
        )
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn compute_component_meta_state_inner(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        whole_hash: Hash16,
        captured: Option<&CapturedComponentMetaInputs>,
        purpose: crate::resolver_core::ComponentMetaResolutionPurpose,
        registry_materialization: RegistryMaterialization,
    ) -> Option<ResolvedComponentMetaState> {
        // Step 6.6.A: reset the per-request dep-signature accumulator
        // so each compute call starts fresh. Inner materialize_until_stable
        // calls accumulate dispatch-side facts; we drain + merge them
        // into the published `fact_versions` below.
        reset_dispatch_dep_signature_accumulator();

        let audit_enabled = self.config.audit_enabled;
        let mut audit_timings = if audit_enabled {
            captured
                .map(|captured| crate::component_meta_audit::RustTimingAudit {
                    capture_inputs_ms: captured.audit_capture_inputs_ms,
                    store_read_ms: captured.audit_store_read_ms,
                    direct_import_proof_ms: captured.audit_direct_import_proof_ms,
                    ..Default::default()
                })
                .unwrap_or_default()
        } else {
            crate::component_meta_audit::RustTimingAudit::default()
        };
        component_meta_trace_custom!(
            "compute_component_meta_state",
            format!(
                "owner={} mode={mode:?} captured={} store_view={} whole_hash={whole_hash:?}",
                canonical,
                captured.is_some(),
                false,
            ),
        );
        self.provenance
            .component_meta_resolved_state_recomputes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let snapshot = captured
            .map(|captured| captured.snapshot.clone())
            .or_else(|| self.get_raw_analysis_snapshot(canonical))?;
        component_meta_trace_custom!(
            "component_meta_snapshot",
            format!(
                "owner={} imports={} macros={} bindings={} has_template={} script_flags={}",
                canonical,
                snapshot.imports.len(),
                snapshot.macros.len(),
                snapshot.bindings.len(),
                snapshot.template.is_some(),
                snapshot.script_flags,
            ),
        );
        // D-Cutover §5.8 WIP-W: retired `shared_owner_engine` /
        // `SessionSolverHost` pair; the resolver host is now a thin
        // wrapper around `VerterHost`.
        let resolver_host = HostComponentMetaResolver { host: self };
        let parts_started = audit_enabled.then(Instant::now);
        let parts = {
            component_meta_trace_custom!(
                "resolve_component_meta_parts",
                format!(
                    "owner={} expanded={} captured={} purpose={:?}",
                    canonical,
                    mode == ProjectionMode::Expanded,
                    captured.is_some(),
                    purpose,
                ),
            );
            crate::resolver_core::resolve_component_meta_parts(
                &resolver_host,
                canonical,
                &snapshot,
                mode == ProjectionMode::Expanded,
                captured,
                purpose,
            )
        };
        if let Some(started) = parts_started {
            audit_timings.solver_ms = started.elapsed().as_secs_f64() * 1000.0;
        }
        let mut parts = parts;
        if let Some(evaluated_types) = parts.evaluated_types.as_mut() {
            enrich_missing_slot_bindings(&parts.resolved_macros, evaluated_types);
        }
        let registry_before = parts.resolved_type_registry.len();
        let append_start = Instant::now();
        let should_materialize_registry = registry_materialization == RegistryMaterialization::Full;
        let should_produce_macro_object_shapes = mode == ProjectionMode::Expanded;
        let solver_audit = if should_materialize_registry || should_produce_macro_object_shapes {
            // D-Cutover §5.8 WIP-W: the retired `shared_owner_engine`
            // is gone — dispatch owns all solve-like operations now.
            let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(self);
            if should_materialize_registry {
                component_meta_trace_custom!(
                    "append_component_meta_registry_entries",
                    format!(
                        "owner={} evaluated_types={} existing_registry={}",
                        canonical,
                        parts.evaluated_types.is_some(),
                        parts.resolved_type_registry.len(),
                    ),
                );
                self.append_component_meta_registry_entries(
                    canonical,
                    &snapshot,
                    parts.evaluated_types.as_ref(),
                    &mut parts.resolved_type_registry,
                    &mut parts.resolved_type_registry_meta,
                    &mut parts.tracked_dependencies,
                    &mut query_engine,
                );
            }
            if should_produce_macro_object_shapes {
                if let Some(eval_source) = captured
                    .and_then(|captured| captured.owner_eval_source.as_deref())
                    .map(str::to_string)
                    .or_else(|| {
                        self.ensure_indexed_ready(canonical).map(|facts| {
                            VerterHost::build_eval_script_source(
                                &facts.raw_source,
                                facts.cached_parse.as_deref(),
                            )
                        })
                    })
                {
                    let mut evaluated_types = parts.evaluated_types.take().unwrap_or_default();
                    {
                        component_meta_trace_custom!(
                            "produce_macro_object_shapes_for_purpose",
                            format!(
                                "owner={} resolved_macros={} registry={} purpose={:?}",
                                canonical,
                                parts.resolved_macros.len(),
                                parts.resolved_type_registry.len(),
                                purpose,
                            ),
                        );
                        produce_macro_object_shapes_for_purpose(
                            canonical,
                            &snapshot,
                            &parts.resolved_macros,
                            &parts.resolved_type_registry,
                            &parts.resolved_type_registry_meta,
                            &eval_source,
                            &mut evaluated_types,
                            &mut query_engine,
                            purpose,
                        );
                    }
                    {
                        component_meta_trace_custom!(
                            "materialize_component_meta_macro_shape_member_types",
                            format!(
                                "owner={} props={} slot_bindings={} define_props={} define_slots={}",
                                canonical,
                                evaluated_types.props.len(),
                                evaluated_types.slot_bindings.len(),
                                evaluated_types.define_props.len(),
                                evaluated_types.define_slots.len(),
                            ),
                        );
                        materialize_component_meta_macro_shape_member_types(
                            canonical,
                            &snapshot,
                            &eval_source,
                            &mut evaluated_types,
                            &mut query_engine,
                        );
                    }
                    if !evaluated_types.is_empty() {
                        enrich_missing_slot_bindings(&parts.resolved_macros, &mut evaluated_types);
                        {
                            component_meta_trace_custom!(
                                "materialize_component_meta_field_types",
                                format!(
                                    "owner={} props={} events={} slot_bindings={} bindings={}",
                                    canonical,
                                    evaluated_types.props.len(),
                                    evaluated_types.emits.len(),
                                    evaluated_types.slot_bindings.len(),
                                    evaluated_types.bindings.len(),
                                ),
                            );
                            materialize_component_meta_field_types(
                                canonical,
                                &snapshot,
                                &eval_source,
                                &parts.resolved_macros,
                                &mut evaluated_types,
                                &mut query_engine,
                            );
                        }
                        parts.evaluated_types = Some(evaluated_types);
                    }
                }
            }
            {
                crate::host_manage::component_meta_trace_custom!(
                    "semantic_graph_stats",
                    format!("owner={} dispatch_authority=true", canonical),
                );
            }
            if query_engine.has_fuse_tripped() {
                for trip in query_engine.fuse_trips() {
                    crate::host_manage::component_meta_trace_custom!(
                        "fuse_tripped",
                        format!(
                            "owner={} fuse={} budget={} actual={}",
                            canonical, trip.fuse_name, trip.budget, trip.actual,
                        ),
                    );
                }
            }
            crate::component_meta_audit::RustSolverAudit {
                total_resolve_steps: 0u64,
                solve_count: 0u32,
            }
        } else {
            crate::host_manage::component_meta_trace_custom!(
                "semantic_graph_stats",
                format!(
                    "owner={} registry_materialization=skipped macro_shapes=skipped",
                    canonical,
                ),
            );
            crate::component_meta_audit::RustSolverAudit::default()
        };
        audit_timings.materialize_ms = append_start.elapsed().as_secs_f64() * 1000.0;
        let store_merge_started = audit_enabled.then(Instant::now);
        // Fact versions must reflect the post-resolution state of the host —
        // mid-request `set_import_dependencies` / `ensure_loaded` calls may
        // have updated import_routes and module_facts that the ambient
        // captured view does not see. Build a fresh snapshot here so the
        // stored facts match the live state at store time; a warm follow-up
        // query will then validate against the same post-resolution state.
        parts.fact_versions =
            self.current_dependency_fact_versions(canonical, &parts.tracked_dependencies);
        if let Some(started) = store_merge_started {
            audit_timings.store_merge_ms = started.elapsed().as_secs_f64() * 1000.0;
        }
        if audit_enabled {
            audit_timings.imported_root_proof_ms =
                crate::component_meta_audit::current_request_audit_snapshot()
                    .imported_root_proof_ms;
        }
        let append_elapsed = append_start.elapsed();
        let registry_after = parts.resolved_type_registry.len();
        if crate::host_manage::component_meta_debug_enabled() {
            let dep_cache_size = self.project_type_store.indexed().len();
            crate::host_manage::component_meta_debug(format!(
                "PROFILE owner={} registry_before={} registry_after={} registry_added={} dep_cache_entries={} append_ms={:.1}",
                canonical,
                registry_before,
                registry_after,
                registry_after - registry_before,
                dep_cache_size,
                append_elapsed.as_secs_f64() * 1000.0,
            ));
        }
        component_meta_trace_custom!(
            "component_meta_parts",
            format!(
                "owner={} resolved_macros={} resolved_type_registry={} has_evaluated_types={} fact_versions={}",
                canonical,
                parts.resolved_macros.len(),
                parts.resolved_type_registry.len(),
                parts.evaluated_types.is_some(),
                parts.fact_versions.len(),
            ),
        );
        // Step 6.6.A: drain accumulated dispatch dep_signatures and
        // merge into fact_versions before publish. Each
        // materialize_until_stable_full call inside the compute body
        // pushed the dispatch round-trip's DepSignature into the
        // thread-local accumulator; here we read + merge so warm
        // cache validation captures the dependency graph the
        // dispatch path discovered.
        let mut merged_fact_versions = parts.fact_versions;
        let dispatch_facts = drain_dispatch_dep_signature_accumulator();
        for fact in dispatch_facts {
            if !merged_fact_versions.contains(&fact) {
                merged_fact_versions.push(fact);
            }
        }

        let state = ResolvedComponentMetaState {
            snapshot,
            mode,
            whole_hash,
            resolved_macros: parts.resolved_macros,
            resolved_type_registry: parts.resolved_type_registry,
            resolved_type_registry_meta: parts.resolved_type_registry_meta,
            evaluated_types: parts.evaluated_types,
            fact_versions: merged_fact_versions,
            compute_audit: audit_enabled.then_some(ResolvedComponentMetaComputeAudit {
                timings: audit_timings,
                solver: solver_audit,
            }),
            // F1 (D3, D34): origin_graph is audit-only. Gate matches LSP's
            // hover-provenance contract at server.rs:6918-6953 — both
            // audit_enabled and footprint_capture must be on.
            origin_graph: (mode == ProjectionMode::Expanded
                && audit_enabled
                && self.config.footprint_capture)
                .then(|| build_origin_graph(self.project_type_store.semantic_graph()))
                .filter(|dto| !dto.edges.is_empty()),
            request_id: 0,
        };
        Some(state)
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn append_component_meta_registry_entries(
        &self,
        owner_canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        evaluated_types: Option<&verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
        resolved_type_registry: &mut Vec<
            verter_semantic::analysis::component_meta::ResolvedTypeAnalysis,
        >,
        resolved_type_registry_meta: &mut Vec<ResolvedTypeRegistryMeta>,
        tracked_dependencies: &mut BTreeSet<String>,
        query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) {
        fn track_component_meta_dependency(
            tracked_dependencies: &mut BTreeSet<String>,
            owner_canonical: &str,
            canonical_id: &str,
        ) {
            if !canonical_id.is_empty() && canonical_id != owner_canonical {
                tracked_dependencies.insert(canonical_id.to_string());
            }
        }
        fn imported_registry_alias_should_stay_symbolic(
            expr: &verter_semantic::analysis::type_expr::TypeExpr,
        ) -> bool {
            use verter_semantic::analysis::type_expr::TypeExpr;

            match expr {
                TypeExpr::Parenthesized(inner) => {
                    imported_registry_alias_should_stay_symbolic(inner)
                }
                TypeExpr::Mapped { .. }
                | TypeExpr::Conditional { .. }
                | TypeExpr::IndexedAccess { .. }
                | TypeExpr::TypeOf(_) => true,
                _ => false,
            }
        }
        fn materialize_component_meta_registry_candidate(
            query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            symbol_name: &str,
            raw_body: Option<&verter_semantic::analysis::type_expr::TypeExpr>,
            prefer_explicit_raw_surface: bool,
        ) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
            use verter_semantic::analysis::type_expr::{
                ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
            };

            let imported_generic_alias_scope: Option<String> = raw_body.and_then(|expr| {
                let TypeExpr::Ref {
                    name,
                    type_arguments,
                } = expr
                else {
                    return None;
                };
                if type_arguments.is_empty() {
                    return None;
                }
                let declaration = query_engine.resolve_type_declaration(scope_canonical_id, name);
                if !declaration.canonical_source.is_empty()
                    && declaration.canonical_source != scope_canonical_id
                {
                    Some(declaration.canonical_source.clone())
                } else {
                    None
                }
            });
            let imported_generic_alias_root = imported_generic_alias_scope.is_some();

            // Path C C11-residual-B: owner-local generic Refs preserve
            // helper-Ref structure. When `Button = ComponentConfig<typeof theme>`
            // is declared in the SAME file as `ComponentConfig` (owner-
            // local), the registry should publish Button as the SHALLOW
            // substituted body — `{ variants: ComponentVariants<...>,
            // slots: ComponentSlots<...>, ui: ComponentUI<...> }` —
            // rather than fully materialising every helper. This keeps
            // the registry consumer's Ref-to-helper navigation path
            // queryable rather than collapsing helper identities into
            // their concrete shapes.
            //
            // Distinct from the imported-alias path
            // (`maybe_refine_imported_generic_alias_object` above) which
            // DOES materialise cross-file aliases (because the consumer
            // can't follow Refs to a cross-file helper through the
            // registry directly).
            if !imported_generic_alias_root {
                if let Some(shallow) = component_meta_owner_local_shallow_substituted_alias_body(
                    query_engine,
                    scope_canonical_id,
                    raw_body,
                ) {
                    return Some(shallow);
                }
            }

            let maybe_refine_imported_generic_alias_object =
                |candidate: TypeExpr,
                 query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>| {
                    if !imported_generic_alias_root {
                        return candidate;
                    }
                    let TypeExpr::Object(object) = candidate else {
                        return candidate;
                    };
                    let properties = object
                        .properties
                        .iter()
                        .map(|member| match member {
                            ObjectMember::Property(property) => {
                                let materialized =
                                    materialize_component_meta_member_surface_expr(
                                        &property.ty,
                                        scope_canonical_id,
                                        query_engine,
                                        true,
                                    );
                                let stabilized =
                                    materialize_component_meta_type_expr_until_stable(
                                        &materialized,
                                        scope_canonical_id,
                                        crate::semantic_query::ProjectionMode::Expanded,
                                        query_engine,
                                    );
                                // For generic Ref members (e.g. ComponentVariants<T>),
                                // try expanding and solving in the correct scope so
                                // concrete args produce concrete member shapes.
                                let solved = match &stabilized {
                                    TypeExpr::Ref { type_arguments, .. }
                                        if !type_arguments.is_empty() =>
                                    {
                                        let materialize_scope =
                                            imported_component_meta_materialization_scope(
                                                &stabilized,
                                                scope_canonical_id,
                                                query_engine,
                                            )
                                            .or_else(|| imported_generic_alias_scope.clone())
                                            .unwrap_or_else(|| {
                                                scope_canonical_id.to_string()
                                            });
                                        let expanded = query_engine
                                            .expand_local_generic_ref_expr(
                                                materialize_scope.as_str(),
                                                &stabilized,
                                            )
                                            .unwrap_or_else(|| stabilized.clone());
                                        query_engine
                                            .solve_expr_type_expr(
                                                materialize_scope.as_str(),
                                                &expanded,
                                            )
                                            .map(|solved| {
                                                materialize_component_meta_member_surface_expr(
                                                    &solved,
                                                    materialize_scope.as_str(),
                                                    query_engine,
                                                    true,
                                                )
                                            })
                                            .unwrap_or_else(|| stabilized.clone())
                                    }
                                    _ => stabilized.clone(),
                                };
                                ObjectMember::Property(ObjectProperty {
                                    name: property.name.clone(),
                                    ty: if component_meta_type_expr_improves(
                                        &solved,
                                        &property.ty,
                                    ) {
                                        solved
                                    } else if component_meta_type_expr_improves(
                                        &stabilized,
                                        &property.ty,
                                    ) {
                                        stabilized
                                    } else if component_meta_type_expr_improves(
                                        &materialized,
                                        &property.ty,
                                    ) {
                                        materialized
                                    } else {
                                        property.ty.clone()
                                    },
                                    optional: property.optional,
                                    readonly: property.readonly,
                                })
                            }
                            other => other.clone(),
                        })
                        .collect();
                    TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties }))
                };

            if prefer_explicit_raw_surface
                && raw_body.is_some_and(component_meta_registry_has_explicit_object_surface)
            {
                return raw_body.cloned().map(|candidate| {
                    maybe_refine_imported_generic_alias_object(candidate, query_engine)
                });
            }
            if raw_body.is_some_and(|expr| {
                component_meta_registry_has_non_object_top_level_surface(expr)
                    && component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                        expr,
                        scope_canonical_id,
                        query_engine,
                    )
            }) {
                return raw_body.cloned();
            }
            if let Some(raw) = raw_body.filter(|expr| {
                component_meta_registry_has_non_object_top_level_surface(expr)
                    && component_meta_registry_prefers_structural_materialization(expr)
            }) {
                return Some(materialize_component_meta_registry_structural_expr(
                    raw,
                    scope_canonical_id,
                    query_engine,
                ));
            }
            query_engine
                .project_type_surface_expr(scope_canonical_id, symbol_name)
                .map(|materialized| {
                    raw_body.map_or_else(
                        || materialized.clone(),
                        |raw| {
                            let preserved_package_refs = preserve_package_backed_symbolic_refs(
                                &materialized,
                                raw,
                                scope_canonical_id,
                                query_engine,
                            );
                            preserve_registry_callable_param_member_routes(
                                &preserved_package_refs,
                                raw,
                            )
                        },
                    )
                })
                .map(|candidate| {
                    maybe_refine_imported_generic_alias_object(candidate, query_engine)
                })
                .or_else(|| {
                    raw_body.and_then(|expr| {
                        (!component_meta_registry_has_non_object_top_level_surface(expr)).then(
                            || {
                                maybe_refine_imported_generic_alias_object(
                                    expr.clone(),
                                    query_engine,
                                )
                            },
                        )
                    })
                })
                .or_else(|| {
                    raw_body.cloned().map(|candidate| {
                        maybe_refine_imported_generic_alias_object(candidate, query_engine)
                    })
                })
        }
        fn build_registry_indexed_access_expr(
            symbol_name: &str,
            path: &[String],
        ) -> verter_semantic::analysis::type_expr::TypeExpr {
            path.iter().fold(
                verter_semantic::analysis::type_expr::TypeExpr::named(symbol_name),
                |object, member| verter_semantic::analysis::type_expr::TypeExpr::IndexedAccess {
                    object: std::sync::Arc::new(object),
                    index: std::sync::Arc::new(
                        verter_semantic::analysis::type_expr::TypeExpr::string_literal(
                            member.clone(),
                        ),
                    ),
                },
            )
        }
        fn wrap_registry_member_path_surface(
            path: &[String],
            leaf: verter_semantic::analysis::type_expr::TypeExpr,
        ) -> verter_semantic::analysis::type_expr::TypeExpr {
            path.iter().rfold(leaf, |child, member| {
                verter_semantic::analysis::type_expr::TypeExpr::Object(std::sync::Arc::new(
                    verter_semantic::analysis::type_expr::ObjectExpr {
                        properties: vec![
                            verter_semantic::analysis::type_expr::ObjectMember::Property(
                                verter_semantic::analysis::type_expr::ObjectProperty {
                                    name: member.clone(),
                                    ty: child,
                                    optional: true,
                                    readonly: false,
                                },
                            ),
                        ],
                    },
                ))
            })
        }
        fn materialize_component_meta_registry_candidate_for_route(
            query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            symbol_name: &str,
            route: &crate::resolver_core::RouteDemand,
            raw_body: Option<&verter_semantic::analysis::type_expr::TypeExpr>,
            prefer_explicit_raw_surface: bool,
        ) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
            use verter_semantic::analysis::type_expr::{
                ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
            };

            match route {
                crate::resolver_core::RouteDemand::Whole => {
                    materialize_component_meta_registry_candidate(
                        query_engine,
                        scope_canonical_id,
                        symbol_name,
                        raw_body,
                        prefer_explicit_raw_surface,
                    )
                }
                crate::resolver_core::RouteDemand::MemberPath(path) if path.is_empty() => {
                    materialize_component_meta_registry_candidate(
                        query_engine,
                        scope_canonical_id,
                        symbol_name,
                        raw_body,
                        prefer_explicit_raw_surface,
                    )
                }
                crate::resolver_core::RouteDemand::MemberPath(path) => {
                    if let Some(projected) = raw_body.and_then(|expr| {
                        component_meta_registry_raw_member_path_surface(expr, path)
                    }) {
                        return Some(materialize_component_meta_member_surface_expr(
                            &projected,
                            scope_canonical_id,
                            query_engine,
                            true,
                        ));
                    }
                    let route_expr = build_registry_indexed_access_expr(symbol_name, path);
                    let leaf = query_engine
                        .project_expr_surface_expr(scope_canonical_id, &route_expr)
                        .unwrap_or(route_expr);
                    if path.len() > 1
                        && !component_meta_registry_has_explicit_object_surface(&leaf)
                        && component_meta_registry_has_non_object_top_level_surface(&leaf)
                        && matches!(leaf, TypeExpr::IndexedAccess { .. })
                    {
                        return None;
                    }
                    if path.len() > 1
                        && !component_meta_registry_has_explicit_object_surface(&leaf)
                        && !component_meta_registry_has_non_object_top_level_surface(&leaf)
                    {
                        return None;
                    }
                    Some(materialize_component_meta_member_surface_expr(
                        &wrap_registry_member_path_surface(path, leaf),
                        scope_canonical_id,
                        query_engine,
                        false,
                    ))
                }
                crate::resolver_core::RouteDemand::Pick(members) => {
                    let mut properties = Vec::new();
                    for member in members {
                        let member_route =
                            crate::resolver_core::RouteDemand::MemberPath(vec![member.clone()]);
                        let route_expr = build_registry_indexed_access_expr(
                            symbol_name,
                            std::slice::from_ref(member),
                        );
                        let projected = query_engine
                            .project_route_surface_expr(
                                scope_canonical_id,
                                symbol_name,
                                &member_route,
                            )
                            .or_else(|| {
                                materialize_member_route_from_alias_body_in_owner_scope(
                                    &route_expr,
                                    scope_canonical_id,
                                    query_engine,
                                )
                            })
                            .or_else(|| {
                                query_engine
                                    .project_expr_surface_expr(scope_canonical_id, &route_expr)
                            })
                            .unwrap_or(route_expr);
                        let member_surface = materialize_component_meta_member_surface_expr(
                            &projected,
                            scope_canonical_id,
                            query_engine,
                            true,
                        );
                        let stabilized_surface = materialize_component_meta_member_surface_expr(
                            &materialize_component_meta_type_expr_until_stable(
                                &member_surface,
                                scope_canonical_id,
                                crate::semantic_query::ProjectionMode::Expanded,
                                query_engine,
                            ),
                            scope_canonical_id,
                            query_engine,
                            true,
                        );
                        let solved_surface = match &stabilized_surface {
                            TypeExpr::Ref { type_arguments, .. } if !type_arguments.is_empty() => {
                                let materialize_scope_canonical_id =
                                    imported_component_meta_materialization_scope(
                                        &stabilized_surface,
                                        scope_canonical_id,
                                        query_engine,
                                    )
                                    .unwrap_or_else(|| scope_canonical_id.to_string());
                                let expanded = query_engine
                                    .expand_local_generic_ref_expr(
                                        materialize_scope_canonical_id.as_str(),
                                        &stabilized_surface,
                                    )
                                    .unwrap_or_else(|| stabilized_surface.clone());
                                query_engine
                                    .solve_expr_type_expr(
                                        materialize_scope_canonical_id.as_str(),
                                        &expanded,
                                    )
                                    .or(Some(expanded))
                                    .map(|solved| {
                                        materialize_component_meta_member_surface_expr(
                                            &solved,
                                            materialize_scope_canonical_id.as_str(),
                                            query_engine,
                                            true,
                                        )
                                    })
                            }
                            TypeExpr::Mapped { .. } => query_engine
                                .solve_expr_type_expr(scope_canonical_id, &stabilized_surface)
                                .map(|solved| {
                                    materialize_component_meta_member_surface_expr(
                                        &solved,
                                        scope_canonical_id,
                                        query_engine,
                                        true,
                                    )
                                }),
                            _ => None,
                        };
                        let best_surface = if let Some(solved_surface) = solved_surface {
                            if component_meta_type_expr_improves(
                                &solved_surface,
                                &stabilized_surface,
                            ) {
                                solved_surface
                            } else {
                                stabilized_surface
                            }
                        } else {
                            stabilized_surface
                        };
                        properties.push(ObjectMember::Property(ObjectProperty {
                            name: member.clone(),
                            ty: if component_meta_type_expr_improves(&best_surface, &member_surface)
                            {
                                best_surface
                            } else {
                                member_surface
                            },
                            optional: true,
                            readonly: false,
                        }));
                    }
                    (!properties.is_empty())
                        .then(|| TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties })))
                        .or_else(|| {
                            query_engine
                                .project_route_surface_expr(scope_canonical_id, symbol_name, route)
                                .map(|projected| {
                                    materialize_component_meta_member_surface_expr(
                                        &projected,
                                        scope_canonical_id,
                                        query_engine,
                                        true,
                                    )
                                })
                                .or_else(|| {
                                    materialize_component_meta_registry_candidate(
                                        query_engine,
                                        scope_canonical_id,
                                        symbol_name,
                                        raw_body,
                                        prefer_explicit_raw_surface,
                                    )
                                })
                        })
                }
                crate::resolver_core::RouteDemand::Omit(omitted) => {
                    let materialized = materialize_component_meta_registry_candidate(
                        query_engine,
                        scope_canonical_id,
                        symbol_name,
                        raw_body,
                        prefer_explicit_raw_surface,
                    )?;
                    Some(match materialized {
                        TypeExpr::Object(object) => {
                            let omitted: rustc_hash::FxHashSet<_> =
                                omitted.iter().map(String::as_str).collect();
                            TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                                properties: object
                                    .properties
                                    .iter()
                                    .filter(|member| match member {
                                        ObjectMember::Property(property) => {
                                            !omitted.contains(property.name.as_str())
                                        }
                                        _ => true,
                                    })
                                    .cloned()
                                    .collect(),
                            }))
                        }
                        other => other,
                    })
                }
            }
        }
        fn collect_imported_component_meta_registry_seed_refs(
            expr: &verter_semantic::analysis::type_expr::TypeExpr,
            published_names: &rustc_hash::FxHashSet<String>,
            queued_names: &mut rustc_hash::FxHashSet<String>,
            output: &mut std::collections::VecDeque<PendingComponentMetaRegistryRef>,
            source_hint: Option<&str>,
        ) {
            use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

            fn drain_filtered_pending(
                published_names: &rustc_hash::FxHashSet<String>,
                queued_names: &mut rustc_hash::FxHashSet<String>,
                output: &mut std::collections::VecDeque<PendingComponentMetaRegistryRef>,
                pending: std::collections::VecDeque<PendingComponentMetaRegistryRef>,
            ) {
                for pending in pending {
                    if matches!(
                        pending.route,
                        crate::resolver_core::RouteDemand::MemberPath(ref path) if path.len() > 1,
                    ) {
                        continue;
                    }
                    enqueue_component_meta_registry_ref(
                        published_names,
                        queued_names,
                        output,
                        pending.name.as_str(),
                        pending.source_hint.as_deref(),
                        pending.exported_name.as_deref(),
                        pending.route,
                    );
                }
            }

            fn collect_one_filtered_expr(
                expr: &verter_semantic::analysis::type_expr::TypeExpr,
                published_names: &rustc_hash::FxHashSet<String>,
                queued_names: &mut rustc_hash::FxHashSet<String>,
                output: &mut std::collections::VecDeque<PendingComponentMetaRegistryRef>,
                source_hint: Option<&str>,
            ) {
                let mut local_queue = std::collections::VecDeque::new();
                let mut local_names = rustc_hash::FxHashSet::default();
                collect_component_meta_registry_refs(
                    expr,
                    published_names,
                    &mut local_names,
                    &mut local_queue,
                    source_hint,
                    false,
                );
                drain_filtered_pending(published_names, queued_names, output, local_queue);
            }

            match expr {
                TypeExpr::Object(obj) => {
                    for member in &obj.properties {
                        match member {
                            ObjectMember::Property(prop) => collect_one_filtered_expr(
                                &prop.ty,
                                published_names,
                                queued_names,
                                output,
                                source_hint,
                            ),
                            ObjectMember::IndexSignature(sig) => {
                                collect_one_filtered_expr(
                                    &sig.key_type,
                                    published_names,
                                    queued_names,
                                    output,
                                    source_hint,
                                );
                                collect_one_filtered_expr(
                                    &sig.value_type,
                                    published_names,
                                    queued_names,
                                    output,
                                    source_hint,
                                );
                            }
                            ObjectMember::CallSignature(func)
                            | ObjectMember::ConstructSignature(func) => collect_one_filtered_expr(
                                &TypeExpr::Function(func.clone().into()),
                                published_names,
                                queued_names,
                                output,
                                source_hint,
                            ),
                            ObjectMember::Method(method) => collect_one_filtered_expr(
                                &TypeExpr::Function(method.function.clone().into()),
                                published_names,
                                queued_names,
                                output,
                                source_hint,
                            ),
                        }
                    }
                }
                TypeExpr::Function(func) => collect_one_filtered_expr(
                    &TypeExpr::Function(func.clone()),
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                ),
                _ => collect_one_filtered_expr(
                    expr,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                ),
            }
        }
        let debug_enabled = crate::host_manage::component_meta_debug_enabled();
        let import_refresh_started = debug_enabled.then(Instant::now);
        for (index, entry) in resolved_type_registry.iter_mut().enumerate() {
            let _entry_started = debug_enabled.then(Instant::now);
            let Some(meta) = resolved_type_registry_meta.get_mut(index) else {
                continue;
            };
            let declaration_source = meta.declaration.canonical_source.clone();
            if declaration_source.is_empty() || declaration_source == owner_canonical {
                continue;
            }
            track_component_meta_dependency(
                tracked_dependencies,
                owner_canonical,
                declaration_source.as_str(),
            );
            if should_skip_imported_registry_seed_refresh(
                owner_canonical,
                &meta.declaration,
                &entry.type_expr,
            ) {
                continue;
            }
            let requested_exported_name = if meta.declaration.resolved_name.is_empty() {
                entry.name.as_str()
            } else {
                meta.declaration.resolved_name.as_str()
            };
            let Some(resolved) = query_engine.resolve_imported_registry_symbol(
                declaration_source.as_str(),
                requested_exported_name,
            ) else {
                continue;
            };
            track_component_meta_dependency(
                tracked_dependencies,
                owner_canonical,
                resolved.canonical_id.as_str(),
            );
            for dependency in &resolved.canonical_dependencies {
                track_component_meta_dependency(
                    tracked_dependencies,
                    owner_canonical,
                    dependency.as_str(),
                );
            }
            meta.declaration.canonical_source = resolved.canonical_id.clone();
            if imported_registry_alias_should_stay_symbolic(&resolved.body) {
                entry.type_expr =
                    verter_semantic::analysis::type_expr::TypeExpr::named(entry.name.clone());
                continue;
            }
            let materialized = materialize_component_meta_registry_candidate(
                query_engine,
                resolved.canonical_id.as_str(),
                resolved.exported_name.as_str(),
                Some(&resolved.body),
                true,
            )
            .unwrap_or_else(|| resolved.body.clone());
            entry.type_expr = merge_component_meta_registry_candidates(
                Some(entry.type_expr.clone()),
                Some(materialized),
            )
            .unwrap_or_else(|| entry.type_expr.clone());
            if let Some(started) = _entry_started {
                let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                if elapsed_ms >= 5.0 {
                    crate::host_manage::component_meta_debug(format!(
                        "REGISTRY_IMPORT_UPDATE owner={} name={} source={} resolved={} elapsed_ms={:.1}",
                        owner_canonical,
                        entry.name,
                        declaration_source,
                        meta.declaration.resolved_name,
                        elapsed_ms,
                    ));
                }
            }
        }
        let import_refresh_elapsed_ms = import_refresh_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or_default();

        let mut referenced_names: VecDeque<PendingComponentMetaRegistryRef> = VecDeque::new();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut published_names: rustc_hash::FxHashSet<String> = resolved_type_registry
            .iter()
            .map(|entry| entry.name.clone())
            .collect();
        let public_field_collect_started = debug_enabled.then(Instant::now);
        if let Some(evaluated_types) = evaluated_types {
            for field in &evaluated_types.props {
                collect_component_meta_registry_public_field_refs(
                    self,
                    owner_canonical,
                    snapshot,
                    field,
                    &published_names,
                    &mut queued_names,
                    &mut referenced_names,
                    Some(owner_canonical),
                );
            }
            for field in &evaluated_types.emits {
                collect_component_meta_registry_public_field_refs(
                    self,
                    owner_canonical,
                    snapshot,
                    field,
                    &published_names,
                    &mut queued_names,
                    &mut referenced_names,
                    Some(owner_canonical),
                );
            }
            for field in &evaluated_types.slot_bindings {
                collect_component_meta_registry_public_field_refs(
                    self,
                    owner_canonical,
                    snapshot,
                    field,
                    &published_names,
                    &mut queued_names,
                    &mut referenced_names,
                    Some(owner_canonical),
                );
            }
        }
        let public_field_collect_elapsed_ms = public_field_collect_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or_default();
        let seed_scan_started = debug_enabled.then(Instant::now);
        for (index, entry) in resolved_type_registry.iter().enumerate() {
            let Some(meta) = resolved_type_registry_meta.get(index) else {
                continue;
            };
            let source_hint = Some(meta.declaration.canonical_source.as_str());
            let entry_import_root = owner_component_meta_registry_import_root(
                self,
                owner_canonical,
                snapshot,
                entry.name.as_str(),
            );
            let entry_is_imported = entry_import_root.as_ref().is_some_and(|(canonical_id, _)| {
                !canonical_id.is_empty() && canonical_id != owner_canonical
            }) || (!meta.declaration.canonical_source.is_empty()
                && meta.declaration.canonical_source != owner_canonical);
            if should_skip_imported_registry_seed_refresh(
                owner_canonical,
                &meta.declaration,
                &entry.type_expr,
            ) {
                continue;
            }
            let source_expr = source_hint
                .filter(|source| source.is_empty() || *source == owner_canonical)
                .and_then(|_| {
                    query_engine.owner_collection_expr(owner_canonical, entry.name.as_str())
                });
            if entry_is_imported {
                collect_imported_component_meta_registry_seed_refs(
                    source_expr.as_ref().unwrap_or(&entry.type_expr),
                    &published_names,
                    &mut queued_names,
                    &mut referenced_names,
                    source_hint,
                );
            } else {
                collect_component_meta_registry_refs(
                    source_expr.as_ref().unwrap_or(&entry.type_expr),
                    &published_names,
                    &mut queued_names,
                    &mut referenced_names,
                    source_hint,
                    false,
                );
            }
        }
        let seed_scan_elapsed_ms = seed_scan_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or_default();

        // Names referenced from already-seeded registry entries.
        // Helpers that a published type transitively references should
        // still be published even when they are imported generic aliases.
        let seeded_dependency_names: rustc_hash::FxHashSet<String> = {
            let mut names = rustc_hash::FxHashSet::default();
            for entry in resolved_type_registry.iter() {
                collect_type_expr_ref_names(&entry.type_expr, &mut names);
            }
            // Also include owner-local names queued alongside a seeded
            // published entry. When the registry already has published
            // entries, any owner-local pending name was transitively
            // enqueued through seed scanning and must keep its own
            // registry entry instead of being inlined as an indexed-access
            // alias. When there are no published entries yet, pending
            // names come purely from public-field scanning and may still
            // be inlined; do not protect them here.
            if !published_names.is_empty() {
                for pending in referenced_names.iter() {
                    if pending
                        .source_hint
                        .as_deref()
                        .is_none_or(|s| s.is_empty() || s == owner_canonical)
                    {
                        names.insert(pending.name.clone());
                    }
                }
            }
            names
        };
        let mut _loop_iterations: usize = 0;
        let mut _loop_materializations: usize = 0;
        let _loop_start = Instant::now();
        while let Some(pending) = referenced_names.pop_front() {
            _loop_iterations += 1;
            if !query_engine.allow_registry_deepening() {
                break;
            }
            let _pending_started =
                crate::host_manage::component_meta_debug_enabled().then(Instant::now);
            let PendingComponentMetaRegistryRef {
                name: type_name,
                source_hint: pending_source_hint_owned,
                exported_name: pending_exported_name_owned,
                route: pending_route,
            } = pending;
            let imported_owner_route = owner_component_meta_registry_import_root(
                self,
                owner_canonical,
                snapshot,
                type_name.as_str(),
            )
            .filter(|_| {
                pending_source_hint_owned
                    .as_deref()
                    .is_none_or(|source| source.is_empty() || source == owner_canonical)
            });
            let pending_source_hint = imported_owner_route
                .as_ref()
                .map(|(canonical_id, _)| canonical_id.as_str())
                .or(pending_source_hint_owned.as_deref());
            let pending_exported_name = imported_owner_route
                .as_ref()
                .map(|(_, exported_name)| exported_name.as_str())
                .or(pending_exported_name_owned.as_deref());
            if matches!(pending_route, crate::resolver_core::RouteDemand::Whole)
                && imported_owner_route
                    .as_ref()
                    .is_some_and(|(canonical_id, _)| canonical_id.contains("/node_modules/"))
            {
                continue;
            }
            if crate::host_manage::component_meta_debug_enabled() {
                crate::host_manage::component_meta_debug(format!(
                    "REGISTRY_PENDING owner={} name={} source_hint={:?} exported={:?} route={:?}",
                    owner_canonical,
                    type_name,
                    pending_source_hint,
                    pending_exported_name,
                    pending_route,
                ));
            }
            let _can_resolve = query_engine.can_resolve_registry_symbol(
                owner_canonical,
                pending_exported_name.unwrap_or(type_name.as_str()),
                pending_source_hint,
            );
            if crate::host_manage::component_meta_debug_enabled() && !_can_resolve {
                crate::host_manage::component_meta_debug(format!(
                    "REGISTRY_SKIP_UNRESOLVABLE owner={} name={} source_hint={:?} exported={:?}",
                    owner_canonical, type_name, pending_source_hint, pending_exported_name,
                ));
            }
            if !_can_resolve {
                continue;
            }
            let requested_exported_name = pending_exported_name.unwrap_or(type_name.as_str());
            if let Some(source_hint) = pending_source_hint
                .filter(|source| !source.is_empty() && *source != owner_canonical)
            {
                if !query_engine.allow_imported_root() {
                    if crate::host_manage::component_meta_debug_enabled() {
                        crate::host_manage::component_meta_debug(format!(
                            "REGISTRY_SKIP_BUDGET owner={} name={}",
                            owner_canonical, type_name,
                        ));
                    }
                    continue;
                }
                track_component_meta_dependency(tracked_dependencies, owner_canonical, source_hint);
                let _imported_pending_started =
                    crate::host_manage::component_meta_debug_enabled().then(Instant::now);
                let _resolved_import = query_engine
                    .resolve_imported_registry_symbol(source_hint, requested_exported_name);
                if crate::host_manage::component_meta_debug_enabled() && _resolved_import.is_none()
                {
                    crate::host_manage::component_meta_debug(format!(
                        "REGISTRY_IMPORT_MISS owner={} name={} source={} exported={}",
                        owner_canonical, type_name, source_hint, requested_exported_name,
                    ));
                }
                if let Some(resolved) = _resolved_import {
                    let imported_resolve_elapsed_ms = _imported_pending_started
                        .map(|started| started.elapsed().as_secs_f64() * 1000.0)
                        .unwrap_or_default();
                    track_component_meta_dependency(
                        tracked_dependencies,
                        owner_canonical,
                        resolved.canonical_id.as_str(),
                    );
                    for dependency in &resolved.canonical_dependencies {
                        track_component_meta_dependency(
                            tracked_dependencies,
                            owner_canonical,
                            dependency.as_str(),
                        );
                    }
                    let declaration_started =
                        crate::host_manage::component_meta_debug_enabled().then(Instant::now);
                    let mut declaration =
                        if matches!(pending_route, crate::resolver_core::RouteDemand::Whole) {
                            query_engine.resolve_type_declaration(
                                resolved.canonical_id.as_str(),
                                resolved.exported_name.as_str(),
                            )
                        } else {
                            query_engine
                                .resolve_direct_prepared_type_declaration_metadata(
                                    resolved.canonical_id.as_str(),
                                    resolved.exported_name.as_str(),
                                )
                                .unwrap_or_else(|| {
                                    query_engine.resolve_type_declaration(
                                        resolved.canonical_id.as_str(),
                                        resolved.exported_name.as_str(),
                                    )
                                })
                        };
                    let declaration_elapsed_ms = declaration_started
                        .map(|started| started.elapsed().as_secs_f64() * 1000.0)
                        .unwrap_or_default();
                    if declaration.canonical_source.is_empty() {
                        declaration.canonical_source = resolved.canonical_id.clone();
                    }
                    let pending_route_is_whole = match &pending_route {
                        crate::resolver_core::RouteDemand::Whole => true,
                        crate::resolver_core::RouteDemand::MemberPath(path) => path.is_empty(),
                        _ => false,
                    };
                    if crate::host_manage::component_meta_debug_enabled() {
                        crate::host_manage::component_meta_debug(format!(
                            "REGISTRY_IMPORTED_GATE owner={} name={} stay_symbolic={} route_whole={} body_variant={:?}",
                            owner_canonical, type_name,
                            imported_registry_alias_should_stay_symbolic(&resolved.body),
                            pending_route_is_whole,
                            std::mem::discriminant(&resolved.body),
                        ));
                    }
                    if pending_route_is_whole
                        && imported_registry_alias_should_stay_symbolic(&resolved.body)
                    {
                        // Imported non-object helpers (mapped/conditional/
                        // indexed-access/typeof aliases) must not be expanded
                        // into the owner registry on a whole-type route — the
                        // consumer will resolve them through member paths.
                        //
                        // If we already published a richer entry under this
                        // name, refresh its declaration metadata (the merge in
                        // upsert_component_meta_registry_entry keeps the
                        // richer body, so the bare Named placeholder is
                        // discarded by `merge_component_meta_registry_candidates`).
                        //
                        // If the name was never published, skip publication
                        // entirely — a bare Named placeholder only leaks a
                        // symbolic helper that the consumer didn't ask for.
                        if published_names.contains(&type_name) {
                            upsert_component_meta_registry_entry(
                                owner_canonical,
                                resolved_type_registry,
                                resolved_type_registry_meta,
                                &mut published_names,
                                &mut queued_names,
                                &mut referenced_names,
                                type_name.clone(),
                                verter_semantic::analysis::type_expr::TypeExpr::named(
                                    type_name.clone(),
                                ),
                                declaration,
                                None,
                            );
                        }
                        continue;
                    }
                    track_component_meta_dependency(
                        tracked_dependencies,
                        owner_canonical,
                        declaration.canonical_source.as_str(),
                    );
                    let surface_started =
                        crate::host_manage::component_meta_debug_enabled().then(Instant::now);
                    let type_expr = materialize_component_meta_registry_candidate_for_route(
                        query_engine,
                        resolved.canonical_id.as_str(),
                        resolved.exported_name.as_str(),
                        &pending_route,
                        Some(&resolved.body),
                        true,
                    )
                    .or_else(|| match &pending_route {
                        crate::resolver_core::RouteDemand::Whole => Some(resolved.body.clone()),
                        crate::resolver_core::RouteDemand::MemberPath(path) if path.is_empty() => {
                            Some(resolved.body.clone())
                        }
                        _ => None,
                    });
                    let Some(type_expr) = type_expr else {
                        continue;
                    };
                    let surface_elapsed_ms = surface_started
                        .map(|started| started.elapsed().as_secs_f64() * 1000.0)
                        .unwrap_or_default();
                    upsert_component_meta_registry_entry(
                        owner_canonical,
                        resolved_type_registry,
                        resolved_type_registry_meta,
                        &mut published_names,
                        &mut queued_names,
                        &mut referenced_names,
                        type_name.clone(),
                        type_expr,
                        declaration,
                        None,
                    );
                    if let Some(started) = _pending_started {
                        let total_elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                        if total_elapsed_ms >= 5.0 {
                            crate::host_manage::component_meta_debug(format!(
                                "REGISTRY_PENDING_IMPORTED owner={} name={} source={} resolved={} resolve_ms={:.1} declaration_ms={:.1} surface_ms={:.1} total_ms={:.1}",
                                owner_canonical,
                                type_name,
                                source_hint,
                                resolved.canonical_id,
                                imported_resolve_elapsed_ms,
                                declaration_elapsed_ms,
                                surface_elapsed_ms,
                                total_elapsed_ms,
                            ));
                        }
                    }
                    continue;
                }
            }

            let declaration_owner = pending_source_hint
                .filter(|source| !source.is_empty())
                .unwrap_or(owner_canonical);
            track_component_meta_dependency(
                tracked_dependencies,
                owner_canonical,
                declaration_owner,
            );
            let mut declaration =
                query_engine.resolve_type_declaration(declaration_owner, type_name.as_str());
            if declaration.canonical_source.is_empty() && declaration_owner != owner_canonical {
                declaration =
                    query_engine.resolve_type_declaration(owner_canonical, type_name.as_str());
            }
            let declaration_body =
                query_engine.named_decl_body(declaration_owner, type_name.as_str());
            let mut materialized = if declaration_owner != owner_canonical {
                materialize_component_meta_registry_candidate_for_route(
                    query_engine,
                    declaration_owner,
                    type_name.as_str(),
                    &pending_route,
                    declaration_body.as_ref(),
                    true,
                )
            } else {
                None
            };
            let owner_collection_expr =
                query_engine.owner_collection_expr(owner_canonical, type_name.as_str());
            // Owner-local type aliases whose body is a generic ref to an
            // imported type should resolve inline via indexed access rather
            // than creating a separate registry entry.
            let pending_route_is_whole_local =
                matches!(pending_route, crate::resolver_core::RouteDemand::Whole)
                    || matches!(
                        pending_route,
                        crate::resolver_core::RouteDemand::MemberPath(ref p) if p.is_empty(),
                    );
            if declaration_owner == owner_canonical
                && !pending_route_is_whole_local
                && !seeded_dependency_names.contains(&type_name)
            {
                if let Some(verter_semantic::analysis::type_expr::TypeExpr::Ref {
                    name: body_ref_name,
                    type_arguments,
                }) = owner_collection_expr.as_ref()
                {
                    if !type_arguments.is_empty() {
                        let body_decl =
                            query_engine.resolve_type_declaration(owner_canonical, body_ref_name);
                        let body_scope = if body_decl.canonical_source.is_empty() {
                            owner_canonical
                        } else {
                            body_decl.canonical_source.as_str()
                        };
                        if body_scope != owner_canonical {
                            continue;
                        }
                    }
                }
            }
            // Owner-local generic aliases publish the full shape so all
            // members (including those from deep indexed-access paths that
            // were already resolved inline) appear in the registry entry.
            let effective_local_route =
                if declaration_owner == owner_canonical && !pending_route_is_whole_local {
                    if owner_collection_expr.as_ref().is_some_and(|expr| {
                        matches!(
                            expr,
                            verter_semantic::analysis::type_expr::TypeExpr::Ref {
                                type_arguments, ..
                            } if !type_arguments.is_empty()
                        )
                    }) {
                        crate::resolver_core::RouteDemand::Whole
                    } else {
                        pending_route.clone()
                    }
                } else {
                    pending_route.clone()
                };
            materialized = materialized.or_else(|| {
                materialize_component_meta_registry_candidate_for_route(
                    query_engine,
                    owner_canonical,
                    type_name.as_str(),
                    &effective_local_route,
                    owner_collection_expr.as_ref(),
                    true,
                )
            });
            if materialized.is_some() && declaration.canonical_source.is_empty() {
                if let Some(import) = snapshot
                    .imports
                    .iter()
                    .find(|imp| imp.bindings.iter().any(|b| b.name == type_name))
                {
                    if let Some(canonical_id) = import.resolved_canonical_id.as_deref() {
                        if let Some(binding) = import.bindings.iter().find(|b| b.name == type_name)
                        {
                            declaration.canonical_source = canonical_id.to_string();
                            declaration.resolved_name = binding
                                .imported_name
                                .as_deref()
                                .unwrap_or("default")
                                .to_string();
                        }
                    }
                }
            }
            track_component_meta_dependency(
                tracked_dependencies,
                owner_canonical,
                declaration.canonical_source.as_str(),
            );
            let Some(materialized) = materialized else {
                continue;
            };
            let collection_expr = if owner_collection_expr.as_ref().is_some_and(|expr| {
                !component_meta_registry_has_explicit_object_surface(expr)
                    && component_meta_registry_has_explicit_object_surface(&materialized)
            }) {
                Some(materialized.clone())
            } else {
                owner_collection_expr.clone()
            };
            if crate::host_manage::component_meta_debug_enabled() {
                crate::host_manage::component_meta_debug(format!(
                    "REGISTRY_PENDING_LOCAL_SURFACE owner={} name={} route={:?} materialized={:?}",
                    owner_canonical, type_name, pending_route, materialized
                ));
            }
            _loop_materializations += 1;
            upsert_component_meta_registry_entry(
                owner_canonical,
                resolved_type_registry,
                resolved_type_registry_meta,
                &mut published_names,
                &mut queued_names,
                &mut referenced_names,
                type_name.clone(),
                materialized,
                declaration,
                collection_expr.as_ref(),
            );
            if let Some(started) = _pending_started {
                let total_elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                if total_elapsed_ms >= 5.0 {
                    crate::host_manage::component_meta_debug(format!(
                        "REGISTRY_PENDING_LOCAL owner={} name={} declaration_owner={} route={:?} total_ms={:.1}",
                        owner_canonical, type_name, declaration_owner, pending_route, total_elapsed_ms,
                    ));
                }
            }
        }
        if crate::host_manage::component_meta_debug_enabled()
            && (_loop_materializations > 0 || _loop_iterations > 0)
        {
            crate::host_manage::component_meta_debug(format!(
                "REGISTRY_LOOP owner={} iterations={} materializations={} published={} loop_ms={:.1}",
                owner_canonical,
                _loop_iterations,
                _loop_materializations,
                published_names.len(),
                _loop_start.elapsed().as_secs_f64() * 1000.0,
                ));
        }
        let loop_elapsed_ms = _loop_start.elapsed().as_secs_f64() * 1000.0;
        let enrich_started = debug_enabled.then(Instant::now);

        // Registry enrichment: materialize imported type expressions through
        // the shared request-scoped engine so projection/instantiation caches
        // are reused across all registry entries in one request.
        for (index, entry) in resolved_type_registry.iter_mut().enumerate() {
            let _entry_started =
                crate::host_manage::component_meta_debug_enabled().then(Instant::now);
            let Some(meta) = resolved_type_registry_meta.get(index) else {
                continue;
            };
            let scope_canonical = if !meta.declaration.canonical_source.is_empty() {
                meta.declaration.canonical_source.as_str()
            } else {
                owner_canonical
            };
            if scope_canonical == owner_canonical {
                continue;
            }
            if !meta.declaration.resolved_name.is_empty()
                && component_meta_registry_expr_references_name(
                    &entry.type_expr,
                    meta.declaration.resolved_name.as_str(),
                )
            {
                continue;
            }
            if component_meta_registry_has_non_object_top_level_surface(&entry.type_expr)
                && component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                    &entry.type_expr,
                    scope_canonical,
                    query_engine,
                )
            {
                continue;
            }
            let raw_body = query_engine.named_decl_body(
                scope_canonical,
                if !meta.declaration.resolved_name.is_empty() {
                    meta.declaration.resolved_name.as_str()
                } else {
                    entry.name.as_str()
                },
            );
            let materialized = materialize_component_meta_member_surface_expr(
                &entry.type_expr,
                scope_canonical,
                query_engine,
                false,
            );
            let preserved_nested_routes = raw_body
                .as_ref()
                .filter(|raw| type_expr_needs_nested_symbolic_route_preservation(raw))
                .map_or(materialized.clone(), |raw| {
                    preserve_nested_symbolic_member_routes(
                        &materialized,
                        raw,
                        scope_canonical,
                        query_engine,
                        false,
                    )
                });
            entry.type_expr = raw_body
                .as_ref()
                .map_or(preserved_nested_routes.clone(), |raw| {
                    preserve_registry_callable_param_member_routes(&preserved_nested_routes, raw)
                });
            if let Some(started) = _entry_started {
                let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                if elapsed_ms >= 5.0 {
                    crate::host_manage::component_meta_debug(format!(
                        "REGISTRY_ENRICH_ENTRY owner={} name={} scope={} elapsed_ms={:.1}",
                        owner_canonical, entry.name, scope_canonical, elapsed_ms,
                    ));
                }
            }
        }
        let enrich_elapsed_ms = enrich_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or_default();
        if debug_enabled {
            crate::host_manage::component_meta_debug(format!(
                "PROFILE_PHASES owner={} import_refresh_ms={:.1} public_field_collect_ms={:.1} seed_scan_ms={:.1} loop_ms={:.1} enrich_ms={:.1}",
                owner_canonical,
                import_refresh_elapsed_ms,
                public_field_collect_elapsed_ms,
                seed_scan_elapsed_ms,
                loop_elapsed_ms,
                enrich_elapsed_ms,
            ));
        }
    }

    /// Get a raw analysis snapshot without any enrichment.
    ///
    /// For owner files in the scheduler, reads the scheduler's latest analysis
    /// (which reflects post-recompile state). For imported deps and non-scheduler
    /// files, reads from `IndexedReadyDb` (materializing on miss). Both paths enrich
    /// the snapshot with resolved imports, destructured bindings, and template
    /// analysis.
    pub(crate) fn get_raw_analysis_snapshot(
        &self,
        canonical: &str,
    ) -> Option<FileAnalysisSnapshot> {
        component_meta_trace_custom!(
            "get_raw_analysis_snapshot",
            format!("owner={} store_view={}", canonical, false),
        );
        let normalized_canonical = self.normalized_analysis_canonical(canonical);
        let canonical = normalized_canonical.as_ref();

        {
            if let Some(cc) = self.compile_cache.get(canonical) {
                if cc.evicted {
                    return None;
                }
            }

            // Route-owned cache fast path for imported-only files: if we
            // already built a raw snapshot via the route-owned shallow state
            // pipeline, reuse it here instead of rebuilding from the
            // scheduler. This is gated on module_facts not holding it (= fully lazy).
            if self
                .project_type_store
                .indexed()
                .get_any(canonical)
                .is_none()
            {
                if let Some(raw_snapshot) = self.cached_route_owned_snapshot(canonical) {
                    self.provenance
                        .route_owned_snapshot_cache_hits
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let mut snapshot = (*raw_snapshot).clone();
                    self.resolve_snapshot_imports(canonical, &mut snapshot);
                    self.enrich_destructured_bindings(&mut snapshot);
                    if self.config.effective_scope().needs_template_analysis() {
                        self.compute_template_analysis_if_missing(canonical, &mut snapshot);
                    }
                    return Some(snapshot);
                }
            }

            // Scheduler-first path for owner files: the scheduler has the
            // latest analysis after recompile, including updated import
            // routes for newly-added dependencies. IndexedReadyDb may hold
            // stale import routes for owner files whose deps changed after
            // materialization.
            if let Some(snapshot) = self.build_snapshot_from_scheduler(canonical) {
                let whole_hash = self
                    .current_or_read_whole_hash(canonical)
                    .unwrap_or_default();
                if !self.store_view_allows_current_whole_hash(canonical, whole_hash) {
                    return None;
                }
                let mut snapshot = snapshot;
                self.resolve_snapshot_imports(canonical, &mut snapshot);
                self.enrich_destructured_bindings(&mut snapshot);
                if self.config.effective_scope().needs_template_analysis() {
                    self.compute_template_analysis_if_missing(canonical, &mut snapshot);
                }
                component_meta_trace_custom!(
                    "get_raw_analysis_snapshot_result",
                    format!(
                        "owner={} imports={} macros={} bindings={} has_template={} source=scheduler",
                        canonical,
                        snapshot.imports.len(),
                        snapshot.macros.len(),
                        snapshot.bindings.len(),
                        snapshot.template.is_some(),
                    ),
                );
                return Some(snapshot);
            }
        }

        if self
            .project_type_store
            .indexed()
            .get_any(canonical)
            .is_none()
        {
            if let Some(raw_snapshot) = self.cached_route_owned_snapshot(canonical) {
                self.provenance
                    .route_owned_snapshot_cache_hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let mut snapshot = (*raw_snapshot).clone();
                self.resolve_snapshot_imports(canonical, &mut snapshot);
                self.enrich_destructured_bindings(&mut snapshot);
                if self.config.effective_scope().needs_template_analysis() {
                    self.compute_template_analysis_if_missing(canonical, &mut snapshot);
                }
                component_meta_trace_custom!(
                    "get_raw_analysis_snapshot_result",
                    format!(
                        "owner={} imports={} macros={} bindings={} has_template={} source=route_owned_snapshot_cache",
                        canonical,
                        snapshot.imports.len(),
                        snapshot.macros.len(),
                        snapshot.bindings.len(),
                        snapshot.template.is_some(),
                    ),
                );
                return Some(snapshot);
            }

            if let Some((raw_source, cached_parse, whole_hash)) =
                self.cached_route_owned_eval_state(canonical)
            {
                if !self.store_view_allows_current_whole_hash(canonical, whole_hash) {
                    return None;
                }
                if cached_parse.is_some() {
                    self.provenance
                        .route_owned_snapshot_cached_parse_hits
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                let mut snapshot = self.build_snapshot_from_source_state(
                    canonical,
                    &raw_source,
                    cached_parse.as_deref(),
                );
                self.resolve_snapshot_imports(canonical, &mut snapshot);
                self.enrich_destructured_bindings(&mut snapshot);
                if self.config.effective_scope().needs_template_analysis() {
                    self.compute_template_analysis_if_missing(canonical, &mut snapshot);
                }
                component_meta_trace_custom!(
                    "get_raw_analysis_snapshot_result",
                    format!(
                        "owner={} imports={} macros={} bindings={} has_template={} source=route_owned_cache",
                        canonical,
                        snapshot.imports.len(),
                        snapshot.macros.len(),
                        snapshot.bindings.len(),
                        snapshot.template.is_some(),
                    ),
                );
                return Some(snapshot);
            }
        }

        // IndexedReadyDb path: covers imported deps and non-scheduler files.
        let facts = self.ensure_indexed_ready(canonical)?;
        let mut snapshot = (*facts.snapshot).clone();
        self.resolve_snapshot_imports(canonical, &mut snapshot);
        self.enrich_destructured_bindings(&mut snapshot);
        if self.config.effective_scope().needs_template_analysis() {
            self.compute_template_analysis_if_missing(canonical, &mut snapshot);
        }
        component_meta_trace_custom!(
            "get_raw_analysis_snapshot_result",
            format!(
                "owner={} imports={} macros={} bindings={} has_template={} source=indexed_ready",
                canonical,
                snapshot.imports.len(),
                snapshot.macros.len(),
                snapshot.bindings.len(),
                snapshot.template.is_some(),
            ),
        );
        Some(snapshot)
    }

    pub(crate) fn try_get_cached_resolved_meta(
        &self,
        canonical: &str,
        mode: ProjectionMode,
    ) -> Option<ResolvedComponentMetaState> {
        let cache_key = resolved_meta_cache_key(canonical, mode);
        let view_for_get = self.resolver_store_view();
        if let Some(cached) = self
            .resolver_runtime()
            .component_meta
            .get_if_valid(&cache_key, &view_for_get)
        {
            self.mirror_cached_resolved_meta_arc(canonical, mode, cached.clone());
            return Some(cached.as_ref().clone());
        }

        let entry = self.compile_cache.get(canonical)?;
        let cached = entry.cached_resolved_meta.get(&mode)?;
        let view = self.resolver_store_view();
        let invalid_details = view.invalid_fact_details(&cached.fact_versions, 6);
        if !invalid_details.is_empty() {
            component_meta_trace_custom!(
                "try_get_cached_component_meta_invalid",
                format!(
                    "owner={} mode={mode:?} cache=legacy facts={} invalid={} details=[{}]",
                    canonical,
                    cached.fact_versions.len(),
                    invalid_details.len(),
                    invalid_details.join(" | "),
                ),
            );
            return None;
        }
        self.resolver_runtime().component_meta.insert_arc(
            cache_key,
            cached.state.clone(),
            cached.fact_versions.clone(),
        );
        Some(cached.state.as_ref().clone())
    }

    pub(crate) fn store_cached_resolved_meta(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        state: &ResolvedComponentMetaState,
        fact_versions: &[crate::resolver_core::FactVersionRef],
    ) {
        component_meta_trace_custom!(
            "store_cached_component_meta_result",
            format!(
                "owner={} mode={mode:?} facts={} macros={} resolved_types={} has_evaluated_types={}",
                canonical,
                fact_versions.len(),
                state.resolved_macros.len(),
                state.resolved_type_registry.len(),
                state.evaluated_types.is_some(),
            ),
        );
        let state = Arc::new(state.clone());
        self.resolver_runtime().component_meta.insert_arc(
            resolved_meta_cache_key(canonical, mode),
            state.clone(),
            fact_versions.to_vec(),
        );
        self.mirror_cached_resolved_meta_arc(canonical, mode, state);
    }

    fn mirror_cached_resolved_meta_arc(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        state: Arc<ResolvedComponentMetaState>,
    ) {
        let cached = crate::types::ResolvedComponentMetaCacheEntry {
            fact_versions: state.fact_versions.clone(),
            state,
        };

        {
            if let Some(mut entry) = self.compile_cache.get_mut(canonical) {
                entry.cached_resolved_meta.insert(mode, cached);
            }
        }
    }

    // ───────────────────────────────────────────────────────────────────────
    // Encoded payload cache (shared by NAPI/WASM)
    // ───────────────────────────────────────────────────────────────────────

    /// Try to return a cached encoded payload for the given meta kind.
    /// Validates fact versions against the live host state.
    pub(crate) fn try_get_cached_meta_payload(
        &self,
        canonical: &str,
        kind: crate::types::MetaPayloadKind,
    ) -> Option<Vec<u8>> {
        use crate::resolver_core::StoreView;
        let entry = self.compile_cache.get(canonical)?;
        let cached = entry.cached_meta_payloads.get(&kind)?;
        let view = self.resolver_store_view();
        if cached.fact_versions.iter().all(|fact| view.validates(fact)) {
            return Some(cached.payload.clone());
        }
        None
    }

    /// Store an encoded payload in the per-file cache.
    pub(crate) fn store_meta_payload(
        &self,
        canonical: &str,
        kind: crate::types::MetaPayloadKind,
        fact_versions: &[crate::resolver_core::FactVersionRef],
        payload: Vec<u8>,
    ) {
        let cached = crate::types::CachedMetaPayload {
            fact_versions: fact_versions.to_vec(),
            payload,
        };

        {
            if let Some(mut entry) = self.compile_cache.get_mut(canonical) {
                entry.cached_meta_payloads.insert(kind, cached);
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
    ) -> Vec<crate::resolver_core::FactVersionRef> {
        let mut facts = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();

        self.append_dependency_fact_versions(canonical, &mut facts, &mut seen);
        for dep in tracked_deps {
            self.append_dependency_fact_versions(dep.as_str(), &mut facts, &mut seen);
        }

        facts
    }

    #[cfg(test)]
    pub(crate) fn fact_versions_match(
        &self,
        fact_versions: &[crate::resolver_core::FactVersionRef],
    ) -> bool {
        let view = self.resolver_store_view();
        fact_versions
            .iter()
            .all(|fact| crate::resolver_core::StoreView::validates(&view, fact))
    }

    fn append_dependency_fact_versions(
        &self,
        canonical: &str,
        facts: &mut Vec<crate::resolver_core::FactVersionRef>,
        seen: &mut rustc_hash::FxHashSet<crate::resolver_core::FactVersionRef>,
    ) {
        if let Some(hash) = self.current_or_read_whole_hash(canonical) {
            let file_fact = crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: canonical.to_string(),
                hash,
            };
            if seen.insert(file_fact.clone()) {
                facts.push(file_fact);
            }
        }

        for kind in [
            crate::resolver_core::DerivedFactKind::Route,
            crate::resolver_core::DerivedFactKind::ImportRoute,
        ] {
            if let Some(hash) = self.current_derived_fact_hash(canonical, kind) {
                let fact = crate::resolver_core::FactVersionRef::DerivedFactHash {
                    canonical_id: canonical.to_string(),
                    kind,
                    hash,
                };
                if seen.insert(fact.clone()) {
                    facts.push(fact);
                }
            }
        }

        // Legacy barrel generation facts removed — provider route cache
        // invalidates via shallow module surface hashes.
    }

    fn current_derived_fact_hash(
        &self,
        canonical_id: &str,
        kind: crate::resolver_core::DerivedFactKind,
    ) -> Option<Hash16> {
        match kind {
            crate::resolver_core::DerivedFactKind::DirectSource => {
                self.current_or_read_whole_hash(canonical_id)
            }
            crate::resolver_core::DerivedFactKind::Route => {
                // Step 8 / F5: read the cached `route_hash` from
                // IndexedReady when available — symmetric to
                // `import_route_hash`. Falls back to recomputing via
                // `hash_route_surface` only when the canonical isn't
                // yet indexed (read-only: this code path must NOT
                // call ensure_indexed because fact validation is
                // side-effect-free). Same content-hash invalidation
                // lifecycle as IndexedReady itself, so the cached
                // hash is current as long as the entry is.
                if let Some(cached) = self
                    .project_type_store
                    .indexed()
                    .get_any(canonical_id)
                    .and_then(|facts| facts.route_hash)
                {
                    return Some(cached);
                }
                let state = self.shallow_file_state(canonical_id)?;
                state
                    .has_resolvable_surface()
                    .then(|| crate::resolver_store::hash_route_surface(&state))
            }
            crate::resolver_core::DerivedFactKind::ImportRoute => {
                // Read-only: ImportRoute fact capture must not promote a
                // shallow-only tracked dependency into full IndexedReady.
                self.current_cached_import_route_hash(canonical_id)
            }
        }
    }

    fn current_cached_import_route_hash(&self, canonical_id: &str) -> Option<Hash16> {
        self.project_type_store
            .indexed()
            .get_any(canonical_id)
            .and_then(|facts| facts.import_route_hash)
            .or_else(|| {
                {
                    self.compile_cache.get(canonical_id).and_then(|entry| {
                        (!entry.import_routes.is_empty()).then(|| {
                            crate::resolver_store::hash_import_route_targets(&entry.import_routes)
                        })
                    })
                }
            })
    }
}

use crate::resolver_core::component_meta_registry::{
    collect_component_meta_registry_public_field_refs, collect_component_meta_registry_refs,
    component_meta_registry_expr_references_name,
    component_meta_registry_has_explicit_object_surface,
    component_meta_registry_has_non_object_top_level_surface,
    component_meta_registry_public_indexed_access_route,
    component_meta_registry_public_utility_route, component_meta_registry_raw_member_path_surface,
    enqueue_component_meta_registry_ref, merge_component_meta_registry_candidates,
    owner_component_meta_registry_import_root, upsert_component_meta_registry_entry,
    PendingComponentMetaRegistryRef,
};

/// Maximum recursion depth for type expression materialization.
/// Prevents stack overflow on deeply nested generic types.
const MAX_MATERIALIZE_DEPTH: u16 = 48;

thread_local! {
    static MATERIALIZE_DEPTH: std::cell::Cell<u16> = const { std::cell::Cell::new(0) };
}

/// Test-only call counter for `materialize_component_meta_type_expr_until_stable`
/// (plan §3 Step 6.2 / D22). Incremented at function entry — memo hits and
/// cold builds both count, since the counter discriminates the *entry*
/// invariant: did the caller route through whole-expression materialization
/// at all? The Step 6.2 FAIL-FIRST test asserts that route/project
/// candidates evaluated by `materialize_component_meta_macro_shape_member_type_expr`
/// satisfy the request without falling through to this entry, so the
/// counter stays at 0 in the success case.
#[cfg(test)]
pub(crate) static MTL_CALL_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Test accessor for [`MTL_CALL_COUNT`].
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn mtl_call_count_for_tests() -> usize {
    MTL_CALL_COUNT.load(std::sync::atomic::Ordering::SeqCst)
}

/// Test reset for [`MTL_CALL_COUNT`].
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn reset_mtl_call_count_for_tests() {
    MTL_CALL_COUNT.store(0, std::sync::atomic::Ordering::SeqCst);
}

/// Step 6.2 fast-path counter — instrumented in
/// `materialize_component_meta_macro_shape_member_type_expr` whenever a
/// route / project candidate satisfies the request directly without
/// falling through to the eager whole-expression materialize path.
/// The static-text test
/// `step6_2_member_route_fast_path_runs_before_eager_materialize`
/// asserts the structural ordering invariant by reading the source
/// file; the counter is kept available for future runtime probes.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) static MEMBER_ROUTE_FAST_PATH_HITS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

fn materialize_component_meta_member_surface_expr(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    nested_surface: bool,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    let mut active = rustc_hash::FxHashSet::default();
    materialize_component_meta_member_surface_expr_with_active_stack(
        expr,
        scope_canonical_id,
        engine,
        nested_surface,
        &mut active,
    )
}

fn materialize_component_meta_macro_shape_member_types(
    scope_canonical_id: &str,
    snapshot: &FileAnalysisSnapshot,
    eval_source: &str,
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) {
    fn slot_member_needs_binding_rescue(
        ty: &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> bool {
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        fn slot_binding_param_needs_materialization(
            ty: &verter_semantic::analysis::type_expr::TypeExpr,
        ) -> bool {
            use verter_semantic::analysis::type_expr::TypeExpr;

            match ty {
                TypeExpr::Parenthesized(inner) => slot_binding_param_needs_materialization(inner),
                TypeExpr::Object(_) => false,
                TypeExpr::Intersection(types) | TypeExpr::Union(types) => {
                    types.iter().any(slot_binding_param_needs_materialization)
                }
                _ => true,
            }
        }

        match ty {
            TypeExpr::Parenthesized(inner) => slot_member_needs_binding_rescue(inner),
            TypeExpr::Intersection(types) | TypeExpr::Union(types) => {
                types.iter().any(slot_member_needs_binding_rescue)
            }
            TypeExpr::Function(function) => {
                if function.parameters.is_empty() {
                    return false;
                }
                function
                    .parameters
                    .iter()
                    .any(|parameter| slot_binding_param_needs_materialization(&parameter.ty))
            }
            TypeExpr::Object(object) => {
                let mut saw_callable = false;
                for member in &object.properties {
                    match member {
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            saw_callable = true;
                            if function.parameters.iter().any(|parameter| {
                                slot_binding_param_needs_materialization(&parameter.ty)
                            }) {
                                return true;
                            }
                        }
                        ObjectMember::Method(method) => {
                            saw_callable = true;
                            if method.function.parameters.iter().any(|parameter| {
                                slot_binding_param_needs_materialization(&parameter.ty)
                            }) {
                                return true;
                            }
                        }
                        ObjectMember::Property(_) | ObjectMember::IndexSignature(_) => {}
                    }
                }
                !saw_callable
            }
            _ => true,
        }
    }

    fn shape_needs_member_rescue(
        scope_canonical_id: &str,
        shape: &verter_semantic::analysis::type_expand::ExpandedObjectShape,
        query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) -> bool {
        shape.properties.iter().any(|property| {
            type_expr_needs_projection_rescue(query_engine, scope_canonical_id, &property.ty)
        })
    }

    fn slot_binding_param_can_stay_symbolic(
        ty: &verter_semantic::analysis::type_expr::TypeExpr,
        scope_canonical_id: &str,
        query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) -> bool {
        use verter_semantic::analysis::type_expr::TypeExpr;

        match ty {
            TypeExpr::Parenthesized(inner) => {
                slot_binding_param_can_stay_symbolic(inner, scope_canonical_id, query_engine)
            }
            TypeExpr::Conditional { .. }
            | TypeExpr::Mapped { .. }
            | TypeExpr::IndexedAccess { .. }
            | TypeExpr::TypeOf(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::TemplateLiteral { .. } => true,
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => types.iter().all(|ty| {
                slot_binding_param_can_stay_symbolic(ty, scope_canonical_id, query_engine)
            }),
            TypeExpr::Ref {
                name,
                type_arguments,
            } if !type_arguments.is_empty()
                && !component_meta_ref_resolves_to_package(
                    scope_canonical_id,
                    name.as_ref(),
                    query_engine,
                ) =>
            {
                let declaration = query_engine.resolve_type_declaration(scope_canonical_id, name);
                let declaration_scope = if declaration.canonical_source.is_empty() {
                    scope_canonical_id
                } else {
                    declaration.canonical_source.as_str()
                };
                let declaration_name = if declaration.resolved_name.is_empty() {
                    name.as_ref()
                } else {
                    declaration.resolved_name.as_str()
                };
                query_engine
                    .named_decl_body(declaration_scope, declaration_name)
                    .is_some_and(|body| {
                        type_expr_has_non_object_top_level_surface(
                            query_engine,
                            declaration_scope,
                            &body,
                        )
                    })
            }
            _ => false,
        }
    }

    fn slot_member_binding_rescue_can_stay_symbolic(
        ty: &verter_semantic::analysis::type_expr::TypeExpr,
        scope_canonical_id: &str,
        query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) -> bool {
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        match ty {
            TypeExpr::Parenthesized(inner) => slot_member_binding_rescue_can_stay_symbolic(
                inner,
                scope_canonical_id,
                query_engine,
            ),
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => types.iter().all(|ty| {
                slot_member_binding_rescue_can_stay_symbolic(ty, scope_canonical_id, query_engine)
            }),
            TypeExpr::Function(function) => {
                !function.parameters.is_empty()
                    && function.parameters.iter().all(|parameter| {
                        slot_binding_param_can_stay_symbolic(
                            &parameter.ty,
                            scope_canonical_id,
                            query_engine,
                        )
                    })
            }
            TypeExpr::Object(object) => {
                let mut saw_callable = false;
                object.properties.iter().all(|member| match member {
                    ObjectMember::CallSignature(function)
                    | ObjectMember::ConstructSignature(function) => {
                        saw_callable = true;
                        !function.parameters.is_empty()
                            && function.parameters.iter().all(|parameter| {
                                slot_binding_param_can_stay_symbolic(
                                    &parameter.ty,
                                    scope_canonical_id,
                                    query_engine,
                                )
                            })
                    }
                    ObjectMember::Method(method) => {
                        saw_callable = true;
                        !method.function.parameters.is_empty()
                            && method.function.parameters.iter().all(|parameter| {
                                slot_binding_param_can_stay_symbolic(
                                    &parameter.ty,
                                    scope_canonical_id,
                                    query_engine,
                                )
                            })
                    }
                    ObjectMember::Property(_) | ObjectMember::IndexSignature(_) => true,
                }) && saw_callable
            }
            _ => false,
        }
    }

    let params =
        verter_semantic::analysis::type_eval_build::collect_define_macro_type_params(eval_source);
    let mut define_props_index = 0usize;
    let mut define_emits_index = 0usize;
    let mut define_slots_index = 0usize;

    for (macro_index, mac) in snapshot.macros.iter().enumerate() {
        if !mac.is_type_based {
            continue;
        }

        match mac.kind {
            verter_semantic::analysis::AnalyzedMacroKind::DefineProps => {
                if let Some(lowered) = params.define_props.get(define_props_index) {
                    if let Some(define_props) = evaluated_types
                        .define_props
                        .iter_mut()
                        .find(|entry| entry.macro_index == macro_index)
                    {
                        let lowered_needs_projection_rescue = type_expr_needs_projection_rescue(
                            query_engine,
                            scope_canonical_id,
                            lowered,
                        );
                        let needs_projection_rescue = lowered_needs_projection_rescue
                            || shape_needs_member_rescue(
                                scope_canonical_id,
                                &define_props.result.value,
                                query_engine,
                            );
                        if needs_projection_rescue {
                            if lowered_needs_projection_rescue
                                && define_props.result.value.properties.is_empty()
                            {
                                if let Some(projected_shape) = query_engine
                                    .project_expr_surface_shape(scope_canonical_id, lowered)
                                    .filter(has_prop_shape_surface)
                                {
                                    let projected_result =
                                        verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                                            projected_shape,
                                        );
                                    if projection_result_beats_solver_shape(
                                        &projected_result,
                                        &define_props.result,
                                    ) {
                                        define_props.result = projected_result;
                                    }
                                }
                            }
                            for property in &mut define_props.result.value.properties {
                                if type_expr_is_slots_member_route(&property.ty) {
                                    continue;
                                }
                                let preserve_symbolic_field_surface = evaluated_types
                                    .props
                                    .iter()
                                    .find(|field| field.name == property.name)
                                    .is_some_and(|field| {
                                        field_should_preserve_shallow_symbolic_raw_type(
                                            scope_canonical_id,
                                            field,
                                            query_engine,
                                        )
                                    });
                                if preserve_symbolic_field_surface {
                                    continue;
                                }
                                if define_props_member_can_stay_symbolic_without_rescue(
                                    &property.ty,
                                    scope_canonical_id,
                                    query_engine,
                                ) {
                                    continue;
                                }
                                let property_needs_projection_rescue =
                                    type_expr_needs_projection_rescue(
                                        query_engine,
                                        scope_canonical_id,
                                        &property.ty,
                                    );
                                if !property_needs_projection_rescue
                                    && !type_expr_needs_member_route_materialization(
                                        &property.ty,
                                        scope_canonical_id,
                                        query_engine,
                                    )
                                {
                                    continue;
                                }
                                component_meta_trace_custom!(
                                    "materialize_define_props_member",
                                    format!(
                                        "owner={} name={} ty={:?}",
                                        scope_canonical_id, property.name, property.ty,
                                    ),
                                );
                                property.ty =
                                    materialize_component_meta_macro_shape_member_type_expr(
                                        lowered,
                                        property.name.as_str(),
                                        &property.ty,
                                        scope_canonical_id,
                                        query_engine,
                                    );
                            }
                        }
                    }
                }
                define_props_index += 1;
            }
            verter_semantic::analysis::AnalyzedMacroKind::DefineEmits => {
                if let Some(lowered) = params.define_emits.get(define_emits_index) {
                    if let Some(define_emits) = evaluated_types
                        .define_emits
                        .iter_mut()
                        .find(|entry| entry.macro_index == macro_index)
                    {
                        let lowered_needs_projection_rescue = type_expr_needs_projection_rescue(
                            query_engine,
                            scope_canonical_id,
                            lowered,
                        );
                        let needs_projection_rescue = lowered_needs_projection_rescue
                            || shape_needs_member_rescue(
                                scope_canonical_id,
                                &define_emits.result.value,
                                query_engine,
                            );
                        if needs_projection_rescue {
                            if lowered_needs_projection_rescue
                                && define_emits.result.value.properties.is_empty()
                            {
                                if let Some(projected_shape) = query_engine
                                    .project_expr_surface_shape(scope_canonical_id, lowered)
                                    .filter(
                                        verter_semantic::analysis::type_eval_build::has_named_shape_surface,
                                    )
                                {
                                    let projected_result =
                                        verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                                            projected_shape,
                                        );
                                    if projection_result_beats_solver_shape(
                                        &projected_result,
                                        &define_emits.result,
                                    ) {
                                        define_emits.result = projected_result;
                                    }
                                }
                            }
                            for property in &mut define_emits.result.value.properties {
                                if !type_expr_needs_projection_rescue(
                                    query_engine,
                                    scope_canonical_id,
                                    &property.ty,
                                ) {
                                    continue;
                                }
                                component_meta_trace_custom!(
                                    "materialize_define_emits_member",
                                    format!(
                                        "owner={} name={} ty={:?}",
                                        scope_canonical_id, property.name, property.ty,
                                    ),
                                );
                                property.ty =
                                    materialize_component_meta_macro_shape_member_type_expr(
                                        lowered,
                                        property.name.as_str(),
                                        &property.ty,
                                        scope_canonical_id,
                                        query_engine,
                                    );
                            }
                        }
                    }
                }
                define_emits_index += 1;
            }
            verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => {
                if let Some(lowered) = params.define_slots.get(define_slots_index) {
                    if let Some(define_slots) = evaluated_types
                        .define_slots
                        .iter_mut()
                        .find(|entry| entry.macro_index == macro_index)
                    {
                        let lowered_needs_projection_rescue = type_expr_needs_projection_rescue(
                            query_engine,
                            scope_canonical_id,
                            lowered,
                        );
                        let needs_projection_rescue = lowered_needs_projection_rescue
                            || shape_needs_member_rescue(
                                scope_canonical_id,
                                &define_slots.result.value,
                                query_engine,
                            );
                        let needs_slot_binding_rescue = define_slots
                            .result
                            .value
                            .properties
                            .iter()
                            .any(|property| slot_member_needs_binding_rescue(&property.ty));
                        if needs_projection_rescue || needs_slot_binding_rescue {
                            if lowered_needs_projection_rescue
                                && define_slots.result.value.properties.is_empty()
                            {
                                if let Some(projected_shape) = query_engine
                                    .project_expr_surface_shape(scope_canonical_id, lowered)
                                    .filter(
                                        verter_semantic::analysis::type_eval_build::has_named_shape_surface,
                                    )
                                {
                                    if needs_projection_rescue {
                                        let projected_result =
                                            verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                                                projected_shape,
                                            );
                                        if projection_result_beats_solver_shape(
                                            &projected_result,
                                            &define_slots.result,
                                        ) {
                                            define_slots.result = projected_result;
                                        }
                                    }
                                }
                            }
                            for property in &mut define_slots.result.value.properties {
                                let property_needs_projection_rescue =
                                    type_expr_needs_projection_rescue(
                                        query_engine,
                                        scope_canonical_id,
                                        &property.ty,
                                    );
                                let binding_rescue_can_stay_symbolic =
                                    slot_member_binding_rescue_can_stay_symbolic(
                                        &property.ty,
                                        scope_canonical_id,
                                        query_engine,
                                    );
                                if !property_needs_projection_rescue
                                    && (binding_rescue_can_stay_symbolic
                                        || !slot_member_needs_binding_rescue(&property.ty))
                                {
                                    continue;
                                }
                                component_meta_trace_custom!(
                                    "materialize_define_slots_member",
                                    format!(
                                        "owner={} name={} ty={:?}",
                                        scope_canonical_id, property.name, property.ty,
                                    ),
                                );
                                property.ty =
                                    materialize_component_meta_macro_shape_member_type_expr(
                                        lowered,
                                        property.name.as_str(),
                                        &property.ty,
                                        scope_canonical_id,
                                        query_engine,
                                    );
                            }
                        }
                    }
                }
                define_slots_index += 1;
            }
            _ => {}
        }
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn materialize_component_meta_macro_shape_member_type_expr(
    lowered: &verter_semantic::analysis::type_expr::TypeExpr,
    member_name: &str,
    current: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    fn wrapped_member_leaf(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        member_name: &str,
    ) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
        let verter_semantic::analysis::type_expr::TypeExpr::Object(object) = expr else {
            return None;
        };
        let [verter_semantic::analysis::type_expr::ObjectMember::Property(property)] =
            object.properties.as_slice()
        else {
            return None;
        };
        (property.name == member_name).then(|| property.ty.clone())
    }

    fn top_level_needs_owner_route_fallback(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> bool {
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        fn inner(expr: &TypeExpr) -> bool {
            match expr {
                TypeExpr::Parenthesized(inner_expr)
                | TypeExpr::KeyOf(inner_expr)
                | TypeExpr::Rest(inner_expr) => inner(inner_expr),
                TypeExpr::Ref { .. }
                | TypeExpr::IndexedAccess { .. }
                | TypeExpr::Conditional { .. }
                | TypeExpr::Mapped { .. }
                | TypeExpr::TypeOf(_)
                | TypeExpr::TypeParameter(_)
                | TypeExpr::Infer { .. } => true,
                TypeExpr::Array { element, .. } => inner(element),
                TypeExpr::Tuple { elements, .. } => {
                    elements.iter().any(|element| inner(&element.ty))
                }
                TypeExpr::Union(types) | TypeExpr::Intersection(types) => types.iter().any(inner),
                TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                    ObjectMember::Property(property) => inner(&property.ty),
                    ObjectMember::Method(method) => {
                        method
                            .function
                            .parameters
                            .iter()
                            .any(|param| inner(&param.ty))
                            || method.function.return_type.as_deref().is_some_and(inner)
                    }
                    ObjectMember::IndexSignature(signature) => {
                        inner(&signature.key_type) || inner(&signature.value_type)
                    }
                    ObjectMember::CallSignature(function)
                    | ObjectMember::ConstructSignature(function) => {
                        function.parameters.iter().any(|param| inner(&param.ty))
                            || function.return_type.as_deref().is_some_and(inner)
                    }
                }),
                TypeExpr::Function(function) => {
                    function.parameters.iter().any(|param| inner(&param.ty))
                        || function.return_type.as_deref().is_some_and(inner)
                }
                TypeExpr::TemplateLiteral { expressions, .. } => expressions.iter().any(inner),
                TypeExpr::Primitive(_)
                | TypeExpr::Literal(_)
                | TypeExpr::Unknown { .. }
                | TypeExpr::RecursiveRef { .. } => false,
            }
        }

        inner(expr)
    }

    let current_is_route_expr = matches!(
        current,
        verter_semantic::analysis::type_expr::TypeExpr::IndexedAccess { .. }
    ) || component_meta_registry_public_utility_route(current)
        .or_else(|| component_meta_registry_public_indexed_access_route(current))
        .is_some();
    if !current_is_route_expr
        && matches!(
            current,
            verter_semantic::analysis::type_expr::TypeExpr::Ref { type_arguments, .. }
                if type_arguments.is_empty()
        )
    {
        return current.clone();
    }
    let materialize_scope_canonical_id = if current_is_route_expr {
        imported_component_meta_materialization_scope(current, scope_canonical_id, query_engine)
            .or_else(|| {
                imported_component_meta_materialization_scope(
                    lowered,
                    scope_canonical_id,
                    query_engine,
                )
            })
    } else {
        imported_component_meta_materialization_scope(lowered, scope_canonical_id, query_engine)
    }
    .unwrap_or_else(|| scope_canonical_id.to_string());
    let route_object_expr = match lowered {
        verter_semantic::analysis::type_expr::TypeExpr::Ref { type_arguments, .. }
            if !type_arguments.is_empty()
                && !expr_has_transitively_recursive_generic_root(
                    query_engine,
                    materialize_scope_canonical_id.as_str(),
                    lowered,
                ) =>
        {
            query_engine
                .expand_local_generic_ref_expr(materialize_scope_canonical_id.as_str(), lowered)
                .unwrap_or_else(|| lowered.clone())
        }
        _ => lowered.clone(),
    };
    let route_expr = verter_semantic::analysis::type_expr::TypeExpr::IndexedAccess {
        object: std::sync::Arc::new(route_object_expr),
        index: std::sync::Arc::new(
            verter_semantic::analysis::type_expr::TypeExpr::string_literal(member_name.to_string()),
        ),
    };
    let inline_route_candidate = current_is_route_expr
        .then(|| {
            component_meta_trace_custom!(
                "materialize_member_route_inline_candidate",
                format!(
                    "owner={} member={} current={:?}",
                    scope_canonical_id, member_name, current,
                ),
            );
            if type_expr_has_package_backed_root(current, scope_canonical_id, query_engine)
                || expr_has_transitively_recursive_generic_root(
                    query_engine,
                    scope_canonical_id,
                    current,
                )
            {
                None
            } else {
                materialize_inline_registry_member_route_from_decl_body(
                    current,
                    scope_canonical_id,
                    query_engine,
                )
                .or_else(|| {
                    materialize_inline_registry_member_route_if_materializable(
                        current,
                        scope_canonical_id,
                        query_engine,
                    )
                })
            }
        })
        .flatten();
    if let Some(candidate) = &inline_route_candidate {
        component_meta_trace_custom!(
            "materialize_member_route_inline_candidate_result",
            format!(
                "owner={} member={} candidate={:?}",
                scope_canonical_id, member_name, candidate,
            ),
        );
    }

    // Step 6.2 reorder (plan §3): try route/project candidates BEFORE
    // the eager whole-expression `materialize_component_meta_type_expr_until_stable(current, …)`
    // call. The pre-Step-6.2 ordering ran `current` materialization
    // first and only consulted route candidates as fallbacks; this
    // unconditionally invoked the heaviest path even for fixtures
    // where a single project/solve hop satisfied the request. The
    // FAIL-FIRST test `materialize_member_route_caller_ordering`
    // exercises the symbolic-intersection case where a route
    // candidate succeeds without falling through to the eager
    // materialize — `MTL_CALL_COUNT` stays at 0 post-fix.
    //
    // Returns Some(early-good-enough-candidate) when a route /
    // project candidate is concrete enough to satisfy the public
    // contract directly; None means we must fall through to the
    // eager materialization path.
    let route_scope_candidates: Vec<String> = if current_is_route_expr {
        Vec::new()
    } else if materialize_scope_canonical_id == scope_canonical_id {
        vec![scope_canonical_id.to_string()]
    } else {
        vec![
            scope_canonical_id.to_string(),
            materialize_scope_canonical_id.clone(),
        ]
    };

    let route_candidates: Vec<verter_semantic::analysis::type_expr::TypeExpr> = {
        let mut acc = Vec::new();
        for candidate_scope in &route_scope_candidates {
            let projected = {
                component_meta_trace_custom!(
                    "materialize_member_route_projected_candidate",
                    format!(
                        "owner={} member={} candidate_scope={} route={:?}",
                        scope_canonical_id, member_name, candidate_scope, route_expr,
                    ),
                );
                query_engine.project_expr_surface_expr(candidate_scope.as_str(), &route_expr)
            };
            let solved = {
                component_meta_trace_custom!(
                    "materialize_member_route_solved_candidate",
                    format!(
                        "owner={} member={} candidate_scope={} route={:?}",
                        scope_canonical_id, member_name, candidate_scope, route_expr,
                    ),
                );
                query_engine.solve_expr_type_expr(candidate_scope.as_str(), &route_expr)
            };
            for candidate in [projected, solved].into_iter().flatten() {
                acc.push(candidate);
            }
        }
        acc
    };

    let candidate_is_good_enough = |candidate: &verter_semantic::analysis::type_expr::TypeExpr| {
        !matches!(
            candidate,
            verter_semantic::analysis::type_expr::TypeExpr::Unknown { .. }
        ) && !type_expr_contains_public_member_route(candidate)
            && !top_level_needs_owner_route_fallback(candidate)
    };

    // Fast-path: any route candidate that is already structurally
    // sufficient short-circuits the eager materialize. This is the
    // observable contract the FAIL-FIRST test asserts — when a route
    // candidate succeeds, MTL_CALL_COUNT stays at 0.
    for candidate in &route_candidates {
        if expr_has_transitively_recursive_generic_root(query_engine, scope_canonical_id, candidate)
        {
            continue;
        }
        if candidate_is_good_enough(candidate) {
            #[cfg(test)]
            MEMBER_ROUTE_FAST_PATH_HITS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return candidate.clone();
        }
    }

    let current_materialized = if inline_route_candidate.is_some() {
        inline_route_candidate.clone().unwrap()
    } else {
        component_meta_trace_custom!(
            "materialize_member_route_current",
            format!(
                "owner={} member={} current={:?}",
                scope_canonical_id, member_name, current,
            ),
        );
        materialize_component_meta_type_expr_until_stable(
            current,
            materialize_scope_canonical_id.as_str(),
            crate::semantic_query::ProjectionMode::Expanded,
            query_engine,
        )
    };
    component_meta_trace_custom!(
        "materialize_member_route_current_result",
        format!(
            "owner={} member={} current_materialized={:?}",
            scope_canonical_id, member_name, current_materialized,
        ),
    );
    let mut best = if current_is_route_expr {
        wrapped_member_leaf(&current_materialized, member_name).unwrap_or(current_materialized)
    } else {
        current_materialized
    };
    if let Some(candidate) = inline_route_candidate {
        if component_meta_type_expr_improves(&candidate, &best) {
            best = candidate;
        }
    }
    if candidate_is_good_enough(&best) {
        return best;
    }
    if current_is_route_expr {
        let alias_route_candidate = {
            component_meta_trace_custom!(
                "materialize_member_route_alias_candidate",
                format!(
                    "owner={} member={} current={:?}",
                    scope_canonical_id, member_name, current,
                ),
            );
            materialize_member_route_from_alias_body_in_owner_scope(
                current,
                scope_canonical_id,
                query_engine,
            )
        };
        if let Some(candidate) = alias_route_candidate {
            if component_meta_type_expr_improves(&candidate, &best) {
                best = candidate;
            }
        }
    }

    // Slow path: previously-cached project/solve candidates that
    // weren't structurally sufficient now feed into the
    // materialize-and-improve loop, same as before. This preserves
    // behavioral coverage for fixtures that need the heavier
    // `materialize_component_meta_type_expr_until_stable(&candidate, …)`
    // recursion.
    for candidate in route_candidates {
        if expr_has_transitively_recursive_generic_root(
            query_engine,
            scope_canonical_id,
            &candidate,
        ) && !component_meta_type_expr_improves(&candidate, &best)
        {
            continue;
        }
        let candidate_materialized = {
            component_meta_trace_custom!(
                "materialize_member_route_candidate_materialized",
                format!(
                    "owner={} member={} candidate={:?}",
                    scope_canonical_id, member_name, candidate,
                ),
            );
            materialize_component_meta_type_expr_until_stable(
                &candidate,
                materialize_scope_canonical_id.as_str(),
                crate::semantic_query::ProjectionMode::Expanded,
                query_engine,
            )
        };
        if component_meta_type_expr_improves(&candidate_materialized, &best) {
            best = candidate_materialized;
        }
    }

    best
}

pub(crate) fn component_meta_type_expr_symbolic_score(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> usize {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    let mut score = 0usize;
    let mut stack = vec![expr];

    while let Some(current) = stack.pop() {
        match current {
            TypeExpr::Primitive(_) | TypeExpr::Literal(_) => {}
            TypeExpr::Parenthesized(inner)
            | TypeExpr::Array { element: inner, .. }
            | TypeExpr::KeyOf(inner)
            | TypeExpr::Rest(inner) => stack.push(inner),
            TypeExpr::Tuple { elements, .. } => {
                for element in elements.iter().rev() {
                    stack.push(&element.ty);
                }
            }
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                for ty in types.iter().rev() {
                    stack.push(ty);
                }
            }
            TypeExpr::Object(object) => {
                for member in object.properties.iter().rev() {
                    match member {
                        ObjectMember::Property(property) => stack.push(&property.ty),
                        ObjectMember::Method(method) => {
                            if let Some(return_type) = method.function.return_type.as_deref() {
                                stack.push(return_type);
                            }
                            for parameter in method.function.parameters.iter().rev() {
                                stack.push(&parameter.ty);
                            }
                        }
                        ObjectMember::IndexSignature(signature) => {
                            stack.push(&signature.value_type);
                            stack.push(&signature.key_type);
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            if let Some(return_type) = function.return_type.as_deref() {
                                stack.push(return_type);
                            }
                            for parameter in function.parameters.iter().rev() {
                                stack.push(&parameter.ty);
                            }
                        }
                    }
                }
            }
            TypeExpr::Function(function) => {
                if let Some(return_type) = function.return_type.as_deref() {
                    stack.push(return_type);
                }
                for parameter in function.parameters.iter().rev() {
                    stack.push(&parameter.ty);
                }
            }
            TypeExpr::Ref { type_arguments, .. } => {
                score += 1;
                for argument in type_arguments.iter().rev() {
                    stack.push(argument);
                }
            }
            TypeExpr::IndexedAccess { object, index } => {
                score += 1;
                stack.push(index);
                stack.push(object);
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                score += 1;
                stack.push(false_type);
                stack.push(true_type);
                stack.push(extends);
                stack.push(check);
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                score += 1;
                if let Some(name_type) = name_type.as_deref() {
                    stack.push(name_type);
                }
                stack.push(value);
                stack.push(source);
            }
            TypeExpr::TemplateLiteral { expressions, .. } => {
                score += 1;
                for expression in expressions.iter().rev() {
                    stack.push(expression);
                }
            }
            TypeExpr::RecursiveRef { type_arguments, .. } => {
                score += 1;
                for argument in type_arguments.iter().rev() {
                    stack.push(argument);
                }
            }
            TypeExpr::TypeOf(_)
            | TypeExpr::Unknown { .. }
            | TypeExpr::TypeParameter(_)
            | TypeExpr::Infer { .. } => {
                score += 1;
            }
        }
    }

    score
}

fn component_meta_type_expr_generic_detail_score(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> usize {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    let mut score = 0usize;
    let mut stack = vec![expr];

    while let Some(current) = stack.pop() {
        match current {
            TypeExpr::TypeParameter(parameter) => {
                score += 1;
                if let Some(default) = parameter.default.as_deref() {
                    stack.push(default);
                }
                if let Some(constraint) = parameter.constraint.as_deref() {
                    stack.push(constraint);
                }
            }
            TypeExpr::Parenthesized(inner)
            | TypeExpr::Array { element: inner, .. }
            | TypeExpr::KeyOf(inner)
            | TypeExpr::Rest(inner) => stack.push(inner),
            TypeExpr::Tuple { elements, .. } => {
                for element in elements.iter().rev() {
                    stack.push(&element.ty);
                }
            }
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                for ty in types.iter().rev() {
                    stack.push(ty);
                }
            }
            TypeExpr::Object(object) => {
                for member in object.properties.iter().rev() {
                    match member {
                        ObjectMember::Property(property) => stack.push(&property.ty),
                        ObjectMember::Method(method) => {
                            for type_parameter in method.function.type_parameters.iter().rev() {
                                score += 1;
                                if let Some(default) = type_parameter.default.as_deref() {
                                    stack.push(default);
                                }
                                if let Some(constraint) = type_parameter.constraint.as_deref() {
                                    stack.push(constraint);
                                }
                            }
                            if let Some(return_type) = method.function.return_type.as_deref() {
                                stack.push(return_type);
                            }
                            for parameter in method.function.parameters.iter().rev() {
                                stack.push(&parameter.ty);
                            }
                        }
                        ObjectMember::IndexSignature(signature) => {
                            stack.push(&signature.value_type);
                            stack.push(&signature.key_type);
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            for type_parameter in function.type_parameters.iter().rev() {
                                score += 1;
                                if let Some(default) = type_parameter.default.as_deref() {
                                    stack.push(default);
                                }
                                if let Some(constraint) = type_parameter.constraint.as_deref() {
                                    stack.push(constraint);
                                }
                            }
                            if let Some(return_type) = function.return_type.as_deref() {
                                stack.push(return_type);
                            }
                            for parameter in function.parameters.iter().rev() {
                                stack.push(&parameter.ty);
                            }
                        }
                    }
                }
            }
            TypeExpr::Function(function) => {
                for type_parameter in function.type_parameters.iter().rev() {
                    score += 1;
                    if let Some(default) = type_parameter.default.as_deref() {
                        stack.push(default);
                    }
                    if let Some(constraint) = type_parameter.constraint.as_deref() {
                        stack.push(constraint);
                    }
                }
                if let Some(return_type) = function.return_type.as_deref() {
                    stack.push(return_type);
                }
                for parameter in function.parameters.iter().rev() {
                    stack.push(&parameter.ty);
                }
            }
            TypeExpr::Ref { type_arguments, .. }
            | TypeExpr::RecursiveRef { type_arguments, .. } => {
                for argument in type_arguments.iter().rev() {
                    stack.push(argument);
                }
            }
            TypeExpr::IndexedAccess { object, index } => {
                stack.push(index);
                stack.push(object);
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                stack.push(false_type);
                stack.push(true_type);
                stack.push(extends);
                stack.push(check);
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                if let Some(name_type) = name_type.as_deref() {
                    stack.push(name_type);
                }
                stack.push(value);
                stack.push(source);
            }
            TypeExpr::TemplateLiteral { expressions, .. } => {
                for expression in expressions.iter().rev() {
                    stack.push(expression);
                }
            }
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::Unknown { .. }
            | TypeExpr::Infer { .. } => {}
        }
    }

    score
}

fn component_meta_type_expr_has_structural_top_level(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner) => component_meta_type_expr_has_structural_top_level(inner),
        TypeExpr::Ref { .. }
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::Infer { .. } => false,
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Object(_)
        | TypeExpr::Function(_)
        | TypeExpr::Array { .. }
        | TypeExpr::Tuple { .. }
        | TypeExpr::Union(_)
        | TypeExpr::Intersection(_)
        | TypeExpr::KeyOf(_)
        | TypeExpr::Rest(_)
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::RecursiveRef { .. } => true,
    }
}

pub(crate) fn component_meta_type_expr_improves(
    candidate: &verter_semantic::analysis::type_expr::TypeExpr,
    current: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    if matches!(
        current,
        verter_semantic::analysis::type_expr::TypeExpr::Unknown { .. }
    ) && !matches!(
        candidate,
        verter_semantic::analysis::type_expr::TypeExpr::Unknown { .. }
    ) {
        return true;
    }

    let candidate_score = component_meta_type_expr_symbolic_score(candidate);
    let current_score = component_meta_type_expr_symbolic_score(current);

    candidate_score < current_score
        || (component_meta_type_expr_has_structural_top_level(candidate)
            && !component_meta_type_expr_has_structural_top_level(current))
        || (candidate_score == current_score
            && component_meta_type_expr_generic_detail_score(candidate)
                > component_meta_type_expr_generic_detail_score(current))
}

fn component_meta_registry_prefers_structural_materialization(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(_)
        | TypeExpr::Array { .. }
        | TypeExpr::Tuple { .. }
        | TypeExpr::Union(_)
        | TypeExpr::Intersection(_)
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::Function(_)
        | TypeExpr::KeyOf(_)
        | TypeExpr::Rest(_) => true,
        TypeExpr::Ref { .. }
        | TypeExpr::Object(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => false,
    }
}

fn materialize_component_meta_registry_structural_expr(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    fn inner(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        scope_canonical_id: &str,
        engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
        active: &mut rustc_hash::FxHashSet<verter_semantic::analysis::type_expr::TypeExpr>,
    ) -> verter_semantic::analysis::type_expr::TypeExpr {
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        if !active.insert(expr.clone()) {
            return expr.clone();
        }

        let result = if let Some((root_symbol, route)) =
            component_meta_registry_public_utility_route(expr)
                .or_else(|| component_meta_registry_public_indexed_access_route(expr))
        {
            engine
                .project_route_surface_expr(scope_canonical_id, &root_symbol, &route)
                .or_else(|| {
                    let declaration =
                        engine.resolve_type_declaration(scope_canonical_id, &root_symbol);
                    (!declaration.canonical_source.is_empty())
                        .then(|| {
                            engine.project_route_surface_expr(
                                declaration.canonical_source.as_str(),
                                if declaration.resolved_name.is_empty() {
                                    root_symbol.as_str()
                                } else {
                                    declaration.resolved_name.as_str()
                                },
                                &route,
                            )
                        })
                        .flatten()
                })
                .unwrap_or_else(|| expr.clone())
        } else {
            match expr {
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } if type_arguments.is_empty() => {
                    if component_meta_ref_resolves_to_package(scope_canonical_id, name, engine) {
                        expr.clone()
                    } else {
                        engine
                            .project_type_surface_expr(scope_canonical_id, name)
                            .or_else(|| {
                                let declaration =
                                    engine.resolve_type_declaration(scope_canonical_id, name);
                                (!declaration.canonical_source.is_empty())
                                    .then(|| {
                                        engine.project_type_surface_expr(
                                            declaration.canonical_source.as_str(),
                                            if declaration.resolved_name.is_empty() {
                                                name
                                            } else {
                                                declaration.resolved_name.as_str()
                                            },
                                        )
                                    })
                                    .flatten()
                            })
                            .unwrap_or_else(|| expr.clone())
                    }
                }
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } => TypeExpr::Ref {
                    name: name.clone(),
                    type_arguments: Arc::from(
                        type_arguments
                            .iter()
                            .map(|arg| inner(arg, scope_canonical_id, engine, active))
                            .collect::<Vec<_>>(),
                    ),
                },
                TypeExpr::Parenthesized(inner_expr) => TypeExpr::Parenthesized(Arc::new(inner(
                    inner_expr,
                    scope_canonical_id,
                    engine,
                    active,
                ))),
                TypeExpr::Array { element, readonly } => TypeExpr::Array {
                    element: Arc::new(inner(element, scope_canonical_id, engine, active)),
                    readonly: *readonly,
                },
                TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
                    elements: Arc::from(
                        elements
                            .iter()
                            .map(
                                |element| verter_semantic::analysis::type_expr::TupleElement {
                                    label: element.label.clone(),
                                    ty: inner(&element.ty, scope_canonical_id, engine, active),
                                    optional: element.optional,
                                    rest: element.rest,
                                },
                            )
                            .collect::<Vec<_>>(),
                    ),
                    readonly: *readonly,
                },
                TypeExpr::Union(types) => TypeExpr::Union(Arc::from(
                    types
                        .iter()
                        .map(|ty| inner(ty, scope_canonical_id, engine, active))
                        .collect::<Vec<_>>(),
                )),
                TypeExpr::Intersection(types) => TypeExpr::Intersection(Arc::from(
                    types
                        .iter()
                        .map(|ty| inner(ty, scope_canonical_id, engine, active))
                        .collect::<Vec<_>>(),
                )),
                TypeExpr::Conditional {
                    check,
                    extends,
                    true_type,
                    false_type,
                } => TypeExpr::Conditional {
                    check: Arc::new(inner(check, scope_canonical_id, engine, active)),
                    extends: Arc::new(inner(extends, scope_canonical_id, engine, active)),
                    true_type: Arc::new(inner(true_type, scope_canonical_id, engine, active)),
                    false_type: Arc::new(inner(false_type, scope_canonical_id, engine, active)),
                },
                TypeExpr::Mapped {
                    parameter,
                    source,
                    optional,
                    readonly,
                    name_type,
                    value,
                } => TypeExpr::Mapped {
                    parameter: parameter.clone(),
                    source: Arc::new(inner(source, scope_canonical_id, engine, active)),
                    optional: *optional,
                    readonly: *readonly,
                    name_type: name_type.as_deref().map(|name_type| {
                        Arc::new(inner(name_type, scope_canonical_id, engine, active))
                    }),
                    value: Arc::new(inner(value, scope_canonical_id, engine, active)),
                },
                TypeExpr::TemplateLiteral {
                    quasis,
                    expressions,
                } => TypeExpr::TemplateLiteral {
                    quasis: quasis.clone(),
                    expressions: Arc::from(
                        expressions
                            .iter()
                            .map(|expr| inner(expr, scope_canonical_id, engine, active))
                            .collect::<Vec<_>>(),
                    ),
                },
                TypeExpr::Function(function) => {
                    let mut function = function.as_ref().clone();
                    for parameter in &mut function.parameters {
                        parameter.ty = inner(&parameter.ty, scope_canonical_id, engine, active);
                    }
                    if let Some(return_type) = function.return_type.as_mut() {
                        *return_type =
                            Arc::new(inner(return_type, scope_canonical_id, engine, active));
                    }
                    for type_parameter in &mut function.type_parameters {
                        if let Some(constraint) = type_parameter.constraint.as_mut() {
                            *constraint =
                                Arc::new(inner(constraint, scope_canonical_id, engine, active));
                        }
                        if let Some(default) = type_parameter.default.as_mut() {
                            *default = Arc::new(inner(default, scope_canonical_id, engine, active));
                        }
                    }
                    TypeExpr::Function(Arc::new(function))
                }
                TypeExpr::KeyOf(inner_expr) => TypeExpr::KeyOf(Arc::new(inner(
                    inner_expr,
                    scope_canonical_id,
                    engine,
                    active,
                ))),
                TypeExpr::Rest(inner_expr) => TypeExpr::Rest(Arc::new(inner(
                    inner_expr,
                    scope_canonical_id,
                    engine,
                    active,
                ))),
                TypeExpr::Object(object) => {
                    let mut object = object.as_ref().clone();
                    for member in &mut object.properties {
                        match member {
                            ObjectMember::Property(property) => {
                                property.ty =
                                    inner(&property.ty, scope_canonical_id, engine, active);
                            }
                            ObjectMember::IndexSignature(signature) => {
                                signature.key_type =
                                    inner(&signature.key_type, scope_canonical_id, engine, active);
                                signature.value_type = inner(
                                    &signature.value_type,
                                    scope_canonical_id,
                                    engine,
                                    active,
                                );
                            }
                            ObjectMember::CallSignature(function)
                            | ObjectMember::ConstructSignature(function) => {
                                for parameter in &mut function.parameters {
                                    parameter.ty =
                                        inner(&parameter.ty, scope_canonical_id, engine, active);
                                }
                                if let Some(return_type) = function.return_type.as_mut() {
                                    *return_type = Arc::new(inner(
                                        return_type,
                                        scope_canonical_id,
                                        engine,
                                        active,
                                    ));
                                }
                            }
                            ObjectMember::Method(method) => {
                                for parameter in &mut method.function.parameters {
                                    parameter.ty =
                                        inner(&parameter.ty, scope_canonical_id, engine, active);
                                }
                                if let Some(return_type) = method.function.return_type.as_mut() {
                                    *return_type = Arc::new(inner(
                                        return_type,
                                        scope_canonical_id,
                                        engine,
                                        active,
                                    ));
                                }
                            }
                        }
                    }
                    TypeExpr::Object(Arc::new(object))
                }
                TypeExpr::IndexedAccess { .. }
                | TypeExpr::Primitive(_)
                | TypeExpr::Literal(_)
                | TypeExpr::Unknown { .. }
                | TypeExpr::RecursiveRef { .. }
                | TypeExpr::TypeOf(_)
                | TypeExpr::TypeParameter(_)
                | TypeExpr::Infer { .. } => expr.clone(),
            }
        };

        active.remove(expr);
        result
    }

    let mut active = rustc_hash::FxHashSet::default();
    inner(expr, scope_canonical_id, engine, &mut active)
}

fn preserve_package_backed_symbolic_refs(
    materialized: &verter_semantic::analysis::type_expr::TypeExpr,
    raw: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use rustc_hash::FxHashMap;
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    match (materialized, raw) {
        (TypeExpr::Object(materialized_object), TypeExpr::Object(raw_object)) => {
            let mut object = materialized_object.as_ref().clone();
            let mut raw_properties = FxHashMap::with_capacity_and_hasher(
                raw_object.properties.len(),
                Default::default(),
            );
            for candidate in &raw_object.properties {
                if let ObjectMember::Property(raw_property) = candidate {
                    raw_properties.insert(raw_property.name.as_str(), raw_property);
                }
            }
            for member in &mut object.properties {
                let ObjectMember::Property(property) = member else {
                    continue;
                };
                let raw_property = raw_properties.get(property.name.as_str()).copied();
                let Some(raw_property) = raw_property else {
                    continue;
                };
                if let TypeExpr::Ref { name, .. } = &raw_property.ty {
                    if component_meta_ref_resolves_to_package(
                        scope_canonical_id,
                        name.as_ref(),
                        engine,
                    ) {
                        property.ty = raw_property.ty.clone();
                        continue;
                    }
                }
                property.ty = preserve_package_backed_symbolic_refs(
                    &property.ty,
                    &raw_property.ty,
                    scope_canonical_id,
                    engine,
                );
            }
            TypeExpr::Object(Arc::new(object))
        }
        _ => materialized.clone(),
    }
}

fn preserve_registry_callable_param_member_routes(
    materialized: &verter_semantic::analysis::type_expr::TypeExpr,
    raw: &verter_semantic::analysis::type_expr::TypeExpr,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use rustc_hash::FxHashMap;
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    fn inner(materialized: &TypeExpr, raw: &TypeExpr, preserve_routes: bool) -> TypeExpr {
        if preserve_routes
            && (component_meta_registry_public_utility_route(raw).is_some()
                || component_meta_registry_public_indexed_access_route(raw).is_some())
        {
            return raw.clone();
        }

        match (materialized, raw) {
            (TypeExpr::Object(materialized_object), TypeExpr::Object(raw_object)) => {
                let mut object = materialized_object.as_ref().clone();
                let mut raw_properties = FxHashMap::with_capacity_and_hasher(
                    raw_object.properties.len(),
                    Default::default(),
                );
                let mut raw_methods = FxHashMap::with_capacity_and_hasher(
                    raw_object.properties.len(),
                    Default::default(),
                );
                let mut raw_callables = FxHashMap::with_capacity_and_hasher(
                    raw_object.properties.len(),
                    Default::default(),
                );
                for candidate in &raw_object.properties {
                    match candidate {
                        ObjectMember::Property(property) => {
                            raw_properties.insert(property.name.as_str(), property);
                            if let TypeExpr::Function(function) = &property.ty {
                                raw_callables
                                    .insert(property.name.as_str(), function.as_ref().clone());
                            }
                        }
                        ObjectMember::Method(method) => {
                            raw_methods.insert(method.name.as_str(), method);
                            raw_callables.insert(method.name.as_str(), method.function.clone());
                        }
                        _ => {}
                    }
                }
                for member in &mut object.properties {
                    match member {
                        ObjectMember::Property(property) => {
                            if let Some(raw_property) = raw_properties.get(property.name.as_str()) {
                                property.ty =
                                    inner(&property.ty, &raw_property.ty, preserve_routes);
                            } else if let TypeExpr::Function(function) = &property.ty {
                                if let Some(raw_callable) =
                                    raw_callables.get(property.name.as_str())
                                {
                                    property.ty = inner(
                                        &TypeExpr::Function(function.clone()),
                                        &TypeExpr::Function(std::sync::Arc::new(
                                            raw_callable.clone(),
                                        )),
                                        preserve_routes,
                                    );
                                }
                            }
                        }
                        ObjectMember::Method(method) => {
                            if let Some(raw_method) = raw_methods.get(method.name.as_str()) {
                                method.function = match inner(
                                    &TypeExpr::Function(std::sync::Arc::new(
                                        method.function.clone(),
                                    )),
                                    &TypeExpr::Function(std::sync::Arc::new(
                                        raw_method.function.clone(),
                                    )),
                                    preserve_routes,
                                ) {
                                    TypeExpr::Function(function) => function.as_ref().clone(),
                                    _ => method.function.clone(),
                                };
                            } else if let Some(raw_callable) =
                                raw_callables.get(method.name.as_str())
                            {
                                method.function = match inner(
                                    &TypeExpr::Function(std::sync::Arc::new(
                                        method.function.clone(),
                                    )),
                                    &TypeExpr::Function(std::sync::Arc::new(raw_callable.clone())),
                                    preserve_routes,
                                ) {
                                    TypeExpr::Function(function) => function.as_ref().clone(),
                                    _ => method.function.clone(),
                                };
                            }
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            let _ = function;
                        }
                        ObjectMember::IndexSignature(_) => {}
                    }
                }
                TypeExpr::Object(std::sync::Arc::new(object))
            }
            (TypeExpr::Function(materialized_function), TypeExpr::Function(raw_function)) => {
                let mut function = materialized_function.as_ref().clone();
                for (parameter, raw_parameter) in function
                    .parameters
                    .iter_mut()
                    .zip(raw_function.parameters.iter())
                {
                    parameter.ty = inner(&parameter.ty, &raw_parameter.ty, true);
                }
                if let (Some(return_type), Some(raw_return_type)) = (
                    function.return_type.as_mut(),
                    raw_function.return_type.as_deref(),
                ) {
                    *return_type = std::sync::Arc::new(inner(return_type, raw_return_type, false));
                }
                TypeExpr::Function(std::sync::Arc::new(function))
            }
            _ => materialized.clone(),
        }
    }

    inner(materialized, raw, false)
}

fn nested_symbolic_member_route_should_stay_symbolic(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

    fn declaration_body_keeps_nested_member_route_symbolic(body: &TypeExpr) -> bool {
        match body {
            TypeExpr::Parenthesized(inner) => {
                declaration_body_keeps_nested_member_route_symbolic(inner)
            }
            TypeExpr::Ref { type_arguments, .. } if !type_arguments.is_empty() => true,
            _ => component_meta_registry_public_utility_route(body)
                .or_else(|| component_meta_registry_public_indexed_access_route(body))
                .is_some(),
        }
    }

    let Some((root_name, _)) = component_meta_registry_public_utility_route(expr)
        .or_else(|| component_meta_registry_public_indexed_access_route(expr))
    else {
        return false;
    };
    let declaration = engine.resolve_type_declaration(scope_canonical_id, root_name.as_str());
    let declaration_scope = if declaration.canonical_source.is_empty() {
        scope_canonical_id.to_string()
    } else {
        declaration.canonical_source
    };
    let declaration_name = if declaration.resolved_name.is_empty() {
        root_name
    } else {
        declaration.resolved_name
    };
    let Some(body) = engine.named_decl_body(declaration_scope.as_str(), declaration_name.as_str())
    else {
        return false;
    };
    declaration_body_keeps_nested_member_route_symbolic(&body)
}

fn type_expr_contains_public_member_route(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    if component_meta_registry_public_utility_route(expr)
        .or_else(|| component_meta_registry_public_indexed_access_route(expr))
        .is_some()
    {
        return true;
    }

    match expr {
        TypeExpr::Parenthesized(inner)
        | TypeExpr::Array { element: inner, .. }
        | TypeExpr::KeyOf(inner)
        | TypeExpr::Rest(inner) => type_expr_contains_public_member_route(inner),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| type_expr_contains_public_member_route(&element.ty)),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            types.iter().any(type_expr_contains_public_member_route)
        }
        TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
            ObjectMember::Property(property) => {
                type_expr_contains_public_member_route(&property.ty)
            }
            ObjectMember::Method(method) => {
                method
                    .function
                    .parameters
                    .iter()
                    .any(|param| type_expr_contains_public_member_route(&param.ty))
                    || method
                        .function
                        .return_type
                        .as_deref()
                        .is_some_and(type_expr_contains_public_member_route)
            }
            ObjectMember::IndexSignature(signature) => {
                type_expr_contains_public_member_route(&signature.key_type)
                    || type_expr_contains_public_member_route(&signature.value_type)
            }
            ObjectMember::CallSignature(function) | ObjectMember::ConstructSignature(function) => {
                function
                    .parameters
                    .iter()
                    .any(|param| type_expr_contains_public_member_route(&param.ty))
                    || function
                        .return_type
                        .as_deref()
                        .is_some_and(type_expr_contains_public_member_route)
            }
        }),
        TypeExpr::Function(function) => {
            function
                .parameters
                .iter()
                .any(|param| type_expr_contains_public_member_route(&param.ty))
                || function
                    .return_type
                    .as_deref()
                    .is_some_and(type_expr_contains_public_member_route)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            type_expr_contains_public_member_route(check)
                || type_expr_contains_public_member_route(extends)
                || type_expr_contains_public_member_route(true_type)
                || type_expr_contains_public_member_route(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            type_expr_contains_public_member_route(source)
                || type_expr_contains_public_member_route(value)
                || name_type
                    .as_deref()
                    .is_some_and(type_expr_contains_public_member_route)
        }
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .any(type_expr_contains_public_member_route),
        TypeExpr::Ref { type_arguments, .. } => type_arguments
            .iter()
            .any(type_expr_contains_public_member_route),
        TypeExpr::IndexedAccess { object, index } => {
            type_expr_contains_public_member_route(object)
                || type_expr_contains_public_member_route(index)
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => false,
    }
}

fn type_expr_needs_nested_symbolic_route_preservation(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Parenthesized(inner)
        | TypeExpr::Array { element: inner, .. }
        | TypeExpr::KeyOf(inner)
        | TypeExpr::Rest(inner) => type_expr_needs_nested_symbolic_route_preservation(inner),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| type_expr_needs_nested_symbolic_route_preservation(&element.ty)),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
            .iter()
            .any(type_expr_needs_nested_symbolic_route_preservation),
        TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
            ObjectMember::Property(property) => match &property.ty {
                TypeExpr::Function(function) => function
                    .parameters
                    .iter()
                    .any(|param| type_expr_contains_public_member_route(&param.ty)),
                other => type_expr_needs_nested_symbolic_route_preservation(other),
            },
            ObjectMember::Method(method) => method
                .function
                .parameters
                .iter()
                .any(|param| type_expr_contains_public_member_route(&param.ty)),
            ObjectMember::IndexSignature(signature) => {
                type_expr_needs_nested_symbolic_route_preservation(&signature.key_type)
                    || type_expr_needs_nested_symbolic_route_preservation(&signature.value_type)
            }
            ObjectMember::CallSignature(function) | ObjectMember::ConstructSignature(function) => {
                function
                    .parameters
                    .iter()
                    .any(|param| type_expr_contains_public_member_route(&param.ty))
            }
        }),
        TypeExpr::Function(function) => function
            .parameters
            .iter()
            .any(|param| type_expr_contains_public_member_route(&param.ty)),
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            type_expr_needs_nested_symbolic_route_preservation(check)
                || type_expr_needs_nested_symbolic_route_preservation(extends)
                || type_expr_needs_nested_symbolic_route_preservation(true_type)
                || type_expr_needs_nested_symbolic_route_preservation(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            type_expr_needs_nested_symbolic_route_preservation(source)
                || type_expr_needs_nested_symbolic_route_preservation(value)
                || name_type
                    .as_deref()
                    .is_some_and(type_expr_needs_nested_symbolic_route_preservation)
        }
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .any(type_expr_needs_nested_symbolic_route_preservation),
        TypeExpr::Ref { type_arguments, .. } => type_arguments
            .iter()
            .any(type_expr_needs_nested_symbolic_route_preservation),
        TypeExpr::IndexedAccess { object, index } => {
            type_expr_needs_nested_symbolic_route_preservation(object)
                || type_expr_needs_nested_symbolic_route_preservation(index)
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => false,
    }
}

fn preserve_nested_symbolic_member_routes(
    materialized: &verter_semantic::analysis::type_expr::TypeExpr,
    raw: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    nested: bool,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use std::sync::Arc;
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    let should_keep_symbolic = nested
        && nested_symbolic_member_route_should_stay_symbolic(raw, scope_canonical_id, engine);
    if should_keep_symbolic {
        return raw.clone();
    }

    match (materialized, raw) {
        (TypeExpr::Object(materialized_object), TypeExpr::Object(raw_object)) => {
            let mut object = materialized_object.as_ref().clone();
            let raw_properties = raw_object
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(property) => Some((property.name.as_str(), member)),
                    ObjectMember::Method(method) => Some((method.name.as_str(), member)),
                    _ => None,
                })
                .collect::<rustc_hash::FxHashMap<_, _>>();
            for member in &mut object.properties {
                match member {
                    ObjectMember::Property(property) => {
                        let Some(raw_member) = raw_properties.get(property.name.as_str()) else {
                            continue;
                        };
                        let ObjectMember::Property(raw_property) = raw_member else {
                            continue;
                        };
                        property.ty = preserve_nested_symbolic_member_routes(
                            &property.ty,
                            &raw_property.ty,
                            scope_canonical_id,
                            engine,
                            true,
                        );
                    }
                    ObjectMember::Method(method) => {
                        let Some(raw_member) = raw_properties.get(method.name.as_str()) else {
                            continue;
                        };
                        let ObjectMember::Method(raw_method) = raw_member else {
                            continue;
                        };
                        for (param, raw_param) in method
                            .function
                            .parameters
                            .iter_mut()
                            .zip(raw_method.function.parameters.iter())
                        {
                            param.ty = preserve_nested_symbolic_member_routes(
                                &param.ty,
                                &raw_param.ty,
                                scope_canonical_id,
                                engine,
                                true,
                            );
                        }
                        if let (Some(return_type), Some(raw_return_type)) = (
                            method.function.return_type.as_mut(),
                            raw_method.function.return_type.as_deref(),
                        ) {
                            *return_type = Arc::new(preserve_nested_symbolic_member_routes(
                                return_type,
                                raw_return_type,
                                scope_canonical_id,
                                engine,
                                true,
                            ));
                        }
                    }
                    _ => {}
                }
            }
            TypeExpr::Object(Arc::new(object))
        }
        (TypeExpr::Function(materialized_function), TypeExpr::Function(raw_function)) => {
            let mut function = materialized_function.as_ref().clone();
            for (param, raw_param) in function
                .parameters
                .iter_mut()
                .zip(raw_function.parameters.iter())
            {
                param.ty = preserve_nested_symbolic_member_routes(
                    &param.ty,
                    &raw_param.ty,
                    scope_canonical_id,
                    engine,
                    true,
                );
            }
            if let (Some(return_type), Some(raw_return_type)) = (
                function.return_type.as_mut(),
                raw_function.return_type.as_deref(),
            ) {
                *return_type = Arc::new(preserve_nested_symbolic_member_routes(
                    return_type,
                    raw_return_type,
                    scope_canonical_id,
                    engine,
                    true,
                ));
            }
            TypeExpr::Function(Arc::new(function))
        }
        (TypeExpr::Parenthesized(materialized_inner), TypeExpr::Parenthesized(raw_inner)) => {
            TypeExpr::Parenthesized(Arc::new(preserve_nested_symbolic_member_routes(
                materialized_inner,
                raw_inner,
                scope_canonical_id,
                engine,
                nested,
            )))
        }
        _ => materialized.clone(),
    }
}

pub(crate) fn component_meta_registry_should_keep_raw_symbolic_non_object_alias(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

    fn ref_stays_symbolic_in_registry(
        scope_canonical_id: &str,
        name: &str,
        engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) -> bool {
        if engine
            .resolve_direct_prepared_type_declaration(scope_canonical_id, name)
            .is_some()
        {
            return false;
        }
        engine
            .resolve_imported_registry_symbol(scope_canonical_id, name)
            .map(|resolved| resolved.canonical_id.contains("/node_modules/"))
            .unwrap_or(true)
    }

    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => true,
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            ref_stays_symbolic_in_registry(scope_canonical_id, name.as_ref(), engine)
                && type_arguments.iter().all(|arg| {
                    component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                        arg,
                        scope_canonical_id,
                        engine,
                    )
                })
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::Parenthesized(element)
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element) => {
            component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                element,
                scope_canonical_id,
                engine,
            )
        }
        TypeExpr::Tuple { elements, .. } => elements.iter().all(|element| {
            component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                &element.ty,
                scope_canonical_id,
                engine,
            )
        }),
        TypeExpr::Union(types)
        | TypeExpr::Intersection(types)
        | TypeExpr::TemplateLiteral {
            expressions: types, ..
        } => types.iter().all(|ty| {
            component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                ty,
                scope_canonical_id,
                engine,
            )
        }),
        TypeExpr::Function(func) => {
            func.parameters.iter().all(|param| {
                component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                    &param.ty,
                    scope_canonical_id,
                    engine,
                )
            }) && func.return_type.as_deref().is_none_or(|return_type| {
                component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                    return_type,
                    scope_canonical_id,
                    engine,
                )
            }) && func.type_parameters.iter().all(|param| {
                param.constraint.as_deref().is_none_or(|constraint| {
                    component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                        constraint,
                        scope_canonical_id,
                        engine,
                    )
                }) && param.default.as_deref().is_none_or(|default| {
                    component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                        default,
                        scope_canonical_id,
                        engine,
                    )
                })
            })
        }
        TypeExpr::Object(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeOf(_) => false,
    }
}

fn component_meta_ref_resolves_to_package(
    scope_canonical_id: &str,
    name: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    let declaration = engine.resolve_type_declaration(scope_canonical_id, name);
    declaration.canonical_source.contains("/node_modules/")
}

fn registry_member_route_inline_materializable(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    fn declaration_body_prefers_inline_materialization(
        body: &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> bool {
        use verter_semantic::analysis::type_expr::TypeExpr;

        match body {
            TypeExpr::Parenthesized(inner) => {
                declaration_body_prefers_inline_materialization(inner)
            }
            TypeExpr::Object(_) => true,
            TypeExpr::Ref { type_arguments, .. } => type_arguments.is_empty(),
            _ if component_meta_registry_public_utility_route(body)
                .or_else(|| component_meta_registry_public_indexed_access_route(body))
                .is_some() =>
            {
                true
            }
            _ => false,
        }
    }

    if type_expr_has_package_backed_root(expr, scope_canonical_id, engine)
        || expr_has_transitively_recursive_generic_root(engine, scope_canonical_id, expr)
    {
        return false;
    }

    let Some((root_name, _)) = component_meta_registry_public_utility_route(expr)
        .or_else(|| component_meta_registry_public_indexed_access_route(expr))
    else {
        return false;
    };
    let prepared_body = engine
        .named_decl_body(scope_canonical_id, root_name.as_str())
        .or_else(|| {
            let declaration =
                engine.resolve_type_declaration(scope_canonical_id, root_name.as_str());
            if matches!(
                declaration.kind,
                crate::resolver_core::ResolvedDeclarationKind::Interface
                    | crate::resolver_core::ResolvedDeclarationKind::Class,
            ) {
                return Some(verter_semantic::analysis::type_expr::TypeExpr::named(
                    root_name.clone(),
                ));
            }
            let declaration_scope = if declaration.canonical_source.is_empty() {
                scope_canonical_id.to_string()
            } else {
                declaration.canonical_source
            };
            let declaration_name = if declaration.resolved_name.is_empty() {
                root_name
            } else {
                declaration.resolved_name
            };
            engine.named_decl_body(declaration_scope.as_str(), declaration_name.as_str())
        });

    prepared_body
        .as_ref()
        .is_some_and(declaration_body_prefers_inline_materialization)
}

fn materialize_inline_registry_member_route_from_decl_body(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    let (root_name, route) = component_meta_registry_public_utility_route(expr)
        .or_else(|| component_meta_registry_public_indexed_access_route(expr))?;
    let declaration = engine.resolve_type_declaration(scope_canonical_id, root_name.as_str());
    let declaration_scope = if declaration.canonical_source.is_empty() {
        scope_canonical_id.to_string()
    } else {
        declaration.canonical_source
    };
    let declaration_name = if declaration.resolved_name.is_empty() {
        root_name
    } else {
        declaration.resolved_name
    };
    let body = engine.named_decl_body(declaration_scope.as_str(), declaration_name.as_str())?;
    let structural = materialize_component_meta_registry_structural_expr(
        &body,
        declaration_scope.as_str(),
        engine,
    );
    let source = if component_meta_registry_has_explicit_object_surface(&structural) {
        structural
    } else {
        body
    };

    match route {
        crate::resolver_core::RouteDemand::Whole => Some(source),
        crate::resolver_core::RouteDemand::MemberPath(path) if path.is_empty() => Some(source),
        crate::resolver_core::RouteDemand::MemberPath(path) => {
            component_meta_registry_raw_member_path_surface(&source, &path)
        }
        _ => None,
    }
}

pub(crate) fn materialize_inline_registry_member_route_if_materializable(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    registry_member_route_inline_materializable(expr, scope_canonical_id, engine).then(|| {
        materialize_inline_registry_member_route_from_decl_body(expr, scope_canonical_id, engine)
    })?
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn materialize_component_meta_member_surface_expr_with_active_stack(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    nested_surface: bool,
    active: &mut rustc_hash::FxHashSet<verter_semantic::analysis::type_expr::TypeExpr>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    if engine.has_fuse_tripped() {
        return expr.clone();
    }
    let prev_depth = MATERIALIZE_DEPTH.get();
    if prev_depth >= MAX_MATERIALIZE_DEPTH {
        return expr.clone();
    }
    MATERIALIZE_DEPTH.set(prev_depth + 1);
    let result = materialize_component_meta_member_surface_expr_with_active_stack_guarded(
        expr,
        scope_canonical_id,
        engine,
        nested_surface,
        active,
    );
    MATERIALIZE_DEPTH.set(prev_depth);
    result
}

fn materialize_component_meta_member_surface_expr_with_active_stack_guarded(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    nested_surface: bool,
    active: &mut rustc_hash::FxHashSet<verter_semantic::analysis::type_expr::TypeExpr>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    fn expand_generic_ref_for_materialization(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        scope_canonical_id: &str,
        engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) -> Option<(
        String,
        verter_semantic::analysis::type_expr::TypeExpr,
        String,
    )> {
        let imported_scope =
            imported_component_meta_materialization_scope(expr, scope_canonical_id, engine);
        let mut expansion_scopes = vec![scope_canonical_id.to_string()];
        if let Some(imported_scope) = imported_scope.as_ref() {
            if imported_scope != scope_canonical_id {
                expansion_scopes.push(imported_scope.clone());
            }
        }

        for expansion_scope in expansion_scopes {
            let Some(expanded_ref) =
                engine.expand_local_generic_ref_expr(expansion_scope.as_str(), expr)
            else {
                continue;
            };
            if expanded_ref == *expr {
                continue;
            }
            let materialize_scope = imported_scope
                .clone()
                .unwrap_or_else(|| expansion_scope.clone());
            return Some((expansion_scope, expanded_ref, materialize_scope));
        }
        None
    }

    if let Some(cached) =
        engine.cached_materialized_member_surface(scope_canonical_id, expr, nested_surface)
    {
        return cached;
    }

    if !active.insert(expr.clone()) {
        return expr.clone();
    }

    if let TypeExpr::IndexedAccess { object, index } = expr {
        if let TypeExpr::Ref { type_arguments, .. } = object.as_ref() {
            if !type_arguments.is_empty() {
                if let Some(expanded_object) =
                    engine.expand_local_generic_ref_expr(scope_canonical_id, object.as_ref())
                {
                    let expanded_route = TypeExpr::IndexedAccess {
                        object: std::sync::Arc::new(expanded_object),
                        index: index.clone(),
                    };
                    if expanded_route != *expr {
                        let result =
                            materialize_component_meta_member_surface_expr_with_active_stack(
                                &expanded_route,
                                scope_canonical_id,
                                engine,
                                nested_surface,
                                active,
                            );
                        engine.store_materialized_member_surface(
                            scope_canonical_id,
                            expr,
                            nested_surface,
                            result.clone(),
                        );
                        active.remove(expr);
                        return result;
                    }
                }
            }
        }
    }

    if nested_surface {
        let is_public_route = component_meta_registry_public_utility_route(expr)
            .or_else(|| component_meta_registry_public_indexed_access_route(expr))
            .is_some();
        let inline_materializable =
            registry_member_route_inline_materializable(expr, scope_canonical_id, engine);

        if inline_materializable {
            if let Some(inline_route_surface) =
                materialize_inline_registry_member_route_from_decl_body(
                    expr,
                    scope_canonical_id,
                    engine,
                )
            {
                if inline_route_surface != *expr {
                    let result = materialize_component_meta_member_surface_expr_with_active_stack(
                        &inline_route_surface,
                        scope_canonical_id,
                        engine,
                        true,
                        active,
                    );
                    engine.store_materialized_member_surface(
                        scope_canonical_id,
                        expr,
                        nested_surface,
                        result.clone(),
                    );
                    active.remove(expr);
                    return result;
                }
            }
        }
        if is_public_route && !inline_materializable {
            if let Some(alias_route_surface) =
                materialize_member_route_from_alias_body_in_owner_scope(
                    expr,
                    scope_canonical_id,
                    engine,
                )
            {
                if alias_route_surface != *expr {
                    let result = materialize_component_meta_member_surface_expr_with_active_stack(
                        &alias_route_surface,
                        scope_canonical_id,
                        engine,
                        true,
                        active,
                    );
                    engine.store_materialized_member_surface(
                        scope_canonical_id,
                        expr,
                        nested_surface,
                        result.clone(),
                    );
                    active.remove(expr);
                    return result;
                }
            }
        }
        if let Some((_, expanded_ref, materialize_scope_canonical_id)) =
            expand_generic_ref_for_materialization(expr, scope_canonical_id, engine)
        {
            let result = materialize_component_meta_member_surface_expr_with_active_stack(
                &expanded_ref,
                materialize_scope_canonical_id.as_str(),
                engine,
                true,
                active,
            );
            engine.store_materialized_member_surface(
                scope_canonical_id,
                expr,
                nested_surface,
                result.clone(),
            );
            active.remove(expr);
            return result;
        }
        let projected = match expr {
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => {
                if component_meta_ref_resolves_to_package(scope_canonical_id, name.as_ref(), engine)
                {
                    None
                } else {
                    engine.project_type_surface_expr(scope_canonical_id, name.as_ref())
                }
            }
            _ if is_public_route && !inline_materializable => None,
            _ => engine.project_expr_surface_expr(scope_canonical_id, expr),
        };
        if let Some(projected) = projected {
            if projected != *expr {
                let result = match expr {
                    TypeExpr::Ref { name, .. } => {
                        let projected_scope_canonical_id =
                            imported_component_meta_materialization_scope(
                                expr,
                                scope_canonical_id,
                                engine,
                            )
                            .or_else(|| {
                                let declaration = engine
                                    .resolve_type_declaration(scope_canonical_id, name.as_ref());
                                (!declaration.canonical_source.is_empty()
                                    && declaration.canonical_source != scope_canonical_id)
                                    .then_some(declaration.canonical_source)
                            })
                            .unwrap_or_else(|| scope_canonical_id.to_string());
                        if projected_scope_canonical_id != scope_canonical_id {
                            materialize_component_meta_member_surface_expr_with_active_stack(
                                &projected,
                                projected_scope_canonical_id.as_str(),
                                engine,
                                true,
                                active,
                            )
                        } else {
                            projected
                        }
                    }
                    _ => materialize_component_meta_member_surface_expr_with_active_stack(
                        &projected,
                        scope_canonical_id,
                        engine,
                        true,
                        active,
                    ),
                };
                engine.store_materialized_member_surface(
                    scope_canonical_id,
                    expr,
                    nested_surface,
                    result.clone(),
                );
                active.remove(expr);
                return result;
            }
        }
    }

    if let Some((_, expanded_ref, materialize_scope_canonical_id)) =
        expand_generic_ref_for_materialization(expr, scope_canonical_id, engine)
    {
        let result = materialize_component_meta_member_surface_expr_with_active_stack(
            &expanded_ref,
            materialize_scope_canonical_id.as_str(),
            engine,
            nested_surface,
            active,
        );
        engine.store_materialized_member_surface(
            scope_canonical_id,
            expr,
            nested_surface,
            result.clone(),
        );
        active.remove(expr);
        return result;
    }

    let structural = if component_meta_registry_public_utility_route(expr)
        .or_else(|| component_meta_registry_public_indexed_access_route(expr))
        .is_some()
        && !registry_member_route_inline_materializable(expr, scope_canonical_id, engine)
    {
        expr.clone()
    } else {
        materialize_component_meta_registry_structural_expr(expr, scope_canonical_id, engine)
    };
    if structural != *expr {
        let result = materialize_component_meta_member_surface_expr_with_active_stack(
            &structural,
            scope_canonical_id,
            engine,
            nested_surface,
            active,
        );
        engine.store_materialized_member_surface(
            scope_canonical_id,
            expr,
            nested_surface,
            result.clone(),
        );
        active.remove(expr);
        return result;
    }

    let result = match expr {
        TypeExpr::Function(function) => {
            let mut function = function.as_ref().clone();
            for param in &mut function.parameters {
                param.ty = materialize_component_meta_member_surface_expr_with_active_stack(
                    &param.ty,
                    scope_canonical_id,
                    engine,
                    true,
                    active,
                );
            }
            if let Some(return_type) = function.return_type.as_mut() {
                let materialized = materialize_component_meta_member_surface_expr_with_active_stack(
                    return_type,
                    scope_canonical_id,
                    engine,
                    true,
                    active,
                );
                *return_type = Arc::new(materialized);
            }
            TypeExpr::Function(Arc::new(function))
        }
        TypeExpr::Object(object) => {
            let mut object = object.as_ref().clone();
            for member in &mut object.properties {
                match member {
                    ObjectMember::Property(property) => {
                        let should_materialize = match &property.ty {
                            TypeExpr::Function(_) => !nested_surface,
                            TypeExpr::Object(_) => true,
                            TypeExpr::IndexedAccess { .. } => {
                                registry_member_route_inline_materializable(
                                    &property.ty,
                                    scope_canonical_id,
                                    engine,
                                )
                            }
                            TypeExpr::Ref { name, .. }
                                if matches!(name.as_ref(), "Pick" | "Omit") =>
                            {
                                registry_member_route_inline_materializable(
                                    &property.ty,
                                    scope_canonical_id,
                                    engine,
                                )
                            }
                            TypeExpr::Ref {
                                name,
                                type_arguments,
                            } => {
                                nested_surface
                                    || (type_arguments.is_empty()
                                        && !component_meta_ref_resolves_to_package(
                                            scope_canonical_id,
                                            name.as_ref(),
                                            engine,
                                        ))
                            }
                            _ => nested_surface,
                        };
                        if should_materialize {
                            property.ty =
                                materialize_component_meta_member_surface_expr_with_active_stack(
                                    &property.ty,
                                    scope_canonical_id,
                                    engine,
                                    true,
                                    active,
                                );
                        }
                    }
                    ObjectMember::IndexSignature(signature) => {
                        signature.key_type =
                            materialize_component_meta_member_surface_expr_with_active_stack(
                                &signature.key_type,
                                scope_canonical_id,
                                engine,
                                true,
                                active,
                            );
                        signature.value_type =
                            materialize_component_meta_member_surface_expr_with_active_stack(
                                &signature.value_type,
                                scope_canonical_id,
                                engine,
                                true,
                                active,
                            );
                    }
                    ObjectMember::CallSignature(function)
                    | ObjectMember::ConstructSignature(function) => {
                        for param in &mut function.parameters {
                            param.ty =
                                materialize_component_meta_member_surface_expr_with_active_stack(
                                    &param.ty,
                                    scope_canonical_id,
                                    engine,
                                    true,
                                    active,
                                );
                        }
                        if let Some(return_type) = function.return_type.as_mut() {
                            let materialized =
                                materialize_component_meta_member_surface_expr_with_active_stack(
                                    return_type,
                                    scope_canonical_id,
                                    engine,
                                    true,
                                    active,
                                );
                            *return_type = Arc::new(materialized);
                        }
                    }
                    ObjectMember::Method(method) => {
                        for param in &mut method.function.parameters {
                            param.ty =
                                materialize_component_meta_member_surface_expr_with_active_stack(
                                    &param.ty,
                                    scope_canonical_id,
                                    engine,
                                    true,
                                    active,
                                );
                        }
                        if let Some(return_type) = method.function.return_type.as_mut() {
                            let materialized =
                                materialize_component_meta_member_surface_expr_with_active_stack(
                                    return_type,
                                    scope_canonical_id,
                                    engine,
                                    true,
                                    active,
                                );
                            *return_type = Arc::new(materialized);
                        }
                    }
                }
            }
            TypeExpr::Object(Arc::new(object))
        }
        TypeExpr::Array { element, readonly } => TypeExpr::Array {
            element: Arc::new(
                materialize_component_meta_member_surface_expr_with_active_stack(
                    element,
                    scope_canonical_id,
                    engine,
                    nested_surface,
                    active,
                ),
            ),
            readonly: *readonly,
        },
        TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
            elements: Arc::from(
                elements
                    .iter()
                    .map(
                        |element| verter_semantic::analysis::type_expr::TupleElement {
                            label: element.label.clone(),
                            ty: materialize_component_meta_member_surface_expr_with_active_stack(
                                &element.ty,
                                scope_canonical_id,
                                engine,
                                nested_surface,
                                active,
                            ),
                            optional: element.optional,
                            rest: element.rest,
                        },
                    )
                    .collect::<Vec<_>>(),
            ),
            readonly: *readonly,
        },
        TypeExpr::Union(types) => {
            engine.reset_union_members();
            TypeExpr::Union(Arc::from(
                types
                    .iter()
                    .map(|ty| {
                        if !engine.allow_union_member() {
                            return ty.clone();
                        }
                        materialize_component_meta_member_surface_expr_with_active_stack(
                            ty,
                            scope_canonical_id,
                            engine,
                            nested_surface,
                            active,
                        )
                    })
                    .collect::<Vec<_>>(),
            ))
        }
        TypeExpr::Intersection(types) => TypeExpr::Intersection(Arc::from(
            types
                .iter()
                .map(|ty| {
                    materialize_component_meta_member_surface_expr_with_active_stack(
                        ty,
                        scope_canonical_id,
                        engine,
                        nested_surface,
                        active,
                    )
                })
                .collect::<Vec<_>>(),
        )),
        TypeExpr::Parenthesized(inner) => TypeExpr::Parenthesized(Arc::new(
            materialize_component_meta_member_surface_expr_with_active_stack(
                inner,
                scope_canonical_id,
                engine,
                nested_surface,
                active,
            ),
        )),
        TypeExpr::Rest(inner) => TypeExpr::Rest(Arc::new(
            materialize_component_meta_member_surface_expr_with_active_stack(
                inner,
                scope_canonical_id,
                engine,
                nested_surface,
                active,
            ),
        )),
        TypeExpr::KeyOf(inner) => TypeExpr::KeyOf(Arc::new(
            materialize_component_meta_member_surface_expr_with_active_stack(
                inner,
                scope_canonical_id,
                engine,
                nested_surface,
                active,
            ),
        )),
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => TypeExpr::Conditional {
            check: Arc::new(
                materialize_component_meta_member_surface_expr_with_active_stack(
                    check,
                    scope_canonical_id,
                    engine,
                    nested_surface,
                    active,
                ),
            ),
            extends: Arc::new(
                materialize_component_meta_member_surface_expr_with_active_stack(
                    extends,
                    scope_canonical_id,
                    engine,
                    nested_surface,
                    active,
                ),
            ),
            true_type: Arc::new(
                materialize_component_meta_member_surface_expr_with_active_stack(
                    true_type,
                    scope_canonical_id,
                    engine,
                    nested_surface,
                    active,
                ),
            ),
            false_type: Arc::new(
                materialize_component_meta_member_surface_expr_with_active_stack(
                    false_type,
                    scope_canonical_id,
                    engine,
                    nested_surface,
                    active,
                ),
            ),
        },
        TypeExpr::Mapped {
            parameter,
            source,
            optional,
            readonly,
            name_type,
            value,
        } => TypeExpr::Mapped {
            parameter: parameter.clone(),
            source: Arc::new(
                materialize_component_meta_member_surface_expr_with_active_stack(
                    source,
                    scope_canonical_id,
                    engine,
                    nested_surface,
                    active,
                ),
            ),
            optional: *optional,
            readonly: *readonly,
            name_type: name_type.as_deref().map(|name_type| {
                Arc::new(
                    materialize_component_meta_member_surface_expr_with_active_stack(
                        name_type,
                        scope_canonical_id,
                        engine,
                        nested_surface,
                        active,
                    ),
                )
            }),
            value: Arc::new(
                materialize_component_meta_member_surface_expr_with_active_stack(
                    value,
                    scope_canonical_id,
                    engine,
                    nested_surface,
                    active,
                ),
            ),
        },
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => TypeExpr::TemplateLiteral {
            quasis: quasis.clone(),
            expressions: Arc::from(
                expressions
                    .iter()
                    .map(|expr| {
                        materialize_component_meta_member_surface_expr_with_active_stack(
                            expr,
                            scope_canonical_id,
                            engine,
                            nested_surface,
                            active,
                        )
                    })
                    .collect::<Vec<_>>(),
            ),
        },
        _ => expr.clone(),
    };

    engine.store_materialized_member_surface(
        scope_canonical_id,
        expr,
        nested_surface,
        result.clone(),
    );
    active.remove(expr);
    result
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn build_origin_graph(
    graph: &Arc<crate::semantic_query_memo::SemanticGraphStore>,
) -> verter_protocol::types::OriginGraphDto {
    use crate::semantic_query::OriginEdgeKind;
    use rustc_hash::FxHashMap;
    use verter_protocol::types::{OriginEdgeDto, OriginGraphDto, OriginNodeDto};

    let all_edges = graph.export_all_origin_edges();
    if all_edges.is_empty() {
        return OriginGraphDto::default();
    }

    let mut node_index: FxHashMap<crate::semantic_query::SemanticNodeId, u32> =
        FxHashMap::default();
    let mut nodes: Vec<OriginNodeDto> = Vec::new();
    let mut meta_strings: Vec<String> = Vec::new();
    let mut meta_index_map: FxHashMap<String, u32> = FxHashMap::default();

    let mut intern_node = |id: crate::semantic_query::SemanticNodeId,
                           graph: &Arc<crate::semantic_query_memo::SemanticGraphStore>|
     -> u32 {
        if let Some(&idx) = node_index.get(&id) {
            return idx;
        }
        let idx = nodes.len() as u32;
        let (kind, label) = graph
            .node_data(id)
            .map(|d| {
                use crate::semantic_query::SemanticNodeData;
                let k = format!("{:?}", &*d).split_once('{').map_or_else(
                    || {
                        format!("{:?}", &*d)
                            .split_once('(')
                            .map_or_else(|| format!("{:?}", &*d), |(name, _)| name.to_string())
                    },
                    |(name, _)| name.to_string(),
                );
                let l = match &*d {
                    SemanticNodeData::Primitive(p) => Some(format!("{p:?}").to_lowercase()),
                    SemanticNodeData::Object(_) => Some("{...}".to_string()),
                    SemanticNodeData::TypeParam { display_name, .. } => {
                        Some(display_name.to_string())
                    }
                    SemanticNodeData::Literal(lit) => Some(format!("{lit:?}")),
                    SemanticNodeData::Array { readonly, .. } => {
                        Some(if *readonly { "readonly T[]" } else { "T[]" }.to_string())
                    }
                    SemanticNodeData::Tuple { .. } => Some("[...]".to_string()),
                    SemanticNodeData::Union(_) => Some("A | B".to_string()),
                    SemanticNodeData::Intersection(_) => Some("A & B".to_string()),
                    SemanticNodeData::Function { .. } => Some("(...) => R".to_string()),
                    _ => None,
                };
                (k, l)
            })
            .unwrap_or_else(|| ("Unknown".to_string(), None));
        nodes.push(OriginNodeDto {
            id: idx,
            kind,
            label,
        });
        node_index.insert(id, idx);
        idx
    };

    let mut edges_dto: Vec<OriginEdgeDto> = Vec::new();
    for (target_node, kind, edge) in &all_edges {
        let target_idx = intern_node(*target_node, graph);
        let edge_kind = match kind {
            OriginEdgeKind::Instantiate => "instantiate",
            OriginEdgeKind::SubstituteTypeParam => "substituteTypeParam",
            OriginEdgeKind::ConditionalSelect => "conditionalSelect",
            OriginEdgeKind::InferBind => "inferBind",
            OriginEdgeKind::ProjectMember => "projectMember",
            OriginEdgeKind::ProjectIndex => "projectIndex",
            OriginEdgeKind::ProjectPath => "projectPath",
            OriginEdgeKind::Normalize => "normalize",
            OriginEdgeKind::AliasResolve => "aliasResolve",
        };
        let meta_str = format!("{:?}", edge.meta);
        let meta_idx = if meta_str == "None" {
            None
        } else {
            let idx = if let Some(&existing) = meta_index_map.get(&meta_str) {
                existing
            } else {
                let idx = meta_strings.len() as u32;
                meta_strings.push(meta_str.clone());
                meta_index_map.insert(meta_str, idx);
                idx
            };
            Some(idx)
        };
        for source in edge.sources.iter() {
            let source_idx = intern_node(*source, graph);
            edges_dto.push(OriginEdgeDto {
                source: source_idx,
                target: target_idx,
                kind: edge_kind.to_string(),
                meta_index: meta_idx,
            });
        }
    }

    OriginGraphDto {
        nodes,
        edges: edges_dto,
        meta_strings,
    }
}

fn resolved_meta_cache_key(
    canonical: &str,
    mode: ProjectionMode,
) -> crate::resolver_core::ResolutionNodeKey {
    crate::resolver_core::ResolutionNodeKey {
        symbol_id: canonical.to_string(),
        node_kind: crate::resolver_core::ResolutionNodeKind::Assemble,
        traversal_lens: crate::resolver_core::TraversalLens::StructuralObject,
        member_path_hash: 0,
        type_args_hash: 0,
        behavior_flags: match mode {
            ProjectionMode::Identity => 1,
            ProjectionMode::Navigate => 2,
            ProjectionMode::Shallow => 3,
            ProjectionMode::Expanded => 4,
        },
    }
}

struct HostComponentMetaResolver<'a> {
    host: &'a VerterHost,
}

impl crate::resolver_core::DeclarationMetadataResolver for HostComponentMetaResolver<'_> {
    fn resolve_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<crate::resolver_core::ResolvedExportTarget> {
        self.host
            .resolve_named_type_export_target(dep_canonical, requested_name)
            .map(
                |(canonical, name)| crate::resolver_core::ResolvedExportTarget {
                    source_canonical_id: (canonical != dep_canonical).then_some(canonical),
                    source_name: name,
                },
            )
    }

    fn get_export_span_follow_reexports(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<verter_span::Span> {
        self.host
            .get_export_span_follow_reexports(dep_canonical, requested_name)
            .map(|(_, start, end)| verter_span::Span::new(start, end))
    }

    fn read_source(&self, canonical_source: &str) -> Option<String> {
        read_full_source(self.host, canonical_source)
    }

    fn type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<verter_semantic::analysis::type_eval::DeclarationId> {
        self.host
            .local_type_declaration_id(canonical_source, resolved_name)
    }

    fn resolve_type_dependency_canonical(
        &self,
        from_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        self.host
            .resolve_type_dependency_canonical(from_canonical, import_source)
    }

    fn resolve_direct_type_reexport_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        self.host
            .resolve_direct_type_reexport_target(dep_canonical, requested_name)
    }

    fn resolve_local_import_symbol_target(
        &self,
        dep_canonical: &str,
        resolved_name: &str,
    ) -> Option<(String, String)> {
        self.host
            .resolve_local_import_symbol_target(dep_canonical, resolved_name)
    }

    fn resolve_local_export_symbol_target(
        &self,
        canonical_source: &str,
        exported_name: &str,
    ) -> Option<String> {
        self.host
            .resolve_local_export_symbol_target(canonical_source, exported_name)
    }

    fn resolve_local_type_symbol_metadata(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<crate::resolver_core::ResolvedLocalTypeSymbolMetadata> {
        let analysis = self.host.external_type_analysis(canonical_source)?;
        let symbol = analysis.local_type_symbol(resolved_name)?;
        let kind = match symbol.kind {
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::TypeAlias => {
                crate::resolver_core::ResolvedDeclarationKind::TypeAlias
            }
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::Interface => {
                crate::resolver_core::ResolvedDeclarationKind::Interface
            }
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::Class => {
                crate::resolver_core::ResolvedDeclarationKind::Class
            }
        };
        Some(crate::resolver_core::ResolvedLocalTypeSymbolMetadata {
            kind,
            span: symbol.span,
        })
    }
}

impl crate::resolver_core::ComponentMetaResolverHost for HostComponentMetaResolver<'_> {
    type Snapshot = FileAnalysisSnapshot;
    type EvalContext = CapturedComponentMetaInputs;

    fn resolve_type_declaration(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> ResolvedTypeDeclaration {
        resolve_type_declaration(self.host, dep_canonical, requested_name)
    }

    fn snapshot_imports<'a>(
        &self,
        snapshot: &'a Self::Snapshot,
    ) -> &'a [verter_semantic::analysis::types::AnalyzedImport] {
        snapshot.imports.as_slice()
    }

    fn snapshot_macros<'a>(
        &self,
        snapshot: &'a Self::Snapshot,
    ) -> &'a [verter_semantic::analysis::types::AnalyzedMacro] {
        snapshot.macros.as_slice()
    }

    fn snapshot_macro_type_deps<'a>(
        &self,
        snapshot: &'a Self::Snapshot,
    ) -> &'a [verter_semantic::analysis::types::MacroTypeDep] {
        snapshot.macro_type_deps.as_slice()
    }

    fn build_eval_outputs(
        &self,
        owner_canonical: &str,
        snapshot: &Self::Snapshot,
        eval_context: Option<&Self::EvalContext>,
        purpose: crate::resolver_core::ComponentMetaResolutionPurpose,
    ) -> ComponentMetaEvalOutputs {
        let eval_started = component_meta_debug_enabled().then(Instant::now);
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "resolve_component_meta owner={} mode={:?} step=evaluated_types:start imports={} macro_type_deps={}",
                owner_canonical,
                ProjectionMode::Expanded,
                snapshot.imports.len(),
                snapshot.macro_type_deps.len(),
            ));
        }
        // Tracked dependencies: snapshot-level candidates + solver-discovered deps.
        // The legacy walker is no longer used for dependency tracking.
        let mut tracked_dependencies = std::collections::BTreeSet::new();
        tracked_dependencies.extend(
            eval_context
                .map(|captured| captured.direct_dependency_candidates.clone())
                .unwrap_or_else(|| {
                    self.host
                        .cache_dependency_candidates_from_snapshot(owner_canonical, snapshot)
                }),
        );
        let compute_eval_start = component_meta_debug_enabled().then(Instant::now);
        // D-Cutover §5.8 WIP-W: the retired `shared_owner_engine` path
        // is gone; all callers go through
        // `compute_evaluated_types_with_tracking_from_owner_context`
        // which internally builds any needed host bridge.
        let computed_eval_types = self
            .host
            .compute_evaluated_types_with_tracking_from_owner_context(
                owner_canonical,
                snapshot,
                eval_context.and_then(|captured| captured.owner_eval_source.as_deref()),
                purpose,
            );
        if let Some(compute_eval_start) = compute_eval_start {
            let elapsed = compute_eval_start.elapsed();
            component_meta_debug(format!(
                "EVAL_TYPES owner={} elapsed_ms={:.1} has_result={}",
                owner_canonical,
                elapsed.as_secs_f64() * 1000.0,
                computed_eval_types.is_some(),
            ));
        }
        if let Some(computed) = computed_eval_types.as_ref() {
            tracked_dependencies.extend(computed.discovered_dependencies.iter().cloned());
        }
        let evaluated_types = computed_eval_types.and_then(|computed| computed.evaluated_types);
        if let Some(eval_started) = eval_started {
            component_meta_debug(format!(
                "resolve_component_meta owner={} mode={:?} evaluated_types took {:?} has_output={}",
                owner_canonical,
                ProjectionMode::Expanded,
                eval_started.elapsed(),
                evaluated_types
                    .as_ref()
                    .is_some_and(|types| !types.is_empty()),
            ));
        }
        ComponentMetaEvalOutputs {
            evaluated_types,
            tracked_dependencies,
        }
    }

    fn projectable_owner_local_macro_roots(
        &self,
        owner_canonical: &str,
        mac: &verter_semantic::analysis::types::AnalyzedMacro,
    ) -> Vec<String> {
        fn macro_lacks_direct_local_surface(
            mac: &verter_semantic::analysis::types::AnalyzedMacro,
        ) -> bool {
            match mac.kind {
                verter_semantic::analysis::AnalyzedMacroKind::DefineProps
                | verter_semantic::analysis::AnalyzedMacroKind::WithDefaults
                | verter_semantic::analysis::AnalyzedMacroKind::DefineModel => {
                    mac.prop_fields.is_empty()
                }
                verter_semantic::analysis::AnalyzedMacroKind::DefineEmits => {
                    mac.emit_fields.is_empty()
                }
                verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => {
                    mac.slot_fields.is_empty()
                }
                verter_semantic::analysis::AnalyzedMacroKind::DefineExpose
                | verter_semantic::analysis::AnalyzedMacroKind::DefineOptions => false,
            }
        }

        let mut candidate_roots = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();

        for resolved in &mac.resolved_local_types {
            let is_direct_local = mac
                .type_references
                .iter()
                .any(|type_name| type_name == &resolved.name);
            if is_direct_local && seen.insert(resolved.name.as_str()) {
                candidate_roots.push(resolved.name.as_str());
            }
        }

        if candidate_roots.is_empty() && macro_lacks_direct_local_surface(mac) {
            let owner_has_symbol = self.host.route_owned_shallow_state(owner_canonical);
            for type_name in &mac.type_references {
                if type_name.contains('.') || !seen.insert(type_name.as_str()) {
                    continue;
                }
                let owner_local_decl = owner_has_symbol
                    .as_ref()
                    .is_some_and(|state| state.symbol(type_name).is_some())
                    || self
                        .resolve_type_declaration(owner_canonical, type_name)
                        .canonical_source
                        == owner_canonical;
                if owner_local_decl {
                    candidate_roots.push(type_name.as_str());
                }
            }
        }

        if candidate_roots.is_empty() {
            return Vec::new();
        }

        let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(self.host);

        candidate_roots
            .into_iter()
            .filter(|root_name| {
                query_engine
                    .project_prepared_type_surface_shape(owner_canonical, root_name)
                    .is_some_and(|shape| match mac.kind {
                        verter_semantic::analysis::AnalyzedMacroKind::DefineProps
                        | verter_semantic::analysis::AnalyzedMacroKind::WithDefaults
                        | verter_semantic::analysis::AnalyzedMacroKind::DefineModel
                        | verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => true,
                        verter_semantic::analysis::AnalyzedMacroKind::DefineEmits => {
                            !shape.properties.is_empty() || !shape.call_signatures.is_empty()
                        }
                        verter_semantic::analysis::AnalyzedMacroKind::DefineExpose
                        | verter_semantic::analysis::AnalyzedMacroKind::DefineOptions => false,
                    })
            })
            .map(str::to_string)
            .collect()
    }

    fn resolve_owner_local_macro_surface(
        &self,
        owner_canonical: &str,
        root_name: &str,
        macro_kind: verter_semantic::analysis::types::AnalyzedMacroKind,
    ) -> Option<crate::resolver_core::surface_projector::ProjectedMacroSurfaces> {
        let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(self.host);
        let shape = query_engine.project_prepared_type_surface_shape(owner_canonical, root_name)?;
        Some(
            crate::resolver_core::component_meta::project_macro_surfaces_from_expanded_shape(
                macro_kind, &shape,
            ),
        )
    }

    fn resolve_macro_elements(
        &self,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements> {
        let _ = visiting;
        self.host.resolve_component_meta_macro_elements(
            owner_canonical,
            import_source,
            exported_name,
            tracked_deps,
            resolution_deps,
            cache,
        )
    }

    fn resolve_imported_macro_surface(
        &self,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<crate::resolver_core::ResolvedImportedMacroSurface> {
        let _ = visiting;
        self.host.resolve_component_meta_macro_surface(
            owner_canonical,
            import_source,
            exported_name,
            tracked_deps,
            resolution_deps,
            cache,
        )
    }

    fn resolve_jsdoc_block(
        &self,
        canonical_source: &str,
        span: verter_span::Span,
        expanded: bool,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<ResolvedJsdocBlock> {
        resolve_jsdoc_block(
            self.host,
            canonical_source,
            span,
            if expanded {
                ProjectionMode::Expanded
            } else {
                ProjectionMode::Identity
            },
            tracked_deps,
            cache,
            visiting,
            verter_workspace::ResolveRequestKind::TypeImport,
        )
    }

    fn sync_transitive_macro_type_dependencies(
        &self,
        canonical_id: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
    ) {
        self.host
            .sync_transitive_macro_type_dependencies(canonical_id, tracked_deps);
    }

    fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
    ) -> Vec<crate::resolver_core::FactVersionRef> {
        self.host
            .current_dependency_fact_versions(canonical, tracked_deps)
    }
}

pub(crate) fn resolve_type_declaration(
    host: &VerterHost,
    dep_canonical: &str,
    requested_name: &str,
) -> ResolvedTypeDeclaration {
    let resolver = HostComponentMetaResolver { host };
    let key =
        crate::resolver_core::symbol_resolver::declaration_node_key(dep_canonical, requested_name);
    let mut ctx = crate::resolver_core::symbol_resolver::ResolveContext::new();
    let permissive_view = crate::resolver_core::PermissiveStoreView;
    let result =
        host.resolver_runtime()
            .symbol
            .resolve_node(key, &permissive_view, &mut ctx, |_| {
                let declaration = crate::resolver_core::resolve_type_declaration(
                    &resolver,
                    dep_canonical,
                    requested_name,
                );
                let mut tracked_deps = std::collections::BTreeSet::new();
                if !declaration.canonical_source.is_empty()
                    && declaration.canonical_source != dep_canonical
                {
                    tracked_deps.insert(declaration.canonical_source.clone());
                }

                crate::resolver_core::symbol_resolver::SymbolNodeResult {
                    value: crate::resolver_core::symbol_resolver::SymbolNodeValue::Declaration(
                        declaration,
                    ),
                    facts: host.current_dependency_fact_versions(dep_canonical, &tracked_deps),
                    diagnostics: Vec::new(),
                }
            });

    match result.value {
        crate::resolver_core::symbol_resolver::SymbolNodeValue::Declaration(declaration) => {
            declaration
        }
        _ => unreachable!("declaration resolution must return a declaration node result"),
    }
}

fn read_full_source(host: &VerterHost, canonical_source: &str) -> Option<String> {
    host.read_analysis_source(canonical_source)
        .map(|source| source.to_string())
}

#[allow(clippy::too_many_arguments)]
fn resolve_jsdoc_block(
    host: &VerterHost,
    canonical_source: &str,
    span: verter_span::Span,
    mode: ProjectionMode,
    tracked_deps: &mut std::collections::BTreeSet<String>,
    cache: &mut crate::resolver_core::ExternalTypeBodyCache,
    visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    kind: verter_workspace::ResolveRequestKind,
) -> Option<ResolvedJsdocBlock> {
    if span.start == 0 && span.end == 0 {
        return None;
    }

    let source = read_full_source(host, canonical_source)?;
    let (description, tags) =
        verter_semantic::analysis::jsdoc::extract_jsdoc_near_offset(&source, span.start);
    if description.is_none() && tags.is_empty() {
        return None;
    }

    Some(ResolvedJsdocBlock {
        description,
        tags: tags
            .into_iter()
            .map(|tag| {
                map_jsdoc_tag(
                    host,
                    canonical_source,
                    mode,
                    tracked_deps,
                    cache,
                    visiting,
                    kind,
                    tag,
                )
            })
            .collect(),
    })
}

#[allow(clippy::too_many_arguments)]
fn map_jsdoc_tag(
    host: &VerterHost,
    canonical_source: &str,
    mode: ProjectionMode,
    tracked_deps: &mut std::collections::BTreeSet<String>,
    _cache: &mut crate::resolver_core::ExternalTypeBodyCache,
    _visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    _kind: verter_workspace::ResolveRequestKind,
    tag: verter_semantic::analysis::types::JsdocTag,
) -> ResolvedJsdocTag {
    let (text, raw_type, subject_name) = parse_jsdoc_tag_payload(tag.name.as_str(), tag.text);
    let resolved_type = if mode == ProjectionMode::Expanded {
        raw_type.as_deref().and_then(|raw_type| {
            resolve_jsdoc_tag_type(host, canonical_source, raw_type, tracked_deps)
        })
    } else {
        None
    };
    ResolvedJsdocTag {
        name: tag.name,
        text,
        raw_type,
        subject_name,
        resolved_type,
    }
}

fn parse_jsdoc_tag_payload(
    tag_name: &str,
    text: Option<String>,
) -> (Option<String>, Option<String>, Option<String>) {
    let Some(text) = text else {
        return (None, None, None);
    };
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix('{') else {
        return (Some(text), None, None);
    };
    // Depth-aware brace matching: find the closing `}` that matches the
    // opening `{`, handling nested braces like `{Record<string, {nested: true}>}`.
    let end = {
        let mut depth = 0u32;
        let mut found = None;
        for (i, ch) in rest.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    if depth == 0 {
                        found = Some(i);
                        break;
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }
        found
    };
    let Some(end) = end else {
        return (Some(text), None, None);
    };

    let raw_type = Some(rest[..end].trim().to_string());
    let trailing = rest[end + 1..].trim();
    if trailing.is_empty() {
        return (None, raw_type, None);
    }

    if matches!(tag_name, "param" | "arg" | "argument") {
        let mut parts = trailing.splitn(2, char::is_whitespace);
        let subject_name = parts.next().map(str::to_string);
        let text = parts
            .next()
            .map(str::trim)
            .filter(|rest| !rest.is_empty())
            .map(str::to_string);
        (text, raw_type, subject_name)
    } else {
        (Some(trailing.to_string()), raw_type, None)
    }
}

fn resolve_jsdoc_tag_type(
    host: &VerterHost,
    canonical_source: &str,
    raw_type: &str,
    tracked_deps: &mut std::collections::BTreeSet<String>,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    let parsed = verter_semantic::analysis::type_expr_lower::parse_type_annotation(raw_type);
    let parsed = if parsed.is_unknown() {
        verter_semantic::analysis::type_expr::TypeExpr::Unknown {
            raw: raw_type.to_string(),
        }
    } else {
        parsed
    };

    // Ensure module facts are materialized so the dispatch path can
    // resolve imports through host-owned caches.
    let _facts = host.ensure_indexed_ready(canonical_source)?;
    tracked_deps.extend(
        host.imported_symbol_dependencies_for_expr(canonical_source, &parsed)
            .into_iter()
            .map(|dependency| dependency.canonical_id),
    );
    // D-Cutover §5.8: route through CMQE (dispatch-backed) instead of
    // the retired SessionSolverHost + TypeQueryEngine. Falls back to
    // the raw parsed annotation when projection misses so the caller
    // still receives the unresolved TypeExpr rather than `None`.
    let mut engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    Some(
        engine
            .project_expr_surface_expr(canonical_source, &parsed)
            .unwrap_or(parsed),
    )
}

#[cfg(test)]
#[path = "meta_resolve_tests.rs"]
mod meta_resolve_tests;
