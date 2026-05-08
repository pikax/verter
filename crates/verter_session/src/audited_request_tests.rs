//! Audit-nesting regression tests.
//!
//! These tests guard the boundary that distinguishes a logical audited
//! request (one `RequestAuditRecord` per audited entry-point) from the
//! semantic dispatch layer (which must NOT publish records of its
//! own).
//!
//! The audit substrate's record lifecycle has two halves:
//!
//! * Audited entry-points (`get_component_meta_with_resolution`, the
//!   compile / analyze / type-resolve / workspace / LSP / MCP wrappers)
//!   construct a `RequestContext`, plant an `AuditRequestRegistration`
//!   on it, install it as TLS via `RequestContextGuard::install`, run
//!   the user query, and finalize the registration with the resulting
//!   `RequestAuditRecord`. Finalisation inserts ONE record into the
//!   host's `AuditRecordsStore`.
//! * The shared semantic dispatcher (`ProjectSemanticDispatch::execute`
//!   and `execute_read`) never publishes records. It performs cooperative
//!   admission, dep-signature accumulation, and warm-cache reads, but it
//!   has no path to `audit_records.insert`. Records emerge only when an
//!   outer entry-point synthesises and finalises them.
//!
//! Future drift in either direction would be a correctness regression:
//!
//! * If `execute_read` (or any raise / build helper) starts publishing
//!   per-dispatch records, every audited request would emit dozens to
//!   thousands of nested records — breaking the consumer contract that
//!   one logical request maps to exactly one record.
//! * If a raw `execute_read` somehow finalises a registration the
//!   caller did not mean to commit, the records store would grow on
//!   paths that have no audited entry-point at all.
//!
//! The tests below characterise the current correct behaviour: a manual
//! `RequestContext` plus raw `dispatch.execute_read` calls produce
//! exactly ZERO records. They are designed to FAIL on any future
//! commit that wires record emission into the dispatch layer.

use std::sync::Arc;

use verter_audit::RequestKind;

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::semantic_query::{
    DeclIdentity, PathSegment, ProjectionMode, QueryResult, SemanticNodeData, SemanticQueryKey,
};
use crate::types::{AnalysisLevel, HostConfig};
use crate::{FileKind, UpsertRequest, VerterHost};

/// Build a host that has audit recording fully enabled. The
/// regression test for raw-dispatch nesting only matters when
/// finalisation could in principle publish records — disabling
/// audit would cause the nested-emission failure mode to be
/// silently masked.
fn audit_enabled_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    }))
}

/// Ergonomic helper: write a non-SFC TS file into the host. Mirrors
/// the convention used by `project_semantic_dispatch/tests.rs` so the
/// regression test stays close in spirit to the dispatch tests it
/// guards.
fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("upsert must succeed for the regression fixture");
}

/// Synthetic owner identity matching the convention from
/// `project_semantic_dispatch/tests.rs::synthetic_macro_owner`. The
/// test does not need a real SFC owner — `ResolveMacroPayload` with
/// an empty `type_args` slice short-circuits to `Opaque(Miss)` for
/// `DefineProps`, which is exactly the path we want to exercise to
/// prove the dispatch arm produced no record.
fn synthetic_macro_owner(canonical: &str) -> DeclIdentity {
    DeclIdentity {
        canonical_id: Arc::from(canonical),
        whole_hash: [0u8; 16],
        decl_name: Arc::from("<sfc-script-setup>"),
    }
}

