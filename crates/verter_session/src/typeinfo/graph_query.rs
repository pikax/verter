#![deny(missing_docs)]
//! The resolve-symbol typeinfo graph operation executor.
//!
//! [`VerterHost::resolve_symbol_graph_with_audit`] is the graph-protocol
//! resolve entry: the operation-DTO envelope in, the typed
//! [`TypeInfoResult`] out — the answer route graph-protocol consumers
//! bind instead of the internal `TypeExpr` JSON transit.
//!
//! Executor flow:
//! 1. **Validation first** — the full envelope validator
//!    ([`validate_type_info_graph_request`]) runs before any semantic
//!    work: a malformed envelope (op/payload mismatch, schema-echo
//!    divergence, missing closure / display policy, out-of-range
//!    expansion budgets, an unsupported `includeDegraded` flag) answers
//!    with the typed wire `error` arm and never reaches resolution.
//!    Unbounded export is therefore structurally rejected: an expanded
//!    closure without explicit in-range budgets cannot pass validation.
//! 2. **Operation gate** — this entry serves the resolve-symbol
//!    operation ONLY. A validated envelope for any other operation is
//!    refused with a typed `MalformedPayload` (fail-closed: no silent
//!    re-route to the string-expression evaluator, no dual-running
//!    authority).
//! 3. **Closure gate** — the resolve-symbol bounded walk serves the
//!    root-only / one-level / expanded closure kinds. A path policy or
//!    a projection-required policy names behavior this operation does
//!    not offer (member-path following / a required projection); it is
//!    refused with a typed `MalformedPayload` rather than answered with
//!    an unrequested shape.
//! 4. **Resolution** — the ONE shared engine
//!    ([`VerterHost::resolve_named_symbol_with_audit`]); this executor
//!    is not a second resolver.
//! 5. **Bounded export** — the resolved node raises through the sealed
//!    output capability
//!    ([`VerterHost::project_node_to_semantic_type_graph`]) and the
//!    terminal `TypeExpr` encodes into the wire
//!    [`SemanticTypeGraph`] under the request's closure budgets (the
//!    inherently bounded closure kinds bound the exported walk's depth
//!    itself, not just its arena). A non-fault miss is a typed
//!    `graph`-arm answer (an opaque `Miss` root); a dispatch fault is a
//!    typed `graph`-arm answer carrying the fault as an opaque error
//!    root — neither is an empty graph, neither loses the audit
//!    envelope.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use verter_audit::payloads::typeinfo_graph::{
    GraphClosurePolicyTag, GraphOperationTag, ReductionDemandTag, TypeInfoGraphPayload,
};
use verter_audit::{
    AuditedResult, ProjectionModeTag, RequestAuditRecord, RequestKind, RequestKindPayload,
    RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit, WaitAudit,
};
use verter_protocol::typeinfo::graph::{
    self as wire, Operation, TypeInfoGraphRequest, TypeInfoGraphResponse, TypeInfoRequestError,
    TYPEINFO_GRAPH_SCHEMA_VERSION,
};
use verter_protocol::typeinfo::graph_export::GraphExportBudgets;
use verter_protocol::verter::v1::{
    graph_closure_policy, graph_query_error, graph_type_node, type_info_graph_request,
    type_info_graph_response, type_info_request_error, GraphOpaque, GraphQueryError,
    GraphQueryErrorMiss, GraphStringTable, GraphTypeNode, SemanticTypeGraph,
};

use crate::host_audit_runtime::AuditRequestRegistration;
use crate::host_resolve_type_audit::TypeResolutionRequestError;
use crate::instant::Instant;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::semantic_query::ProjectionMode;
use crate::VerterHost;

