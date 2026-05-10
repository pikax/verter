//! Host stage executor — connects the scheduler to real parse/analysis/compile logic.
//!
//! Implements [`StageExecutor`] by calling into the host's existing parse pipeline.
//! Each stage produces a snapshot with host-specific data that can be downcast
//! by the host facade.

use std::sync::Arc;

use crate::instant::Instant;

use verter_scheduler::executor::{ExtractedDeps, StageError, StageExecutor};
use verter_scheduler::node::{
    AnalysisSnapshot, ArtifactSnapshot, EmptyData, FileKind, SnapshotData, SourceSnapshot,
};

use crate::types::{HostConfig, ParseSnapshot};

/// Host-specific data stored in a [`SourceSnapshot`].
///
/// Wraps a `ParseSnapshot` — the result of SFC tokenization, hashing, and analysis.
/// Also carries the cached parsed SFC (for Vue files), the host-level file kind,
/// the authoritative `source_type` computed once at parse time, and the measured
/// parse duration for performance tracking.
#[derive(Debug)]
#[allow(dead_code)] // Fields read progressively during 3 migration
pub struct HostSourceData {
    pub(crate) parse: ParseSnapshot,
    /// Cached parsed SFC from `parse_vue_snapshot`. Reused during compilation
    /// to avoid re-parsing. `None` for non-SFC files.
    pub(crate) cached_parse: Option<Arc<verter_compiler::parser::types::ParsedSfc>>,
    /// Discriminates Vue SFC vs non-SFC (host-level enum, not scheduler's).
    pub(crate) file_kind: crate::types::FileKind,
    /// Authoritative `SourceType` for downstream type-resolution cache keys,
    /// computed once during `execute_source` with full access to the parsed
    /// SFC. Readers must prefer this value over recomputing from raw source +
    /// `cached_parse` (which is unstable when `cached_parse` is dropped).
    pub(crate) source_type: oxc_span::SourceType,
    /// Wall-clock parse duration in milliseconds.
    pub(crate) parse_duration_ms: f64,
}

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
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields are part of the get_analysis surface.
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
#[derive(Debug)]
#[allow(dead_code)] // arcs field is part of the get_analysis surface.
pub struct HostAnalysisData {
    pub(crate) script_analysis: verter_semantic::analysis::ScriptAnalysisSnapshot,
    pub(crate) export_signatures: Vec<verter_semantic::analysis::ExportSignature>,
    pub(crate) style_analyses: Arc<Vec<verter_semantic::analysis::StyleBlockAnalysis>>,
    pub(crate) arcs: AnalysisArcs,
}

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
#[derive(Debug)]
#[allow(dead_code)] // Read via Any::downcast_ref in scheduler_artifact_outputs/diagnostics
pub struct HostArtifactData {
    pub(crate) outputs:
        rustc_hash::FxHashMap<crate::types::VirtualNodeKind, crate::types::CachedVirtualFile>,
    pub(crate) diagnostics: crate::types::DiagnosticsSnapshot,
}

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
pub struct HostStageExecutor {
    pub config: HostConfig,
    pub workspace: Arc<parking_lot::RwLock<Arc<dyn verter_workspace::WorkspaceAccess>>>,
}

