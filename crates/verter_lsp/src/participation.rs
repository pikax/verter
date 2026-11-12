//! The LSP carrier of per-profile participation (COX0; contract
//! `docs/arch/kernel/cox0d-participation.md`).
//!
//! The server owns one `verter_session::participation::ParticipationService`
//! for its editor scope. The host declares participation in
//! `initializationOptions.participation` and replaces it whole with
//! [`UPDATE_PARTICIPATION_METHOD`]; both carry the
//! `verter_protocol::participation` input, never editor extension ids.
//! Every semantic request and every publication admits its capability cell
//! here before any producer starts, and serves its answer only while the
//! demand epoch it was admitted under is current. Static capability
//! advertisement is never permission: a withdrawn cell is refused even when
//! the client keeps sending the method.

use std::borrow::Cow;

use tower_lsp_server::jsonrpc;
use tower_lsp_server::ls_types::{Position, Uri};
use verter_identity::identity::SourceUnitId;
use verter_language::participation::{
    AdmissionRequest, CapabilityId, CapabilityOperation, CatalogSupport, DemandPurpose,
    EffectiveMode, NonExecution, ProfileRef, ProviderRequirement, RequestSite,
};
use verter_protocol::participation::{
    DecodedParticipation, InvalidEvidence, ParticipationInput, ParticipationInputError,
    ProfileInput, ProfileKind,
};
use verter_session::participation::{
    file_profile, file_unit, language_profile, served_profiles, HostParticipationResolver,
    ParticipationPermit, Transition,
};

use crate::documents::{uri_to_canonical_id_from_str, DocumentRegistry};

/// The request a host replaces its whole participation input with.
pub const UPDATE_PARTICIPATION_METHOD: &str = "$/verter/updateParticipation";

/// LSP `RequestFailed`: the typed answer of a refused demand. Its `data`
/// names the primary non-execution reason and every restriction, so a
/// client never reads a withdrawal as an empty result.
const REQUEST_FAILED: i64 = -32803;

/// The refusal a client receives for a withdrawn, unsupported or otherwise
/// ineligible cell.
pub(crate) fn refusal_error(refused: NonExecution) -> jsonrpc::Error {
    let restrictions: Vec<&str> = refused
        .restrictions
        .iter()
        .map(|reason| reason.key())
        .collect();
    jsonrpc::Error {
        code: jsonrpc::ErrorCode::ServerError(REQUEST_FAILED),
        message: Cow::Owned(format!(
            "verter does not serve this request: {}",
            refused.primary.key()
        )),
        data: Some(serde_json::json!({
            "verterParticipation": {
                "primary": refused.primary.key(),
                "restrictions": restrictions,
            }
        })),
    }
}

/// The answer of work admitted under a superseded demand epoch.
pub(crate) fn superseded_error() -> jsonrpc::Error {
    jsonrpc::Error::new(jsonrpc::ErrorCode::ContentModified)
}

/// The site of one document demand: its participation profile and unit.
pub(crate) struct DocumentSite {
    capability_profile: ProfileRef,
    unit: SourceUnitId,
    /// The carrier-minted role of the region a positional demand lands in;
    /// `None` for a document-wide demand.
    region_role: Option<&'static str>,
}

impl DocumentSite {
    /// The site of a document: an open document is qualified by its
    /// authoritative resolved language (an unsaved carrier has no carrier
    /// path), any other by its path.
    pub(crate) fn of(documents: &DocumentRegistry, uri: &Uri) -> Self {
        let (canonical, language) = documents.participation_subject(uri);
        Self {
            capability_profile: language_profile(documents.host(), &language),
            unit: file_unit(&canonical),
            region_role: None,
        }
    }

    pub(crate) fn of_canonical(documents: &DocumentRegistry, canonical: &str) -> Self {
        match documents.canonical_id_to_uri(canonical) {
            Some(uri) => Self::of(documents, &uri),
            None => Self {
                capability_profile: file_profile(documents.host(), canonical),
                unit: file_unit(canonical),
                region_role: None,
            },
        }
    }

