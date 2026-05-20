#![deny(missing_docs)]
//! `VerterHost::evaluate_type_expression_with_audit` —
//! synthesise a scratch TypeScript file rooted at the requested
//! `scope`, evaluate one trailing
//! `type __VerterScratch = <expression>;` declaration, and return
//! the resolved semantic-graph node id.
//!
//! See [`super::types::EvaluateTypeExpressionRequest`] for the
//! request shape and §5.3 of the typeinfo plan for the scratch URI
//! contract:
//!
//! ```text
//! verter://typeinfo/<sha256(scope_canonical || "\0"
//!                          || expression
//!                          || "\0"
//!                          || serialize(extra_imports))[..16]>.ts
//! ```
//!
//! Cacheable requests publish `(uri, node_id)` to the host-owned
//! [`super::scratch_cache::ScratchCache`] so a repeat request reuses
//! the synthesised file. Non-cacheable requests evict the upserted
//! scratch file at the end of the call.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use verter_audit::{
    ProjectionModeTag, RequestAuditRecord, RequestKind, RequestKindPayload, RequestMemoryAudit,
    RequestStoreAudit, RequestTimingAudit, TypeResolutionPayload, WaitAudit,
};

use super::types::{EvaluateTypeExpressionRequest, ImportSpec, NamedImport};
use crate::host_audit_runtime::AuditRequestRegistration;
use crate::instant::Instant;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::semantic_query::{
    ProjectionMode, QueryResult, ResolveDeclKey, ScopeId, SemanticNodeData, SemanticNodeId,
    SemanticQueryApi, SemanticQueryKey,
};
use crate::types::{FileKind, UpsertRequest};
use crate::VerterHost;

/// Name of the synthetic alias produced by the scratch file. The
/// expression body is wrapped as `type <NAME> = <expression>;` and
/// resolved by name afterwards.
const SCRATCH_ALIAS_NAME: &str = "__VerterScratch";

/// `verter://typeinfo/<hash>.ts` — base URI scheme for synthesised
/// scratch files. Distinct from `verter-virtual` (LSP's scheme) so
/// scratch files never collide with user-visible virtual ids.
const SCRATCH_URI_PREFIX: &str = "verter://typeinfo/";

