//! Optional semantic enrichment and analysis serialization.
//!
//! The editor projection host stays on the latency-critical path. Rich semantic
//! work lives behind this isolated, debounced lane and only publishes immutable
//! snapshots after source/version revalidation.

use super::*;

/// Edit-debounce quiet window before optional semantic enrichment runs.
/// Production and tests share this duration; tests drive it with paused
/// Tokio time rather than compiling the sleep out.
pub(crate) const SEMANTIC_ANALYSIS_QUIET_WINDOW: std::time::Duration =
    std::time::Duration::from_millis(750);

pub(crate) fn type_expr_contains_boolean(expression: &verter_type_expr::TypeExpr) -> bool {
    use verter_type_expr::{LiteralValue, PrimitiveName, TypeExpr};

    let mut pending = vec![expression];
    while let Some(expression) = pending.pop() {
        match expression {
            TypeExpr::Primitive(PrimitiveName::Boolean)
            | TypeExpr::Literal(LiteralValue::Boolean(_)) => return true,
            TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
                pending.extend(members.iter());
            }
            TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) => pending.push(inner),
            _ => {}
        }
    }
    false
}

fn merge_semantic_prop_definitions(
    native: &mut Vec<verter_semantic::analysis::AnalyzedPropDefinition>,
    semantic: Vec<verter_semantic::analysis::AnalyzedPropDefinition>,
) {
    let mut semantic = semantic
        .into_iter()
        .map(|prop| (prop.name.clone(), prop))
        .collect::<std::collections::HashMap<_, _>>();

    for native_prop in native.iter_mut() {
        let Some(enrichment) = semantic.remove(&native_prop.name) else {
            continue;
        };
        if enrichment.type_annotation.is_some() {
            native_prop.type_annotation = enrichment.type_annotation;
        }
        native_prop.callable_role = enrichment.callable_role;
        native_prop.has_default = enrichment.has_default;
        native_prop.is_required = enrichment.is_required;
        native_prop.is_boolean = enrichment.is_boolean;
        // Authored span and usage facts remain native-analysis authority.
    }

    // Semantic-only declarations have no honest authored span in this file.
    // Append them without disturbing native source order or fabricating one.
    let mut remaining = semantic.into_values().collect::<Vec<_>>();
    remaining.sort_unstable_by(|left, right| left.name.cmp(&right.name));
    native.extend(remaining);
}

#[derive(Clone)]
pub(super) struct SemanticSnapshot {
    pub(super) document_revision: DocumentRevisionId,
    pub(super) semantic_generation: u64,
    pub(super) analysis: Arc<verter_session::FileAnalysisSnapshot>,
}

#[derive(Clone, Debug)]
pub(crate) struct SemanticReady {
    pub canonical_id: String,
    pub uri: String,
    pub version: i32,
    pub document_revision: DocumentRevisionId,
}

impl DocumentRegistry {
    fn current_semantic_generation(&self) -> u64 {
        self.semantic_generation
            .load(std::sync::atomic::Ordering::Acquire)
    }

    fn semantic_generation_is_current(&self, generation: u64) -> bool {
        self.semantic_analysis_enabled() && self.current_semantic_generation() == generation
    }

    fn invalidate_semantic_publications(&self) {
        for mut document in self.documents.iter_mut() {
            let Some(feature) = document.feature_snapshot.as_ref() else {
                continue;
            };
            if feature.analysis.is_some() {
                document.feature_snapshot = Some(Arc::new(feature.without_semantic_analysis()));
            }
        }
        self.semantic_snapshots.clear();
    }

    /// Enable or disable optional native semantic enrichment. Disabling is
    /// immediate and drops cached results; the projection/type-provider lane is
    /// unaffected in either state.
    pub fn set_semantic_analysis_enabled(&self, enabled: bool) {
        self.semantic_enabled
            .store(enabled, std::sync::atomic::Ordering::Release);
        if !enabled {
            self.semantic_generation
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            self.invalidate_semantic_publications();
        }
    }

    #[must_use]
    pub fn semantic_analysis_enabled(&self) -> bool {
        self.semantic_enabled
            .load(std::sync::atomic::Ordering::Acquire)
    }