    /// The site of a positional demand: the document's site, in the region
    /// `position` lands in. A stylesheet-region withdrawal therefore refuses
    /// a hover in a `<style>` section while template hover stays served.
    pub(crate) fn at(documents: &DocumentRegistry, uri: &Uri, position: &Position) -> Self {
        Self {
            region_role: region_role_at(documents, uri, position),
            ..Self::of(documents, uri)
        }
    }

    pub(crate) fn capability(&self, operation: CapabilityOperation) -> CapabilityId {
        CapabilityId::new(self.capability_profile.clone(), operation)
    }

    pub(crate) fn request_site(&self) -> RequestSite<'_> {
        RequestSite {
            unit: Some(&self.unit),
            region_role: self.region_role,
        }
    }
}

/// The carrier-minted role of the section `position` lands in, read from the
/// document's registered structure inventory: `style`, `template` or
/// `script`. Outside every section, in a custom section, or before the
/// document's structure is registered, the demand is document-wide (`None`).
pub(crate) fn region_role_at(
    documents: &DocumentRegistry,
    uri: &Uri,
    position: &Position,
) -> Option<&'static str> {
    use verter_language::parse_artifact::carrier_inventory::{CarrierBlock, SectionRole};
    let document = documents.get(uri)?;
    let snapshot = document.feature_snapshot.as_ref()?;
    let offset = document.line_index.position_to_offset(position)?;
    let role = snapshot
        .structure()
        .inventory()
        .blocks()
        .iter()
        .find_map(|block| match block {
            CarrierBlock::Section { role, syntax, .. }
                if syntax.content_span.start <= offset && offset <= syntax.content_span.end =>
            {
                Some(role)
            }
            _ => None,
        })?;
    match role {
        SectionRole::Style { .. } => Some("style"),
        SectionRole::TemplateHost => Some("template"),
        SectionRole::Script { .. } => Some("script"),
        SectionRole::Custom { .. } => None,
    }
}

/// Admit one demand of `operation` on `uri`. A workspace-wide demand (no
/// document) spans every profile this host serves, so it is admitted only
/// when every served profile admits it: one withdrawn profile must never be
/// served through another profile's admission.
pub(crate) fn admit(
    documents: &DocumentRegistry,
    operation: CapabilityOperation,
    uri: Option<&Uri>,
    purpose: DemandPurpose,
    provider: ProviderRequirement,
) -> Result<ParticipationPermit, NonExecution> {
    admit_at(documents, operation, uri, None, purpose, provider)
}

/// [`admit`], for a demand at `position` in its document: admitted against
/// the cell of the region the position lands in.
pub(crate) fn admit_at(
    documents: &DocumentRegistry,
    operation: CapabilityOperation,
    uri: Option<&Uri>,
    position: Option<&Position>,
    purpose: DemandPurpose,
    provider: ProviderRequirement,
) -> Result<ParticipationPermit, NonExecution> {
    let service = documents.participation();
    let request = |capability: &CapabilityId, site: RequestSite<'_>| {
        service.admit(&AdmissionRequest {
            capability,
            site,
            purpose,
            provider,
            catalog: CatalogSupport::Supported,
        })
    };
    match uri {
        Some(uri) => {
            let site = match position {
                Some(position) => DocumentSite::at(documents, uri, position),
                None => DocumentSite::of(documents, uri),
            };
            request(&site.capability(operation), site.request_site())
        }
        None => {
            let mut admitted = None;
            for profile in served_profiles(documents.host()) {
                admitted = Some(request(
                    &CapabilityId::new(profile, operation),
                    RequestSite::default(),
                )?);
            }
            Ok(admitted.expect("a host serves at least its script carrier profiles"))
        }
    }
}

/// Whether some served profile may admit an interactive demand of
/// `operation` at some site: the condition a dynamically registered method
/// stays registered under. A withdrawal narrower than a whole profile, or a
/// profile-wide withdrawal with a narrower re-admission, keeps it registered
/// and is decided per request instead.
pub(crate) fn admitted_anywhere(
    documents: &DocumentRegistry,
    operation: CapabilityOperation,
) -> bool {
    let state = documents.participation().state();
    served_profiles(documents.host())
        .into_iter()
        .any(|profile| state.mask().may_admit_interactive(&profile, operation))
}

