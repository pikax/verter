//! Shared harness for the 25 fact-matrix slices.
//!
//! Each slice consumes:
//! - `make_host(canonical, source)` — build a hermetic host with a
//!   workspace-backed canonical file at the requested path.
//! - `read_app_config_proof_installs(host)` — observability counter
//!   read for the AppConfigNoOverrideProofDb producer.
//! - `read_materialize_structure_installs(host)` — same for
//!   `MaterializeStructureDb`.
//! - `read_ref_cycle_installs(host)` — same for `RefCycleResultDb`.
//! - `read_memo_entry_installs(host)` — same for the memo
//!   (`SemanticGraphStore::execute_cooperative` cold builds).
//! - `read_owner_import_surface_installs(host)` — same for
//!   `OwnerImportSurfaceDb`.

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

pub fn make_host(canonical: &str, source: &str) -> Arc<VerterHost> {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });
    let _ = host.analyze_with_audit(canonical);
    host
}

pub fn read_app_config_proof_installs(host: &VerterHost) -> u64 {
    host.provenance()
        .app_config_proof_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed)
}

pub fn read_materialize_structure_installs(host: &VerterHost) -> u64 {
    host.provenance()
        .materialize_structure_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed)
}

pub fn read_ref_cycle_installs(host: &VerterHost) -> u64 {
    host.provenance()
        .ref_cycle_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed)
}

pub fn read_memo_entry_installs(host: &VerterHost) -> u64 {
    host.provenance()
        .memo_entry_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed)
}

pub fn read_owner_import_surface_installs(host: &VerterHost) -> u64 {
    host.provenance()
        .owner_import_surface_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed)
}
