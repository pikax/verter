//! Host stage executor — connects the scheduler to real parse/analysis/compile logic.
//!
//! Implements [`StageExecutor`] by calling into the host's existing parse pipeline.
//! Each stage produces a snapshot with host-specific data that can be downcast
//! by the host facade.

use std::sync::Arc;

use crate::instant::Instant;

use verter_language::FileLanguage;
use verter_scheduler::executor::{ExtractedDeps, StageError, StageExecutor};
use verter_scheduler::node::{
    AnalysisSnapshot, ArtifactSnapshot, EmptyData, SnapshotData, SourceSnapshot,
};

use crate::types::{HostConfig, ParseSnapshot};

/// Host-specific data stored in a [`SourceSnapshot`].
///
/// Wraps a `ParseSnapshot` — the result of SFC tokenization, hashing, and analysis.
/// Also carries the framework-neutral parse artifact (for carrier files), the
/// file's language row, the authoritative `source_type` computed once at parse
/// time, and the measured parse duration for performance tracking.
#[derive(Debug)]
#[allow(dead_code)] // Fields read progressively during 3 migration
pub struct HostSourceData {
    pub(crate) parse: ParseSnapshot,
    /// Framework-neutral parse artifact from the carrier producer
    /// (`parse_vue_snapshot` wraps the Vue parse into it). Reused during
    /// compilation to avoid re-parsing. `None` for plain scripts.
    pub(crate) framework_parse: Option<Arc<verter_language::FrameworkParseArtifact>>,
    /// Sole registered envelope owner for carrier sources.
    pub(crate) structure: Option<crate::carrier_publication_store::RegisteredFileStructure>,
    pub(crate) revision_token: crate::carrier_publication_store::HostSourceRevisionToken,
    /// The file's language row (framework carrier vs. plain script).
    pub(crate) file_language: FileLanguage,
    /// Authoritative `SourceType` for downstream type-resolution cache keys,
    /// computed once during `execute_source` with full access to the parse
    /// artifact. Readers must prefer this value over recomputing from raw
    /// source + `framework_parse` (which is unstable when `framework_parse`
    /// is dropped).
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
    pub(crate) script_analysis: Arc<verter_semantic::analysis::ScriptAnalysisSnapshot>,
    pub(crate) export_signatures: Vec<verter_semantic::analysis::ExportSignature>,
    pub(crate) style_analyses: Arc<Vec<verter_semantic::analysis::StyleBlockAnalysis>>,
    pub(crate) markup_class_tokens: Arc<Vec<verter_semantic::analysis::MarkupClassToken>>,
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
    /// Host-owned provenance counters. The executor bumps `sfc_parses`
    /// once per Vue SFC structure parse so the cold-build dedup
    /// counters observe scheduler-stage parses (rayon workers have no
    /// capture-token TLS).
    pub provenance: Arc<crate::types::MetaProvenance>,
    pub source_authority:
        Arc<verter_language::registered_source_authority::RegisteredSourceAuthority>,
    pub grammar_authority: Arc<verter_language::carrier_grammar::CarrierGrammarAuthority>,
    pub publication_store: Arc<crate::carrier_publication_store::CarrierPublicationStore>,
    pub host_instance: crate::carrier_publication_store::HostInstanceId,
    pub registered_envelope_ingest: Arc<
        parking_lot::Mutex<
            rustc_hash::FxHashMap<
                String,
                crate::carrier_publication_store::RegisteredFileStructure,
            >,
        >,
    >,
}

impl HostStageExecutor {
    pub fn new(
        config: HostConfig,
        workspace: Arc<parking_lot::RwLock<Arc<dyn verter_workspace::WorkspaceAccess>>>,
        provenance: Arc<crate::types::MetaProvenance>,
        source_authority: Arc<
            verter_language::registered_source_authority::RegisteredSourceAuthority,
        >,
        grammar_authority: Arc<verter_language::carrier_grammar::CarrierGrammarAuthority>,
        publication_store: Arc<crate::carrier_publication_store::CarrierPublicationStore>,
        host_instance: crate::carrier_publication_store::HostInstanceId,
        registered_envelope_ingest: Arc<
            parking_lot::Mutex<
                rustc_hash::FxHashMap<
                    String,
                    crate::carrier_publication_store::RegisteredFileStructure,
                >,
            >,
        >,
    ) -> Self {
        Self {
            config,
            workspace,
            provenance,
            source_authority,
            grammar_authority,
            publication_store,
            host_instance,
            registered_envelope_ingest,
        }
    }
}