impl VerterHost {
    /// Resolve a named declaration through the typeinfo graph operation
    /// envelope, answering with the bounded wire [`TypeInfoGraphResponse`]
    /// (the `graph` arm on a resolved / missed / faulted answer, the
    /// `error` arm on a typed rejection) plus the request's audit record.
    ///
    /// Validation-first: [`validate_type_info_graph_request`] runs before
    /// any semantic work, so a malformed envelope is rejected with the
    /// typed wire `error` arm. See the module docs for the full contract.
    #[must_use]
    pub fn resolve_symbol_graph_with_audit(
        &self,
        envelope: TypeInfoGraphRequest,
    ) -> AuditedResult<TypeInfoGraphResponse, TypeInfoRequestError> {
        let request_id = self.next_request_id();
        crate::request_context::increment_requests_created();

        let footprint_capture = self.config.footprint_capture && self.config.audit_enabled;
        let timing_capture = self.config.audit_timing_capture && self.config.audit_enabled;
        // Best-effort audit target: the resolve payload's canonical.
        let canonical_for_audit = resolve_target_canonical(&envelope);
        let target_identity = if canonical_for_audit.is_empty() {
            verter_audit::RequestTargetIdentity::NotApplicable
        } else {
            verter_audit::RequestTargetIdentity::registered(canonical_for_audit.clone())
        };
        let ctx = RequestContext::with_kind_timing_and_projection_budget(
            request_id,
            Arc::<str>::from(canonical_for_audit.as_str()),
            RequestKind::TypeInfoGraph,
            footprint_capture,
            timing_capture,
            None,
            self.config.projection_op_budget,
        );
        let registration = Arc::new(AuditRequestRegistration::new(self, Arc::clone(&ctx)));
        let _ = ctx.install_audit_registration(Arc::clone(&registration));

        let request_start = Instant::now();
        let (response, payload) = match registration.as_ref() {
            AuditRequestRegistration::Active(_) => {
                let _ctx_guard = RequestContextGuard::install(Arc::clone(&ctx));
                self.execute_resolve_symbol_graph(envelope)
            }
            AuditRequestRegistration::Noop => {
                let _noop_guard = verter_audit::install_noop_observer();
                self.execute_resolve_symbol_graph(envelope)
            }
        };
        let total_ms = request_start.elapsed().as_secs_f64() * 1000.0;

        // VALIDATION-FIRST CONTRACT: the `error` arm is exactly the typed
        // rejection; both arms are audited identically.
        let outcome = response_outcome(&response);

        if matches!(registration.as_ref(), AuditRequestRegistration::Noop) {
            let state = if self.config.audit_enabled {
                verter_audit::AuditCaptureState::FilteredNoop
            } else {
                verter_audit::AuditCaptureState::AuditDisabled
            };
            let record = RequestAuditRecord {
                request_id,
                canonical_id: canonical_for_audit.to_string(),
                target_identity: Some(target_identity),
                kind: RequestKind::TypeInfoGraph,
                parent_request_id: ctx.parent_request_id.map(|id| id.to_string()),
                from_cache: false,
                timings: RequestTimingAudit::default(),
                memory: RequestMemoryAudit::default(),
                store: RequestStoreAudit::default(),
                footprint: None,
                scheduler: None,
                files: Vec::new(),
                waits: None,
                kind_payload: RequestKindPayload::TypeInfoGraph(payload),
                capture_state: state,
                trace_id: ctx.trace_id.clone(),
            };
            return match outcome {
                Ok(()) => AuditedResult::ok(response, record),
                Err(error) => AuditedResult::err(error, record),
            };
        }

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
            target_identity: Some(target_identity),
            canonical_id: canonical_for_audit,
            kind: RequestKind::TypeInfoGraph,
            parent_request_id: ctx.parent_request_id.map(|id| id.to_string()),
            from_cache: false,
            timings,
            memory,
            store,
            footprint: None,
            scheduler: ctx.scheduler_audit.lock().clone(),
            files: Vec::new(),
            waits,
            kind_payload: RequestKindPayload::TypeInfoGraph(payload),
            capture_state: verter_audit::AuditCaptureState::ActiveStored,
            trace_id: ctx.trace_id.clone(),
        };