impl VerterHost {
    /// Evaluate `req.expression` in `req.scope`'s context and return
    /// the resolved semantic-graph node id alongside the audit
    /// record.
    ///
    /// **Scratch URI** (per §5.3): a sha256 of
    /// `scope_canonical || \0 || expression || \0 || serialize(extra_imports)`,
    /// truncated to 16 bytes (32 hex chars), prefixed with
    /// `verter://typeinfo/`, and suffixed `.ts`. Two scopes with the
    /// same expression produce different URIs — `scope_canonical` is
    /// hashed explicitly.
    ///
    /// **Cache discipline** (per §5.3):
    /// - `cacheable: true` — first call synthesises + upserts +
    ///   resolves; subsequent calls with the same URI return the
    ///   cached `node_id` directly (the audit record still emits, with
    ///   `from_cache: true`).
    /// - `cacheable: false` — synthesis is one-shot; the scratch file
    ///   is removed at the end of the call.
    ///
    /// **LRU eviction**: at default capacity 64 the oldest-accessed
    /// entry is evicted on cold insertion of a 65th URI. The evicted
    /// scratch file is also removed from the host so memory does not
    /// grow unbounded.
    ///
    /// Always emits exactly one `RequestAuditRecord` when the
    /// audit-config consumer filter accepts `RequestKind::TypeResolution`.
    pub fn evaluate_type_expression_with_audit(
        &self,
        req: EvaluateTypeExpressionRequest,
    ) -> (Option<SemanticNodeId>, Option<RequestAuditRecord>) {
        let request_id = self.next_request_id();
        crate::request_context::increment_requests_created();

        let footprint_capture = self.config.footprint_capture && self.config.audit_enabled;
        let timing_capture = self.config.audit_timing_capture && self.config.audit_enabled;
        let canonical_scope: Arc<str> = Arc::from(req.scope.as_str());
        let ctx = RequestContext::with_kind_and_timing(
            request_id,
            Arc::clone(&canonical_scope),
            RequestKind::TypeResolution,
            footprint_capture,
            timing_capture,
            None,
        );

        let registration = Arc::new(AuditRequestRegistration::new(self, Arc::clone(&ctx)));
        debug_assert!(ctx.audit_registration.get().is_none());
        let _ = ctx.install_audit_registration(Arc::clone(&registration));

        let request_start = Instant::now();
        let scratch_uri = compute_scratch_uri(&req.scope, &req.expression, &req.extra_imports);

        let (resolved, from_cache) = match registration.as_ref() {
            AuditRequestRegistration::Active(_) => {
                let _ctx_guard = RequestContextGuard::install(Arc::clone(&ctx));
                evaluate_inner(self, &req, &scratch_uri)
            }
            AuditRequestRegistration::Noop => {
                let _noop_guard = verter_audit::install_noop_observer();
                evaluate_inner(self, &req, &scratch_uri)
            }
        };
        let total_ms = request_start.elapsed().as_secs_f64() * 1000.0;

        if matches!(registration.as_ref(), AuditRequestRegistration::Noop) {
            return (resolved, None);
        }

        let payload = TypeResolutionPayload {
            query_mode: ProjectionModeTag::from(req.mode),
            hops: u32::try_from(ctx.type_resolution_hops.load(Ordering::Relaxed))
                .unwrap_or(u32::MAX),
            navigations: u32::try_from(ctx.type_resolution_navigations.load(Ordering::Relaxed))
                .unwrap_or(u32::MAX),
            expansions: u32::try_from(ctx.type_resolution_expansions.load(Ordering::Relaxed))
                .unwrap_or(u32::MAX),
            conditional_decisions: u32::try_from(
                ctx.type_resolution_conditional_decisions
                    .load(Ordering::Relaxed),
            )
            .unwrap_or(u32::MAX),
            ref_root_cycle_hits: u32::try_from(
                ctx.type_resolution_ref_root_cycle_hits
                    .load(Ordering::Relaxed),
            )
            .unwrap_or(u32::MAX),
            projection_ops_executed: u32::try_from(
                ctx.type_resolution_projection_ops.load(Ordering::Relaxed),
            )
            .unwrap_or(u32::MAX),
            depth_high_water: ctx.type_resolution_depth_high_water.load(Ordering::Relaxed),
            recursion_limit_reached: ctx
                .type_resolution_recursion_limit_reached
                .load(Ordering::Relaxed),
            walker_diagnostics: Vec::new(),
            cache_suppress: false,
        };
        let timings = RequestTimingAudit {
            total_ms,
            ..RequestTimingAudit::default()
        };
        let store = RequestStoreAudit {
            cache_layers: crate::component_meta_audit::snapshot_cache_layers_from_tls(),
            bypass_diagnostics: crate::component_meta_audit::snapshot_bypass_diagnostics_from_tls(),
            ..RequestStoreAudit::default()
        };
        let memory = RequestMemoryAudit {
            process_rss_peak_bytes: ctx.process_rss_peak_bytes.load(Ordering::Relaxed),
            ..RequestMemoryAudit::default()
        };
        let waits = if ctx.timing_capture {
            Some(WaitAudit {
                lock_wait_ns: ctx.lock_wait_ns.load(Ordering::Relaxed),
                queue_wait_ns: ctx.queue_wait_ns.load(Ordering::Relaxed),
                lock_acquisitions: ctx.lock_acquisitions.load(Ordering::Relaxed),
            })
        } else {
            None
        };

        let record = RequestAuditRecord {
            request_id,
            canonical_id: req.scope.clone(),
            kind: RequestKind::TypeResolution,
            parent_request_id: ctx.parent_request_id.map(|id| id.to_string()),
            from_cache,
            timings,
            memory,
            store,
            footprint: None,
            scheduler: ctx.scheduler_audit.lock().clone(),
            files: Vec::new(),
            waits,
            kind_payload: RequestKindPayload::TypeResolution(payload),
            trace_id: ctx.trace_id.clone(),
        };
        let cloned = record.clone();
        registration.finalize(record);
        (resolved, Some(cloned))
    }
}

