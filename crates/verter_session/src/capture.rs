//! Root-entry selection of optional audit capture.
//!
//! Audit records, footprints, timers and the host audit runtime are
//! optional state (see `docs/arch/semantic-observe.md`). Each audited root
//! entry selects its concrete execution mode ONCE ([`select_mode`]): a build
//! with `semantic-observe` captures a request only when the host enables
//! audit and the consumer filter admits its kind; a default build carries
//! none of that state, normalizes the host's capture flags off at
//! construction and labels every audited entry's record
//! [`AuditCaptureState::Unavailable`]. Both modes install the same REQUIRED
//! request context ([`root_entry_context`]).

use std::sync::Arc;

use verter_audit::observe::ObserveMode;
use verter_audit::{AuditCaptureState, CaptureAvailability, RequestKind};
use verter_execution::cancellation::CancellationToken;
use verter_type_engine::request_context::RequestContext;
use verter_type_engine::request_footprint::RequestFootprintAccumulator;

use crate::{HostConfig, VerterHost};

/// Select the concrete execution mode of one audited root entry: captured
/// only when this build compiles capture in, the host enables audit and the
/// consumer filter admits `kind`. The installed request context carries the
/// choice through every scheduler resumption; nothing re-selects it.
/// A build without `semantic-observe` has no capture to select: every
/// entry runs uncaptured.
#[cfg(feature = "semantic-observe")]
#[must_use]
pub(crate) fn select_mode(host: &VerterHost, kind: &RequestKind) -> ObserveMode {
    ObserveMode::select(
        host.config.audit_enabled
            && host
                .host_audit_runtime()
                .audit_config()
                .consumer_filter
                .allows(kind),
    )
}

/// The capture execution an audited root entry of `kind` selects, for an
/// entry that must specialise on it before it builds its request context
/// (one whose body arrives as a closure). Always
/// [`ObserveMode::Uncaptured`] in a build without `semantic-observe`, where
/// `kind` is never built.
#[must_use]
pub(crate) fn entry_mode(host: &VerterHost, kind: impl FnOnce() -> RequestKind) -> ObserveMode {
    #[cfg(feature = "semantic-observe")]
    {
        select_mode(host, &kind())
    }
    #[cfg(not(feature = "semantic-observe"))]
    {
        let _ = (host, kind);
        ObserveMode::Uncaptured
    }
}

/// The request context an audited root entry installs whichever mode it
/// selects: the same canonical, the host's projection-operation budget and
/// `cancellation`, so a captured and an uncaptured run answer, refuse and
/// cancel alike. Only the optional capture (footprint, timing, accumulator,
/// observer) follows the mode. `kind` is built only when capture is
/// compiled in.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub(crate) fn root_entry_context(
    host: &VerterHost,
    request_id: u64,
    canonical_id: Arc<str>,
    kind: impl FnOnce() -> RequestKind,
    footprint_capture: bool,
    timing_capture: bool,
    accumulator: Option<Arc<RequestFootprintAccumulator>>,
    cancellation: CancellationToken,
) -> Arc<RequestContext> {
    #[cfg(feature = "semantic-observe")]
    let (kind, mode) = {
        let kind = kind();
        let mode = select_mode(host, &kind);
        (move || kind, mode)
    };
    #[cfg(not(feature = "semantic-observe"))]
    let mode = ObserveMode::Uncaptured;
    root_entry_context_in_mode(
        host,
        request_id,
        canonical_id,
        kind,
        mode,
        footprint_capture,
        timing_capture,
        accumulator,
        cancellation,
    )
}

/// [`root_entry_context`] for an entry that already selected `mode` with
/// [`entry_mode`]: the context carries that one selection.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub(crate) fn root_entry_context_in_mode(
    host: &VerterHost,
    request_id: u64,
    canonical_id: Arc<str>,
    kind: impl FnOnce() -> RequestKind,
    mode: ObserveMode,
    footprint_capture: bool,
    timing_capture: bool,
    accumulator: Option<Arc<RequestFootprintAccumulator>>,
    cancellation: CancellationToken,
) -> Arc<RequestContext> {
    RequestContext::for_root_entry(
        request_id,
        canonical_id,
        kind,
        mode,
        footprint_capture,
        timing_capture,
        accumulator,
        host.config.projection_op_budget,
        cancellation,
    )
}

