//! Root-entry selection of optional audit capture.
//!
//! Audit records, footprints, timers and the host audit runtime are
//! optional state (see `docs/arch/semantic-observe.md`). A build with
//! `semantic-observe` captures every request it runs and publishes a record
//! when the host config and consumer filter ask for one; a default build
//! carries none of that state, normalizes the host's capture flags off at
//! construction and labels every audited entry's record
//! [`AuditCaptureState::Unavailable`].

use verter_audit::{AuditCaptureState, CaptureAvailability};

use crate::HostConfig;

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