    pub(crate) fn subscribe_semantic_ready(
        &self,
    ) -> tokio::sync::broadcast::Receiver<SemanticReady> {
        self.semantic_ready_tx.subscribe()
    }

    /// Install one workspace authority into both isolated hosts. The semantic host
    /// may not exist yet; in that case the workspace is retained for lazy creation.
    pub fn set_workspace(&self, workspace: Arc<verter_workspace::FilesystemWorkspace>) {
        self.semantic_generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.invalidate_semantic_publications();
        let projection_workspace: Arc<dyn verter_workspace::WorkspaceAccess> = workspace.clone();
        self.host.set_workspace(projection_workspace);
        *self.semantic_workspace.write() = Some(Arc::clone(&workspace));
        if let Some(host) = self.semantic_host.read().clone() {
            let semantic_workspace: Arc<dyn verter_workspace::WorkspaceAccess> = workspace;
            host.set_workspace(semantic_workspace);
        }
        // A task may have been scheduled after the pre-install fence while a
        // host still held the old workspace. Advance once more after every host
        // observes the new authority; only tasks scheduled after this method
        // returns may publish into the new generation.
        self.semantic_generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.invalidate_semantic_publications();
    }

    fn semantic_host(&self) -> Arc<VerterHost> {
        if let Some(host) = self.semantic_host.read().clone() {
            return host;
        }
        let mut slot = self.semantic_host.write();
        if let Some(host) = slot.as_ref() {
            return Arc::clone(host);
        }
        let host = Arc::new(VerterHost::new_standalone(verter_session::HostConfig {
            analysis_scope: Some(verter_semantic::analysis::AnalysisScope::LSP),
            // Isolation is also a fairness boundary: native enrichment cannot
            // occupy the projection scheduler or saturate all host CPU workers.
            host_cpu_threads: Some(1),
            ..verter_session::HostConfig::default()
        }));
        if let Some(workspace) = self.semantic_workspace.read().clone() {
            let workspace: Arc<dyn verter_workspace::WorkspaceAccess> = workspace;
            host.set_workspace(workspace);
        }
        *slot = Some(Arc::clone(&host));
        host
    }

    /// Queue full native semantic enrichment for the current document snapshot.
    ///
    /// Work is edit-debounced, globally serialized, and executed on a blocking
    /// worker against the isolated semantic host. A stale completion is discarded
    /// by version + source identity checks, so handlers only ever observe a fully
    /// committed immutable snapshot and never wait for its construction.
    pub fn schedule_semantic_analysis(self: &Arc<Self>, uri: &Uri) {
        let _ = self.spawn_semantic_analysis(uri);
    }

