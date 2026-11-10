//! The participation gate of every semantic custom method (COX0).
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
        if transition.advanced() {
            let now = tokio::time::Instant::now();
            for (uri, was_disabled) in open.iter().zip(was_disabled) {
                let canonical_id = crate::documents::uri_to_canonical_id(uri);
                let uri_str = uri.as_str().to_string();
                if was_disabled
                    && !crate::participation::document_owes_no_eager_work(&self.documents, uri)
                {
                    self.documents.schedule_semantic_analysis(uri);
                    self.sync_coordinator.signal(canonical_id, uri_str, now);
                } else {
                    self.sync_coordinator
                        .signal_diagnostics_only(canonical_id, uri_str, now);
                }
            }
            self.refresh_deferred_decorations();
        }
        Ok(receipt)
    }
}