/// The capture execution mode an execution unit starting on this thread
/// adopts: the mode the installed request carries
/// ([`verter_type_engine::request_context::installed_observe_mode`]). The
/// unit reads it once, where it starts, and specialises on it with
/// [`verter_audit::specialise!`]; nothing it runs re-reads it.
#[inline]
#[must_use]
pub(crate) fn installed_mode() -> ObserveMode {
    verter_type_engine::request_context::installed_observe_mode()
}

/// `true` when this build compiles audit capture in (`semantic-observe`).
/// A per-request capture branch guarded by it compiles away in a default
/// build.
pub(crate) const COMPILED: bool = cfg!(feature = "semantic-observe");

/// Whether this build can capture audit records.
#[must_use]
pub const fn availability() -> CaptureAvailability {
    CaptureAvailability::compiled()
}

/// Read using the calling unit's bound capture receivers. The uncaptured
/// specialization calls only the plain workspace read.
pub(crate) fn read_file_in<M: verter_audit::observe::ObserveExec>(
    workspace: &dyn verter_workspace::WorkspaceRead,
    observation: &verter_type_engine::request_observers::CaptureTargets,
    canonical: &str,
) -> Option<Arc<str>> {
    let _ = observation;
    if !M::CAPTURED {
        return workspace.read_file(canonical);
    }
    #[cfg(feature = "semantic-observe")]
    if let Some(ctx) = observation.request.as_ref() {
        return workspace.read_file_captured(
            canonical,
            &verter_workspace::audit_sink::VfsReadCapture {
                request_id: ctx.request_id,
                timing: ctx.timing_capture,
            },
        );
    }
    workspace.read_file(canonical)
}

/// The host config a host actually runs with: without `semantic-observe`
/// the audit, footprint and timing capture flags are cleared, because no
/// state exists to honour them.
#[must_use]
pub(crate) fn normalize_host_config(config: HostConfig) -> HostConfig {
    #[cfg(feature = "semantic-observe")]
    {
        config
    }
    #[cfg(not(feature = "semantic-observe"))]
    {
        HostConfig {
            audit_enabled: false,
            footprint_capture: false,
            audit_timing_capture: false,
            ..config
        }
    }
}