/// Whether a demand of `operation` with no document is admitted: every
/// served profile admits it profile-wide, as [`admit`] requires of a
/// workspace-wide request.
pub(crate) fn admitted_workspace_wide(
    documents: &DocumentRegistry,
    operation: CapabilityOperation,
) -> bool {
    let state = documents.participation().state();
    served_profiles(documents.host())
        .into_iter()
        .all(|profile| {
            state
                .mask()
                .admit(&AdmissionRequest {
                    capability: &CapabilityId::new(profile, operation),
                    site: RequestSite::default(),
                    purpose: DemandPurpose::Interactive,
                    provider: ProviderRequirement::NotRequired,
                    catalog: CatalogSupport::Supported,
                })
                .is_ok()
        })
}

/// Whether the scope of a document of `language` at `canonical` disables
/// every cell: an opening of it retains bytes only (PC04).
pub(crate) fn language_disables_every_cell(
    documents: &DocumentRegistry,
    language: &verter_language::FileLanguage,
    canonical: &str,
) -> bool {
    let unit = file_unit(canonical);
    documents
        .participation()
        .state()
        .mask()
        .disables_every_cell(
            &language_profile(documents.host(), language),
            RequestSite {
                unit: Some(&unit),
                region_role: None,
            },
        )
}

/// Whether this open document's scope disables every cell.
pub(crate) fn document_disables_every_cell(documents: &DocumentRegistry, uri: &Uri) -> bool {
    let site = DocumentSite::of(documents, uri);
    documents
        .participation()
        .state()
        .mask()
        .disables_every_cell(&site.capability_profile, site.request_site())
}

/// Whether no facet of this document's scope is `Full`: the scope admits at
/// most explicitly demanded workspace semantics, so no eager open, edit,
/// provider or publication work is owed to it (DEM0 `DEM-D06`).
pub(crate) fn document_owes_no_eager_work(documents: &DocumentRegistry, uri: &Uri) -> bool {
    let site = DocumentSite::of(documents, uri);
    let state = documents.participation().state();
    state
        .mask()
        .admits_no_full_cell(&site.capability_profile, site.request_site())
}

/// Whether `operation` on `uri` is admitted for `purpose` right now: the
/// gate of a supplement computed inside another admitted request.
pub(crate) fn admits(
    documents: &DocumentRegistry,
    operation: CapabilityOperation,
    uri: &Uri,
    purpose: DemandPurpose,
) -> bool {
    admit(
        documents,
        operation,
        Some(uri),
        purpose,
        ProviderRequirement::NotRequired,
    )
    .is_ok()
}

/// Whether `operation` at `position` in `uri` is admitted for `purpose`
/// right now: the gate of one produced item of a document-wide answer, by
/// the region it lands in.
pub(crate) fn admits_at(
    documents: &DocumentRegistry,
    operation: CapabilityOperation,
    uri: &Uri,
    position: &Position,
    purpose: DemandPurpose,
) -> bool {
    admit_at(
        documents,
        operation,
        Some(uri),
        Some(position),
        purpose,
        ProviderRequirement::NotRequired,
    )
    .is_ok()
}

/// The completion-item data key carrying the participation fence the item
/// was issued under.
const COMPLETION_FENCE_KEY: &str = "verterParticipationFence";

/// Stamp every completion item that carries resolve data with the fence it
/// was issued under, so a resolve after any participation transition is
/// refused instead of reviving the item's basis.
pub(crate) fn stamp_completion(
    response: tower_lsp_server::ls_types::CompletionResponse,
    fence: u64,
) -> tower_lsp_server::ls_types::CompletionResponse {
    use tower_lsp_server::ls_types::CompletionResponse;
    let stamp = |items: &mut Vec<tower_lsp_server::ls_types::CompletionItem>| {
        for item in items {
            if let Some(serde_json::Value::Object(data)) = item.data.as_mut() {
                data.insert(COMPLETION_FENCE_KEY.to_owned(), serde_json::json!(fence));
            }
        }
    };
    match response {
        CompletionResponse::Array(mut items) => {
            stamp(&mut items);
            CompletionResponse::Array(items)
        }
        CompletionResponse::List(mut list) => {
            stamp(&mut list.items);
            CompletionResponse::List(list)
        }
    }
}

