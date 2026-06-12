//! `impl VerterHost` — read-side scheduler / override-aware views.
//!
//! Contains:
//! - lock-free reads from the scheduler's `ArcSwap`-backed source,
//!   analysis, and artifact snapshots
//! - override-aware "effective" projections of file state, style
//!   analyses, and meta (profile content/style overrides merged on top
//!   of the raw scheduler reads)
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
    /// synthetic source, meta, script_analysis, and cached_parse. Otherwise
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
        canonical_id: &str,
        profile: Option<u64>,
    ) -> Option<EffectiveFileState> {
        use crate::host_executor::HostSourceData;

        let hd = snap.downcast_data::<HostSourceData>()?;

        if let Some(profile_hash) = profile {
            if let Some(cc) = self.compile_cache().get(canonical_id) {
                if let Some(ovr) = cc.content_overrides.get(&profile_hash) {
                    return Some(EffectiveFileState {
                        source: ovr.source.clone(),
                        meta: ovr.parse.meta.clone(),
                        script_analysis: ovr.parse.script_analysis.clone(),
                        cached_parse: ovr.cached_parse.clone(),
                        whole_hash: ovr.parse.whole_hash,
                    });
                }
            }
        }

        Some(EffectiveFileState {
            source: snap.source.clone(),
            meta: hd.parse.meta.clone(),
            script_analysis: hd.parse.script_analysis.clone(),
            cached_parse: hd.cached_parse.clone(),
            whole_hash: hd.parse.whole_hash,
        })
    }

    /// Override-aware style analyses for a profile.
    ///
    /// Merges per-index overrides from `StyleOverrideWithAnalysis` with raw
    /// style analyses from the scheduler. Returns `None` if file not in scheduler.
    #[allow(dead_code)] // Used by css_var_flow migration (upcoming)
    pub(crate) fn effective_style_analyses(
        &self,
        canonical_id: &str,
        profile: Option<u64>,
    ) -> Option<Vec<verter_semantic::analysis::StyleBlockAnalysis>> {
        use crate::host_executor::HostAnalysisData;

        let analysis_snap = self.scheduler.try_get_analysis(canonical_id)?;
        let ad = analysis_snap.downcast_data::<HostAnalysisData>()?;
        let raw = &ad.style_analyses;

        if let Some(profile_hash) = profile {
            if let Some(cc) = self.compile_cache().get(canonical_id) {
                if let Some(so) = cc.style_overrides.get(&profile_hash) {
                    let merged: Vec<_> = raw
                        .iter()
                        .enumerate()
                        .map(|(idx, raw_sa)| {
                            if let Some(Some(override_sa)) = so.analyses.get(idx) {
                                override_sa.clone()
                            } else {
                                raw_sa.clone()
                            }
                        })
                        .collect();
                    return Some(merged);
                }
            }
        }

        Some(raw.as_ref().clone())
    }

    /// Override-aware meta projection: applies `style_langs` overrides
    /// from `StyleOverrideWithAnalysis` over a CALLER-SUPPLIED base
    /// meta. The compile pipeline passes the meta from its single
    /// per-request source snapshot (see
    /// [`Self::effective_file_state_from_snapshot`]) so the effective
    /// meta cannot be derived from a newer source version than the
    /// compiled bytes.
    pub(crate) fn effective_meta_from_base(
        &self,
        mut meta: FileMeta,
        canonical_id: &str,
        profile: Option<u64>,
    ) -> FileMeta {
        if let Some(profile_hash) = profile {
            if let Some(cc) = self.compile_cache().get(canonical_id) {
                if let Some(so) = cc.style_overrides.get(&profile_hash) {
                    for (idx, lang) in so.lang_overrides.iter().enumerate() {
                        if let Some(ref l) = lang {
                            if idx < meta.style_langs.len() {
                                meta.style_langs[idx] = Some(l.clone());
                            }
                        }
                    }
                }
            }
        }

        meta
    }
}
