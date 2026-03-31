//! Host stage executor — connects the scheduler to real parse/analysis/compile logic.
//!
//! Implements [`StageExecutor`] by calling into the host's existing parse pipeline.
//! Each stage produces a snapshot with host-specific data that can be downcast
//! by the host facade.

#[cfg(feature = "scheduler")]
use std::sync::Arc;

#[cfg(feature = "scheduler")]
use verter_scheduler::executor::{ExtractedDeps, StageError, StageExecutor};
#[cfg(feature = "scheduler")]
use verter_scheduler::node::{
    AnalysisSnapshot, ArtifactSnapshot, EmptyData, FileKind, SnapshotData, SourceSnapshot,
};

#[cfg(feature = "scheduler")]
use crate::types::{HostConfig, ParseSnapshot};

/// Host-specific data stored in a [`SourceSnapshot`].
///
/// Wraps a `ParseSnapshot` — the result of SFC tokenization, hashing, and analysis.
/// Also carries the cached parsed SFC (for Vue files), the host-level file kind,
/// and the measured parse duration for performance tracking.
#[cfg(feature = "scheduler")]
#[derive(Debug)]
#[allow(dead_code)] // Fields read progressively during Phase 2-3 migration
pub struct HostSourceData {
    pub(crate) parse: ParseSnapshot,
    /// Cached parsed SFC from `parse_vue_snapshot`. Reused during compilation
    /// to avoid re-parsing. `None` for non-SFC files.
    pub(crate) cached_parse: Option<Arc<verter_compiler::parser::types::ParsedSfc>>,
    /// Discriminates Vue SFC vs non-SFC (host-level enum, not scheduler's).
    pub(crate) file_kind: crate::types::FileKind,
    /// Wall-clock parse duration in milliseconds.
    pub(crate) parse_duration_ms: f64,
}

#[cfg(feature = "scheduler")]
impl SnapshotData for HostSourceData {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Pre-computed Arc-wrapped views of immutable `ScriptAnalysisSnapshot` fields.
///
/// Built once during `execute_analysis()` from the parse results. These fields
/// are never mutated after construction, so Arc sharing across all `get_analysis()`
/// calls is safe and avoids repeated cloning.
#[cfg(feature = "scheduler")]
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields read in Phase 2A get_analysis migration
pub struct AnalysisArcs {
    pub(crate) module_references: Arc<Vec<verter_semantic::analysis::AnalyzedModuleReference>>,
    pub(crate) macros: Arc<Vec<verter_semantic::analysis::AnalyzedMacro>>,
    pub(crate) macro_type_deps: Arc<Vec<verter_semantic::analysis::MacroTypeDep>>,
    pub(crate) vue_api_calls: Arc<Vec<verter_semantic::analysis::types::VueApiCallSite>>,
    pub(crate) dom_query_calls: Arc<Vec<verter_semantic::analysis::types::DomQueryCallSite>>,
    pub(crate) css_var_manipulations:
        Arc<Vec<verter_semantic::analysis::types::CssVarManipulation>>,
    pub(crate) script_binding_occurrences:
        Arc<Vec<verter_semantic::analysis::types::ScriptBindingOccurrence>>,
    pub(crate) store_usages: Arc<Vec<verter_semantic::analysis::types::StoreUsage>>,
    pub(crate) store_definitions: Arc<Vec<verter_semantic::analysis::types::StoreDefinition>>,
}

#[cfg(feature = "scheduler")]
impl AnalysisArcs {
    /// Build Arc-wrapped caches from a script analysis snapshot.
    pub(crate) fn from_analysis(sa: &verter_semantic::analysis::ScriptAnalysisSnapshot) -> Self {
        Self {
            module_references: Arc::new(sa.module_references.clone()),
            macros: Arc::new(sa.macros.clone()),
            macro_type_deps: Arc::new(sa.macro_type_deps.clone()),
            vue_api_calls: Arc::new(sa.vue_api_calls.clone()),
            dom_query_calls: Arc::new(sa.dom_query_calls.clone()),
            css_var_manipulations: Arc::new(sa.css_var_manipulations.clone()),
            script_binding_occurrences: Arc::new(sa.script_binding_occurrences.clone()),
            store_usages: Arc::new(sa.store_usages.clone()),
            store_definitions: Arc::new(sa.store_definitions.clone()),
        }
    }
}

/// Host-specific data stored in an [`AnalysisSnapshot`].
///
/// Contains real script analysis, export signatures, style analyses, and
/// pre-computed Arc-wrapped analysis fields for cheap sharing.
#[cfg(feature = "scheduler")]
#[derive(Debug)]
#[allow(dead_code)] // arcs field read in Phase 2A get_analysis migration
pub struct HostAnalysisData {
    pub(crate) script_analysis: verter_semantic::analysis::ScriptAnalysisSnapshot,
    pub(crate) export_signatures: Vec<verter_semantic::analysis::ExportSignature>,
    pub(crate) style_analyses: Arc<Vec<verter_semantic::analysis::StyleBlockAnalysis>>,
    pub(crate) arcs: AnalysisArcs,
}

#[cfg(feature = "scheduler")]
impl SnapshotData for HostAnalysisData {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Host-specific data stored in an [`ArtifactSnapshot`].
///
/// Contains compiled virtual files and diagnostics from a compile slot.
/// Fields are accessed via `downcast_data::<HostArtifactData>()` which the
/// compiler's static analysis cannot see through.
#[cfg(feature = "scheduler")]
#[derive(Debug)]
#[allow(dead_code)] // Read via Any::downcast_ref in scheduler_artifact_outputs/diagnostics
pub struct HostArtifactData {
    pub(crate) outputs:
        rustc_hash::FxHashMap<crate::types::VirtualNodeKind, crate::types::CachedVirtualFile>,
    pub(crate) diagnostics: crate::types::DiagnosticsSnapshot,
}

#[cfg(feature = "scheduler")]
impl SnapshotData for HostArtifactData {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Stage executor that calls into the host's parse pipeline.
///
/// For the Source stage, calls `parse_vue_snapshot` or `parse_non_sfc_snapshot`.
/// Analysis and Artifact stages delegate back to the host facade (the scheduler
/// provides coordination, the host provides domain logic).
#[cfg(feature = "scheduler")]
pub struct HostStageExecutor {
    pub config: HostConfig,
}

#[cfg(feature = "scheduler")]
impl HostStageExecutor {
    pub fn new(config: HostConfig) -> Self {
        Self { config }
    }
}

#[cfg(feature = "scheduler")]
impl StageExecutor for HostStageExecutor {
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn execute_source(
        &self,
        canonical_id: &str,
        file_kind: FileKind,
        content: Arc<str>,
        generation: u64,
    ) -> Result<SourceSnapshot, StageError> {
        let is_vue = matches!(file_kind, FileKind::VueSfc);
        let host_file_kind = match file_kind {
            FileKind::VueSfc => crate::types::FileKind::VueSfc,
            FileKind::NonSfc => crate::types::FileKind::NonSfc,
        };

        let parse_start = std::time::Instant::now();

        if is_vue {
            let (parse_snapshot, parsed_sfc) = crate::parse::parse_vue_snapshot(
                canonical_id,
                &content,
                self.config.effective_scope(),
            );
            let parse_duration_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
            Ok(SourceSnapshot {
                source: content,
                whole_hash: parse_snapshot.whole_hash,
                semantic_hash: parse_snapshot.semantic_hash,
                generation,
                data: Arc::new(HostSourceData {
                    parse: parse_snapshot,
                    cached_parse: Some(Arc::new(parsed_sfc)),
                    file_kind: host_file_kind,
                    parse_duration_ms,
                }),
            })
        } else {
            let parse_snapshot = crate::parse::parse_non_sfc_snapshot(canonical_id, &content);
            let parse_duration_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
            Ok(SourceSnapshot {
                source: content,
                whole_hash: parse_snapshot.whole_hash,
                semantic_hash: parse_snapshot.semantic_hash,
                generation,
                data: Arc::new(HostSourceData {
                    parse: parse_snapshot,
                    cached_parse: None,
                    file_kind: host_file_kind,
                    parse_duration_ms,
                }),
            })
        }
    }

