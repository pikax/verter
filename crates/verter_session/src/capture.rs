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

/// `true` when this build compiles audit capture in (`semantic-observe`).
/// A per-request capture branch guarded by it compiles away in a default
/// build.
pub(crate) const COMPILED: bool = cfg!(feature = "semantic-observe");

/// Whether this build can capture audit records.
#[must_use]
pub const fn availability() -> CaptureAvailability {
    if COMPILED {
        CaptureAvailability::Available
    } else {
        CaptureAvailability::Unavailable
    }
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
        assert_eq!(enabled_mode, ObserveMode::select(true));

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
        assert_eq!(
            availability() == CaptureAvailability::Available,
            cfg!(feature = "semantic-observe")
        );
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
