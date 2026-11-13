//! The participation gate of every semantic custom method.
//!
//! Each public method admits its own capability cell before its body runs
//! and serves the answer only while that demand epoch is current, exactly
//! like the standard requests. Inspection displays are `Explain` demands, so
//! a `workspace` cell refuses them like any other interactive presentation;
//! a document drop produces an edit and is `Interactive`. Control-plane
//! methods (file notifications, statistics, audit-record reads) are not
//! semantic capabilities and stay ungated.

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{Uri, WorkspaceEdit};
use verter_language::participation::{CapabilityOperation as Op, DemandPurpose};

use super::super::protocol_types::*;
use super::super::VerterLanguageServer;

/// The document a string-addressed custom request names, when it parses.
fn document(uri: &str) -> Option<Uri> {
    uri.parse().ok()
}

impl VerterLanguageServer {
    async fn explain<T>(
        &self,
        operation: Op,
        uri: Option<&Uri>,
        answer: impl std::future::Future<Output = Result<T>>,
    ) -> Result<T> {
        self.participating(operation, uri, DemandPurpose::Explain, answer)
            .await
    }

    /// `$/verter/documentStructure`.
    pub async fn document_structure(
        &self,
        params: DocumentStructureRequestV1,
    ) -> Result<DocumentStructureResponseV1> {
        let uri = params.text_document.uri.clone();
        self.explain(
            Op::DocumentStructure,
            Some(&uri),
            self.document_structure_admitted(params),
        )
        .await
    }

    /// `$/getCompiledCode`.
    pub async fn get_compiled_code(
        &self,
        params: GetCompiledCodeParams,
    ) -> Result<Option<CompiledCodeResponse>> {
        let uri = document(&params.uri);
        self.explain(
            Op::CompiledCodeInspection,
            uri.as_ref(),
            self.get_compiled_code_admitted(params),
        )
        .await
    }

    /// `$/verter/documentDropEdit`.
    pub async fn document_drop_edit(
        &self,
        params: DocumentDropEditParams,
    ) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document.uri.clone();
        self.interactive(
            Op::DocumentDropEdit,
            Some(&uri),
            self.document_drop_edit_admitted(params),
        )
        .await
    }

    /// `$/verter/getVirtualFiles`.
    pub async fn get_virtual_files(
        &self,
        params: GetVirtualFilesParams,
    ) -> Result<Option<VirtualFilesResponse>> {
        let uri = document(&params.uri);
        self.explain(
            Op::VirtualFileInspection,
            uri.as_ref(),
            self.get_virtual_files_admitted(params),
        )
        .await
    }

    /// `$/verter/getAnalysis`.
    pub async fn get_analysis(
        &self,
        params: GetAnalysisParams,
    ) -> Result<Option<serde_json::Value>> {
        let uri = document(&params.uri);
        self.explain(
            Op::AnalysisInspection,
            uri.as_ref(),
            self.get_analysis_admitted(params),
        )
        .await
    }

    /// `$/verter/getProjectOverview`: workspace-wide.
    pub async fn get_project_overview(
        &self,
        params: serde_json::Value,
    ) -> Result<ProjectOverviewResponse> {
        self.explain(
            Op::ProjectOverview,
            None,
            self.get_project_overview_admitted(params),
        )
        .await
    }

    /// `$/verter/getRouteTree`: workspace-wide.
    pub async fn get_route_tree(&self, params: serde_json::Value) -> Result<serde_json::Value> {
        self.explain(Op::RouteTree, None, self.get_route_tree_admitted(params))
            .await
    }

    /// `$/verter/getBindingTypes`.
    pub async fn get_binding_types(&self, params: GetAnalysisParams) -> Result<serde_json::Value> {
        let uri = document(&params.uri);
        self.explain(
            Op::BindingTypeInspection,
            uri.as_ref(),
            self.get_binding_types_admitted(params),
        )
        .await
    }

    /// `$/verter/getComponentParents`.
    pub async fn get_component_parents(
        &self,
        params: GetComponentParentsParams,
    ) -> Result<ComponentParentsResponse> {
        let uri = document(&params.uri);
        self.explain(
            Op::ComponentParents,
            uri.as_ref(),
            self.get_component_parents_admitted(params),
        )
        .await
    }

    /// `$/verter/getComponentMeta`.
    pub async fn get_component_meta(
        &self,
        params: GetComponentMetaParams,
    ) -> Result<Option<serde_json::Value>> {
        let uri = document(&params.uri);
        self.explain(
            Op::ComponentMetadata,
            uri.as_ref(),
            self.get_component_meta_admitted(params),
        )
        .await
    }

    /// `$/verter/getComponentMetaSurface`.
    pub async fn get_component_meta_surface(
        &self,
        params: GetComponentMetaSurfaceParams,
    ) -> Result<Option<Vec<u8>>> {
        let uri = document(&params.uri);
        self.explain(
            Op::ComponentMetaSurface,
            uri.as_ref(),
            self.get_component_meta_surface_admitted(params),
        )
        .await
    }

    /// `$/verter/getComponentMetaTypeExpansion`: a type handle carries no
    /// document, so the expansion is admitted against the served profiles.
    pub async fn get_component_meta_type_expansion(
        &self,
        params: GetComponentMetaTypeExpansionParams,
    ) -> Result<GetComponentMetaTypeExpansionResponse> {
        self.explain(
            Op::TypeHandleExpansion,
            None,
            self.get_component_meta_type_expansion_admitted(params),
        )
        .await
    }
}

