//! `host_manage::eval_program` — eval-source / parsed-program / external-type-analysis bridge.
//!
//! Domain D. Holds the host-instance-scoped
//! parsed-program / type-context caches and the bridge between raw
//! source loading and the OXC-based external-type analyzer. Public
//! surface remains rooted at `crate::host_manage::*`; this file
//! contributes a private `impl VerterHost { … }` block that
//! continues the parent shell's impl chain.

use std::sync::Arc;

use crate::types::*;
use crate::VerterHost;

use super::{
    component_meta_trace_custom, is_raw_import_specifier_id, read_analysis_source_result_detail,
};

impl VerterHost {
    pub(crate) fn store_view_allows_current_whole_hash(
        &self,
        _canonical_id: &str,
        _whole_hash: Hash16,
    ) -> bool {
        // No ambient request view gates hash acceptance. Live-host
        // probes operate directly on the project-global caches, whose
        // candidates fact-validate on warm read against the live
        // `StoreView`.
        true
    }

    /// Build script-setup generic type parameter bindings for a Vue SFC.
    /// Called once during `PreparedDeclBundle` materialization. Returns an
    /// empty map for non-Vue files or Vue files without `<script setup>` generics.
    ///
    /// The result type is `FxHashMap<String, TypeParamBinding>` —
    /// script-setup parameters are stored directly rather than
    /// wrapped in a `PreparedTypeDecl`. Constraint / default lowering
    /// threads its own `name_resolution` table through
    /// `shallow_lower_type_expr`, so a separate wrapper-side
    /// `name_resolution` table on the binding would be dead
    /// allocation.
    pub(super) fn build_script_setup_type_bindings(
        &self,
        canonical_id: &str,
        _state: &crate::resolver_core::ShallowFileState,
        _dep_edges: &rustc_hash::FxHashMap<String, String>,
    ) -> rustc_hash::FxHashMap<String, crate::resolver_core::prepared_decl::TypeParamBinding> {
        use crate::resolver_core::prepared_decl::TypeParamBinding;

        let mut bindings = rustc_hash::FxHashMap::default();

        let Some((raw_source, framework_parse, _)) = self.current_eval_state(canonical_id) else {
            return bindings;
        };

        for (idx, param) in crate::host_resolve::sfc_script_setup_type_params(
            raw_source.as_ref(),
            framework_parse.as_deref(),
        )
        .into_iter()
        .enumerate()
        {
            bindings.insert(
                param.name.clone(),
                TypeParamBinding {
                    name: std::sync::Arc::from(param.name.as_str()),
                    // 0-based clause position so multiple
                    // `<script setup generic="T, U">` params get
                    // distinct identity tuples.
                    ordinal: u16::try_from(idx).unwrap_or(u16::MAX),
                    constraint: param.constraint.clone(),
                    default: param.default.clone(),
                },
            );
        }

        bindings
    }

    /// Central caller for the `IndexedReady.eval_source` body.
    ///
    /// For a framework CARRIER (any adapter — Vue, Svelte, …) this returns the
    /// **position-preserving** script-only source: each script block's content
    /// sits at its RAW carrier byte offsets and every other byte is
    /// whitespace-blanked (line terminators preserved), so every OXC-produced
    /// span — eval-env decls, shallow-index facts, member/signature spans —
    /// is carrier-absolute by construction. The blanking is CARRIER-NEUTRAL: it
    /// reads the neutral `FrameworkParseCommon.script_regions` the carrier's
    /// producer populated (BOTH the instance and module script blocks), so a new
    /// carrier needs no per-adapter eval-source branch here. For a non-carrier
    /// file the source is returned unchanged (its offsets are already
    /// file-absolute).
    pub(crate) fn build_eval_script_source(
        canonical_id: &str,
        source: &str,
        framework_parse: Option<&verter_language::FrameworkParseArtifact>,
    ) -> String {
        Self::build_eval_script_source_with_extraction(canonical_id, source, framework_parse).0
    }

