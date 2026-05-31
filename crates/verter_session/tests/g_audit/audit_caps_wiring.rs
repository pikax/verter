//! Discriminator test for the production wiring of
//! [`HostConfig::audit_caps`] to the per-request
//! [`RequestFootprintAccumulator`].
//!
//! ## Why this test exists
//!
//! The audit accumulator carries its own [`AuditCaps`] copy and the
//! raw push lanes (`push_structured_event`, `push_vfs_read`,
//! `push_materialization`, etc.) consult those caps at push time —
//! pushes beyond the cap are dropped and the matching
//! `TruncationCounters::*_truncated` field is incremented. The
//! cap-bounded push behaviour was already exercised at the
//! accumulator surface by `audit_caps_truncation_tests.rs`.
//!
//! Codex flagged a wiring gap during the Phase B fix-cycle: the
//! production accumulator-construction site at
//! `host_manage/component_meta_entry.rs::get_component_meta_with_resolution`
//! built the per-request accumulator with
//! `RequestFootprintAccumulator::new()` — which hardcodes
//! `AuditCaps::default()` (every cap → `DEFAULT_*` = 10_000) regardless
//! of what the host operator configured on
//! [`HostConfig::audit_caps`]. The post-mining `derivation_nodes` cap
//! was correctly wired (it reaches `mine_footprint` via the existing
//! `&self.config.audit_caps` parameter), but every other lane — raw
//! pushes like `structured_events`, `vfs_reads`, `materializations` —
//! quietly applied the 10_000 default cap instead of the host's
//! configured override.
//!
//! ## Discriminating contract
//!
//! Build a hermetic host with `audit_caps.vfs_reads = Some(1)` and
//! `audit_caps.structured_events = Some(1)` — both well below what a
//! real component-meta resolution produces. Drive a multi-file SFC
//! resolution (`/tabs.vue` + two `.ts` deps) and assert:
//!
//! - `footprint.vfs_reads.len() <= 1` AND
//!   `footprint.truncation_counters.vfs_reads_truncated > 0`.
//! - `footprint.structured_events.len() <= 1` AND
//!   `footprint.truncation_counters.structured_events_truncated > 0`.
//!
//! **Pre-fix** (production site uses `::new()` → default 10_000 caps):
//! the resolution's vfs_reads + structured_events sit comfortably
//! under 10_000, no truncation fires, `*_truncated` stays zero. The
//! `assert > 0` assertions FAIL.
//!
//! **Post-fix** (production site uses
//! `::with_caps(self.config.audit_caps.clone())`): the host's caps
//! reach the accumulator, both lanes truncate immediately, both
//! `*_truncated` counters become non-zero. The assertions PASS.
//!
//! ## File injection vs pre-upsert
//!
//! The test bypasses `AuditedRequestBuilder::files(...)` — that helper
//! pre-`upsert`s files, which means the resolver hits the warm
//! scheduler cache and skips `workspace.read_file`. We need the cold
//! `ensure_loaded → scheduler → workspace.read_file` path so the
//! `SessionVfsSink` fans into the per-request accumulator and the
//! `vfs_reads` lane actually populates. Files are therefore injected
//! directly into a [`MemoryWorkspace`] and the host is built
//! manually, mirroring the existing
//! `tests/component_meta_audit/harness.rs::build_hermetic_host`
//! pattern.

use std::sync::Arc;

use verter_audit::AuditCaps;
use verter_session::audited_request::{AuditedRequest, AuditedRequestError};
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const TABS_VUE: &str = include_str!("../../test_fixtures/tabs.vue");
const TABS_TYPES_TS: &str = include_str!("../../test_fixtures/tabs_types.ts");
const TABS_HELPER_TS: &str = include_str!("../../test_fixtures/tabs_helper.ts");

/// Build a hermetic host with the given `audit_caps` and the tabs
/// fixture set injected directly into a [`MemoryWorkspace`]. Mirrors
/// the existing `component_meta_audit::harness::build_hermetic_host`
/// pattern so the resolver's first file touch goes through
/// `ensure_loaded → scheduler → workspace.read_file` (which fans into
/// `SessionVfsSink`), NOT through a pre-upsert warm path.
fn build_capped_host(audit_caps: AuditCaps) -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file("/tabs.vue".into(), Arc::from(TABS_VUE));
    workspace.inject_file("/tabs_types.ts".into(), Arc::from(TABS_TYPES_TS));
    workspace.inject_file("/tabs_helper.ts".into(), Arc::from(TABS_HELPER_TS));
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            audit_caps,
            ..HostConfig::default()
        },
        ws_access,
    ))
}

