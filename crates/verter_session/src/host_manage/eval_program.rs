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
    /// wrapped in a `PreparedTypeDecl`. The clause is parsed TRANSIENTLY
    /// here to collect the two facts the binding stores (name + ordinal);
    /// the constraint / default typed IR is dropped — at query time the
    /// ONE dispatch helper re-borrows the clause lease-only from the
    /// pinned `IndexedReady` and lowers the selected parameter's bounds.
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
                },
            );
        }

        bindings
    }

    /// Central caller for the `IndexedReady.eval_source` body.
    ///
    /// For a framework CARRIER this returns the **position-preserving**
    /// script-only `Arc<str>` produced by one semantic-catalog lookup
    /// (adapter × artifact epoch × Semantic). For a non-carrier file the
    /// source is returned unchanged (its offsets are already
    /// file-absolute). Catalog miss and a classified carrier without its
    /// parse artifact are `None` — no blanked projection, no parse, no
    /// publication.
    #[cfg(test)]
    pub(crate) fn build_eval_script_source(
        canonical_id: &str,
        source: &str,
        framework_parse: Option<&verter_compiler::framework_common::FrameworkParseArtifact>,
    ) -> Option<Arc<str>> {
        Self::build_eval_script_source_with_extraction(canonical_id, source, framework_parse)
            .map(|(eval, _)| eval)
    }

    /// [`Self::build_eval_script_source`] plus the extraction provenance: the
    /// `bool` is `true` iff the returned text is the position-preserving
    /// extracted CARRIER script — exactly the case where the flight's
    /// eval-program parse over this text IS the snapshot's script program and
    /// can be threaded into the snapshot build
    /// ([`crate::parse::VueScriptProgram::Shared`] for Vue,
    /// [`crate::parse::FrameworkScriptProgram::Shared`] for other carriers).
    /// `false` means the raw source passed through unchanged (non-carrier
    /// files), where the eval program covers different bytes than a
    /// script-program walk would.
    ///
    /// Catalog semantic authorities own carrier eval-source. A classified
    /// carrier without its registered artifact, or a catalog miss, is
    /// typed refusal (`None`) before parse/lease/publication. A
    /// non-carrier file's raw source is already script: `<script>` text
    /// inside it is documentation/data, not structure.
    pub(crate) fn build_eval_script_source_with_extraction(
        canonical_id: &str,
        source: &str,
        framework_parse: Option<&verter_compiler::framework_common::FrameworkParseArtifact>,
    ) -> Option<(Arc<str>, bool)> {
        if let Some(artifact) = framework_parse {
            let eval = crate::parse::catalog_eval_source(artifact, source)?;
            return Some((eval, true));
        }
        if !verter_language::LanguageRegistry::global()
            .classify_static(canonical_id)
            .static_resolution()
            .is_framework_carrier()
        {
            return Some((Arc::from(source), false));
        }
        None
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
                    self.test_force.audit.record_read(canonical_id);
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
                    self.test_force.audit.record_read(canonical_id);
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

    #[cfg(test)]
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