    /// [`Self::build_eval_script_source`] plus the extraction provenance: the
    /// `bool` is `true` iff the returned text is the position-preserving
    /// extracted CARRIER script — exactly the case where the flight's
    /// eval-program parse over this text IS the snapshot's script program and
    /// can be threaded into the snapshot build
    /// ([`crate::parse::VueScriptProgram::Shared`] for Vue,
    /// [`crate::parse::FrameworkScriptProgram::Shared`] for other carriers).
    /// `false` means the raw source passed through unchanged (non-carrier
    /// files, or a carrier with no extractable script), where the eval program
    /// covers different bytes than a script-program walk would.
    ///
    /// Carrier-NEUTRAL: a non-Vue carrier blanks from the producer's recorded
    /// `script_regions` (both instance + module blocks), Vue keeps its exact
    /// `extract_vue_script_content` behaviour, and a non-carrier passes through.
    ///
    /// Extraction is gated on the file's LANGUAGE CLASSIFICATION, never on the
    /// raw text: `canonical_id` classifies through the single static registry
    /// (the same authority `resolve_route_type_edge` uses), and ONLY a
    /// framework-carrier file may script-extract when no parse artifact is
    /// available. A non-carrier `.ts` / `.d.ts` whose TEXT happens to contain a
    /// `<script ...>` ... `</script>` pair — a JSDoc `@example` block in a
    /// package declaration file (vue-router@5, @regle/core, unhead dist all
    /// ship one) — passes through UNCHANGED; the former unconditional forgiving
    /// raw scan blanked such a file down to its documentation example,
    /// destroying its whole type surface (empty shallow inventory → every
    /// dependent member value unresolvable).
    pub(crate) fn build_eval_script_source_with_extraction(
        canonical_id: &str,
        source: &str,
        framework_parse: Option<&verter_language::FrameworkParseArtifact>,
    ) -> (String, bool) {
        // A NON-Vue framework carrier blanks NEUTRALLY from the producer's
        // recorded `script_regions` (BOTH the instance and module script blocks)
        // — so a `.svelte` eval-source carries both scripts at their raw carrier
        // offsets with the markup/styles whitespace-blanked. A new carrier needs
        // no per-adapter branch here. The blanked text IS a position-preserving
        // extracted script (the eval program over it is shareable), so the
        // provenance is `true`.
        if let Some(artifact) = framework_parse {
            let is_vue = crate::typeinfo::adapters::vue::vue_parse(artifact).is_some();
            if !is_vue {
                let mut spans: Vec<(u32, u32)> = artifact
                    .common
                    .script_regions
                    .iter()
                    .map(|region| (region.span.start, region.span.end))
                    .filter(|(start, end)| end > start)
                    .collect();
                spans.sort_by_key(|(start, _)| *start);
                // Even with NO script regions (a pure-markup `.svelte`) the
                // eval-source is the FULLY-BLANKED, line-preserving source — never
                // the raw markup. `build_position_preserving_script_source` over an
                // empty span set blanks every non-line-terminator byte, so a shared
                // eval-source consumer parses an empty TS program, not HTML.
                let blanked =
                    crate::host_resolve::build_position_preserving_script_source(source, &spans);
                return (blanked, true);
            }
        }
        // Vue (and the CARRIER no-artifact fallback) keep the EXACT existing
        // extraction: the parser-vs-raw-scan agreement + the forgiving raw scan
        // when no parsed SFC is available + the inter-script `\n` injection —
        // a Vue file arriving without a framework_parse still extracts its
        // `<script>` from the raw markup.
        let parsed = framework_parse.and_then(crate::typeinfo::adapters::vue::vue_parse);
        // A file with NO parse artifact script-extracts ONLY when its
        // canonical CLASSIFIES as a framework carrier. A non-carrier file's
        // raw source is already script: `<script>` text inside it is
        // documentation/data, not structure, and the forgiving raw scan must
        // never blank it (typed classification decides, never text sniffing).
        if parsed.is_none()
            && !verter_language::LanguageRegistry::global()
                .classify_static(canonical_id)
                .static_resolution()
                .is_framework_carrier()
        {
            return (source.to_string(), false);
        }
        match crate::host_resolve::extract_vue_script_content(source, parsed.map(|p| p.as_ref())) {
            Some(script) => (script, true),
            None => (source.to_string(), false),
        }
    }