        let cloned = record.clone();
        registration.finalize(record);
        match outcome {
            Ok(()) => AuditedResult::ok(response, cloned),
            Err(error) => AuditedResult::err(error, cloned),
        }
    }

    /// The validation-first executor body. NEVER reaches semantic work
    /// before the envelope validator returns `Ok`.
    fn execute_resolve_symbol_graph(
        &self,
        envelope: TypeInfoGraphRequest,
    ) -> (TypeInfoGraphResponse, TypeInfoGraphPayload) {
        let validated = match crate::typeinfo::request_validation::validate_type_info_graph_request(
            &envelope,
        ) {
            Ok(v) => v,
            Err(error) => {
                let payload =
                    TypeInfoGraphPayload::from_validation_error(GraphOperationTag::ResolveSymbol);
                return (error_response(error), payload);
            }
        };
        let request = validated.into_inner();

        // OPERATION GATE — this entry serves resolve-symbol only. The
        // validator proved the operation/payload pair is coherent; a
        // coherent envelope for a DIFFERENT operation is refused here,
        // fail-closed, before any resolution.
        if request.operation != Operation::ResolveSymbol as i32 {
            let payload =
                TypeInfoGraphPayload::from_validation_error(GraphOperationTag::ResolveSymbol);
            return (
                error_response(malformed(
                    "this entry serves the resolve-symbol operation only",
                )),
                payload,
            );
        }
        let Some(type_info_graph_request::Payload::ResolveSymbol(resolve)) = request.payload else {
            // Unreachable after validation; refused rather than assumed.
            let payload =
                TypeInfoGraphPayload::from_validation_error(GraphOperationTag::ResolveSymbol);
            return (
                error_response(malformed(
                    "resolve-symbol payload arm missing after validation",
                )),
                payload,
            );
        };

        let mode = resolve
            .context
            .as_ref()
            .map(|context| wire_projection_mode(context.mode))
            .unwrap_or(ProjectionMode::Identity);
        // CLOSURE GATE — the bounded walk below serves the three closure
        // kinds resolve-symbol defines. A path policy (member-path
        // following) or a projection-required policy (a required
        // projection) cannot shape this answer; serving it anyway would
        // publish an unrequested shape, so it is refused fail-closed.
        if !matches!(
            resolve.closure.as_ref().and_then(|c| c.kind.as_ref()),
            Some(graph_closure_policy::Kind::RootOnly(_))
                | Some(graph_closure_policy::Kind::OneLevel(_))
                | Some(graph_closure_policy::Kind::Expanded(_))
        ) {
            let payload =
                TypeInfoGraphPayload::from_validation_error(GraphOperationTag::ResolveSymbol);
            return (
                error_response(malformed(
                    "the resolve-symbol operation serves root-only, one-level, and expanded \
                     closure policies only",
                )),
                payload,
            );
        }
        let budgets = closure_budgets(resolve.closure.as_ref());
        let query = query_identity(&resolve);

        // The ONE shared engine — no second resolver, no string
        // evaluator on this route.
        let (outcome, _inner_record) = self
            .resolve_named_symbol_with_audit(&resolve.canonical_id, &resolve.name, Some(mode))
            .into_parts();
        let graph = match outcome {
            Ok(Some(node)) => match self.project_node_to_semantic_type_graph(node, &budgets) {
                Some(mut graph) => {
                    graph.query = Some(query);
                    graph
                }
                None => single_opaque_root_graph(miss_error(), None, Some(query)),
            },
            // A non-fault miss is a typed graph answer: the opaque Miss
            // root, never an empty graph and never a fabricated shape.
            Ok(None) => single_opaque_root_graph(miss_error(), None, Some(query)),
            // A dispatch fault is a typed graph answer carrying the
            // fault — the envelope was valid; the resolution faulted.
            Err(fault) => {
                let (error, detail) = fault_error(&fault);
                single_opaque_root_graph(error, detail, Some(query))
            }
        };
        let payload = graph_payload(mode, &resolve, &graph);
        (graph_response(graph), payload)
    }
}

/// The payload's canonical, best-effort for the audit target.
fn resolve_target_canonical(envelope: &TypeInfoGraphRequest) -> String {
    match &envelope.payload {
        Some(type_info_graph_request::Payload::ResolveSymbol(r)) => r.canonical_id.clone(),
        _ => String::new(),
    }
}