impl VerterLanguageServer {
    /// Bring the client's dynamic registrations in line with the current
    /// participation mask: a method no served profile admits is
    /// unregistered, a re-admitted one registered again.
    ///
    /// The transaction runs detached: a cancelled request that started it
    /// never drops a client request in flight before its bookkeeping is
    /// recorded, so the recorded registrations always match the client's.
    pub(crate) async fn reconcile_registrations(&self) {
        if self.dynamic_registrations.get().is_none() {
            return;
        }
        let server = self.clone();
        let transaction = tokio::spawn(async move {
            if let Some(registrations) = server.dynamic_registrations.get() {
                registrations
                    .reconcile(&server.client, &server.documents)
                    .await;
            }
        });
        if let Err(error) = transaction.await {
            tracing::warn!("participation registration task failed: {error}");
        }
    }

    /// `$/verter/updateParticipation`: replace this scope's participation
    /// input whole. An effective change publishes the new mask and demand
    /// epoch before any newer demand is admitted. Every open document then
    /// republishes under it (withdrawn facets clear, re-admitted ones refill),
    /// a document leaving a disabled scope receives the open work it was
    /// owed, and pulled decorations are re-requested where the client
    /// supports a refresh. An equivalent input changes nothing.
    pub async fn update_participation(
        &self,
        input: verter_protocol::participation::ParticipationInput,
    ) -> Result<crate::participation::ParticipationReceipt> {
        // The edit-commit fence is taken before the open documents are read
        // and the mask changes, so no await separates accepting the input
        // from republishing under it (a cancelled request never leaves a
        // live mask unpublished), and an open or close committing while this
        // update waited is seen by every step below.
        let edit_commit = self.did_change_mutex.lock().await;
        let open: Vec<Uri> = self
            .documents
            .open_uris()
            .iter()
            .filter_map(|uri| uri.parse().ok())
            .filter(|uri| self.documents.get_virtual_source_uri(uri).is_none())
            .collect();
        let was_disabled: Vec<bool> = open
            .iter()
            .map(|uri| crate::participation::document_owes_no_eager_work(&self.documents, uri))
            .collect();
        let (transition, receipt) = crate::participation::accept(&self.documents, &input)?;
        // A document whose bytes were retained only gets the open it was
        // owed, on its newest bytes, before any work is re-armed for it. Also
        // on an unchanged mask (and on the document's next edit), so no
        // retained document outlives its scope's re-admission.
        for uri in self.documents.retained_uris() {
            if !crate::participation::document_disables_every_cell(&self.documents, &uri) {
                let _ = self.documents.materialize_retained(&uri);
            }
        }
        drop(edit_commit);
        if transition.advanced() {
            let now = tokio::time::Instant::now();
            for (uri, was_disabled) in open.iter().zip(was_disabled) {
                let canonical_id = crate::documents::uri_to_canonical_id(uri);
                let uri_str = uri.as_str().to_string();
                if was_disabled
                    && !crate::participation::document_owes_no_eager_work(&self.documents, uri)
                {
                    // Its open and edits skipped every eager step: re-arm the
                    // sync work bits, the dependency tracking and the
                    // import-dependency receipt an open owes, or the
                    // coordinator would skip the provider sync and a
                    // diagnostics pull could wait on a receipt nothing mints.
                    self.touch_mru(&canonical_id);
                    if crate::server::carrier_language_for(
                        self.documents.language_classifier(),
                        &canonical_id,
                    )
                    .is_some()
                    {
                        self.refresh_carrier_dependency_tracking(&canonical_id);
                    }
                    self.documents.schedule_semantic_analysis(uri);
                    self.needs_ide_sync.insert(canonical_id.clone());
                    self.needs_deferred_sync.insert(canonical_id.clone());
                    self.sync_coordinator.signal(canonical_id, uri_str, now);
                    self.spawn_import_dependency_publication(
                        uri,
                        super::super::import_publication::PublicationUrgency::Immediate,
                    );
                } else {
                    self.sync_coordinator
                        .signal_diagnostics_only(canonical_id, uri_str, now);
                }
            }
            self.refresh_deferred_decorations();
        }
        // Also on an unchanged mask: a registration the client refused
        // earlier is retried here.
        self.reconcile_registrations().await;
        Ok(receipt)
    }
}