    /// Selects the `.vue` snapshot build's script-program input for a cold
    /// materialise flight — the SINGLE decision point both the base and overlay
    /// flights share. The flight's eval program IS the snapshot's script program
    /// exactly when the eval source is the position-preserving extracted script:
    /// [`crate::parse::VueScriptProgram::Shared`] on a live parse,
    /// [`crate::parse::VueScriptProgram::SharedFatal`] on a fatal one (a re-parse
    /// of the same bytes under the same source type fails identically), and
    /// [`crate::parse::VueScriptProgram::ParseHere`] when the raw source passed
    /// through unextracted (the eval program covers different bytes than a
    /// script-program walk would).
    pub(crate) fn vue_flight_script_program<'a>(
        eval_is_extracted_script: bool,
        parsed_eval_program: Option<&'a crate::ParsedEvalProgram>,
    ) -> crate::parse::VueScriptProgram<'a> {
        if !eval_is_extracted_script {
            return crate::parse::VueScriptProgram::ParseHere;
        }
        match parsed_eval_program {
            Some(program) => crate::parse::VueScriptProgram::Shared(program),
            None => crate::parse::VueScriptProgram::SharedFatal,
        }
    }

    /// The carrier-neutral analog of [`Self::vue_flight_script_program`] — the
    /// SINGLE decision point selecting a non-Vue carrier snapshot's script
    /// program for a cold materialise flight (Svelte today). The flight's eval
    /// program IS the snapshot's script program exactly when the eval source is
    /// the position-preserving extracted carrier script, so the snapshot walks
    /// the retained parse instead of re-parsing the same bytes.
    pub(crate) fn framework_flight_script_program<'a>(
        eval_is_extracted_script: bool,
        parsed_eval_program: Option<&'a crate::ParsedEvalProgram>,
    ) -> crate::parse::FrameworkScriptProgram<'a> {
        if !eval_is_extracted_script {
            return crate::parse::FrameworkScriptProgram::ParseHere;
        }
        match parsed_eval_program {
            Some(program) => crate::parse::FrameworkScriptProgram::Shared(program),
            None => crate::parse::FrameworkScriptProgram::SharedFatal,
        }
    }

    /// Resolve the authoritative `source_type` for cache-key purposes.
    ///
    /// Prefers the scheduler-stored [`crate::host_executor::HostSourceData::source_type`]
    /// (set once at `execute_source` time). Falls back to a pure recomputation for
    /// canonicals the scheduler has not yet processed — WASM path, first-time routing,
    /// or snapshot construction for files the scheduler does not own.
    pub(crate) fn imported_eval_source_type_for(
        &self,
        canonical_id: &str,
        framework_parse: Option<&verter_language::FrameworkParseArtifact>,
    ) -> oxc_span::SourceType {
        {
            if let Some(st) = self.authoritative_source_type_for(canonical_id) {
                return st;
            }
        }
        crate::parse::imported_eval_source_type(
            &self.language_classifier.classify(canonical_id),
            framework_parse,
        )
    }

    /// View-aware variant of [`Self::read_analysis_source`].
    ///
    /// Consults `view.source(canonical)` FIRST so overlay-only
    /// sources are visible to the resolver tier; falls back to the
    /// base host's read path on miss.
    ///
    /// Per R17: the view does NOT mutate the host. Per R18: the view
    /// is threaded explicitly (no TLS view globals). Only call sites
    /// with a view in scope (those rooted at
    /// `get_component_meta_via_view` / `evaluate_types` session
    /// entry-points) use this variant.
    ///
    /// Substrate-only API. Resolver-tier migration of deep callers
    /// (e.g., `extract_component_meta_from_inputs`,
    /// `compute_template_analysis_if_missing`) is consumer-side
    /// work tracked separately.
    #[allow(dead_code)]
    pub(crate) fn read_analysis_source_via_view(
        &self,
        canonical_id: &str,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<Arc<str>> {
        if canonical_id.is_empty() {
            return None;
        }
        if let Some(source) = view.source(canonical_id) {
            component_meta_trace_custom!(
                "read_analysis_source_result",
                read_analysis_source_result_detail(
                    canonical_id,
                    "session-view",
                    source.len(),
                    false
                ),
            );
            return Some(source);
        }
        self.read_analysis_source(canonical_id)
    }

    pub(crate) fn read_analysis_source(&self, canonical_id: &str) -> Option<Arc<str>> {
        component_meta_trace_custom!("read_analysis_source", format!("owner={canonical_id}"));
        if canonical_id.is_empty() {
            component_meta_trace_custom!(
                "read_analysis_source_result",
                read_analysis_source_result_detail("", "empty-canonical", 0, true),
            );
            return None;
        }
        if let Some(source) = self.get_source(canonical_id) {
            component_meta_trace_custom!(
                "read_analysis_source_result",
                read_analysis_source_result_detail(canonical_id, "host-cache", source.len(), false,),
            );
            return Some(source);
        }

        // Project-global IndexedReady cache for cached raw_source —
        // **current-content-pinned** (no `get_any`). `read_analysis_source`
        // feeds the routed-shallow prepared-decl materialiser (a
        // route-fact producer) and the cold analysis path; a stale pre-edit
        // `IndexedReady` (which can linger past a same-canonical edit with
        // the own-canonical drain retired) would seed those producers with
        // pre-edit source. `artifact_current_indexed` answers ONLY for a
        // genuinely artifact-only canonical (no scheduler `DerivedRawState`)
        // — exactly the foreign-source / test-seed scope this fallback
        // exists for. A scheduler-tracked canonical (whose authoritative
        // source is the scheduler, read above via `get_source`) gets `None`
        // here, so a live-but-stale scope falls through to `ensure_loaded`
        // rather than reading the stale artifact.
        if let Some(facts) = self.artifact_current_indexed(canonical_id) {
            component_meta_trace_custom!(
                "read_analysis_source_result",
                read_analysis_source_result_detail(
                    canonical_id,
                    "module-facts-db",
                    facts.raw_source.len(),
                    false,
                ),
            );
            return Some(Arc::clone(&facts.raw_source));
        }

        if is_raw_import_specifier_id(canonical_id) {
            component_meta_trace_custom!(
                "read_analysis_source_result",
                read_analysis_source_result_detail(canonical_id, "raw-import-specifier", 0, true,),
            );
            return None;
        }

        // Native: scheduler is the sole parser + source authority. On a cache
        // miss, submit through `ensure_loaded` (canonical loading path via
        // the scheduler). If the scheduler still has no source, the file
        // genuinely doesn't exist.
        //
        // WASM: no scheduler; the `files` map is the authority and the
        // workspace fallback is legitimate.
        {
            if self.ensure_loaded(canonical_id) {
                if let Some(source) = self.get_source(canonical_id) {
                    component_meta_trace_custom!(
                        "read_analysis_source_result",
                        read_analysis_source_result_detail(
                            canonical_id,
                            "ensure-loaded",
                            source.len(),
                            false,
                        ),
                    );
                    // supplement §5.D.0 r17 — fresh load,
                    // count for the host-level test audit. Fresh
                    // means we missed both `get_source` and
                    // `FileArtifactStore::get_any` before reaching here.
                    #[cfg(test)]
                    self.test_audit.record_read(canonical_id);
                    return Some(source);
                }
                // Post-`ensure_loaded` artifact fallback — content-pinned
                // via `artifact_current_indexed` for the same reason as the
                // pre-`ensure_loaded` read above: a stale lingering
                // artifact must not seed the cold analysis / route-fact
                // producers with pre-edit source.
                if let Some(facts) = self.artifact_current_indexed(canonical_id) {
                    component_meta_trace_custom!(
                        "read_analysis_source_result",
                        read_analysis_source_result_detail(
                            canonical_id,
                            "ensure-loaded-module-facts",
                            facts.raw_source.len(),
                            false,
                        ),
                    );
                    #[cfg(test)]
                    self.test_audit.record_read(canonical_id);
                    return Some(Arc::clone(&facts.raw_source));
                }
            }
            component_meta_trace_custom!(
                "read_analysis_source_result",
                read_analysis_source_result_detail(canonical_id, "not-loaded", 0, true,),
            );
            None
        }
    }

    pub(super) fn analysis_source_exists(&self, canonical_id: &str) -> bool {
        if canonical_id.is_empty() {
            return false;
        }

        {
            if let Some(state) = self.effective_file_state(canonical_id, None) {
                return self.store_view_allows_current_whole_hash(canonical_id, state.whole_hash);
            }
        }

        // Artifact-only existence probe — gated by the single authority
        // predicate (`artifact_only_authority_allows`) so a retained
        // artifact the accessors reject (absent file,
        // scheduler-superseded scope) does not claim existence here.
        // Non-normalizing by contract: this probe is the oracle the
        // canonical normalizer consults.
        if self.artifact_only_entry_exists(canonical_id) {
            return true;
        }

        if is_raw_import_specifier_id(canonical_id) {
            return false;
        }

        self.ws().file_exists(canonical_id)
    }
}