/// Decode the wire projection-mode tag onto the host enum (the validator
/// proved presence + range for validated requests; the fallback only
/// covers a validator change that introduces a new mode without a host
/// mapping, and stays fail-closed to `Identity` — the narrowest mode, so
/// an unmapped tag can never silently buy the broadest projection).
fn wire_projection_mode(tag: i32) -> ProjectionMode {
    match tag {
        x if x == wire::ProjectionMode::Identity as i32 => ProjectionMode::Identity,
        x if x == wire::ProjectionMode::Navigate as i32 => ProjectionMode::Navigate,
        x if x == wire::ProjectionMode::Shallow as i32 => ProjectionMode::Shallow,
        x if x == wire::ProjectionMode::Skeleton as i32 => ProjectionMode::Skeleton,
        x if x == wire::ProjectionMode::Expanded as i32 => ProjectionMode::Expanded,
        _ => ProjectionMode::Identity,
    }
}

/// Derive the bounded-export budgets from the (validated, gated) closure
/// policy. An expanded closure carries its own in-range budgets — a zero
/// axis means "no limit on that axis" and maps to the validator cap. The
/// inherently bounded closure kinds bound the exported WALK itself:
/// root-only is the root node (every child degrades to the explicit
/// depth marker) and one level adds exactly one child level; the node
/// axis stays at the validator cap because the depth axis is what these
/// closures tighten.
fn closure_budgets(closure: Option<&wire::ClosurePolicy>) -> GraphExportBudgets {
    use crate::typeinfo::request_validation::{
        MAX_EXPANSION_DEPTH_BUDGET, MAX_EXPANSION_NODE_BUDGET,
    };
    match closure.and_then(|policy| policy.kind.as_ref()) {
        Some(graph_closure_policy::Kind::Expanded(expanded)) => GraphExportBudgets {
            node_budget: if expanded.node_budget == 0 {
                MAX_EXPANSION_NODE_BUDGET
            } else {
                expanded.node_budget
            },
            depth_budget: if expanded.depth_budget == 0 {
                MAX_EXPANSION_DEPTH_BUDGET
            } else {
                expanded.depth_budget
            },
        },
        Some(graph_closure_policy::Kind::RootOnly(_)) => GraphExportBudgets {
            node_budget: MAX_EXPANSION_NODE_BUDGET,
            depth_budget: 1,
        },
        // Post-gate this is one-level; the defensive no-closure fallback
        // takes the same narrow shape (a missing policy never buys the
        // deepest walk).
        _ => GraphExportBudgets {
            node_budget: MAX_EXPANSION_NODE_BUDGET,
            depth_budget: 2,
        },
    }
}

/// Build the wire query identity echoed onto the exported graph.
fn query_identity(resolve: &wire::ResolveSymbolGraphRequest) -> wire::QueryIdentity {
    wire::QueryIdentity {
        operation: Operation::ResolveSymbol as i32,
        path: Vec::new(),
        closure: resolve.closure.clone(),
        context: resolve.context,
        display_policy: resolve.display_policy,
        substitutions: Vec::new(),
        solver_options_hash: Vec::new(),
        parse_env_hash: Vec::new(),
        resolve_env_hash: Vec::new(),
        type_env_hash: Vec::new(),
        lib_env_hash: Vec::new(),
        project_identity: Vec::new(),
        resolver_version: 0,
        include_provenance: resolve.include_provenance,
        include_diagnostics: resolve.include_diagnostics,
        include_projection: resolve.include_projection.iter().map(|p| p.kind).collect(),
        resolved_roots: Vec::new(),
    }
}

fn miss_error() -> GraphQueryError {
    GraphQueryError {
        kind: Some(graph_query_error::Kind::Miss(GraphQueryErrorMiss {})),
    }
}