/// Whether a completion item was issued under a superseded demand epoch.
pub(crate) fn completion_item_superseded(
    documents: &DocumentRegistry,
    item: &tower_lsp_server::ls_types::CompletionItem,
) -> bool {
    item.data
        .as_ref()
        .and_then(|data| data.get(COMPLETION_FENCE_KEY))
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|fence| fence != documents.participation().fence())
}

/// What the host learns from one accepted input.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipationReceipt {
    /// Lower-hex identity of the published mask.
    pub mask: String,
    /// The retirement fence of the current demand epoch.
    pub fence: u64,
    /// Whether this input changed the effective mask.
    pub changed: bool,
    /// Observations that established nothing, by index and cause.
    pub invalid_evidence: Vec<String>,
    /// The effective interactive cell of every facet of every served
    /// semantic profile, document-wide and in stylesheet regions. A host
    /// gates its own client-local producers on these cells and never
    /// re-decides participation itself.
    pub cells: Vec<ReceiptCell>,
}

/// One effective cell, as a host consumes it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptCell {
    pub profile: ProfileInput,
    pub operation: String,
    /// `None` for the document-wide cell, `Some("style")` for stylesheet regions.
    pub region_role: Option<String>,
    /// `disabled`, `workspaceOnly` or `full`.
    pub mode: String,
    /// Whether an interactive demand of this cell is admitted.
    pub admitted: bool,
}

/// The facets a host can serve from a stylesheet region of a carrier.
const STYLE_REGION_OPERATIONS: [CapabilityOperation; 6] = [
    CapabilityOperation::Hover,
    CapabilityOperation::Completion,
    CapabilityOperation::DocumentHighlight,
    CapabilityOperation::DocumentColor,
    CapabilityOperation::ColorPresentation,
    CapabilityOperation::LintDiagnostics,
];

/// The receipt of this scope's current state.
pub(crate) fn current_receipt(documents: &DocumentRegistry) -> ParticipationReceipt {
    let state = documents.participation().state();
    let mask = state.mask();
    let mut cells = Vec::new();
    for profile in served_profiles(documents.host()) {
        if !matches!(profile, ProfileRef::Framework(_)) {
            continue;
        }
        let profile_input = ProfileInput {
            kind: ProfileKind::Framework,
            id: profile.digest().to_hex(),
        };
        for &operation in CapabilityOperation::ALL {
            for role in [None, Some("style")] {
                if role.is_some() && !STYLE_REGION_OPERATIONS.contains(&operation) {
                    continue;
                }
                let capability = CapabilityId::new(profile.clone(), operation);
                let site = RequestSite {
                    unit: None,
                    region_role: role,
                };
                let mode = match mask.cell(&capability, site).mode {
                    EffectiveMode::Disabled => "disabled",
                    EffectiveMode::WorkspaceOnly => "workspaceOnly",
                    EffectiveMode::Full => "full",
                };
                let admitted = mask
                    .admit(&AdmissionRequest {
                        capability: &capability,
                        site,
                        purpose: DemandPurpose::Interactive,
                        provider: ProviderRequirement::NotRequired,
                        catalog: CatalogSupport::Supported,
                    })
                    .is_ok();
                cells.push(ReceiptCell {
                    profile: profile_input.clone(),
                    operation: operation.key().to_owned(),
                    region_role: role.map(str::to_owned),
                    mode: mode.to_owned(),
                    admitted,
                });
            }
        }
    }
    ParticipationReceipt {
        mask: state.epoch().mask().digest().to_hex(),
        fence: state.epoch().fence(),
        changed: false,
        invalid_evidence: Vec::new(),
        cells,
    }
}

