//! The public host's ACTIVE-occupancy window reads the one aggregate
//! account the host's semantic workload charges: a workload's retained
//! charges and an ACTIVE charge on the host's account land on the account
//! the window and the host's retention snapshot read, and an open window
//! changes no answer.

use std::sync::{Arc, Mutex};

use verter_session::{HostConfig, UpsertRequest, VerterHost};
use verter_session_query::retention::active_window::ActiveWitnessRejection;
use verter_session_query::retention::{
    ChargeClass, SemanticRetentionAccount, ACTIVE_OCCUPANCY_WINDOW_AVAILABLE,
};

/// Every host charges the one process account, so tests that read its ACTIVE
/// transitions run one at a time (other tests in this binary reserve no
/// ACTIVE bytes).
static PROCESS_ACCOUNT: Mutex<()> = Mutex::new(());

const ICON: &str = "/src/types/icon.ts";

// The test build reports the window it carries.
const _: () = assert!(ACTIVE_OCCUPANCY_WINDOW_AVAILABLE);

fn host() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(ICON.to_string()),
        input_id: ICON.to_string(),
        source: Arc::from(
            "export interface IconProps { name: string; size: number }\n\
             export interface IconBox { props: IconProps; label: string }\n",
        ),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static(ICON)
            .static_resolution(),
        aliases: Vec::new(),
    });
    host
}

/// The ordinary public request; its answer rendered for comparison.
fn request_icon_box(host: &VerterHost) -> String {
    let graph = host.project_type_store().semantic_graph();
    let answer = host
        .resolve_named_symbol(ICON, "IconBox", None)
        .expect("IconBox resolves");
    let data = graph.node_data(answer.id()).expect("the answer is live");
    format!("{data:?}")
}

#[test]
fn a_host_window_measures_the_account_its_semantic_workload_charges() {
    let _serial = PROCESS_ACCOUNT.lock().unwrap_or_else(|e| e.into_inner());
    let host = host();
    let account = Arc::clone(host.project_type_store().retention_account());
    assert!(
        Arc::ptr_eq(&account, &SemanticRetentionAccount::process_local()),
        "a public host charges the one process-local account"
    );
    let before = host.retention_snapshot();
    let owner = host.mint_active_occupancy_owner();
    let window = host.open_active_occupancy_window(&owner);
    let measured = request_icon_box(&host);
    let after_workload = host.retention_snapshot();
    assert!(
        after_workload.retained_bytes > before.retained_bytes,
        "the workload charged the account the window reads"
    );
    let held = account
        .reserve_as(&owner, ChargeClass::Active, 4_096)
        .admitted()
        .expect("active admitted");
    assert_eq!(
        host.retention_snapshot().active_bytes,
        after_workload.active_bytes + 4_096
    );
    assert_eq!(window.observe().active_at_close, 4_096);
    drop(held);
    let report = window.finish();
    assert_eq!(
        report.active_at_open + report.rise_bytes - report.fall_bytes,
        report.active_at_close
    );
    let peak = report
        .attributed()
        .expect("a workload alone on the account is attributable");
    assert!(peak.crossed(4_096));

    assert_eq!(
        request_icon_box(&self::host()),
        measured,
        "an open window changes no answer"
    );
}

/// Foreign ACTIVE work that starts and ends strictly inside the host window,
/// by another owner or by an untagged charge, never satisfies the measured
/// owner's crossing check: an identified owner is excluded, an unknown one
/// rejects the witness.
#[test]
fn a_host_window_excludes_foreign_work_and_rejects_unknown_provenance() {
    let _serial = PROCESS_ACCOUNT.lock().unwrap_or_else(|e| e.into_inner());
    let host = host();
    let account = Arc::clone(host.project_type_store().retention_account());
    let (owner, foreign) = (
        host.mint_active_occupancy_owner(),
        host.mint_active_occupancy_owner(),
    );

    let window = host.open_active_occupancy_window(&owner);
    drop(
        account
            .reserve_as(&foreign, ChargeClass::Active, 1 << 20)
            .admitted()
            .expect("active admitted"),
    );
    let _ = request_icon_box(&host);
    let report = window.finish();
    let peak = report
        .attributed()
        .expect("identified foreign work is excluded");
    assert!(!peak.crossed(1));

    let window = host.open_active_occupancy_window(&owner);
    drop(
        account
            .reserve(ChargeClass::Active, 512)
            .admitted()
            .expect("active admitted"),
    );
    let report = window.finish();
    assert!(matches!(
        report.attributed(),
        Err(ActiveWitnessRejection::UnknownProvenance { transitions: 2 })
    ));
}