    fn extract_deps(&self, canonical_id: &str, source: &SourceSnapshot) -> ExtractedDeps {
        let host_data = match source.downcast_data::<HostSourceData>() {
            Some(d) => d,
            None => return ExtractedDeps::default(),
        };
        let parse = &host_data.parse;

        let mut forward_deps = Vec::new();
        let mut blocker_ids = Vec::new();

        // Forward deps from external src blocks (e.g. <script src="./setup.ts">).
        for req in &parse.external_requests {
            forward_deps.push(req.resolved_canonical_id.clone());
        }

        // Forward deps from relative imports.
        for imp in &parse.script_analysis.imports {
            if imp.source.starts_with('.') || imp.source.starts_with("../") {
                let resolved = crate::id::resolve_external(canonical_id, &imp.source);
                forward_deps.push(resolved);
            }
        }

        // Blocker IDs from macro type deps (defineProps<ExternalType>(), etc.).
        // These files must reach Analysis before this file's Artifact can proceed,
        // because the Artifact stage needs resolved type shapes for codegen.
        for dep in &parse.script_analysis.macro_type_deps {
            if dep.import_source.starts_with('.') || dep.import_source.starts_with("../") {
                let resolved = crate::id::resolve_external(canonical_id, &dep.import_source);
                blocker_ids.push(resolved);
            }
            // Bare specifier deps (e.g. "motion") are resolved via the workspace
            // resolver, not here. They'll be handled when exact resolutions are set.
        }

        ExtractedDeps {
            forward_deps,
            blocker_ids,
        }
    }

    fn execute_analysis(
        &self,
        _canonical_id: &str,
        source: &SourceSnapshot,
        generation: u64,
    ) -> Result<AnalysisSnapshot, StageError> {
        // In the current host architecture, analysis is computed during parse.
        // Extract the real data from HostSourceData and commit it in the
        // AnalysisSnapshot so read-side consumers get real payloads.
        if let Some(host_data) = source.downcast_data::<HostSourceData>() {
            let arcs = AnalysisArcs::from_analysis(&host_data.parse.script_analysis);
            Ok(AnalysisSnapshot {
                generation,
                data: Arc::new(HostAnalysisData {
                    script_analysis: host_data.parse.script_analysis.clone(),
                    export_signatures: host_data.parse.export_signatures.clone(),
                    style_analyses: Arc::new(host_data.parse.style_analyses.clone()),
                    arcs,
                }),
            })
        } else {
            Ok(AnalysisSnapshot::new_empty(generation))
        }
    }

    fn execute_artifact(
        &self,
        _canonical_id: &str,
        _source: &SourceSnapshot,
        _analysis: &AnalysisSnapshot,
        profile_hash: u64,
        generation: u64,
    ) -> Result<ArtifactSnapshot, StageError> {
        // Compilation requires the full host context (CompileProfile, external sources).
        // The host facade handles compilation directly using scheduler snapshots as input.
        Ok(ArtifactSnapshot {
            generation,
            profile_hash,
            data: Arc::new(EmptyData),
        })
    }
}