/// Decode a host input against this server's authorities.
pub(crate) fn decode(
    documents: &DocumentRegistry,
    input: &ParticipationInput,
) -> Result<DecodedParticipation, ParticipationInputError> {
    let resolver = HostParticipationResolver::new(documents.host(), |uri: &str| {
        Some(uri_to_canonical_id_from_str(uri))
    });
    input.decode(&resolver)
}

/// Decode and publish one host input.
pub(crate) fn accept(
    documents: &DocumentRegistry,
    input: &ParticipationInput,
) -> Result<(Transition, ParticipationReceipt), jsonrpc::Error> {
    let decoded = decode(documents, input)
        .map_err(|error| invalid_params(format!("participation input refused: {error:?}")))?;
    publish(documents, decoded)
}

/// Publish a decoded input and describe the result.
pub(crate) fn publish(
    documents: &DocumentRegistry,
    decoded: DecodedParticipation,
) -> Result<(Transition, ParticipationReceipt), jsonrpc::Error> {
    let DecodedParticipation { inputs, invalid } = decoded;
    let transition = documents
        .participation()
        .publish_host_inputs(inputs)
        .map_err(|error| invalid_params(format!("participation input refused: {error:?}")))?;
    let mut receipt = current_receipt(documents);
    receipt.changed = transition.advanced();
    receipt.invalid_evidence = invalid.iter().map(describe_invalid).collect();
    Ok((transition, receipt))
}

fn describe_invalid(invalid: &InvalidEvidence) -> String {
    match invalid {
        InvalidEvidence::Ownership { index, cause } => format!("ownership[{index}]: {cause}"),
        InvalidEvidence::UnknownClientOperation(key) => {
            format!("unsupportedClientOperations: unknown operation {key}")
        }
    }
}

pub(crate) fn invalid_params(message: String) -> jsonrpc::Error {
    jsonrpc::Error {
        code: jsonrpc::ErrorCode::InvalidParams,
        message: Cow::Owned(message),
        data: None,
    }
}

/// Feed this scope the selected TypeScript authority's availability from
/// its owner (COX0 `ProviderUnavailable`). Verter-only mode selects no
/// authority; an editor-owned one is the editor's to report. A provider whose
/// one lifecycle gives up for the session withdraws it then, and every open
/// document republishes: its TypeScript facet refuses as
/// `providerUnavailable` while its Verter facets keep publishing.
pub(crate) fn observe_provider_availability(
    documents: &std::sync::Arc<DocumentRegistry>,
    kind: crate::TypeProviderKind,
    provider: Option<&dyn crate::TypeProvider>,
    coordinator: &crate::sync_coordinator::SyncCoordinatorHandle,
) {
    use verter_language::participation::ProviderAvailability;
    let Some(provider) = provider else {
        if kind != crate::TypeProviderKind::EditorTsserver {
            let _ = documents
                .participation()
                .publish_provider(ProviderAvailability::Unavailable);
        }
        return;
    };
    let Some(mut exhaustion) = provider.exhaustion() else {
        return;
    };
    // Weak on both sides: the observer must not keep the session (or, through
    // the coordinator, the provider and this watch's sender) alive.
    let documents = std::sync::Arc::downgrade(documents);
    let coordinator = coordinator.downgrade();
    tokio::spawn(async move {
        // A dropped lifecycle owner reports nothing more.
        if exhaustion.wait_for(|exhausted| *exhausted).await.is_err() {
            return;
        }
        let (Some(documents), Some(coordinator)) = (documents.upgrade(), coordinator.upgrade())
        else {
            return;
        };
        let transition = documents
            .participation()
            .publish_provider(ProviderAvailability::Unavailable);
        if !transition.advanced() {
            return;
        }
        let now = tokio::time::Instant::now();
        for uri in documents.open_uris() {
            let Ok(parsed) = uri.parse::<Uri>() else {
                continue;
            };
            if documents.get_virtual_source_uri(&parsed).is_some() {
                continue;
            }
            let canonical = crate::documents::uri_to_canonical_id(&parsed);
            coordinator.signal_diagnostics_only(canonical, uri, now);
        }
    });
}