#[test]
fn host_config_audit_caps_reach_raw_push_lanes_via_production_accumulator() {
    // Configure tight caps on the two raw push lanes the resolution
    // is guaranteed to populate: `vfs_reads` (the entry SFC plus
    // each dep file) and `structured_events` (request markers +
    // dispatch / route events).
    //
    // `Some(1)` is intentionally tight: a successful `/tabs.vue`
    // resolution reads at least the entry plus one ts dep and
    // emits at least one structured event, both well above the
    // configured cap. The cap therefore truncates and the matching
    // `*_truncated` counter becomes non-zero post-fix.
    let host = build_capped_host(AuditCaps {
        vfs_reads: Some(1),
        structured_events: Some(1),
        ..AuditCaps::default()
    });

    let result = AuditedRequest::builder()
        .attach_to(host)
        .resolve_component_meta("/tabs.vue");

    let record = match result {
        Ok((_analysis, _resolution, record)) => record,
        Err(AuditedRequestError::ResolutionFailed) => panic!(
            "discriminator requires a successful resolution to populate raw push lanes; \
             /tabs.vue should resolve hermetically against the bundled test_fixtures \
             — re-check that the fixture set is intact"
        ),
        Err(other) => panic!(
            "unexpected audit error — audit-wiring regression, not a hermetic-dep gap: {other:?}"
        ),
    };

    let footprint = record
        .footprint
        .as_ref()
        .expect("footprint MUST attach when footprint_capture=true");

    // -- vfs_reads lane -------------------------------------------------
    // Post-fix: cap=1, resolution reads multiple files → at most 1
    // record kept, at least 1 record truncated.
    assert!(
        footprint.vfs_reads.len() <= 1,
        "vfs_reads cap should bound the surfaced records to <=1; got {} entries — \
         this means the production accumulator was constructed with default caps \
         (RequestFootprintAccumulator::new) instead of host caps \
         (RequestFootprintAccumulator::with_caps(config.audit_caps.clone()))",
        footprint.vfs_reads.len(),
    );
    assert!(
        footprint.truncation_counters.vfs_reads_truncated > 0,
        "vfs_reads_truncated must be > 0 when host configures audit_caps.vfs_reads = Some(1) \
         and the resolution reads multiple files — got {}. Counter staying at zero means \
         the production accumulator was constructed with default caps \
         (RequestFootprintAccumulator::new) instead of host caps \
         (RequestFootprintAccumulator::with_caps(config.audit_caps.clone()))",
        footprint.truncation_counters.vfs_reads_truncated,
    );

    // -- structured_events lane -----------------------------------------
    // Post-fix: cap=1, resolution emits multiple structured events
    // → at most 1 record kept, at least 1 record truncated.
    assert!(
        footprint.structured_events.len() <= 1,
        "structured_events cap should bound the surfaced records to <=1; got {} entries — \
         host caps did not reach the production accumulator construction site",
        footprint.structured_events.len(),
    );
    assert!(
        footprint.truncation_counters.structured_events_truncated > 0,
        "structured_events_truncated must be > 0 when host configures \
         audit_caps.structured_events = Some(1) and the resolution emits multiple events — \
         got {}. Counter staying at zero means host caps did not reach the production \
         accumulator construction site",
        footprint.truncation_counters.structured_events_truncated,
    );
}

#[test]
fn host_config_audit_caps_default_does_not_truncate_typical_resolution() {
    // Companion guard: with the host running on default
    // [`AuditCaps::default`] (every cap → 10_000), a typical
    // `/tabs.vue` resolution must NOT trigger any truncation. This
    // protects against a future refactor that accidentally tightens
    // the default caps or changes the cap-resolution order on the
    // production path.
    let host = build_capped_host(AuditCaps::default());

    let result = AuditedRequest::builder()
        .attach_to(host)
        .resolve_component_meta("/tabs.vue");

    let record = match result {
        Ok((_analysis, _resolution, record)) => record,
        Err(AuditedRequestError::ResolutionFailed) => {
            panic!("/tabs.vue must resolve hermetically against the bundled test_fixtures",)
        }
        Err(other) => panic!("unexpected audit error: {other:?}"),
    };

    let footprint = record
        .footprint
        .as_ref()
        .expect("footprint MUST attach when footprint_capture=true");

    // Discriminating: a default-caps resolution of a 3-file SFC
    // should sit comfortably under every 10_000 cap.
    assert_eq!(
        footprint.truncation_counters.vfs_reads_truncated, 0,
        "default caps (10_000) should never truncate a 3-file hermetic resolution",
    );
    assert_eq!(
        footprint.truncation_counters.structured_events_truncated, 0,
        "default caps (10_000) should never truncate a 3-file hermetic resolution",
    );
    assert_eq!(
        footprint.truncation_counters.materializations_truncated, 0,
        "default caps (10_000) should never truncate a 3-file hermetic resolution",
    );
}