impl HostStageExecutor {
    pub fn new(
        config: HostConfig,
        workspace: Arc<parking_lot::RwLock<Arc<dyn verter_workspace::WorkspaceAccess>>>,
    ) -> Self {
        Self { config, workspace }
    }
}

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

        // Per-file timing capture is gated on the active request's
        // `audit_timing_capture` flag. The host's own `parse_duration_ms`
        // on `HostSourceData` keeps its existing semantics; the audit
        // ledger push happens only when timing capture is on AND a
        // request context is installed.
        let timing_on = verter_scheduler::request_context::current_timing_enabled();

        let parse_start = Instant::now();

        let snapshot = if is_vue {
            let (parse_snapshot, parsed_sfc) = crate::parse::parse_vue_snapshot(
                canonical_id,
                &content,
                self.config.effective_scope(),
            );
            let parse_duration_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
            let source_type =
                imported_eval_source_type(canonical_id, content.as_ref(), Some(&parsed_sfc));
            SourceSnapshot {
                source: content,
                whole_hash: parse_snapshot.whole_hash,
                semantic_hash: parse_snapshot.semantic_hash,
                generation,
                data: Arc::new(HostSourceData {
                    parse: parse_snapshot,
                    cached_parse: Some(Arc::new(parsed_sfc)),
                    file_kind: host_file_kind,
                    source_type,
                    parse_duration_ms,
                }),
            }
        } else {
            let parse_snapshot = crate::parse::parse_non_sfc_snapshot(canonical_id, &content);
            let parse_duration_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
            let source_type = imported_eval_source_type(canonical_id, content.as_ref(), None);
            SourceSnapshot {
                source: content,
                whole_hash: parse_snapshot.whole_hash,
                semantic_hash: parse_snapshot.semantic_hash,
                generation,
                data: Arc::new(HostSourceData {
                    parse: parse_snapshot,
                    cached_parse: None,
                    file_kind: host_file_kind,
                    source_type,
                    parse_duration_ms,
                }),
            }
        };

        // Push per-file parse-timing into the active request's
        // accumulator when timing capture is on. The lower-phase is
        // bundled with parse for both Vue and non-SFC sources here, so
        // `lower_ns` is reported as `0` — the parse total carries the
        // observable `parse_ns` for the request's critical-path
        // accounting.
        if timing_on {
            let total_ns = parse_start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
            if let Some(acc) = crate::request_context::current_accumulator() {
                acc.push_file_parse_timing(crate::component_meta_audit::FileParseTiming {
                    canonical_id: Arc::from(canonical_id),
                    parse_ns: total_ns,
                    lower_ns: 0,
                });
            }
        }

        // Test harness hook: when a CaptureToken is bound on the current
        // thread, increment the parse-count for `canonical_id`. The
        // scheduler invokes this path on rayon worker threads, so the
        // harness's thread-local lookup is ABSENT on workers — tests
        // that assert on parse counts must arrange to bind the token on
        // the same thread that calls back into the parse path. Tests in
        // the smoke suite call `with_active_capture(...)` directly to
        // simulate a parse-completion event without touching the
        // scheduler. Returns immediately when no token is bound (the
        // production hot path).
        crate::capture_token::with_active_capture(|t| {
            t.record_parse(canonical_id);
        });

        Ok(snapshot)
    }

    fn extract_deps(&self, canonical_id: &str, source: &SourceSnapshot) -> ExtractedDeps {
        let host_data = match source.downcast_data::<HostSourceData>() {
            Some(d) => d,
            None => return ExtractedDeps::default(),
        };
        let parse = &host_data.parse;
        let workspace = self.workspace.read().clone();
        let normalize_dep = |dep_id: String| {
            crate::host_manage::resolve_eval_dependency_canonical_with(&dep_id, |candidate| {
                workspace.file_exists(candidate)
            })
            .unwrap_or(dep_id)
        };

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
                forward_deps.push(normalize_dep(resolved));
            }
        }

        // Blocker IDs from macro type deps (defineProps<ExternalType>(), etc.).
        // These files must reach Analysis before this file's Artifact can proceed,
        // because the Artifact stage needs resolved type shapes for codegen.
        for dep in &parse.script_analysis.macro_type_deps {
            if dep.import_source.starts_with('.') || dep.import_source.starts_with("../") {
                let resolved = crate::id::resolve_external(canonical_id, &dep.import_source);
                blocker_ids.push(normalize_dep(resolved));
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

/// Re-export of the pure source-type helper owned by [`crate::parse`].
///
/// Owner of the single source-type computation: on native the scheduler calls
/// this once at [`HostStageExecutor::execute_source`] time and stores the result
/// on [`HostSourceData::source_type`]. Downstream cache-key callers must prefer
/// the scheduler-stored value (via [`crate::VerterHost::authoritative_source_type_for`])
/// over recomputing — recomputation with `cached_parse: None` produces a
/// different `SourceType` than with `Some(parsed)` for the same `.vue` file
/// whose `<script>` block uses `lang="tsx"` / `lang="jsx"` / `lang="js"`.
///
/// The underlying function lives in [`crate::parse::imported_eval_source_type`]
/// so WASM-only fall-back paths can reach it without the scheduler feature.
pub(crate) use crate::parse::imported_eval_source_type;