/// Map a resolution dispatch fault onto the wire query-error taxonomy —
/// the fault rides the graph as a typed opaque root, never as bare text.
///
/// Returns the typed error plus, for text-bearing faults, the detail the
/// answer must intern so it stays self-describing: the catch-all arms
/// (`UnsupportedIntrinsic` / `Cancelled` / `Other`) degrade to the opaque
/// `Other` arm CARRYING their own text, and a budget domain with no wire
/// variant (the projection-operation runaway fuse) does the same — a
/// tripped fuse is never re-labeled as an unrelated closed wire domain.
fn fault_error(fault: &TypeResolutionRequestError) -> (GraphQueryError, Option<String>) {
    match fault {
        TypeResolutionRequestError::BudgetExceeded(failure) => {
            match budget_domain(failure.domain) {
                Some(domain) => (
                    GraphQueryError {
                        kind: Some(graph_query_error::Kind::BudgetExceeded(
                            verter_protocol::verter::v1::GraphQueryErrorBudgetExceeded {
                                domain: domain as i32,
                                limit: u32::try_from(failure.limit).unwrap_or(u32::MAX),
                                actual: failure.actual,
                                context_name_id: 0,
                            },
                        )),
                    },
                    None,
                ),
                None => textual_fault(format!(
                    "budget exceeded: {} (limit {}, actual {})",
                    budget_domain_label(failure.domain),
                    failure.limit,
                    failure.actual
                )),
            }
        }
        TypeResolutionRequestError::UnstableState { attempts } => (
            GraphQueryError {
                kind: Some(graph_query_error::Kind::UnstableState(
                    verter_protocol::verter::v1::GraphQueryErrorUnstableState {
                        attempts: u32::from(*attempts),
                    },
                )),
            },
            None,
        ),
        TypeResolutionRequestError::AliasCycle { .. } => (
            GraphQueryError {
                kind: Some(graph_query_error::Kind::AliasCycle(
                    verter_protocol::verter::v1::GraphQueryErrorAliasCycle {},
                )),
            },
            None,
        ),
        TypeResolutionRequestError::UnsupportedIntrinsic { name } => {
            textual_fault(format!("unsupported intrinsic: {name}"))
        }
        TypeResolutionRequestError::Cancelled => {
            textual_fault("resolution cancelled before completion".to_string())
        }
        TypeResolutionRequestError::Other(text) => textual_fault(text.to_string()),
    }
}

/// The opaque `Other` arm with `message_name_id` left for the string
/// table to fill (the detail text interns beside the graph).
fn textual_fault(message: String) -> (GraphQueryError, Option<String>) {
    (
        GraphQueryError {
            kind: Some(graph_query_error::Kind::Other(
                verter_protocol::verter::v1::GraphQueryErrorOther { message_name_id: 0 },
            )),
        },
        Some(message),
    )
}

/// The wire counterpart of a session budget domain, or `None` when the
/// wire enum has no variant for it. `None` is not a fallback: aliasing
/// an unmapped domain onto a closed wire variant (the projection-
/// operation fuse onto solver resolve steps) would misreport WHAT
/// tripped to every graph consumer.
fn budget_domain(
    domain: crate::resolver_core::shallow_file_state::BudgetDomain,
) -> Option<wire::BudgetDomain> {
    use crate::resolver_core::shallow_file_state::BudgetDomain as Session;
    match domain {
        Session::LocalClosure => Some(wire::BudgetDomain::LocalClosure),
        Session::Frontier => Some(wire::BudgetDomain::Frontier),
        Session::BuilderExpansion => Some(wire::BudgetDomain::BuilderExpansion),
        Session::SolverResolveSteps => Some(wire::BudgetDomain::SolverResolveSteps),
        Session::SolverArenaNodes => Some(wire::BudgetDomain::SolverArenaNodes),
        Session::SolverInstantiationDepth => Some(wire::BudgetDomain::SolverInstantiationDepth),
        Session::ProjectionOperation => None,
    }
}

/// The stable display label of a session budget domain (used only for
/// domains with no wire variant, so the textual arm still names WHAT
/// tripped). Exhaustive by construction: a new domain fails to compile
/// until it is labeled or mapped.
fn budget_domain_label(
    domain: crate::resolver_core::shallow_file_state::BudgetDomain,
) -> &'static str {
    use crate::resolver_core::shallow_file_state::BudgetDomain as Session;
    match domain {
        Session::LocalClosure => "local closure",
        Session::Frontier => "frontier",
        Session::BuilderExpansion => "builder expansion",
        Session::SolverResolveSteps => "solver resolve steps",
        Session::SolverArenaNodes => "solver arena nodes",
        Session::SolverInstantiationDepth => "solver instantiation depth",
        Session::ProjectionOperation => "projection operation",
    }
}