    fn spawn_semantic_analysis(self: &Arc<Self>, uri: &Uri) -> Option<tokio::task::JoinHandle<()>> {
        let semantic_generation = self.current_semantic_generation();
        if !self.semantic_generation_is_current(semantic_generation) {
            return None;
        }
        let document = self.documents.get(uri.as_str())?;
        if document.virtual_source_uri.is_some() {
            return None;
        }
        let uri_key = uri.as_str().to_string();
        let canonical_id = document.canonical_id.clone();
        let version = document.version;
        let document_revision = document.document_revision;
        let source = Arc::clone(&document.source);
        let file_language = self.document_file_language(&document.language_id, &canonical_id);
        let is_framework_carrier = file_language.is_framework_carrier();
        let registered_structure = is_framework_carrier
            .then(|| self.host.registered_file_structure(&canonical_id))
            .flatten();
        if !self.semantic_generation_is_current(semantic_generation) {
            return None;
        }
        drop(document);

        let registry = Arc::clone(self);
        Some(tokio::spawn(async move {
            #[cfg(test)]
            registry.semantic_task_armed.notify_waiters();
            tokio::time::sleep(SEMANTIC_ANALYSIS_QUIET_WINDOW).await;

            if !registry.semantic_generation_is_current(semantic_generation)
                || !registry.document_snapshot_is_current(&uri_key, document_revision)
            {
                return;
            }
            let serial = Arc::clone(&registry.semantic_serial);
            let _guard = serial.lock().await;
            if !registry.semantic_generation_is_current(semantic_generation)
                || !registry.document_snapshot_is_current(&uri_key, document_revision)
            {
                return;
            }

            let host = registry.semantic_host();
            let work_source = Arc::clone(&source);
            let work_canonical = canonical_id.clone();
            let analysis = tokio::task::spawn_blocking(move || {
                let request = UpsertRequest {
                    canonical_id: Some(work_canonical.clone()),
                    input_id: work_canonical.clone(),
                    source: work_source,
                    file_language,
                    aliases: Vec::new(),
                };
                let _ = match registered_structure {
                    Some(structure) => host.upsert_registered_envelope(request, structure),
                    None => host.upsert(request),
                }
                .ok()?;
                let mut analysis = host.get_analysis(&work_canonical)?;
                if is_framework_carrier {
                    let mut semantic_props = host
                        .get_component_meta_output(&work_canonical)
                        .ok()
                        .flatten()
                        .map(|output| {
                            let (component_meta, _resolution, types) = output.into_parts();
                            component_meta
                                .props
                                .into_iter()
                                .zip(types.into_lanes().props)
                                .map(|(prop, publication)| {
                                    let is_boolean = publication
                                        .materialized_type()
                                        .is_some_and(type_expr_contains_boolean);
                                    let type_annotation =
                                        publication.terminal_display().text().map(str::to_string);
                                    verter_semantic::analysis::AnalyzedPropDefinition {
                                        name: prop.name,
                                        callable_role: prop.callable_role,
                                        type_annotation,
                                        has_default: prop.has_default,
                                        is_required: prop.required,
                                        is_boolean,
                                        used_in_template: false,
                                        used_in_script: false,
                                        span: verter_span::Span::new(0, 0),
                                    }
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    if semantic_props.is_empty() {
                        semantic_props = host
                            .get_public_api_projection(&work_canonical)
                            .ok()
                            .flatten()
                            .and_then(|projection| match projection.contract {
                                verter_session::framework::ComponentContractAvailability::Supported(
                                    contract,
                                ) => Some(contract),
                                verter_session::framework::ComponentContractAvailability::Unsupported(
                                    _,
                                ) => None,
                            })
                            .map(|contract| {
                                contract
                                    .props
                                    .iter()
                                    .map(|prop| {
                                        let materialized = prop.ty.publication.materialized_type();
                                        verter_semantic::analysis::AnalyzedPropDefinition {
                                            name: prop.name.to_string(),
                                            callable_role:
                                                verter_type_expr::PropCallableRole::default(),
                                            type_annotation: materialized.and_then(|expression| {
                                                verter_type_expr::render_type_expr_display(expression)
                                                    .ok()
                                                    .map(|rendered| rendered.text)
                                            }),
                                            has_default: prop.has_default,
                                            is_required: !prop.optional,
                                            is_boolean: materialized
                                                .is_some_and(type_expr_contains_boolean),
                                            used_in_template: false,
                                            used_in_script: false,
                                            span: verter_span::Span::new(0, 0),
                                        }
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                    }
                    if !semantic_props.is_empty() {
                        let mut template =
                            analysis.template.as_deref().cloned().unwrap_or_default();
                        merge_semantic_prop_definitions(
                            &mut template.prop_definitions,
                            semantic_props,
                        );
                        analysis.template = Some(Arc::new(template));
                    }
                }
                let semantic_structure = is_framework_carrier
                    .then(|| host.registered_file_structure_snapshot(&work_canonical))
                    .flatten();
                Some((analysis, semantic_structure))
            })
            .await
            .ok()
            .flatten();

            if let Some((analysis, semantic_structure)) = analysis {
                #[cfg(test)]
                let semantic_structure = {
                    let mut semantic_structure = semantic_structure;
                    if let Some(hook) = registry.before_semantic_publish_hook.lock().take() {
                        let hook_uri: Uri = uri_key.parse().expect("scheduled URI remains valid");
                        if let Some(planted_structure) = hook(&registry, &hook_uri) {
                            semantic_structure =
                                semantic_structure.map(|(_, semantic_host_revision)| {
                                    (planted_structure, semantic_host_revision)
                                });
                        }
                    }
                    semantic_structure
                };

                if !registry.semantic_generation_is_current(semantic_generation) {
                    return;
                }
                let analysis = Arc::new(analysis);
                let Some(mut document) = registry.documents.get_mut(&uri_key) else {
                    return;
                };
                if document.document_revision != document_revision
                    || !registry.semantic_generation_is_current(semantic_generation)
                {
                    return;
                }
                if is_framework_carrier {
                    let (Some(feature), Some((structure, semantic_host_revision))) =
                        (document.feature_snapshot.as_ref(), semantic_structure)
                    else {
                        return;
                    };
                    if structure.artifact_id() != feature.structure.artifact_id() {
                        return;
                    }
                    let same_envelope =
                        Arc::ptr_eq(structure.envelope(), feature.structure.envelope());
                    verter_debug_assert!(
                        same_envelope,
                        "validated semantic and document artifact IDs must retain one sealed envelope"
                    );
                    if !same_envelope {
                        return;
                    }
                    document.feature_snapshot = Some(Arc::new(DocumentFeatureSnapshot {
                        document_revision,
                        client_version: document.version,
                        line_index: Arc::clone(&document.line_index),
                        structure: feature.structure.clone(),
                        projection_host_revision: feature.projection_host_revision,
                        analysis: Some(SemanticAnalysisEnvelope {
                            document_revision,
                            semantic_generation,
                            semantic_host_revision,
                            structure,
                            analysis: Arc::clone(&analysis),
                        }),
                    }));
                }
                #[cfg(test)]
                if let Some(hook) = registry.after_semantic_admission_hook.lock().take() {
                    let hook_uri: Uri = uri_key.parse().expect("scheduled URI remains valid");
                    hook(&registry, &hook_uri);
                }
                #[cfg(test)]
                if let Some(hook) = registry
                    .before_semantic_cache_publication_hook
                    .lock()
                    .take()
                {
                    let hook_uri: Uri = uri_key.parse().expect("scheduled URI remains valid");
                    hook(&registry, &hook_uri);
                }
                registry.semantic_snapshots.insert(
                    canonical_id.clone(),
                    SemanticSnapshot {
                        document_revision,
                        semantic_generation,
                        analysis,
                    },
                );
                let _ = registry.semantic_ready_tx.send(SemanticReady {
                    canonical_id,
                    uri: uri_key,
                    version,
                    document_revision,
                });
                drop(document);
            }
        }))
    }

    #[cfg(test)]
    pub(super) fn set_before_semantic_publish_hook_for_test(
        &self,
        hook: super::BeforeSemanticPublishHook,
    ) {
        *self.before_semantic_publish_hook.lock() = Some(hook);
    }

    #[cfg(test)]
    pub(super) fn set_after_early_semantic_invalidation_window_hook_for_test(
        &self,
        hook: super::AfterCompileHook,
    ) {
        *self.after_early_semantic_invalidation_window_hook.lock() = Some(hook);
    }

    #[cfg(test)]
    pub(crate) fn set_before_change_document_reacquire_hook_for_test(
        &self,
        hook: super::AfterCompileHook,
    ) {
        *self.before_change_document_reacquire_hook.lock() = Some(hook);
    }

    #[cfg(test)]
    pub(super) fn set_after_semantic_admission_hook_for_test(&self, hook: super::AfterCompileHook) {
        *self.after_semantic_admission_hook.lock() = Some(hook);
    }

    #[cfg(test)]
    pub(super) fn set_before_semantic_cache_publication_hook_for_test(
        &self,
        hook: super::AfterCompileHook,
    ) {
        *self.before_semantic_cache_publication_hook.lock() = Some(hook);
    }

    #[cfg(test)]
    pub(super) fn schedule_semantic_analysis_for_test(
        self: &Arc<Self>,
        uri: &Uri,
    ) -> Option<tokio::task::JoinHandle<()>> {
        self.spawn_semantic_analysis(uri)
    }

    fn document_snapshot_is_current(
        &self,
        uri: &str,
        document_revision: DocumentRevisionId,
    ) -> bool {
        self.documents
            .get(uri)
            .is_some_and(|document| document.document_revision == document_revision)
    }

    /// Return optional full enrichment when current, otherwise the bounded BUILD
    /// snapshot that the IDE projection already paid to construct.
    pub fn get_analysis(&self, uri: &Uri) -> Option<verter_session::FileAnalysisSnapshot> {
        let canonical_id = self.get_canonical_id(uri)?;
        let semantic_generation = self.current_semantic_generation();
        if let Some(document) = self.documents.get(uri.as_str()) {
            // The feature snapshot is the atomic document-revision witness.
            // Its semantic envelope remains valid for an unchanged document
            // while the secondary canonical-id index is invalidated/rebuilt by
            // dependency publication. Prefer it so native features do not
            // transiently disappear during downstream churn.
            if self.semantic_generation_is_current(semantic_generation) {
                if let Some(analysis) = document
                    .feature_snapshot
                    .as_ref()
                    .filter(|feature| feature.document_revision == document.document_revision)
                    .and_then(|feature| feature.analysis.as_ref())
                    .filter(|analysis| {
                        analysis.document_revision == document.document_revision
                            && analysis.semantic_generation == semantic_generation
                    })
                {
                    let result = analysis.analysis.as_ref().clone();
                    if self.semantic_generation_is_current(semantic_generation) {
                        return Some(result);
                    }
                }
                if let Some(snapshot) = self.semantic_snapshots.get(&canonical_id) {
                    if snapshot.document_revision == document.document_revision
                        && snapshot.semantic_generation == semantic_generation
                    {
                        let result = snapshot.analysis.as_ref().clone();
                        if self.semantic_generation_is_current(semantic_generation) {
                            return Some(result);
                        }
                    }
                }
            }
        }
        self.host
            .config()
            .effective_scope()
            .contains(verter_semantic::analysis::AnalysisScope::BUILD)
            .then(|| self.host.get_analysis(&canonical_id))
            .flatten()
    }

    /// Clone one source-feature view under one document-shard read. A carrier
    /// capture is admitted only when its feature snapshot names this exact
    /// client revision and source bytes.
    pub(crate) fn capture_source_feature_document(
        &self,
        uri: &Uri,
    ) -> Option<SourceFeatureDocumentCapture> {
        let document = self.documents.get(uri.as_str())?;
        let expected_host_revision = match document.feature_snapshot.as_ref() {
            Some(feature)
                if feature.document_revision == document.document_revision
                    && feature.client_version == document.version
                    && feature.source() == document.source.as_ref() =>
            {
                Some(feature.projection_host_revision)
            }
            Some(_) => return None,
            None if crate::server::server_utils::carrier_language_for(&document.canonical_id)
                .is_some() =>
            {
                return None;
            }
            None => None,
        };
        let identity = DocumentSnapshotIdentity {
            version: document.version,
            revision: document.document_revision,
            source: Arc::clone(&document.source),
        };
        Some(SourceFeatureDocumentCapture {
            document: document.clone(),
            identity,
            expected_host_revision,
            semantic_generation: self.current_semantic_generation(),
        })
    }

    /// Check the host-side source witness paired with a request-local capture.
    pub(crate) fn source_feature_host_revision_is_current(
        &self,
        capture: &SourceFeatureDocumentCapture,
    ) -> bool {
        match capture.expected_host_revision {
            Some(expected) => {
                self.host
                    .registered_source_revision_token(&capture.document.canonical_id)
                    == Some(expected)
            }
            None => self
                .host
                .get_source(&capture.document.canonical_id)
                .is_some_and(|source| *source == *capture.document.source),
        }
    }

    /// Final request-local admission: the document identity and semantic
    /// generation must remain live, and the host revision is rechecked while
    /// the document shard is held so an edit cannot enter a check/use gap.
    pub(crate) fn source_feature_capture_is_current(
        &self,
        uri: &Uri,
        capture: &SourceFeatureDocumentCapture,
    ) -> bool {
        self.current_semantic_generation() == capture.semantic_generation
            && self
                .with_current_snapshot_identity(uri, &capture.identity, |document| {
                    document.source == capture.document.source
                        && match capture.expected_host_revision {
                            Some(expected) => {
                                document.feature_snapshot.as_ref().is_some_and(|feature| {
                                    feature.document_revision == document.document_revision
                                        && feature.client_version == document.version
                                        && feature.projection_host_revision == expected
                                }) && self
                                    .host
                                    .registered_source_revision_token(&document.canonical_id)
                                    == Some(expected)
                            }
                            None => self
                                .host
                                .get_source(&document.canonical_id)
                                .is_some_and(|source| *source == *document.source),
                        }
                })
                .unwrap_or(false)
    }

    fn current_analysis_for_source_feature_capture(
        &self,
        capture: &SourceFeatureDocumentCapture,
    ) -> Option<verter_session::FileAnalysisSnapshot> {
        if self.semantic_generation_is_current(capture.semantic_generation) {
            if let Some(analysis) = capture
                .document
                .feature_snapshot
                .as_ref()
                .and_then(|feature| feature.analysis.as_ref())
                .filter(|analysis| {
                    analysis.document_revision == capture.document.document_revision
                        && analysis.semantic_generation == capture.semantic_generation
                })
            {
                return Some(analysis.analysis.as_ref().clone());
            }
            if let Some(snapshot) = self.semantic_snapshots.get(&capture.document.canonical_id) {
                if snapshot.document_revision == capture.document.document_revision
                    && snapshot.semantic_generation == capture.semantic_generation
                {
                    return Some(snapshot.analysis.as_ref().clone());
                }
            }
        }
        self.host
            .config()
            .effective_scope()
            .contains(verter_semantic::analysis::AnalysisScope::BUILD)
            .then(|| self.host.get_analysis(&capture.document.canonical_id))
            .flatten()
    }

    /// Build progressive analysis from one already-captured document view and
    /// the script facts read under that view's host revision witness.
    pub(crate) fn source_feature_analysis_from_capture(
        &self,
        capture: &SourceFeatureDocumentCapture,
        current_svelte_evidence: &verter_session::framework::script_facts::ScriptFactEvidence<
            verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts,
        >,
    ) -> Option<verter_session::FileAnalysisSnapshot> {
        let current = self.current_analysis_for_source_feature_capture(capture);
        let current_source = Arc::clone(&capture.document.source);
        let Some(progressive) = capture.document.progressive_analysis.as_ref() else {
            return current;
        };

        let prior = progressive.analysis();
        let prior_source = progressive.source();
        let mut stable_imports = prior
            .imports
            .iter()
            .filter(|import| {
                super::progressive_span_is_current(prior_source, &current_source, import.span)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut stable_bindings = prior
            .bindings
            .iter()
            .filter(|binding| {
                super::progressive_span_is_current(prior_source, &current_source, binding.span)
            })
            .cloned()
            .collect::<Vec<_>>();
        let stable_props = prior
            .template
            .as_deref()
            .into_iter()
            .flat_map(|template| template.prop_definitions.iter())
            .filter_map(|prop| {
                let witness = progressive.prop_owner_witness_for(prop)?;
                (super::progressive_prop_owner_witness_is_current(
                    prior_source,
                    &current_source,
                    witness,
                ) && super::exact_svelte_evidence_matches_prop_owner(
                    current_svelte_evidence,
                    witness,
                ))
                .then(|| {
                    let mut prop = prop.clone();
                    if !super::exact_svelte_evidence_reproves_callable_role(
                        current_svelte_evidence,
                        &prop.name,
                    ) {
                        prop.callable_role = verter_type_expr::PropCallableRole::Other;
                    }
                    prop
                })
            })
            .collect::<Vec<_>>();

        let mut result = current.unwrap_or_default();
        for import in stable_imports.drain(..) {
            if !result.imports.iter().any(|candidate| {
                candidate.span == import.span
                    && candidate.source == import.source
                    && candidate.bindings == import.bindings
            }) {
                result.imports.push(import);
            }
        }
        for binding in stable_bindings.drain(..) {
            if !result
                .bindings
                .iter()
                .any(|candidate| candidate.span == binding.span && candidate.name == binding.name)
            {
                result.bindings.push(binding);
            }
        }
        if !stable_props.is_empty() {
            let mut template = result.template.as_deref().cloned().unwrap_or_default();
            for prop in stable_props {
                if !template
                    .prop_definitions
                    .iter()
                    .any(|candidate| candidate.name == prop.name && candidate.span == prop.span)
                {
                    template.prop_definitions.push(prop);
                }
            }
            result.template = Some(Arc::new(template));
        }

        (!result.imports.is_empty()
            || !result.bindings.is_empty()
            || result.template.is_some()
            || !result.macros.is_empty())
        .then_some(result)
    }

    /// Current analysis enriched only with declarations from the last usable
    /// source revision whose complete authored producer evidence is unchanged.
    /// The whole read is fenced by one document identity and one host revision;
    /// a torn view is retried once and then fails closed.
    pub(crate) fn source_feature_analysis(
        &self,
        uri: &Uri,
    ) -> Option<verter_session::FileAnalysisSnapshot> {
        for _ in 0..2 {
            let Some(capture) = self.capture_source_feature_document(uri) else {
                continue;
            };
            if !self.source_feature_host_revision_is_current(&capture) {
                continue;
            }
            let evidence = self
                .host
                .resolve_svelte_script_facts(&capture.document.canonical_id);
            let analysis = self.source_feature_analysis_from_capture(&capture, &evidence);
            if self.source_feature_host_revision_is_current(&capture)
                && self.source_feature_capture_is_current(uri, &capture)
            {
                return analysis;
            }
        }
        None
    }

    /// Return an already-published optional semantic snapshot by canonical id.
    /// This is cache-only: callers never schedule or wait for enrichment.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn cached_semantic_analysis(
        &self,
        canonical_id: &str,
    ) -> Option<verter_session::FileAnalysisSnapshot> {
        let generation = self.current_semantic_generation();
        let result = self
            .semantic_generation_is_current(generation)
            .then(|| self.semantic_snapshots.get(canonical_id))
            .flatten()
            .filter(|snapshot| snapshot.semantic_generation == generation)
            .map(|snapshot| snapshot.analysis.as_ref().clone());
        result.filter(|_| self.semantic_generation_is_current(generation))
    }
}

/// Recursively convert semantic byte spans to the client-negotiated encoding.
///
/// One document publish rewrites every span in the analysis payload, so the
/// source is indexed once (shared `verter_ffi` owner) and every span converts
/// against that index instead of rescanning the prefix per span.
pub(super) fn convert_analysis_spans_json(
    value: &mut serde_json::Value,
    source: &str,
    encoding: &PositionEncodingKind,
) {
    let encoding = if *encoding == PositionEncodingKind::UTF16 {
        verter_ffi::convert::OffsetEncoding::Utf16
    } else if *encoding == PositionEncodingKind::UTF32 {
        verter_ffi::convert::OffsetEncoding::Utf32
    } else {
        verter_ffi::convert::OffsetEncoding::Utf8
    };
    if encoding == verter_ffi::convert::OffsetEncoding::Utf8 {
        return;
    }
    let index = verter_ffi::convert::OffsetIndex::new(source);
    convert_analysis_spans_with_index(value, &index, encoding);
}

fn convert_analysis_spans_with_index(
    value: &mut serde_json::Value,
    index: &verter_ffi::convert::OffsetIndex<'_>,
    encoding: verter_ffi::convert::OffsetEncoding,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if let Some(byte_offset) = val
                    .as_u64()
                    .filter(|_| key == "spanStart" || key == "spanEnd")
                {
                    let converted = index.convert(byte_offset as u32, encoding);
                    *val = serde_json::Value::Number(serde_json::Number::from(converted));
                } else {
                    convert_analysis_spans_with_index(val, index, encoding);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                convert_analysis_spans_with_index(value, index, encoding);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_semantic::analysis::AnalyzedPropDefinition;

    fn prop(name: &str, span: verter_span::Span) -> AnalyzedPropDefinition {
        AnalyzedPropDefinition {
            name: name.to_string(),
            callable_role: verter_type_expr::PropCallableRole::default(),
            type_annotation: Some("string".to_string()),
            has_default: false,
            is_required: false,
            is_boolean: false,
            used_in_template: true,
            used_in_script: true,
            span,
        }
    }

    #[test]
    fn semantic_prop_enrichment_preserves_authored_spans_and_native_only_rows() {
        let authored_span = verter_span::Span::new(11, 19);
        let untouched_span = verter_span::Span::new(25, 31);
        let mut native = vec![
            prop("enabled", authored_span),
            prop("nativeOnly", untouched_span),
        ];
        let role = verter_type_expr::PropCallableRole::SvelteSnippet {
            symbol: verter_type_expr::ResolvedSymbolIdentity {
                canonical_id: Arc::from("/node_modules/svelte/index.d.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                symbol: Arc::from("Snippet"),
            },
            exactness: verter_type_expr::ResolutionExactness::ExactSymbolic,
            provenance: verter_type_expr::ResolutionProvenance::FrameworkSurface,
        };
        let semantic = vec![AnalyzedPropDefinition {
            name: "enabled".to_string(),
            callable_role: role.clone(),
            type_annotation: Some("boolean".to_string()),
            has_default: true,
            is_required: true,
            is_boolean: true,
            used_in_template: false,
            used_in_script: false,
            span: verter_span::Span::new(0, 0),
        }];

        merge_semantic_prop_definitions(&mut native, semantic);

        assert_eq!(native.len(), 2, "native-only declarations stay published");
        assert_eq!(native[0].span, authored_span);
        assert!(native[0].used_in_template && native[0].used_in_script);
        assert!(native[0].is_boolean && native[0].has_default && native[0].is_required);
        assert_eq!(native[0].callable_role, role);
        assert_eq!(native[1].name, "nativeOnly");
        assert_eq!(native[1].span, untouched_span);
    }

    /// The negotiated-encoding walker converts `expressionDiagnostics` spans:
    /// a non-ASCII document with a malformed template expression must publish
    /// UTF-16 offsets on that field, not raw UTF-8 byte offsets. Discriminates
    /// the nested-`span` wire shape, which the walker (keyed on
    /// `spanStart`/`spanEnd`) would silently leave unconverted.
    #[test]
    fn expression_diagnostic_spans_convert_to_negotiated_encoding() {
        // "é" is 2 UTF-8 bytes / 1 UTF-16 unit; four of them shift the
        // diagnostic span by 4 units between the two encodings.
        let source = "éééé{{ count === }}";
        let diag_start = source.find("count").unwrap() as u32;
        let diag_end = diag_start + "count ===".len() as u32;
        let snapshot = verter_semantic::analysis::template::TemplateAnalysisSnapshot {
            expression_diagnostics: vec![
                verter_semantic::analysis::template::TemplateExpressionDiagnostic {
                    severity:
                        verter_semantic::analysis::template::TemplateDiagnosticSeverity::Error,
                    code: "XInvalidExpression".to_string(),
                    message: "invalid expression".to_string(),
                    span: verter_span::Span::new(diag_start, diag_end),
                },
            ],
            ..Default::default()
        };
        let mut json = serde_json::to_value(&snapshot).expect("serialize snapshot");
        convert_analysis_spans_json(&mut json, source, &PositionEncodingKind::UTF16);
        let diag = &json["expressionDiagnostics"][0];
        assert_eq!(
            diag["spanStart"],
            diag_start - 4,
            "spanStart must be converted to UTF-16 units"
        );
        assert_eq!(
            diag["spanEnd"],
            diag_end - 4,
            "spanEnd must be converted to UTF-16 units"
        );
        assert!(
            diag.get("span").is_none(),
            "no nested span object may reach the wire"
        );
    }
}