impl StageExecutor for HostStageExecutor {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn execute_source(
        &self,
        canonical_id: &str,
        file_language: FileLanguage,
        content: Arc<str>,
        generation: u64,
    ) -> Result<SourceSnapshot, StageError> {
        // Carrier dispatch: a framework CARRIER file whose carrier language
        // the registry serves routes its parse through the compiler-side
        // carrier registry — the SINGLE carrier parse path, with Vue served
        // by its bridge. EVERY OTHER framework row (a framework TEMPLATE, an
        // unregistered carrier adapter, a same-adapter NON-carrier language)
        // is the typed unsupported-language state — never a silent empty,
        // never a panic. Plain scripts take the script parse path. The
        // dispatchable predicate is keyed on the FULL `(adapter_id, carrier
        // language id)` row, never adapter id alone.
        let dispatchable_carrier = match (
            file_language.adapter_id(),
            file_language.carrier_language_id(),
        ) {
            (Some(adapter_id), Some(carrier_language_id)) => {
                crate::parse::carrier_compiler_registry()
                    .compiler_for_carrier_language(adapter_id, carrier_language_id)
                    .is_some()
            }
            _ => false,
        };
        if let (false, Some(adapter_id)) = (dispatchable_carrier, file_language.adapter_id()) {
            // A framework row that is NOT a dispatchable carrier (template,
            // unregistered carrier, same-adapter non-carrier) is unsupported.
            return Err(StageError::unsupported_language(adapter_id.clone()));
        }

        // Per-file timing capture is gated on the active request's
        // `audit_timing_capture` flag. The host's own `parse_duration_ms`
        // on `HostSourceData` keeps its existing semantics; the audit
        // ledger push happens only when timing capture is on AND a
        // request context is installed.
        let timing_on = verter_scheduler::request_context::current_timing_enabled();

        let parse_start = Instant::now();

        let snapshot = if dispatchable_carrier {
            use verter_language::carrier_grammar::CarrierGrammarConfig;
            use verter_language::registered_source_authority::{
                CanonicalFileId, FileIncarnation, SourceGeneration,
            };
            let ingested = self.registered_envelope_ingest.lock().remove(canonical_id);
            let (framework_parse, structure, file_incarnation, source_generation) =
                if let Some(structure) = ingested {
                    let registered = structure.envelope().source();
                    if registered.canonical().as_str() != canonical_id
                        || registered.bytes() != content.as_ref()
                        || registered.resolved_file_language() != &file_language
                    {
                        return Err(StageError::new(
                            "registered envelope/source identity mismatch",
                        ));
                    }
                    let file_incarnation = registered.file_incarnation();
                    let source_generation = registered.generation();
                    (
                        Arc::clone(structure.artifact()),
                        structure,
                        file_incarnation,
                        source_generation,
                    )
                } else {
                    let registered = self
                        .source_authority
                        .register_source(
                            CanonicalFileId::new(canonical_id),
                            FileIncarnation::new(self.host_instance.get()),
                            SourceGeneration::new(generation),
                            file_language.clone(),
                            Arc::clone(&content),
                        )
                        .map_err(|_| {
                            StageError::new("registered source authority rejected source")
                        })?;
                    let grammar_config = if file_language.adapter_id().is_some_and(|id| id.is_vue())
                    {
                        CarrierGrammarConfig::vue("{{", "}}", std::iter::empty::<&str>())
                            .expect("default Vue grammar")
                    } else {
                        CarrierGrammarConfig::Svelte
                    };
                    let accepted = self
                        .grammar_authority
                        .accept_registered_source(
                            &self.source_authority,
                            &registered,
                            &grammar_config,
                        )
                        .map_err(|_| StageError::new("registered grammar rejected source"))?;
                    let request = crate::carrier_publication_store::PublicationRequestContext::new(
                        crate::carrier_publication_store::AuditRequestId::new(generation),
                        crate::carrier_publication_store::PublicationSurface::ProjectionHost,
                        verter_scheduler::cancellation::current_job_cancellation_token()
                            .unwrap_or_default(),
                        registered.snapshot_id().clone(),
                    );
                    let envelope = self
                        .publication_store
                        .publish_or_get(&accepted, request)
                        .into_envelope()
                        .ok_or_else(|| StageError::new("carrier publication did not admit"))?;
                    (
                        Arc::clone(envelope.artifact()),
                        crate::carrier_publication_store::RegisteredFileStructure::new(envelope),
                        registered.file_incarnation(),
                        registered.generation(),
                    )
                };
            let mut parse_snapshot = crate::parse::carrier_snapshot_from_artifact(
                canonical_id,
                &content,
                self.config.effective_scope(),
                &file_language,
                &self.provenance,
                &framework_parse,
            )
            .expect("published carrier artifact matches its registered language");
            // Sealed-identity wire tokens attach ONCE at record build, so
            // every serve reuses the stored styles Arc unchanged.
            crate::parse::attach_style_block_tokens(&structure, &mut parse_snapshot.style_analyses);
            let revision_token = crate::carrier_publication_store::HostSourceRevisionToken {
                host_instance: self.host_instance,
                file_incarnation,
                source_generation,
            };
            crate::block_content::attach_external_request_tokens(
                &structure,
                revision_token,
                &mut parse_snapshot.external_requests,
            );
            let parse_duration_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
            let source_type =
                imported_eval_source_type(&file_language, Some(framework_parse.as_ref()));
            SourceSnapshot {
                source: content,
                whole_hash: parse_snapshot.whole_hash,
                semantic_hash: parse_snapshot.semantic_hash,
                generation,
                data: Arc::new(HostSourceData {
                    parse: parse_snapshot,
                    framework_parse: Some(framework_parse),
                    structure: Some(structure),
                    revision_token,
                    file_language,
                    source_type,
                    parse_duration_ms,
                }),
            }
        } else {
            let parse_snapshot = crate::parse::parse_non_sfc_snapshot(
                canonical_id,
                &content,
                &file_language,
                &self.provenance,
            );
            let parse_duration_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
            let source_type = imported_eval_source_type(&file_language, None);
            SourceSnapshot {
                source: content,
                whole_hash: parse_snapshot.whole_hash,
                semantic_hash: parse_snapshot.semantic_hash,
                generation,
                data: Arc::new(HostSourceData {
                    parse: parse_snapshot,
                    framework_parse: None,
                    structure: None,
                    revision_token: crate::carrier_publication_store::HostSourceRevisionToken {
                        host_instance: self.host_instance,
                        file_incarnation:
                            verter_language::registered_source_authority::FileIncarnation::new(
                                self.host_instance.get(),
                            ),
                        source_generation:
                            verter_language::registered_source_authority::SourceGeneration::new(
                                generation,
                            ),
                    },
                    file_language,
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
        #[cfg(any(test, feature = "test-support"))]
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
        let resolve_dep = |specifier: &str, kind| match workspace
            .resolve_import_outcome(
                canonical_id,
                specifier,
                verter_workspace::ResolutionContext {
                    phase: verter_workspace::ResolvePhase::CodegenBlocker,
                    kind,
                },
            )
            .into_publication()
        {
            verter_workspace::ResolutionPublication::Admitted(admitted) => admitted
                .into_result()
                .map(|resolution| resolution.source_id),
            verter_workspace::ResolutionPublication::Refused(_) => None,
        };

        let mut forward_deps = Vec::new();
        let mut blocker_ids = Vec::new();

        // Forward deps from external src blocks (e.g. <script src="./setup.ts">).
        for req in &parse.external_requests {
            let Some(resolved) = resolve_dep(
                &req.specifier,
                verter_workspace::ResolveRequestKind::SfcSrcAttr,
            ) else {
                return ExtractedDeps::default();
            };
            forward_deps.push(resolved);
        }

        // Forward deps from relative imports.
        for imp in &parse.script_analysis.imports {
            if imp.source.starts_with('.') || imp.source.starts_with("../") {
                let Some(resolved) =
                    resolve_dep(&imp.source, verter_workspace::ResolveRequestKind::EsmImport)
                else {
                    return ExtractedDeps::default();
                };
                forward_deps.push(resolved);
            }
        }

        // Blocker IDs from macro type deps (defineProps<ExternalType>(), etc.).
        // These files must reach Analysis before this file's Artifact can proceed,
        // because the Artifact stage needs resolved type shapes for codegen.
        for dep in &parse.script_analysis.macro_type_deps {
            if dep.import_source.starts_with('.') || dep.import_source.starts_with("../") {
                let Some(resolved) = resolve_dep(
                    &dep.import_source,
                    verter_workspace::ResolveRequestKind::TypeImport,
                ) else {
                    return ExtractedDeps::default();
                };
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
                    // `Arc::clone` of the shared snapshot — refcount bump, not
                    // a deep copy of ~18 owned vectors.
                    script_analysis: Arc::clone(&host_data.parse.script_analysis),
                    export_signatures: host_data.parse.export_signatures.clone(),
                    style_analyses: Arc::new(host_data.parse.style_analyses.clone()),
                    markup_class_tokens: Arc::new(host_data.parse.markup_class_tokens.clone()),
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
/// over recomputing — recomputation with `framework_parse: None` produces a
/// different `SourceType` than with `Some(artifact)` for the same `.vue` file
/// whose `<script>` block uses `lang="tsx"` / `lang="jsx"` / `lang="js"`.
///
/// The underlying function lives in [`crate::parse::imported_eval_source_type`]
/// so WASM-only fall-back paths can reach it without the scheduler feature.
pub(crate) use crate::parse::imported_eval_source_type;