/// Inner synthesis + resolution logic shared by the audit /
/// non-audit entry-points.
///
/// Returns `(resolved_node, from_cache)`. `from_cache = true` means
/// the scratch URI was found in the host's cache and resolution did
/// not re-synthesise / re-upsert.
fn evaluate_inner(
    host: &VerterHost,
    req: &EvaluateTypeExpressionRequest,
    scratch_uri: &str,
) -> (Option<SemanticNodeId>, bool) {
    // Cache fast-path. Only consulted when the caller asked for
    // caching — otherwise the cache is bypassed in both directions.
    if req.cacheable {
        let mut guard = host.scratch_cache().lock();
        if let Some(node_id) = guard.get(scratch_uri) {
            return (Some(node_id), true);
        }
    }

    // Synthesise the scratch source. Each import in `extra_imports`
    // produces one `import` declaration; the expression body is
    // wrapped in a single trailing
    // `type __VerterScratch = <expression>;`. When the scope's
    // eval-source is available it is inlined as a prelude so the
    // scratch's lookup environment carries every top-level binding
    // the scope publishes — including the SFC-synthesised `default`
    // for `.vue` scopes (see `vue_default_synth`).
    let scope_eval_source = host
        .ensure_indexed_ready(&req.scope)
        .map(|indexed| Arc::clone(&indexed.eval_source));
    let source = synthesise_source(
        &req.expression,
        &req.extra_imports,
        scope_eval_source.as_deref(),
    );

    // Upsert the scratch file. The canonical id is the URI; aliases
    // remain empty — typeinfo URIs do not appear in user-visible
    // alias maps. `FileKind::from_path` on a `.ts` URI returns
    // `NonSfc`.
    let upsert_result = host.upsert(UpsertRequest {
        canonical_id: Some(scratch_uri.to_string()),
        input_id: scratch_uri.to_string(),
        source: Arc::from(source.as_str()),
        file_kind: FileKind::from_path(scratch_uri),
        aliases: Vec::new(),
    });
    if upsert_result.is_err() {
        return (None, false);
    }

    // Resolve the synthesised alias by dispatching through
    // `Instantiate { args: [], body_mode }`. The scratch alias has
    // no declaration-site type parameters so `args = []` is
    // correct; the dispatch lifts the `DeclPlaceholder` into a
    // concrete body in the requested mode.
    let dispatch = ProjectSemanticDispatch::new(host);
    let scratch_canonical: Arc<str> = Arc::from(scratch_uri);
    let Some(shallow) = host.shallow_file_state(scratch_uri) else {
        cleanup_scratch(host, scratch_uri, req.cacheable);
        return (None, false);
    };
    let scope_node = crate::semantic_query::NodeScopeId::File {
        canonical_id: Arc::clone(&scratch_canonical),
        whole_hash: shallow.whole_hash,
        local_scope: None,
    };
    let identity =
        crate::semantic_query::DeclIdentity::from_scope(&scope_node, Arc::from(SCRATCH_ALIAS_NAME));
    let instantiate_key = SemanticQueryKey::Instantiate {
        base: identity,
        args: Arc::from(Vec::new().into_boxed_slice()),
        body_mode: req.mode,
    };
    let resolved_alias_node = match dispatch.execute(instantiate_key) {
        QueryResult::Value(node) | QueryResult::Recursive(node) => node,
        QueryResult::Error(_) => {
            // Fall back to the bare-decl path so the caller sees a
            // node id even when the body could not materialise (the
            // audit record still emits with the chosen mode).
            let resolve_decl_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                scope: ScopeId {
                    canonical_id: Arc::clone(&scratch_canonical),
                    local_scope: None,
                },
                name: Arc::from(SCRATCH_ALIAS_NAME),
            });
            match dispatch.execute(resolve_decl_key) {
                QueryResult::Value(n) | QueryResult::Recursive(n) => n,
                QueryResult::Error(_) => {
                    cleanup_scratch(host, scratch_uri, req.cacheable);
                    return (None, false);
                }
            }
        }
    };

    // Apply the requested terminal mode. `Identity` returns the
    // alias node verbatim. Everything else walks the alias /
    // placeholder chain to land on a concrete body — the same
    // policy as `resolve_named_symbol`.
    let final_node = match req.mode {
        ProjectionMode::Identity => resolved_alias_node,
        _ => materialize_through_aliases(host, &dispatch, resolved_alias_node, req.mode),
    };

    // Publish to cache if asked. The cached node id is the one we
    // return so a later cache hit and a fresh request produce the
    // same value.
    if req.cacheable {
        let mut guard = host.scratch_cache().lock();
        let evicted = guard.insert(scratch_uri.to_string(), final_node);
        drop(guard);
        if let Some(evicted_uri) = evicted {
            // Drop the evicted scratch file from the host so memory
            // does not grow unbounded. Best-effort — failures are
            // ignored.
            host.evict(&evicted_uri);
        }
    } else {
        cleanup_scratch(host, scratch_uri, false);
    }

    (Some(final_node), false)
}