/// The diagnostic facets one document's publication may carry (DEM0
/// `DEM-D01`): each producer runs only when its own cell admits
/// `DiagnosticsPublication`, so a withdrawn facet costs no producer work
/// and publishes nothing, while the document's other facets still publish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DiagnosticFacets {
    /// Compiler, projection-limit and environment diagnostics.
    pub(crate) compiler: bool,
    pub(crate) component_usage: bool,
    pub(crate) lint: bool,
    /// The selected TypeScript authority's diagnostics.
    pub(crate) typescript: bool,
}

impl DiagnosticFacets {
    #[cfg(test)]
    pub(crate) const ALL: Self = Self {
        compiler: true,
        component_usage: true,
        lint: true,
        typescript: true,
    };

    pub(crate) fn admitted(documents: &DocumentRegistry, canonical: &str) -> Self {
        let site = DocumentSite::of_canonical(documents, canonical);
        let service = documents.participation();
        let admits = |operation, provider| {
            service
                .admit(&AdmissionRequest {
                    capability: &site.capability(operation),
                    site: site.request_site(),
                    purpose: DemandPurpose::DiagnosticsPublication,
                    provider,
                    catalog: CatalogSupport::Supported,
                })
                .is_ok()
        };
        let verter = ProviderRequirement::NotRequired;
        Self {
            compiler: admits(CapabilityOperation::CompilerDiagnostics, verter),
            component_usage: admits(CapabilityOperation::ComponentUsageDiagnostics, verter),
            lint: admits(CapabilityOperation::LintDiagnostics, verter),
            // Only this facet needs the selected TypeScript authority: its
            // absence refuses it as `providerUnavailable` and never touches
            // the Verter facets.
            typescript: admits(
                CapabilityOperation::TypeScriptDiagnostics,
                ProviderRequirement::TypeScript,
            ),
        }
    }

    /// Whether any Verter-owned producer may run.
    pub(crate) fn any_verter(self) -> bool {
        self.compiler || self.component_usage || self.lint
    }

    /// Whether every Verter-owned producer may run: the complete Verter set,
    /// whatever the TypeScript facet admits.
    pub(crate) fn all_verter(self) -> bool {
        self.compiler && self.component_usage && self.lint
    }
}

/// The cell a produced code action belongs to, by its LSP kind: refactors,
/// organize-imports, and every other fix (quick fixes, fix-all, commands).
pub(crate) fn code_action_operation(
    action: &tower_lsp_server::ls_types::CodeActionOrCommand,
) -> CapabilityOperation {
    use tower_lsp_server::ls_types::CodeActionOrCommand;
    match action {
        CodeActionOrCommand::CodeAction(action) => action
            .kind
            .as_ref()
            .map_or(CapabilityOperation::QuickFix, code_action_kind_operation),
        CodeActionOrCommand::Command(_) => CapabilityOperation::QuickFix,
    }
}

/// The cell of one LSP code-action kind.
pub(crate) fn code_action_kind_operation(
    kind: &tower_lsp_server::ls_types::CodeActionKind,
) -> CapabilityOperation {
    use tower_lsp_server::ls_types::CodeActionKind;
    let kind = kind.as_str();
    if kind.starts_with(CodeActionKind::REFACTOR.as_str()) {
        CapabilityOperation::Refactor
    } else if kind.starts_with(CodeActionKind::SOURCE_ORGANIZE_IMPORTS.as_str()) {
        CapabilityOperation::OrganizeImports
    } else {
        CapabilityOperation::QuickFix
    }
}

/// The LSP code-action kind a code-action cell's producers are keyed by.
pub(crate) fn code_action_kind(
    operation: CapabilityOperation,
) -> tower_lsp_server::ls_types::CodeActionKind {
    use tower_lsp_server::ls_types::CodeActionKind;
    match operation {
        CapabilityOperation::Refactor => CodeActionKind::REFACTOR,
        CapabilityOperation::OrganizeImports => CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
        _ => CodeActionKind::QUICKFIX,
    }
}