/// The capture state of a record an audited entry returns without having
/// captured one: unavailable in a build without `semantic-observe`,
/// otherwise filtered by the consumer filter or disabled by host config.
#[must_use]
pub(crate) fn uncaptured_state(audit_enabled: bool) -> AuditCaptureState {
    if COMPILED {
        if audit_enabled {
            AuditCaptureState::FilteredNoop
        } else {
            AuditCaptureState::AuditDisabled
        }
    } else {
        AuditCaptureState::Unavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "semantic-observe")]
    #[test]
    fn captured_source_jobs_keep_workspace_read_attribution() {
        use verter_execution::request_context::OpaqueRequestContext;
        use verter_scheduler::{
            job::CompletionState,
            scheduler::Request,
            stage::{Priority, TargetStage},
        };
        use verter_workspace::WorkspaceAccess;
        let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(Default::default()));
        workspace.inject_file(
            "/dependency.ts".into(),
            Arc::from("export interface Props { value: string }"),
        );
        let host = VerterHost::new(
            HostConfig {
                audit_enabled: true,
                footprint_capture: true,
                audit_timing_capture: true,
                ..HostConfig::default()
            },
            workspace.clone(),
        );
        let accumulator = Arc::new(RequestFootprintAccumulator::new());
        let ctx = root_entry_context(
            &host,
            42,
            Arc::from("/entry.vue"),
            || RequestKind::ComponentMeta,
            true,
            true,
            Some(accumulator.clone()),
            CancellationToken::new(),
        );
        let sink = crate::session_vfs_sink::SessionVfsSink::new(42, accumulator.clone());
        let handle = workspace.register_audit_sink(sink).unwrap();
        let access: Arc<dyn WorkspaceAccess> = workspace.clone();
        let _registration =
            verter_workspace::audit_sink::SinkRegistration::new(handle, Arc::downgrade(&access));
        let loaded = host.scheduler.submit_request(Request {
            file_id: "/dependency.ts".into(),
            source: None,
            target: TargetStage::Source,
            priority: Priority::Interactive,
            file_language: None,
            request_context: Some(OpaqueRequestContext(ctx)),
        });
        assert!(matches!(
            host.scheduler.wait_or_drive(&loaded),
            CompletionState::Ready(_)
        ));
        let state = accumulator.drain();
        assert_eq!(state.vfs_reads.len(), 1);
        assert_eq!(state.vfs_reads[0].canonical_id.as_ref(), "/dependency.ts");
        assert_eq!(state.vfs_reads[0].request_id, 42);
        assert_eq!(state.file_read_timings.len(), 1);
        assert!(state.file_read_timings[0].read_ns.is_some());
    }

    #[test]
    fn uncaptured_component_meta_refusals_have_no_measurement_payload() {
        use crate::component_meta_host::ComponentMetaHost;
        for enabled in [false, true] {
            let host = ComponentMetaHost::new_standalone(HostConfig {
                audit_enabled: enabled,
                footprint_capture: enabled,
                ..HostConfig::default()
            });
            let session = host.open_session().unwrap();
            if !enabled {
                let (outcome, record) = session
                    .get_component_meta_with_audit("/missing.vue")
                    .into_parts();
                assert!(outcome.is_err());
                assert!(record.component_meta_payload().is_none());
            }
            session.close();
            let (outcome, record) = session
                .get_component_meta_with_audit("/missing.vue")
                .into_parts();
            assert!(outcome.is_err());
            assert_eq!(record.capture_state, uncaptured_state(enabled && COMPILED));
            assert!(record.component_meta_payload().is_none());
        }
    }

    #[test]
    fn compatibility_zeros_exist_only_in_captured_component_meta_records() {
        use crate::component_meta_host::ComponentMetaHost;
        #[cfg(not(feature = "semantic-observe"))]
        use crate::component_meta_host::ComponentMetaHostError;
        let host = ComponentMetaHost::new_standalone(HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        });
        host.upsert_base(
            "/compat.vue",
            "<script setup lang=\"ts\">defineProps<{ value: string }>()</script>",
        )
        .unwrap();
        let session = host.open_session().unwrap();
        for attempt in 0..2 {
            let (outcome, record) = session
                .get_component_meta_with_audit("/compat.vue")
                .into_parts();
            #[cfg(feature = "semantic-observe")]
            {
                outcome
                    .expect("captured metadata resolves")
                    .expect("the fixture has component metadata");
                assert_eq!(record.capture_state, AuditCaptureState::ActiveStored);
                assert_eq!(record.from_cache, attempt > 0);
                let payload =
                    serde_json::to_value(record.component_meta_payload().unwrap()).unwrap();
                assert_eq!(payload["materialize_structure_calls"], "0");
                assert_eq!(payload["materialize_structure_cache_hits"], "0");
                let bypass = serde_json::to_value(&record.store.bypass_diagnostics).unwrap();
                assert_eq!(bypass["resolver_store_view_calls"], "0");
            }
            #[cfg(not(feature = "semantic-observe"))]
            {
                assert!(matches!(
                    outcome,
                    Err(ComponentMetaHostError::AuditNotEnabled)
                ));
                assert_eq!(record.capture_state, AuditCaptureState::Unavailable);
                assert!(
                    record.component_meta_payload().is_none(),
                    "attempt {attempt}"
                );
            }
        }
    }

    /// Every audited entry installs the same REQUIRED request context
    /// whichever capture mode it selects: an MCP tool's body sees its own
    /// canonical, the host's projection budget and a live cancellation scope
    /// with audit disabled, enabled and (in an instrumented build) filtered —
    /// never the unrelated outer request installed on the calling thread.
    /// Only the observer differs: it is planted for a captured request alone.
    #[test]
    fn every_capture_mode_installs_the_same_required_request_context() {
        use verter_type_engine::request_context::RequestContextGuard;
        let observe = |host: Arc<VerterHost>| {
            let _outer = RequestContextGuard::install(RequestContext::for_root_entry(
                9_999,
                Arc::from("/outer.ts"),
                || RequestKind::TypeResolution,
                ObserveMode::Uncaptured,
                false,
                false,
                None,
                1,
                CancellationToken::new(),
            ));
            let (outcome, _record) = host
                .audit_mcp_tool_call("probe", "/probe.ts", 0, |_| {
                    let ctx = verter_type_engine::request_context::current_request_context()
                        .expect("the tool runs under its own request context");
                    #[cfg(feature = "semantic-observe")]
                    assert_eq!(
                        verter_audit::observer::current_observer().is_some(),
                        ctx.observe_mode().is_captured(),
                        "only a captured request plants the observer"
                    );
                    Ok::<_, ()>(crate::host_mcp_audit::McpToolSuccess {
                        value: (
                            (
                                ctx.canonical_id.to_string(),
                                ctx.projection_budget.effective_projection_op_budget(),
                                ctx.is_cancelled(),
                            ),
                            ctx.observe_mode(),
                        ),
                        result_size_bytes: 0,
                    })
                })
                .into_parts();
            outcome.expect("the probe tool succeeds")
        };
        let config = |audit_enabled| HostConfig {
            audit_enabled,
            projection_op_budget: 37,
            ..HostConfig::default()
        };
        let required = ("/probe.ts".to_string(), 37, false);

        let (disabled, disabled_mode) =
            observe(Arc::new(VerterHost::new_standalone(config(false))));
        assert_eq!(disabled, required);
        assert_eq!(disabled_mode, ObserveMode::Uncaptured);

        let (enabled, enabled_mode) = observe(Arc::new(VerterHost::new_standalone(config(true))));
        assert_eq!(
            enabled, required,
            "capture leaves the required context unchanged"
        );
        // A caller may explicitly activate the substrate without this
        // owner's capture feature. That exposes the shared capability but
        // cannot construct a session capture envelope or enable its flags.
        assert_eq!(
            enabled_mode,
            if COMPILED {
                ObserveMode::select(true)
            } else {
                ObserveMode::Uncaptured
            }
        );

        #[cfg(feature = "semantic-observe")]
        {
            let mut host = VerterHost::new_standalone(config(true));
            host.replace_host_audit_runtime_for_test(verter_audit::AuditConfig {
                consumer_filter: verter_audit::AuditConsumerFilter::deny_all(),
                ..verter_audit::AuditConfig::default()
            });
            let (filtered, filtered_mode) = observe(Arc::new(host));
            assert_eq!(
                filtered, required,
                "a filtered request keeps the required context"
            );
            assert_eq!(filtered_mode, ObserveMode::Uncaptured);
        }
    }

    #[test]
    fn availability_follows_the_build() {
        assert_eq!(availability(), CaptureAvailability::compiled());
    }

    #[test]
    fn a_build_without_capture_runs_hosts_with_capture_flags_cleared() {
        let requested = HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            audit_timing_capture: true,
            ..HostConfig::default()
        };
        let effective = normalize_host_config(requested);
        let captured = cfg!(feature = "semantic-observe");
        assert_eq!(effective.audit_enabled, captured);
        assert_eq!(effective.footprint_capture, captured);
        assert_eq!(effective.audit_timing_capture, captured);
        assert_eq!(
            uncaptured_state(true),
            if captured {
                AuditCaptureState::FilteredNoop
            } else {
                AuditCaptureState::Unavailable
            }
        );
    }
}
