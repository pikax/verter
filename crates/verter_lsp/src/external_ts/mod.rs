//! The tsserver external-TypeScript-engine backend (LSP side).
//!
//! This module owns the Rust publish authority for the tsserver engine: the on-disk
//! content-addressed carrier-snapshot store + atomic manifest
//! ([`carrier_publish_store`]) that the Node `@verter/typescript-plugin` reads
//! synchronously, and the [`EngineBackend`](verter_session::external_ts::EngineBackend)
//! implementation ([`tsserver_backend`]) that drives it.
//!
//! The contract layer (the seam, DTOs, the witness type-state) lives in
//! `verter_session::external_ts`; this module is the LSP-side CONCRETE backend that
//! consumes the project-bound sync seam's `PublishSnapshot` DTOs and writes the
//! on-disk mirror.

pub mod carrier_publish_store;
pub mod carrier_sync;
pub mod membership_ledger;
pub mod membership_reconciler;
pub mod publish_coordinator;
pub mod tsgo_backend;
pub mod tsserver_backend;

pub use carrier_publish_store::{
    carrier_store_dir_for, default_carrier_store_dir_string, default_carrier_store_host_version,
    CarrierPublishStore, Manifest, ManifestRole, ManifestScriptKind, OwnedSource, ProjectEntry,
    PublishBatch, ReadyFile,
};

#[cfg(test)]
pub use carrier_publish_store::test_store_dir_override;
pub(crate) use carrier_sync::{
    carrier_close_target, project_ownership_diagnostics_for, reconcile_carrier_source,
    AdmitOutcome, CarrierMembershipCtx, CarrierNotOwned, CarrierProviderDelivery,
    CarrierSyncDecision, CarrierSyncRequest, CarrierTransactionCoordinator, SettleClass,
};
pub(crate) use publish_coordinator::resolve_carrier_ownership_over_vfs;
pub use publish_coordinator::{CarrierCompanion, CarrierPublishCoordinator, CarrierPublishError};
pub use tsgo_backend::TsgoEngineBackend;
pub use tsserver_backend::TsserverEngineBackend;

pub use membership_ledger::{
    AbsentReason, CanonicalSource, LedgerCommitError, LedgerCompanion, MembershipLedger,
    MembershipRecord, ProjectUri, SessionGen,
};
pub use membership_reconciler::{
    BootstrapKind, BootstrapState, CarrierMembershipCommitter, CommitErr, CommitFuture,
    CompanionFingerprint, DesiredMembership, MembershipReconciler, PendingProviderReady,
    ProviderGeneration, ProviderReadyReceipt, ReconcileErr, ReconcileOutcome, ReconcileReason,
};
