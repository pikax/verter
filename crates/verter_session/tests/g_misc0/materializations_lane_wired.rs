//! Discriminator: the `materializations` lane on the per-request
//! `RequestFootprintAccumulator` MUST contain at least one
//! `PreparedDeclBundle` subject after a hermetic resolution drives the
//! cold prepared-decl-bundle materialiser.
//!
//! ## Why this test exists
//!
//! Empirical investigation surfaced a footprint blind spot: every
//! production materialisation path (the two `prepared_decl_bundles`
//! cold producers in
//! `crates/verter_session/src/host_manage/prepared_decl.rs`) ran
//! without ever pushing a `MaterializationRecord` onto the
//! per-request accumulator. The `materializations` lane therefore
//! stayed empty on every audited request — even when the cold
//! prepared-decl-bundle path executed >5.9M times for a single
//! ChatMessages.vue resolution. Footprint-bench investigators
//! consequently had no per-envelope duration breakdown for the
//! dominant cost lane.
//!
//! ## Discriminating contract
//!
//! Build a hermetic host with `audit_enabled = true,
//! footprint_capture = true` and the bundled `/tabs.vue` +
//! `.ts` dep fixtures. Drive a `resolve_component_meta("/tabs.vue")`
//! call (the same audited entry the `audit_caps_wiring` discriminator
//! uses, which goes through the cold
//! `ensure_loaded → ensure_indexed_ready_serve → materialize_prepared_decl_bundle`
//! path). Assert:
//!
//! - `footprint.materializations.len() >= 1`
//! - at least one entry's `subject` is the
//!   `MaterializationSubject::PreparedDeclBundle { cold: true, .. }`
//!   variant
//!
//! ## Pre-fix behaviour (lane wiring absent)
//!
//! The two `materialize_prepared_decl_bundle_*` cold builders inserted
//! into `prepared_decl_bundles` but did NOT call
//! `record_materialization`. The accumulator's `materializations` Vec
//! therefore stayed empty across the resolution. Both assertions
//! FAIL.
//!
//! ## Post-fix behaviour (wiring landed)
//!
//! Both cold builders fence the materialisation envelope with
//! `Instant::now()` + `record_materialization(
//! MaterializationSubject::PreparedDeclBundle { canonical, cold:
//! true }, duration_ms)`. At least one `PreparedDeclBundle` subject
//! lands on the lane. Both assertions PASS.

use std::sync::Arc;

use verter_audit::AuditCaps;
use verter_session::audited_request::{AuditedRequest, AuditedRequestError};
use verter_session::component_meta_audit::MaterializationSubject;
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const TABS_VUE: &str = include_str!("../../test_fixtures/tabs.vue");
const TABS_TYPES_TS: &str = include_str!("../../test_fixtures/tabs_types.ts");
const TABS_HELPER_TS: &str = include_str!("../../test_fixtures/tabs_helper.ts");

/// Build a hermetic host with default caps + footprint capture on
/// and the bundled tabs fixture set injected directly into a
/// [`MemoryWorkspace`]. Mirrors the
/// `tests/audit_caps_wiring.rs::build_capped_host` pattern — files
/// land cold so the prepared-decl bundle path actually runs.
fn build_footprint_host() -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file("/tabs.vue".into(), Arc::from(TABS_VUE));
    workspace.inject_file("/tabs_types.ts".into(), Arc::from(TABS_TYPES_TS));
    workspace.inject_file("/tabs_helper.ts".into(), Arc::from(TABS_HELPER_TS));
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            audit_caps: AuditCaps::default(),
            ..HostConfig::default()
        },
        ws_access,
    ))
}

#[test]
fn materializations_lane_contains_prepared_decl_bundle_subjects() {
    let host = build_footprint_host();

    let result = AuditedRequest::builder()
        .attach_to(host)
        .resolve_component_meta("/tabs.vue");

    let record = match result {
        Ok((_analysis, _resolution, record)) => record,
        Err(AuditedRequestError::ResolutionFailed) => panic!(
            "discriminator requires a successful resolution to drive the cold \
             prepared-decl-bundle path; /tabs.vue must resolve hermetically against \
             the bundled test_fixtures"
        ),
        Err(other) => panic!(
            "unexpected audit error — audit-wiring regression, not a hermetic-dep gap: {other:?}"
        ),
    };

    let footprint = record
        .footprint
        .as_ref()
        .expect("footprint MUST attach when footprint_capture=true");

    // Discriminating assertion #1: the `materializations` Vec is
    // non-empty. Pre-fix: every cold prepared-decl-bundle build
    // returned without recording a materialization envelope, so this
    // count stayed at zero.
    assert!(
        !footprint.materializations.is_empty(),
        "materializations lane must be non-empty after a cold resolution \
         — got 0 entries; pre-fix, the two `materialize_prepared_decl_bundle*` \
         cold builders inserted into `prepared_decl_bundles` but never called \
         `record_materialization`. The wiring contract pushes a \
         `MaterializationSubject::PreparedDeclBundle {{ cold: true, .. }}` \
         entry from both cold paths. Counter staying at zero means the \
         wiring regressed."
    );

    // Discriminating assertion #2: at least one entry is the
    // `PreparedDeclBundle` subject (not just any pre-existing
    // subject — the wiring contract is specifically about this
    // variant).
    let prepared_decl_bundle_count = footprint
        .materializations
        .iter()
        .filter(|m| {
            matches!(
                &m.subject,
                MaterializationSubject::PreparedDeclBundle { cold: true, .. }
            )
        })
        .count();
    let subject_dump: Vec<String> = footprint
        .materializations
        .iter()
        .map(|m| format!("{:?}", m.subject))
        .collect();
    assert!(
        prepared_decl_bundle_count >= 1,
        "at least one `MaterializationSubject::PreparedDeclBundle {{ cold: true, .. }}` \
         entry must appear in the `materializations` lane after a cold resolution — \
         got {prepared_decl_bundle_count} entries. The two production cold producers \
         are `materialize_prepared_decl_bundle_from_routed_shallow` and \
         `materialize_prepared_decl_bundle` in \
         `crates/verter_session/src/host_manage/prepared_decl.rs`; both MUST call \
         `record_materialization` at the cold-build exit. \
         Got materializations vec: {subject_dump:#?}"
    );

    // Bound: the cold prepared-decl-bundle is keyed per canonical,
    // so for a 3-file hermetic resolution the count is at most a
    // small multiple of the file count. This catches a regression
    // where the wiring fires on every warm read (which would push
    // hundreds of records and would itself be a defect).
    assert!(
        prepared_decl_bundle_count <= 64,
        "PreparedDeclBundle materialisation count exceeded a sanity bound — \
         the wiring must fire only on cold builds, not on warm reads. \
         Got {prepared_decl_bundle_count} entries for a 3-file resolution; \
         expected a small multiple of the file count."
    );
}