/// A single-opaque-root graph — the typed miss / fault answer. Follows
/// the bounded export's reserved-slot conventions: node id 0 and string
/// id 0 are the wire absent-sentinels, so the root sits at node id 1 and
/// a text-bearing `Other` root interns its detail at string id 1 (the
/// fault's own message, never a generic filler).
fn single_opaque_root_graph(
    error: GraphQueryError,
    detail: Option<String>,
    query: Option<wire::QueryIdentity>,
) -> SemanticTypeGraph {
    let mut strings = vec![String::new()];
    let mut error = error;
    if let (Some(message), Some(graph_query_error::Kind::Other(other))) =
        (detail, error.kind.as_mut())
    {
        other.message_name_id = 1;
        strings.push(message);
    }
    SemanticTypeGraph {
        schema_version: TYPEINFO_GRAPH_SCHEMA_VERSION,
        query,
        nodes: vec![
            GraphTypeNode { kind: None },
            GraphTypeNode {
                kind: Some(graph_type_node::Kind::Opaque(GraphOpaque {
                    error: Some(error),
                })),
            },
        ],
        symbols: Vec::new(),
        signatures: Vec::new(),
        edges: Vec::new(),
        root_ids: vec![1],
        exactness: Vec::new(),
        diagnostics: Vec::new(),
        node_id_map: Vec::new(),
        symbol_id_map: Vec::new(),
        strings: Some(GraphStringTable { entries: strings }),
        relation_proofs: Vec::new(),
    }
}

fn graph_response(graph: SemanticTypeGraph) -> TypeInfoGraphResponse {
    TypeInfoGraphResponse {
        kind: Some(type_info_graph_response::Kind::Graph(graph)),
    }
}

fn error_response(error: TypeInfoRequestError) -> TypeInfoGraphResponse {
    TypeInfoGraphResponse {
        kind: Some(type_info_graph_response::Kind::Error(error)),
    }
}

fn malformed(detail: &str) -> TypeInfoRequestError {
    TypeInfoRequestError {
        kind: Some(type_info_request_error::Kind::MalformedPayload(
            verter_protocol::typeinfo::graph::wire_error_malformed_payload(detail),
        )),
    }
}

/// Classify the response into the audited `Ok`/`Err` outcome — the
/// `error` arm is the typed wire rejection.
fn response_outcome(response: &TypeInfoGraphResponse) -> Result<(), TypeInfoRequestError> {
    match &response.kind {
        Some(type_info_graph_response::Kind::Error(error)) => Err(error.clone()),
        _ => Ok(()),
    }
}