/// Regression: raw `ProjectSemanticDispatch::execute_read` calls,
/// even when invoked inside a hand-installed `RequestContext` with
/// `RequestKind::ComponentMeta`, MUST publish zero
/// `RequestAuditRecord`s. The audit boundary lives at the audited
/// entry-points (`get_component_meta_with_resolution` and friends),
/// not at the dispatch level.
///
/// FAILS on any future commit that wires record emission into
/// `execute_read` or any of its `build_*` helpers — a per-dispatch
/// record would pop the records store size above zero.
///
/// Discriminating: the assertions inspect the records store via
/// `host_audit_runtime().snapshot()` AND drive a direct
/// `take_audit_record(request_id)` lookup. A drift commit that, say,
/// inserted a record under the active request id without bumping
/// the snapshot would still surface as a `Some(_)` from `take`.
#[test]
fn raw_dispatch_execute_emits_no_audit_records() {
    let host = audit_enabled_host();

    // Pre-condition: the records store is empty before the test
    // touches the dispatch layer. If the host construction itself
    // started leaking records, the regression target would already
    // be invalid.
    let pre = host.host_audit_runtime().snapshot();
    assert_eq!(
        pre.records_store_size, 0,
        "fresh host must start with zero audit records, got {pre:?}",
    );
    assert_eq!(
        pre.active_request_count, 0,
        "fresh host must have no active requests, got {pre:?}",
    );

    // Seed a tiny TS file so the workspace is non-empty. The raw
    // dispatch reads we issue do not depend on this file resolving
    // — `ResolveMacroPayload { ..DefineProps, type_args: [] }`
    // short-circuits to `Opaque(Miss)` and the `ProjectPath` walk
    // we issue starts from a primitive node interned directly into
    // the semantic graph — but a populated workspace exercises the
    // resolver context the same way an audited entry-point would,
    // which keeps the regression close to production behaviour.
    upsert_ts(&host, "/w/types.ts", "export type Foo = { x: number }");

    // The request id we will later look up MUST be drained from the
    // host's id allocator so the test cannot accidentally collide
    // with a future internal request.
    let request_id = host.next_request_id();

    {
        // Manual `RequestContext` matching the construction sequence
        // performed by the production audited entry-points (see
        // `host_manage/component_meta_entry.rs::component_meta_request`).
        // We DELIBERATELY skip `AuditRequestRegistration::new` — the
        // whole point of this regression is to prove that the
        // dispatch layer does not finalise a record on its own.
        let footprint_capture = host.config.footprint_capture && host.config.audit_enabled;
        let accumulator = if footprint_capture {
            Some(Arc::new(
                crate::component_meta_audit::RequestFootprintAccumulator::new(),
            ))
        } else {
            None
        };
        let ctx = RequestContext::with_kind_and_timing(
            request_id,
            Arc::<str>::from("/w/types.ts"),
            RequestKind::ComponentMeta,
            footprint_capture,
            host.config.audit_timing_capture && host.config.audit_enabled,
            accumulator,
        );

        // Install the context as TLS so any opportunistic accumulator
        // sink the dispatcher might consult sees a real context. This
        // also tightens the regression: a drift commit that wired a
        // record-emit into `execute_read` keyed off TLS would have
        // every reason to fire here.
        let _ctx_guard = RequestContextGuard::install(ctx);

        let dispatch = ProjectSemanticDispatch::new(host.as_ref());

        // First raw dispatch: ResolveMacroPayload with an empty
        // type-arg slice. Per the build rule for
        // `DefineProps` / `WithDefaults` with 0 args, this
        // short-circuits to `Opaque(Miss)` without entering any
        // file resolution path — a deliberate choice so the
        // regression does not depend on workspace state.
        let owner = synthetic_macro_owner("/w/c.vue");
        let macro_key = SemanticQueryKey::ResolveMacroPayload {
            owner,
            macro_index: 0,
            macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
            type_args: Arc::from(Vec::new().into_boxed_slice()),
            mode: ProjectionMode::Expanded,
        };
        let macro_read = dispatch.execute_read(macro_key);
        // Discriminating side-check: prove `execute_read` actually
        // exercised the build path. A future refactor that turned
        // `execute_read` into an unconditional short-circuit would
        // hide nesting drift behind a no-op; this assertion catches
        // that. `DefineProps` with empty `type_args` lowers to
        // `Opaque(Miss)`, which is still a `Value` node id.
        assert!(
            matches!(macro_read.value, QueryResult::Value(_)),
            "ResolveMacroPayload(0-arg DefineProps) must return Value(Opaque(Miss)); got {:?}",
            macro_read.value
        );

        // Second raw dispatch: ProjectPath rooted at a primitive
        // node interned directly into the graph. This exercises a
        // distinct dispatch family (`build_project_path`) so the
        // regression catches drift in either of the two arms a
        // typical component-meta entry-point would touch.
        let graph = host.project_type_store().semantic_graph();
        let primitive_base = graph.intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::String,
        ));
        let path_key = SemanticQueryKey::ProjectPath {
            base: primitive_base,
            path: Arc::from(vec![PathSegment::Member(Arc::from("nonexistent"))].into_boxed_slice()),
            mode: ProjectionMode::Identity,
        };
        let path_read = dispatch.execute_read(path_key);
        // Discriminating side-check (same rationale as above): the
        // `ProjectPath` arm must have actually run. Projecting a
        // non-existent member off a primitive surface yields a
        // `Value(Opaque(Miss))` node id — still a `Value` arm.
        assert!(
            matches!(path_read.value, QueryResult::Value(_)),
            "ProjectPath(primitive, [member]) must return Value; got {:?}",
            path_read.value
        );

        // The guard drops here, restoring the prior TLS slots.
        // CRITICAL: dropping a bare `RequestContext` does NOT
        // finalise a record (no `AuditRequestRegistration` was
        // planted). If a future commit ever made `Drop` for
        // `RequestContext` finalise a record, that would itself be a
        // regression and the assertions below would catch it.
    }

    // Post-condition 1: the records store is STILL empty. The two
    // raw `execute_read` calls finished, the context dropped, and
    // no record was published anywhere.
    let post = host.host_audit_runtime().snapshot();
    assert_eq!(
        post.records_store_size, 0,
        "raw execute_read must not publish records; got snapshot {post:?}",
    );

    // Post-condition 2: looking up the specific request id we
    // assigned to the manual context returns `None`. This catches a
    // drift commit that inserted under our id without incrementing
    // the snapshot counter.
    let drained = host.take_audit_record(request_id);
    assert!(
        drained.is_none(),
        "no audit record should be filed against request_id={request_id}; got {drained:?}",
    );

    // Post-condition 3: the active-request registry never recorded
    // anything either, even though the context lived briefly. A
    // future commit that started auto-registering RequestContexts
    // on construction would leak entries into this registry without
    // ever balancing them with a finalize, and this assertion would
    // catch it.
    assert_eq!(
        post.active_request_count, 0,
        "no active request must be tracked for raw dispatch; got {post:?}",
    );
}