/// Drop the synthesised scratch file. Called for non-cacheable
/// requests at the end of the call and for evicted entries.
fn cleanup_scratch(host: &VerterHost, uri: &str, cacheable: bool) {
    if !cacheable {
        host.evict(uri);
    }
}

/// Walk the alias / placeholder chain on a resolved node until we
/// land on a concrete body, matching the
/// [`super::resolve_named_symbol::resolve_named_symbol_with_audit`]
/// policy. Bounded by a small step budget so a pathological cycle
/// can't hang the resolver.
fn materialize_through_aliases(
    host: &VerterHost,
    dispatch: &ProjectSemanticDispatch<'_>,
    start: SemanticNodeId,
    mode: ProjectionMode,
) -> SemanticNodeId {
    debug_assert!(!matches!(mode, ProjectionMode::Identity));
    let store = host.project_type_store().semantic_graph();
    let mut current = start;
    for _ in 0..16 {
        let data = store.node_data(current);
        match data.as_deref() {
            Some(SemanticNodeData::Alias(inner)) => {
                current = *inner;
                continue;
            }
            Some(SemanticNodeData::Opaque(
                crate::semantic_query::QueryError::DeclPlaceholder {
                    canonical_id,
                    name,
                    whole_hash,
                },
            )) => {
                let identity = crate::semantic_query::DeclIdentity {
                    canonical_id: Arc::clone(canonical_id),
                    whole_hash: *whole_hash,
                    decl_name: Arc::clone(name),
                };
                let key = SemanticQueryKey::Instantiate {
                    base: identity,
                    args: Arc::from(Vec::new().into_boxed_slice()),
                    body_mode: mode,
                };
                match dispatch.execute(key) {
                    QueryResult::Value(next) | QueryResult::Recursive(next) => {
                        if next == current {
                            return current;
                        }
                        current = next;
                        continue;
                    }
                    QueryResult::Error(_) => return current,
                }
            }
            _ => return current,
        }
    }
    current
}

