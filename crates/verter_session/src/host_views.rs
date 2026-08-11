//! `impl VerterHost` — read-side scheduler views.
//!
//! Contains:
//! - lock-free reads from the scheduler's `ArcSwap`-backed source,
//!   analysis, and artifact snapshots
//! - request-pinned projections of raw file state, style analyses, and meta
//!
//! These methods do not mutate the scheduler or compile cache; they are
//! the read surface consumers use to assemble compile inputs.

use std::sync::Arc;

use crate::host_executor;
use crate::types::{DiagnosticsSnapshot, EffectiveFileState, FileMeta};
use crate::VerterHost;

impl VerterHost {
    /// Get the scheduler instance.
    pub fn scheduler(&self) -> &Arc<verter_scheduler::scheduler::Scheduler> {
        &self.scheduler
    }

    /// Get the scheduler's source snapshot for a file.
    ///
    /// Returns `None` if the file hasn't been upserted or the snapshot is stale.
    /// This is a lock-free `ArcSwap` read — no contention with upsert/compile.
    pub fn scheduler_source(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<verter_scheduler::node::SourceSnapshot>> {
        self.scheduler.try_get_source(canonical_id)
    }

    /// Clone the sole registered envelope owner for a committed carrier source.
    #[doc(hidden)]
    pub fn registered_file_structure(
        &self,
        canonical_id: &str,
    ) -> Option<crate::carrier_publication_store::RegisteredFileStructure> {
        let canonical_id = self.resolve_alias_or_canonical(canonical_id);
        self.scheduler
            .try_get_source(&canonical_id)?
            .downcast_data::<host_executor::HostSourceData>()?
            .structure
            .clone()
    }

    /// Clone the structure and revision stamp from one committed source record.
    #[doc(hidden)]
    pub fn registered_file_structure_snapshot(
        &self,
        canonical_id: &str,
    ) -> Option<(
        crate::carrier_publication_store::RegisteredFileStructure,
        crate::carrier_publication_store::HostSourceRevisionToken,
    )> {
        let canonical_id = self.resolve_alias_or_canonical(canonical_id);
        let source = self.scheduler.try_get_source(&canonical_id)?;
        let data = source.downcast_data::<host_executor::HostSourceData>()?;
        Some((data.structure.clone()?, data.revision_token))
    }

    /// Return the projection host's local revision stamp for the same Source-stage
    /// record that owns the registered carrier structure.
    #[doc(hidden)]
    pub fn registered_source_revision_token(
        &self,
        canonical_id: &str,
    ) -> Option<crate::carrier_publication_store::HostSourceRevisionToken> {
        let canonical_id = self.resolve_alias_or_canonical(canonical_id);
        Some(
            self.scheduler
                .try_get_source(&canonical_id)?
                .downcast_data::<host_executor::HostSourceData>()?
                .revision_token,
        )
    }

    /// Return the content-free schema-8 projection for one committed carrier.
    /// The projection is derived solely from the registered envelope.
    pub fn ordered_sfc_structure(
        &self,
        canonical_id: &str,
    ) -> Option<verter_semantic::analysis::component_meta::OrderedSfcStructureAnalysis> {
        let structure = self.registered_file_structure(canonical_id)?;
        Some(crate::host_resolve::ordered_sfc_structure_analysis(
            &structure,
        ))
    }

    /// Get the scheduler's analysis snapshot for a file.
    pub fn scheduler_analysis(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<verter_scheduler::node::AnalysisSnapshot>> {
        self.scheduler.try_get_analysis(canonical_id)
    }

    /// Get export signatures from the scheduler's analysis snapshot.
    ///
    /// This is the lock-free read path — returns data from the scheduler's
    /// `ArcSwap` snapshots without touching any host RwLock.
    pub fn scheduler_export_signatures(
        &self,
        canonical_id: &str,
    ) -> Option<Vec<verter_semantic::analysis::ExportSignature>> {
        let snap = self.scheduler.try_get_analysis(canonical_id)?;
        let data = snap.downcast_data::<host_executor::HostAnalysisData>()?;
        Some(data.export_signatures.clone())
    }

    /// Get script analysis from the scheduler's analysis snapshot.
    ///
    /// Returns an `Arc::clone` of the shared snapshot — a refcount bump, not a
    /// deep copy of the ~18 owned vectors. Callers that need an owned copy call
    /// `.as_ref().clone()`.
    pub fn scheduler_script_analysis(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<verter_semantic::analysis::ScriptAnalysisSnapshot>> {
        let snap = self.scheduler.try_get_analysis(canonical_id)?;
        let data = snap.downcast_data::<host_executor::HostAnalysisData>()?;
        Some(Arc::clone(&data.script_analysis))
    }

    /// Get compiled virtual files from the scheduler's artifact snapshot.
    ///
    /// Returns the compile output for a specific profile if available.
    #[allow(dead_code)] // Tested via scheduler_tests; LSP will call once migrated
    pub(crate) fn scheduler_artifact_outputs(
        &self,
        canonical_id: &str,
        profile_hash: u64,
    ) -> Option<rustc_hash::FxHashMap<crate::types::VirtualNodeKind, crate::types::CachedVirtualFile>>
    {
        let snap = self
            .scheduler
            .try_get_artifact(canonical_id, profile_hash)?;
        let data = snap.downcast_data::<host_executor::HostArtifactData>()?;
        Some(data.outputs.clone())
    }

    /// Get artifact diagnostics from the scheduler's artifact snapshot.
    #[allow(dead_code)] // Tested via scheduler_tests; LSP will call once migrated
    pub(crate) fn scheduler_artifact_diagnostics(
        &self,
        canonical_id: &str,
        profile_hash: u64,
    ) -> Option<DiagnosticsSnapshot> {
        let snap = self
            .scheduler
            .try_get_artifact(canonical_id, profile_hash)?;
        let data = snap.downcast_data::<host_executor::HostArtifactData>()?;
        Some(data.diagnostics.clone())
    }

    /// Get style analyses from the scheduler's analysis snapshot.
    pub fn scheduler_style_analyses(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<Vec<verter_semantic::analysis::StyleBlockAnalysis>>> {
        let snap = self.scheduler.try_get_analysis(canonical_id)?;
        let data = snap.downcast_data::<host_executor::HostAnalysisData>()?;
        Some(Arc::clone(&data.style_analyses))
    }

    /// Override-aware file state for a profile.
    ///
    /// When a content override exists for `profile`, returns the override's
    /// synthetic source, meta, script_analysis, and framework_parse. Otherwise
    /// returns raw scheduler data. Returns `None` if file not in scheduler.
    pub(crate) fn effective_file_state(
        &self,
        canonical_id: &str,
        profile: Option<u64>,
    ) -> Option<EffectiveFileState> {
        let snap = self.scheduler.try_get_source(canonical_id)?;
        self.effective_file_state_from_snapshot(&snap, canonical_id, profile)
    }

    /// [`Self::effective_file_state`] over a CALLER-HELD source
    /// snapshot.
    ///
    /// The compile pipeline reads the scheduler source exactly once
    /// per request and derives every content-determined compile input
    /// — the compiled bytes, the script analysis, and the
    /// `whole_hash` that keys a `Content`-mode publish — from that
    /// single snapshot. An independent re-read here could observe a
    /// newer source version than the rest of the request, pairing
    /// bytes from one content version with the key hash of another.
    /// Returns `None` when the snapshot carries no host source data.
    pub(crate) fn effective_file_state_from_snapshot(
        &self,
        snap: &Arc<verter_scheduler::node::SourceSnapshot>,
        _canonical_id: &str,
        profile: Option<u64>,
    ) -> Option<EffectiveFileState> {
        use crate::host_executor::HostSourceData;

        let hd = snap.downcast_data::<HostSourceData>()?;

        let _ = profile;

        // One effective-file-state projection duplicates the whole parse
        // snapshot's analysis surface per call.
        verter_audit::attribute!(AnalysisSnapshotCopy);
        Some(EffectiveFileState {
            source: snap.source.clone(),
            meta: hd.parse.meta.clone(),
            script_analysis: hd.parse.script_analysis.clone(),
            framework_parse: hd.framework_parse.clone(),
            whole_hash: hd.parse.whole_hash,
        })
    }

    /// Raw style analyses from the scheduler.
    ///
    /// Processed block content is compiler input and never mutates the carrier's
    /// authored analysis. Returns `None` if the file is not in the scheduler.
    #[allow(dead_code)] // Used by css_var_flow migration (upcoming)
    pub(crate) fn effective_style_analyses(
        &self,
        canonical_id: &str,
        _profile: Option<u64>,
    ) -> Option<Vec<verter_semantic::analysis::StyleBlockAnalysis>> {
        use crate::host_executor::HostAnalysisData;

        let analysis_snap = self.scheduler.try_get_analysis(canonical_id)?;
        let ad = analysis_snap.downcast_data::<HostAnalysisData>()?;
        let raw = &ad.style_analyses;

        Some(raw.as_ref().clone())
    }

    /// Return the carrier meta from the request's pinned source snapshot.
    /// Processed block metadata is projected directly into compiler inputs and
    /// never mutates or overlays this authored carrier metadata.
    pub(crate) fn effective_meta_from_base(
        &self,
        meta: FileMeta,
        _canonical_id: &str,
        _profile: Option<u64>,
    ) -> FileMeta {
        meta
    }
}