/// Build the audit payload from the request shape + the produced graph.
fn graph_payload(
    mode: ProjectionMode,
    resolve: &wire::ResolveSymbolGraphRequest,
    graph: &SemanticTypeGraph,
) -> TypeInfoGraphPayload {
    let closure_tag = match resolve.closure.as_ref().and_then(|c| c.kind.as_ref()) {
        Some(graph_closure_policy::Kind::RootOnly(_)) => GraphClosurePolicyTag::RootOnly,
        Some(graph_closure_policy::Kind::Path(_)) => GraphClosurePolicyTag::Path,
        Some(graph_closure_policy::Kind::OneLevel(_)) => GraphClosurePolicyTag::OneLevel,
        Some(graph_closure_policy::Kind::Expanded(_)) => GraphClosurePolicyTag::Expanded,
        Some(graph_closure_policy::Kind::ProjectionRequired(_)) => {
            GraphClosurePolicyTag::ProjectionRequired
        }
        None => GraphClosurePolicyTag::RootOnly,
    };
    let demand_tag = match resolve.context.as_ref().map(|c| c.demand) {
        x if x == Some(wire::ReductionDemand::StructuralTransit as i32) => {
            ReductionDemandTag::StructuralTransit
        }
        _ => ReductionDemandTag::Published,
    };
    TypeInfoGraphPayload {
        operation: GraphOperationTag::ResolveSymbol,
        mode: ProjectionModeTag::from(mode),
        demand: demand_tag,
        roots_count: 0,
        closure: closure_tag,
        schema_version: graph.schema_version,
        snapshot_node_count: u32::try_from(graph.nodes.len()).unwrap_or(u32::MAX),
        snapshot_edge_count: u32::try_from(graph.edges.len()).unwrap_or(u32::MAX),
        snapshot_symbol_count: u32::try_from(graph.symbols.len()).unwrap_or(u32::MAX),
        ..TypeInfoGraphPayload::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_resolve_type_audit::TypeResolutionRequestError;
    use crate::resolver_core::shallow_file_state::BudgetDomain as SessionBudgetDomain;
    use crate::semantic_query::BudgetExceededFailure;

    #[test]
    fn a_projection_operation_budget_fault_never_aliases_a_closed_wire_domain() {
        let fault = TypeResolutionRequestError::BudgetExceeded(BudgetExceededFailure {
            domain: SessionBudgetDomain::ProjectionOperation,
            limit: 2000,
            actual: 2001,
            context: "semantic dispatch".to_string(),
        });
        let (error, detail) = fault_error(&fault);
        // The projection-operation fuse has no wire variant; it must
        // degrade to the textual Other arm naming itself — never re-label
        // onto solver resolve steps (or any other closed domain).
        match error.kind.as_ref() {
            Some(graph_query_error::Kind::Other(_)) => {}
            other => {
                panic!("projection-operation budget must not alias a closed domain, got {other:?}")
            }
        }
        let detail = detail.expect("an unmapped domain carries its label");
        assert!(
            detail.contains("projection operation"),
            "the textual arm names the tripped fuse, got {detail:?}"
        );

        // A domain WITH a wire variant keeps the typed BudgetExceeded arm.
        let fault = TypeResolutionRequestError::BudgetExceeded(BudgetExceededFailure {
            domain: SessionBudgetDomain::SolverResolveSteps,
            limit: 5000,
            actual: 5001,
            context: String::new(),
        });
        let (error, detail) = fault_error(&fault);
        match error.kind.as_ref() {
            Some(graph_query_error::Kind::BudgetExceeded(exceeded)) => assert_eq!(
                exceeded.domain,
                wire::BudgetDomain::SolverResolveSteps as i32
            ),
            other => panic!("a mapped domain keeps the typed arm, got {other:?}"),
        }
        assert!(detail.is_none(), "a typed arm interns no filler text");
    }

    #[test]
    fn text_bearing_faults_keep_their_own_text() {
        let fault = TypeResolutionRequestError::Other(std::sync::Arc::from("resolver exploded"));
        let (error, detail) = fault_error(&fault);
        assert!(matches!(
            error.kind,
            Some(graph_query_error::Kind::Other(_))
        ));
        assert_eq!(detail.as_deref(), Some("resolver exploded"));

        let fault = TypeResolutionRequestError::UnsupportedIntrinsic {
            name: std::sync::Arc::from("Uppercase"),
        };
        let (_, detail) = fault_error(&fault);
        assert!(
            detail.as_deref().is_some_and(|d| d.contains("Uppercase")),
            "the intrinsic's name survives, got {detail:?}"
        );

        // The interned detail lands in the fault graph's string table.
        let graph = single_opaque_root_graph(
            fault_error(&TypeResolutionRequestError::Other(std::sync::Arc::from(
                "resolver exploded",
            )))
            .0,
            Some("resolver exploded".to_string()),
            None,
        );
        let entries = graph.strings.as_ref().map(|t| t.entries.as_slice());
        assert_eq!(
            entries.map(|e| e.first().map(String::as_str)),
            Some(Some("")),
            "string id 0 is the reserved absent sentinel"
        );
        assert_eq!(
            entries.map(|e| e.get(1).map(String::as_str)),
            Some(Some("resolver exploded")),
            "the fault's own text is interned at the message id"
        );
    }

    #[test]
    fn bounded_closure_kinds_bound_the_exported_walk_depth() {
        let root_only = closure_budgets(Some(&wire::ClosurePolicy {
            kind: Some(graph_closure_policy::Kind::RootOnly(
                wire::ClosureRootOnly {},
            )),
        }));
        assert_eq!(root_only.depth_budget, 1, "root-only is the root node");
        let one_level = closure_budgets(Some(&wire::ClosurePolicy {
            kind: Some(graph_closure_policy::Kind::OneLevel(
                wire::ClosureOneLevel {},
            )),
        }));
        assert_eq!(
            one_level.depth_budget, 2,
            "one level adds exactly one child level"
        );
    }
}