/// Compute the scratch URI per §5.3.
///
/// Hash inputs: `scope_canonical || \0 || expression || \0 ||
/// serialize(extra_imports)`. The serialised form for imports is a
/// stable text encoding so two structurally identical
/// `Vec<ImportSpec>`s always hash the same — ordering of bindings
/// and imports is preserved verbatim.
pub(crate) fn compute_scratch_uri(
    scope: &str,
    expression: &str,
    extra_imports: &[ImportSpec],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(scope.as_bytes());
    hasher.update(b"\0");
    hasher.update(expression.as_bytes());
    hasher.update(b"\0");
    for imp in extra_imports {
        hasher.update(b"i:");
        hasher.update(imp.specifier.as_bytes());
        hasher.update(b"\0");
        for binding in &imp.bindings {
            match binding {
                NamedImport::Default { local_name } => {
                    hasher.update(b"d:");
                    hasher.update(local_name.as_bytes());
                }
                NamedImport::Named {
                    exported_name,
                    local_alias,
                    type_only,
                } => {
                    hasher.update(b"n:");
                    if *type_only {
                        hasher.update(b"t:");
                    }
                    hasher.update(exported_name.as_bytes());
                    if let Some(alias) = local_alias {
                        hasher.update(b"=");
                        hasher.update(alias.as_bytes());
                    }
                }
                NamedImport::Namespace { local_name } => {
                    hasher.update(b"s:");
                    hasher.update(local_name.as_bytes());
                }
            }
            hasher.update(b"\n");
        }
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    let mut hex_buf = String::with_capacity(32);
    for byte in &digest[..16] {
        use std::fmt::Write;
        let _ = write!(&mut hex_buf, "{byte:02x}");
    }
    format!("{SCRATCH_URI_PREFIX}{hex_buf}.ts")
}

/// Synthesise the scratch TS source. Layout:
///
/// ```text
/// // optional scope eval-source prelude
/// import <imports>...;
/// type __VerterScratch = <expression>;
/// ```
///
/// `scope_eval_source`, when provided, is the textual eval-source
/// for `req.scope` — the same TS body the host parses to build that
/// scope's shallow inventory. Inlining it here makes the scratch's
/// name resolution truly "rooted at the scope" per the
/// `EvaluateTypeExpressionRequest::scope` contract: every top-level
/// declaration that exists in the scope (types, value bindings,
/// imports, the SFC-synthesised `default`) is visible to the
/// trailing `__VerterScratch` alias without forcing the caller to
/// enumerate them through `extra_imports`. Without this prelude,
/// expressions like `InstanceType<typeof default>['$props']`
/// evaluated against a `.vue` scope would have no `default` in
/// their lookup environment and never reduce.
///
/// Comments / blank lines are emitted at file head as needed so
/// downstream parsers see a clean source.
fn synthesise_source(
    expression: &str,
    extra_imports: &[ImportSpec],
    scope_eval_source: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str("// Auto-generated by VerterHost::evaluate_type_expression\n");
    if let Some(prelude) = scope_eval_source {
        if !prelude.is_empty() {
            out.push_str("// --- begin scope eval-source prelude ---\n");
            out.push_str(prelude);
            if !prelude.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("// --- end scope eval-source prelude ---\n\n");
        }
    }
    for imp in extra_imports {
        push_import(&mut out, imp);
    }
    if !extra_imports.is_empty() {
        out.push('\n');
    }
    out.push_str("type ");
    out.push_str(SCRATCH_ALIAS_NAME);
    out.push_str(" = ");
    out.push_str(expression);
    out.push_str(";\n");
    out
}

/// Render one [`ImportSpec`] as a TypeScript import declaration.
///
/// Splits `Default` / `Namespace` from `Named` because TS allows at
/// most one default + at most one namespace + a single named-binding
/// list per import. Namespace imports are emitted on their own line
/// when mixed with named bindings; default imports merge with named
/// bindings on the same import declaration.
fn push_import(out: &mut String, imp: &ImportSpec) {
    let mut default_name: Option<&str> = None;
    let mut namespace_name: Option<&str> = None;
    let mut named: Vec<&NamedImport> = Vec::new();
    for b in &imp.bindings {
        match b {
            NamedImport::Default { local_name } => default_name = Some(local_name.as_str()),
            NamedImport::Namespace { local_name } => namespace_name = Some(local_name.as_str()),
            NamedImport::Named { .. } => named.push(b),
        }
    }
    // Namespace imports cannot mix with named/default — emit them
    // on a separate line.
    if let Some(ns) = namespace_name {
        out.push_str("import * as ");
        out.push_str(ns);
        out.push_str(" from \"");
        out.push_str(&imp.specifier);
        out.push_str("\";\n");
    }
    if default_name.is_some() || !named.is_empty() {
        out.push_str("import ");
        if let Some(default) = default_name {
            out.push_str(default);
            if !named.is_empty() {
                out.push_str(", ");
            }
        }
        if !named.is_empty() {
            out.push_str("{ ");
            for (idx, b) in named.iter().enumerate() {
                if idx > 0 {
                    out.push_str(", ");
                }
                if let NamedImport::Named {
                    exported_name,
                    local_alias,
                    type_only,
                } = b
                {
                    if *type_only {
                        out.push_str("type ");
                    }
                    out.push_str(exported_name);
                    if let Some(alias) = local_alias {
                        out.push_str(" as ");
                        out.push_str(alias);
                    }
                }
            }
            out.push_str(" }");
        }
        out.push_str(" from \"");
        out.push_str(&imp.specifier);
        out.push_str("\";\n");
    }
}
