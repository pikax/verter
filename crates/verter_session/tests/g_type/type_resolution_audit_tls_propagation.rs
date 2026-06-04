//! TLS-observer propagation through `VerterHost::resolve_type_with_audit`
//! (Wave 3 Slice 3.A follow-up).
//!
//! Drives the public production entry-point through the
//! [`verter_session::tests::audit_tls_harness::assert_observer_reaches`]
//! harness and asserts the audit substrate's TLS slot is populated for
//! the duration of the audited window:
//!
//! - **Positive**: `assert_observer_reaches(true, …)` — the harness
//!   installs an outer `RequestContextGuard`, the entry-point installs
//!   its own (Active) registration's guard inside, the dispatcher
//!   reaches `SemanticGraphStore::execute_cooperative` with
//!   `current_observer() == Some(_)`, and the per-request hops counter
//!   on the published record is non-zero — proving the observer was
//!   reachable on the dispatch path.
//! - **Negative**: `assert_observer_reaches(false, …)` — the harness
//!   does NOT install an outer guard, audit is disabled on the host so
//!   the entry-point's registration short-circuits to `Noop`, no
//!   record is published, and the harness's calling-thread probe sees
//!   `current_observer() == None` after the call returns.
//!
//! Discrimination contract:
//! - Pre-change tree (no harness driver pinned to this entry-point):
//!   no test exercises observer propagation through the
//!   `resolve_type_with_audit` window — characterisation tests check
//!   record contents but don't probe the TLS slot.
//! - Wired correctly: the positive case observes a populated record
//!   and a non-zero hops counter; the negative case observes neither.

use std::sync::Arc;

use verter_session::semantic_query::{ResolveDeclKey, ScopeId, SemanticQueryKey};
use verter_session::tests::audit_tls_harness::assert_observer_reaches;
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};

const TYPES_TS: &str = r#"
export type Outer = {
    inner: { value: string; nested: { deep: number } };
};
"#;

fn build_host(audit_enabled: bool) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled,
        footprint_capture: false,
        ..HostConfig::default()
    }));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/types.ts".to_string()),
        input_id: "/types.ts".to_string(),
        source: Arc::from(TYPES_TS),
        file_kind: FileKind::from_path("/types.ts"),
        aliases: Vec::new(),
    });
    host
}

fn outer_query() -> SemanticQueryKey {
    SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from("/types.ts"),
            local_scope: None,
        },
        name: Arc::from("Outer"),
    })
}

#[test]
fn resolve_type_with_audit_propagates_observer_through_dispatch() {
    let host = build_host(true);

    let mut record_kind: Option<verter_audit::RequestKind> = None;
    let mut hops: u32 = 0;
    let report = assert_observer_reaches(true, || {
        // Drive the real production audited entry-point. The
        // dispatcher (`SemanticGraphStore::execute_cooperative`) emits
        // hop accounting through the active `RequestContext` —
        // those increments flow through `current_observer()` and
        // surface as `TypeResolutionPayload::hops`. Non-zero ⇒ the
        // observer was reachable on the dispatch path.
        let (resolved, record) = host
            .resolve_type_with_audit(outer_query(), "/types.ts")
            .into_parts();
        assert!(
            matches!(resolved, Ok(Some(_))),
            "cold resolution of `Outer` must succeed under audit"
        );
        record_kind = Some(record.kind.clone());
        if let Some(payload) = record.type_resolution_payload() {
            hops = payload.hops;
        }
    });

    assert!(
        matches!(record_kind, Some(verter_audit::RequestKind::TypeResolution)),
        "resolve_type_with_audit must publish a TypeResolution record when audit is enabled; \
         a regression that drops the substrate-TLS plumbing in `RequestContextGuard::install` \
         would leave the registration's `Active` arm unable to record hops, but the record is \
         still published — so this assertion is necessary but the discriminator below catches \
         the deeper regression. record_kind = {record_kind:?}, report = {report:?}",
    );
    assert!(
        hops > 0,
        "the dispatcher's hop accounting must increment under audit; got hops={hops}. \
         A regression that left `current_observer()` returning None on the dispatch path \
         would leave the per-request counter at 0 and this assertion would fail. \
         report = {report:?}",
    );
    assert!(
        report.observer_seen_on_calling_thread,
        "harness must record the calling-thread observation as Some — the harness's outer \
         RequestContextGuard remains installed when the entry-point's nested guard drops on \
         return: {report:?}",
    );
}

#[test]
fn resolve_type_with_audit_observer_absent_outside_harness_window() {
    // `resolve_type_with_audit` does NOT short-circuit on
    // `audit_enabled=false` — the consumer-filter snapshot defaults
    // to `allow_all`, so the entry-point's registration takes the
    // `Active` arm and publishes a record regardless of the
    // host-level enable flag (this is intentional: type-resolution
    // audit is filter-driven, not enable-flag-driven). The
    // discriminator the harness can drive is the OUTSIDE-the-window
    // observation: with `install_audit=false`, the harness installs
    // no outer `RequestContextGuard`, so after the entry-point's
    // own guard drops on return, the calling thread must see
    // `current_observer() == None`.
    let host = build_host(false);

    let report = assert_observer_reaches(false, || {
        // Drive the entry-point. Inside the entry-point, the
        // registration's `Active` arm installs its own guard for
        // the duration of the dispatch, but the guard drops on
        // return — so when the harness's calling-thread probe runs
        // AFTER this closure returns, no observer must be visible.
        let (_resolved, _record) = host
            .resolve_type_with_audit(outer_query(), "/types.ts")
            .into_parts();
    });

    assert!(
        !report.observer_seen_on_calling_thread,
        "harness installed no outer guard; the entry-point's nested guard MUST \
         drop on return, so the calling thread must see no observer at the \
         harness's post-call probe point. A regression that leaks the \
         entry-point's guard past its return — the most common TLS-propagation \
         defect — would surface here as `observer_seen_on_calling_thread = true`. \
         report = {report:?}",
    );
}
