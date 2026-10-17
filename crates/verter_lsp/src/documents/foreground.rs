//! The foreground request context: one immutable capture of the inputs a
//! request answers from, taken at admission, and the one disposition that
//! decides whether the answer computed against it reaches the client.
//!
//! Three questions are kept apart:
//! - coherence: the answer was computed against one admitted revision of the
//!   requested document and one project authority, and every provider answer
//!   inside it was decoded through a provider surface whose content epoch,
//!   incarnation and owner epoch held from its capture to the answer's decode
//!   (a failed decode bracket drops that provider contribution) and still hold
//!   at settlement (the request keeps every surface it decoded through), and
//!   every imported source a native contribution was read from is still
//!   committed with the content it was read at, and every published child
//!   contract it was read through still validates against the producer read
//!   sets it was derived from;
//! - applicability: the admitted revision and authority still describe what
//!   the client holds when the answer is delivered — the disposition below;
//! - publication freshness: whether background diagnostics are current. That
//!   is the diagnostics publication's question (`ReadinessBasis`), never a
//!   foreground one, so a diagnostics-generation move cannot cost a request its
//!   answer.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tower_lsp_server::ls_types::{CodeAction, CodeActionOrCommand, Uri, WorkspaceEdit};

use super::{uri_to_canonical_id, DocumentRegistry, DocumentSnapshotIdentity};
use crate::features::action_utils::{
    bind_workspace_edit, EditRefusal, EditTargetRevision, WorkspaceEditSupport,
};
use crate::provider_surface_store::ProviderSurfaceSnapshot;
use verter_session::framework::api_projector::ComponentApiProjectionWitness;
use verter_session::CommittedSourceContent;
use verter_session_query::facts::fact_cache::FactVersionRef;

/// Every foreground LSP route whose answer is settled against a request
/// snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ForegroundRoute {
    Hover,
    SignatureHelp,
    Definition,
    TypeDefinition,
    References,
    DocumentHighlight,
    PrepareRename,
    Rename,
    Completion,
    CompletionResolve,
    CodeAction,
    InlayHint,
    SemanticTokens,
    /// `$/verter/getBindingTypes`: the provider quick-info of every script
    /// binding of the requested document.
    BindingTypes,
}

/// The contract a route's answer carries to the client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResponseClass {
    /// Describes the admitted revision (hover, signature help, inlay hints,
    /// highlights, semantic tokens, binding types). It stays coherent across background
    /// churn that changes no input, and it captures no other document.
    Informational,
    /// Locations into the admitted revision. Every location in another open
    /// document is decoded through the bytes captured for that document
    /// ([`ForegroundRequest::target_source`]); a captured target edited before
    /// settlement fails the whole request with `ContentModified`.
    Navigation,
    /// Edits a client applies. Every open document an edit was computed
    /// against is captured like a navigation target and validated at
    /// settlement; a newly created path is recorded before that settlement
    /// ([`ForegroundRequest::record_absent_creates`]) and the delivered edit
    /// names each captured version ([`ForegroundRequest::bind_edits`]).
    Edit,
}

impl ForegroundRoute {
    pub(crate) const fn class(self) -> ResponseClass {
        match self {
            Self::Hover
            | Self::SignatureHelp
            | Self::DocumentHighlight
            | Self::InlayHint
            | Self::SemanticTokens
            | Self::BindingTypes => ResponseClass::Informational,
            Self::Definition | Self::TypeDefinition | Self::References => ResponseClass::Navigation,
            Self::PrepareRename
            | Self::Rename
            | Self::Completion
            | Self::CompletionResolve
            | Self::CodeAction => ResponseClass::Edit,
        }
    }
}

/// Why a computed answer no longer describes its admitted inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Superseded {
    /// The requested document was edited, closed or reopened.
    Revision,
    /// The project authority the document was answered under was replaced.
    Authority,
    /// A provider surface an answer was decoded through changed content, map,
    /// incarnation or owner — possibly changing back — before settlement.
    ProviderSurface,
    /// An imported source a native contribution was read from was committed
    /// with other content, or a published child contract it was read through no longer validates
    /// against its producer read sets, before settlement.
    Dependency,
    /// Another open document a location or edit was decoded through was
    /// edited, closed or reopened before settlement, or held different bytes
    /// than the semantic answer addressed when it was decoded.
    Target,
}

/// How a computed answer settles.
pub(crate) enum Settled<T> {
    /// Deliver this answer.
    Answer(Option<T>),
    /// The admitted inputs no longer describe the client's document.
    ContentModified,
}

impl<T> Settled<T> {
    pub(crate) fn into_result(self) -> tower_lsp_server::jsonrpc::Result<Option<T>> {
        match self {
            Self::Answer(answer) => Ok(answer),
            Self::ContentModified => Err(tower_lsp_server::jsonrpc::Error::new(
                tower_lsp_server::jsonrpc::ErrorCode::ContentModified,
            )),
        }
    }
}

tokio::task_local! {
    /// The foreground request the current task computes an answer for. Set
    /// only while [`ForegroundRequest::compute`] polls the request's
    /// computation, so a surface decoded anywhere on that computation's path
    /// joins the request's settlement bracket, and work outside a foreground
    /// request (diagnostics publication, background sync) records nothing.
    static ACTIVE_REQUEST: Arc<ForegroundRequest>;
}

/// One foreground request's admitted inputs. Every field is an `Arc` or a
/// copy, so a warm admission allocates nothing beyond the URI clone, and the
/// pins are released when the request ends, however it ends.
pub(crate) struct ForegroundRequest {
    route: ForegroundRoute,
    /// How the client applies an edit, as negotiated at `initialize`.
    edit_support: WorkspaceEditSupport,
    uri: Uri,
    /// Open incarnation, edit generation, client version and source bytes of
    /// the requested document; `None` when it was not open.
    document: Option<DocumentSnapshotIdentity>,
    /// The host authority identity the request was answered under, which
    /// settlement compares. Nothing is retained: an equivalent root
    /// republication, a cache eviction or a cache-row removal leaves it
    /// unchanged and every authority replacement changes it.
    authority: Arc<verter_session::HostAuthorityView>,
    /// Every provider surface a provider answer of this request was decoded
    /// through, in decode order. Each stays bracketed until settlement, so two
    /// contributions decoded through different epochs of one surface cannot
    /// both settle, and a surface that moves after its decode supersedes the
    /// answer built from it.
    decoded_surfaces: parking_lot::Mutex<Vec<Arc<ProviderSurfaceSnapshot>>>,
    queries: parking_lot::Mutex<Vec<verter_type_runtime::provider_query::ProviderQuery>>,
    /// Authored source and rewrite maps proven against retained provider bytes.
    mapped_targets: parking_lot::Mutex<Vec<Arc<ProviderSurfaceSnapshot>>>,
    closed_targets: parking_lot::Mutex<Vec<(String, ClosedTargetProof)>>,
    absent_targets: parking_lot::Mutex<Vec<String>>,
    /// The committed content hash of every imported source a native
    /// contribution of this request was read from — the explicit dependency
    /// evidence that ties native child-contract enrichment to one basis with the
    /// provider answer it is delivered beside. Two reads of one source with
    /// different content cannot both settle; an eviction and identical reload
    /// of the source, which re-commits the same bytes, keeps the evidence.
    dependencies: parking_lot::Mutex<Vec<(Box<str>, Option<CommittedSourceContent>)>>,
    native_facts: parking_lot::Mutex<Vec<FactVersionRef>>,
    /// The producer witness of every published child contract a native
    /// contribution of this request was read from. The witness carries the
    /// contract's complete read sets, so a change to anything the contract
    /// was derived from — an imported props type included — supersedes the
    /// answer even when no source the request read directly moved.
    contract_publications: parking_lot::Mutex<Vec<Arc<ComponentApiProjectionWitness>>>,
    /// The revision of every other open document a navigation location or an
    /// edit of this request was decoded through, captured at its first decode.
    /// Each must still be the open revision at settlement, and it is the
    /// version a delivered edit to that document names.
    targets: parking_lot::Mutex<Vec<(Uri, DocumentSnapshotIdentity)>>,
    /// Set when a decode found an open target holding bytes other than the
    /// ones the semantic answer addressed; the answer cannot settle.
    target_incoherent: AtomicBool,
    /// Set when reads of one imported source never observed a single
    /// committed content, so the native contribution they served is missing
    /// or mixed; the answer cannot settle, whatever content the source ends
    /// at.
    dependency_unsettled: AtomicBool,
}

impl ForegroundRequest {
    /// Admit a request for `uri` on `route`, for a client that applies edits
    /// as `edit_support`.
    pub(crate) fn admit(
        documents: &DocumentRegistry,
        route: ForegroundRoute,
        uri: &Uri,
        edit_support: WorkspaceEditSupport,
    ) -> Arc<Self> {
        Arc::new(Self {
            route,
            edit_support,
            uri: uri.clone(),
            document: documents.snapshot_identity(uri),
            authority: documents.host().capture_authority_view(),
            decoded_surfaces: parking_lot::Mutex::new(Vec::new()),
            queries: parking_lot::Mutex::new(Vec::new()),
            mapped_targets: parking_lot::Mutex::new(Vec::new()),
            closed_targets: parking_lot::Mutex::new(Vec::new()),
            absent_targets: parking_lot::Mutex::new(Vec::new()),
            dependencies: parking_lot::Mutex::new(Vec::new()),
            native_facts: parking_lot::Mutex::new(Vec::new()),
            contract_publications: parking_lot::Mutex::new(Vec::new()),
            targets: parking_lot::Mutex::new(Vec::new()),
            target_incoherent: AtomicBool::new(false),
            dependency_unsettled: AtomicBool::new(false),
        })
    }

    /// Whether the requested document was open at admission.
    pub(crate) fn document_was_open(&self) -> bool {
        self.document.is_some()
    }

    /// Poll `computation` as this request's computation: every provider
    /// surface it decodes an answer through joins this request's settlement
    /// bracket.
    pub(crate) fn compute<F: Future>(
        self: &Arc<Self>,
        computation: F,
    ) -> impl Future<Output = F::Output> {
        ACTIVE_REQUEST.scope(Arc::clone(self), computation)
    }

    /// Record that the current task's foreground request decoded a provider
    /// answer through `snapshot`, after that decode's own bracket held. A no-op
    /// outside a foreground request.
    pub(crate) fn bracket_decoded_surface(snapshot: &Arc<ProviderSurfaceSnapshot>) {
        let _ = ACTIVE_REQUEST.try_with(|request| {
            let mut surfaces = request.decoded_surfaces.lock();
            if !surfaces.iter().any(|known| Arc::ptr_eq(known, snapshot)) {
                surfaces.push(Arc::clone(snapshot));
            }
        });
    }

    /// Keep the runtime's evaluated-query evidence through final settlement.
    pub(crate) fn bracket_query(query: &verter_type_runtime::provider_query::ProviderQuery) {
        let _ = ACTIVE_REQUEST.try_with(|request| request.queries.lock().push(query.clone()));
    }

    /// Record that the current task's foreground request read a native
    /// contribution from the imported source `canonical_id` committed with
    /// content `content`. A no-op outside a foreground request.
    pub(crate) fn bracket_dependency(canonical_id: &str, content: CommittedSourceContent) {
        Self::bracket_source_state(canonical_id, Some(content));
    }

    /// Record either captured content or an unavailable dependency observed
    /// while selecting an export from multiple re-export branches.
    pub(crate) fn bracket_source_state(
        canonical_id: &str,
        content: Option<CommittedSourceContent>,
    ) {
        let _ = ACTIVE_REQUEST.try_with(|request| {
            let mut dependencies = request.dependencies.lock();
            if !dependencies
                .iter()
                .any(|(known, at)| **known == *canonical_id && *at == content)
            {
                dependencies.push((Box::from(canonical_id), content));
            }
        });
    }

    /// Retain the producer's native semantic read set, including inherited
    /// component dependencies not read as source directly by this request.
    pub(crate) fn bracket_native_facts(facts: &[FactVersionRef]) {
        let _ =
            ACTIVE_REQUEST.try_with(|request| request.native_facts.lock().extend_from_slice(facts));
    }

    /// Record that the current task's foreground request read an imported
    /// source whose content moved across every read of it: the answer
    /// settles `ContentModified`. A no-op outside a foreground request.
    pub(crate) fn mark_dependency_unsettled() {
        let _ = ACTIVE_REQUEST.try_with(|request| {
            request.dependency_unsettled.store(true, Ordering::Release);
        });
    }

    /// Record that the current task's foreground request read a native
    /// contribution from the published child contract `witness` vouches for.
    /// A no-op outside a foreground request.
    pub(crate) fn bracket_contract_publication(witness: &Arc<ComponentApiProjectionWitness>) {
        let _ = ACTIVE_REQUEST.try_with(|request| {
            let mut publications = request.contract_publications.lock();
            if !publications.iter().any(|known| Arc::ptr_eq(known, witness)) {
                publications.push(Arc::clone(witness));
            }
        });
    }

    /// The authored bytes a navigation location or edit of the current task's
    /// foreground request addresses in `path`.
    ///
    /// For a navigation or edit request, a `path` open in the client resolves
    /// to the open revision's bytes, and that revision joins the request's
    /// captured targets: the first capture of a document is the one every later
    /// decode of it in this request reads and the one settlement validates.
    /// Rewritten bytes require a recorded map whose authored source equals
    /// that captured revision and whose provider bytes equal the query's
    /// retained decode bytes. The range decoder uses that same evidence.
    /// An incoherent target drops the navigation location and refuses the
    /// whole edit. Anything else — a closed file, an informational
    /// route, work outside a foreground request — is read through `read`.
    pub(crate) fn target_source(
        documents: &DocumentRegistry,
        path: &str,
        read: impl FnOnce() -> Option<Arc<str>>,
    ) -> Option<Arc<str>> {
        let retained = ACTIVE_REQUEST
            .try_with(|request| {
                request
                    .queries
                    .lock()
                    .iter()
                    .find_map(|query| query.decoded_target(path))
            })
            .ok()
            .flatten();
        if let Some(Some(identity)) = Self::capture_target(documents, path) {
            match retained {
                None => return Some(identity.source),
                Some(bytes) if bytes.as_ref() == identity.source.as_ref() => {
                    return Some(identity.source);
                }
                Some(bytes) => {
                    let surface = documents.provider_surfaces().current_snapshot(path);
                    if let Some(surface) = surface.filter(|surface| {
                        same_document_path(&surface.source_canonical, path)
                            && surface.carrier_source.as_ref() == identity.source.as_ref()
                            && surface.provider_content.as_ref() == bytes.as_ref()
                            && surface.source_map.is_some()
                    }) {
                        Self::bracket_decoded_surface(&surface);
                        let _ = ACTIVE_REQUEST.try_with(|request| {
                            let mut targets = request.mapped_targets.lock();
                            if !targets.iter().any(|known| Arc::ptr_eq(known, &surface)) {
                                targets.push(surface);
                            }
                        });
                        return Some(identity.source);
                    }
                    Self::refuse_incoherent_edit_target();
                    return None;
                }
            }
        }
        let source = retained.or_else(read)?;
        Self::host_target_source(documents, path, source, ClosedTargetAuthority::Workspace)
    }

    /// Decode retained offsets through the exact rewrite evidence accepted by
    /// `target_source`, then emit authored coordinates in the client encoding.
    pub(crate) fn target_range(
        path: &str,
        source: &str,
        start: u32,
        end: u32,
        encoding: tower_lsp_server::ls_types::PositionEncodingKind,
    ) -> Option<tower_lsp_server::ls_types::Range> {
        use super::line_index::LineIndex;
        let mapped = ACTIVE_REQUEST
            .try_with(|request| {
                request
                    .mapped_targets
                    .lock()
                    .iter()
                    .find(|surface| {
                        same_document_path(&surface.stamp.provider_path, path)
                            && surface.carrier_source.as_ref() == source
                    })
                    .cloned()
            })
            .ok()
            .flatten();
        if let Some(surface) = mapped {
            let range = crate::type_provider::merge::api_surface_range_to_carrier_range(
                start,
                end,
                &surface.provider_utf16_line_index,
                surface.source_map.as_deref()?,
                &surface.carrier_utf16_line_index,
                &LineIndex::new(&surface.carrier_source, encoding),
            );
            if range.is_none() {
                Self::refuse_incoherent_edit_target();
            }
            return range;
        }
        let index = LineIndex::new(source, encoding);
        Some(tower_lsp_server::ls_types::Range {
            start: index.offset_to_position(start)?,
            end: index.offset_to_position(end)?,
        })
    }

    fn refuse_incoherent_edit_target() {
        let _ = ACTIVE_REQUEST.try_with(|request| {
            if request.route.class() == ResponseClass::Edit {
                request.target_incoherent.store(true, Ordering::Release);
            }
        });
    }

    /// Pin an open document at `path` without recording a closed target.
    /// A closed path is the caller's to prove after its read is accepted.
    pub(crate) fn pin_open_target(documents: &DocumentRegistry, path: &str) {
        let _ = Self::capture_target(documents, path);
    }

    /// [`Self::target_source`] for positions the host's semantic tables
    /// computed against `host_source`. When `path` is an open captured target
    /// holding other bytes, the positions cannot address the client's
    /// document: the request is marked incoherent (it settles
    /// `ContentModified`) and nothing decodes.
    ///
    /// `authority` chooses the closed-target proof. Provider-decoded bytes
    /// stay on the workspace read. Host-committed bytes stay on the
    /// scheduler content hash, so a commit the workspace cannot read is
    /// still the bytes the span indexes.
    pub(crate) fn host_target_source(
        documents: &DocumentRegistry,
        path: &str,
        host_source: Arc<str>,
        authority: ClosedTargetAuthority,
    ) -> Option<Arc<str>> {
        match Self::capture_target(documents, path) {
            Some(Some(identity)) if *identity.source != *host_source => {
                Self::mark_target_incoherent();
                None
            }
            Some(Some(identity)) => Some(identity.source),
            Some(None) => {
                let incoming = closed_target_proof(documents, path, &host_source, authority);
                let _ = ACTIVE_REQUEST.try_with(|request| {
                    let mut targets = request.closed_targets.lock();
                    if let Some((_, known)) = targets
                        .iter()
                        .find(|(known, _)| same_document_path(known, path))
                    {
                        if !closed_proofs_agree(documents, path, known, &host_source, &incoming) {
                            request.target_incoherent.store(true, Ordering::Release);
                        }
                    } else if let Some(proof) = incoming {
                        targets.push((path.to_string(), proof));
                    }
                });
                Some(host_source)
            }
            None => Some(host_source),
        }
    }

    /// Record that the current task's foreground request decoded a location or
    /// edit through `identity`, the open revision of `uri`. A no-op outside a
    /// navigation or edit request.
    pub(crate) fn bracket_target(uri: &Uri, identity: DocumentSnapshotIdentity) {
        let _ = ACTIVE_REQUEST.try_with(|request| {
            if request.route.class() == ResponseClass::Informational {
                return;
            }
            let mut targets = request.targets.lock();
            match targets.iter().find(|(known, _)| known == uri) {
                Some((_, known)) if !known.same_revision(&identity) => {
                    request.target_incoherent.store(true, Ordering::Release);
                }
                Some(_) => {}
                None => targets.push((uri.clone(), identity)),
            }
        });
    }

    /// Record that the current task's foreground request decoded through a
    /// target holding other bytes than its answer addressed: the answer
    /// settles `ContentModified`. A no-op outside a foreground request.
    pub(crate) fn mark_target_incoherent() {
        let _ = ACTIVE_REQUEST.try_with(|request| {
            request.target_incoherent.store(true, Ordering::Release);
        });
    }

    /// The captured revision of the open document at `path` for the current
    /// navigation or edit request: `None` outside one, `Some(None)` when the
    /// document is not open.
    fn capture_target(
        documents: &DocumentRegistry,
        path: &str,
    ) -> Option<Option<DocumentSnapshotIdentity>> {
        let request = ACTIVE_REQUEST
            .try_with(|request| {
                (request.route.class() != ResponseClass::Informational).then(|| Arc::clone(request))
            })
            .ok()
            .flatten()?;
        let uri = {
            let targets = request.targets.lock();
            if let Some((_, identity)) = targets
                .iter()
                .find(|(uri, _)| same_document_path(&uri_to_canonical_id(uri), path))
            {
                return Some(Some(identity.clone()));
            }
            drop(targets);
            match documents.open_uri_for_fs_path(path) {
                Some(uri) => uri,
                None => return Some(None),
            }
        };
        let identity = if uri == request.uri {
            request.document.clone()
        } else {
            documents.snapshot_identity(&uri)
        };
        let Some(identity) = identity else {
            return Some(None);
        };
        Self::bracket_target(&uri, identity.clone());
        Some(Some(identity))
    }

    /// Record every `ResourceOp::Create` in `response` whose path is not open
    /// and not readable. [`Self::settle`] checks those paths are still absent,
    /// so this runs after the edit is computed and before settlement. A path
    /// that already exists is left unrecorded; [`Self::bind_edits`] refuses it.
    pub(crate) fn record_absent_creates<T: EditBearing>(
        &self,
        documents: &DocumentRegistry,
        response: &T,
    ) {
        response.visit_edits(&mut |edit| {
            for uri in created_uris(edit) {
                if self.created_path_is_absent(documents, uri) {
                    self.note_absent_create(uri);
                }
            }
        });
    }

    /// Bind every edit in `response` to the revisions this request captured,
    /// in the client's negotiated shape. An edit reaching a target open in the
    /// client whose revision the request never captured could address bytes
    /// the client no longer holds, so it is never delivered: `Err` refuses the
    /// whole answer, or the answer withdraws that alternative
    /// ([`EditBearing`]).
    pub(crate) fn bind_edits<T: EditBearing>(
        &self,
        documents: &DocumentRegistry,
        response: &mut T,
    ) -> Result<(), EditRefusal> {
        response.bind_each_edit(&mut |edit| {
            for uri in created_uris(edit) {
                if !self.created_path_is_absent(documents, uri) {
                    return Err(EditRefusal::UnboundTarget(uri.clone()));
                }
                self.note_absent_create(uri);
            }
            let mut revision_of = |target: &Uri| self.edit_target_revision(documents, target);
            bind_workspace_edit(edit, self.edit_support, &mut revision_of)
        })
    }

    fn created_path_is_absent(&self, documents: &DocumentRegistry, uri: &Uri) -> bool {
        let path = uri_to_canonical_id(uri);
        documents.open_uri_for_fs_path(&path).is_none()
            && documents.host().workspace_read().read_file(&path).is_none()
    }

    fn note_absent_create(&self, uri: &Uri) {
        let path = uri_to_canonical_id(uri);
        let mut absent = self.absent_targets.lock();
        if absent.iter().any(|known| same_document_path(known, &path)) {
            return;
        }
        absent.push(path);
    }

    fn edit_target_revision(
        &self,
        documents: &DocumentRegistry,
        target: &Uri,
    ) -> EditTargetRevision {
        let target_path = uri_to_canonical_id(target);
        if *target == self.uri || same_document_path(&target_path, &uri_to_canonical_id(&self.uri))
        {
            return match &self.document {
                Some(document) => EditTargetRevision::Open(document.version),
                None => EditTargetRevision::Closed,
            };
        }
        if let Some((_, identity)) = self.targets.lock().iter().find(|(uri, _)| {
            uri == target || same_document_path(&uri_to_canonical_id(uri), &target_path)
        }) {
            return EditTargetRevision::Open(identity.version);
        }
        if documents.snapshot_identity(target).is_some()
            || documents.open_uri_for_fs_path(&target_path).is_some()
        {
            EditTargetRevision::Uncaptured
        } else if self
            .closed_targets
            .lock()
            .iter()
            .any(|(path, _)| same_document_path(path, &target_path))
            || self
                .absent_targets
                .lock()
                .iter()
                .any(|path| same_document_path(path, &target_path))
        {
            EditTargetRevision::Closed
        } else {
            EditTargetRevision::Uncaptured
        }
    }

    /// The single disposition of a computed answer. The answer — an empty one
    /// included — is delivered exactly when the requested revision, its project
    /// authority, every provider surface an answer was decoded through, every
    /// imported source a native contribution was read from and every published
    /// child contract it was read through are still the admitted ones; the
    /// diagnostics generation is never consulted.
    pub(crate) fn settle<T>(
        &self,
        documents: &DocumentRegistry,
        response: Option<T>,
    ) -> Settled<T> {
        match self.superseded(documents) {
            None => Settled::Answer(response),
            Some(superseded) => {
                #[cfg(feature = "semantic-observe")]
                tracing::debug!(
                    uri = self.uri.as_str(),
                    route = ?self.route,
                    class = ?self.route.class(),
                    ?superseded,
                    "foreground answer superseded"
                );
                #[cfg(not(feature = "semantic-observe"))]
                let _ = superseded;
                Settled::ContentModified
            }
        }
    }

    fn superseded(&self, documents: &DocumentRegistry) -> Option<Superseded> {
        let revision_is_current = match &self.document {
            Some(document) => documents.snapshot_identity_is_current(&self.uri, document),
            None => documents.snapshot_identity(&self.uri).is_none(),
        };
        if !revision_is_current {
            return Some(Superseded::Revision);
        }
        if !self.authority.is_current(documents.host()) {
            return Some(Superseded::Authority);
        }
        let surfaces = documents.provider_surfaces();
        let surfaces_are_current = self
            .decoded_surfaces
            .lock()
            .iter()
            .all(|surface| surfaces.captured_surface_is_current(surface));
        if !surfaces_are_current
            || !self
                .queries
                .lock()
                .iter()
                .all(|query| query.result_is_current())
        {
            return Some(Superseded::ProviderSurface);
        }
        let targets_are_current = !self.target_incoherent.load(Ordering::Acquire)
            && self
                .targets
                .lock()
                .iter()
                .all(|(uri, identity)| documents.snapshot_identity_is_current(uri, identity));
        let targets_are_current = targets_are_current
            && self.closed_targets.lock().iter().all(|(path, proof)| {
                documents.open_uri_for_fs_path(path).is_none()
                    && closed_proof_is_current(documents, path, proof)
            })
            && self.absent_targets.lock().iter().all(|path| {
                documents.open_uri_for_fs_path(path).is_none()
                    && documents.host().workspace_read().read_file(path).is_none()
            });
        if !targets_are_current {
            return Some(Superseded::Target);
        }
        let host = documents.host();
        let dependencies_are_current = !self.dependency_unsettled.load(Ordering::Acquire)
            && self
                .dependencies
                .lock()
                .iter()
                .all(|(canonical_id, revision)| {
                    host.registered_source_whole_hash(canonical_id) == *revision
                });
        let dependencies_are_current = dependencies_are_current
            && self
                .contract_publications
                .lock()
                .iter()
                .all(|witness| witness.is_current(host));
        let dependencies_are_current =
            dependencies_are_current && host.native_facts_are_current(&self.native_facts.lock());
        (!dependencies_are_current).then_some(Superseded::Dependency)
    }
}

/// Proof that `host_source` is the closed content `authority` names, or
/// `None` when that store does not currently hold those bytes.
fn closed_target_proof(
    documents: &DocumentRegistry,
    path: &str,
    host_source: &Arc<str>,
    authority: ClosedTargetAuthority,
) -> Option<ClosedTargetProof> {
    let host = documents.host();
    match authority {
        ClosedTargetAuthority::Workspace => {
            if host.workspace_read().read_file(path).as_deref() != Some(&**host_source) {
                ForegroundRequest::mark_target_incoherent();
                return None;
            }
            Some(ClosedTargetProof::Workspace(Arc::clone(host_source)))
        }
        ClosedTargetAuthority::HostCommit => {
            let Some(hash) = host.registered_source_whole_hash(path) else {
                ForegroundRequest::mark_target_incoherent();
                return None;
            };
            if host.get_source(path).as_deref() != Some(&**host_source) {
                ForegroundRequest::mark_target_incoherent();
                return None;
            }
            Some(ClosedTargetProof::Committed(hash))
        }
    }
}

/// Whether `known` and a new observation of `host_source` are one content.
fn closed_proofs_agree(
    documents: &DocumentRegistry,
    path: &str,
    known: &ClosedTargetProof,
    host_source: &Arc<str>,
    incoming: &Option<ClosedTargetProof>,
) -> bool {
    let Some(incoming) = incoming else {
        return false;
    };
    match (known, incoming) {
        (ClosedTargetProof::Workspace(known), ClosedTargetProof::Workspace(_)) => {
            **known == **host_source
        }
        (ClosedTargetProof::Committed(known), ClosedTargetProof::Committed(hash)) => known == hash,
        (ClosedTargetProof::Workspace(known), ClosedTargetProof::Committed(_)) => {
            **known == **host_source
        }
        (ClosedTargetProof::Committed(known), ClosedTargetProof::Workspace(_)) => {
            documents.host().registered_source_whole_hash(path) == Some(*known)
                && documents.host().get_source(path).as_deref() == Some(&**host_source)
        }
    }
}

fn closed_proof_is_current(
    documents: &DocumentRegistry,
    path: &str,
    proof: &ClosedTargetProof,
) -> bool {
    let host = documents.host();
    match proof {
        ClosedTargetProof::Workspace(source) => {
            host.workspace_read().read_file(path).as_deref() == Some(&**source)
        }
        ClosedTargetProof::Committed(hash) => {
            host.registered_source_whole_hash(path) == Some(*hash)
        }
    }
}

/// Which store a closed target's bytes were decoded against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClosedTargetAuthority {
    /// Provider or caller bytes. Settlement re-reads the workspace file.
    Workspace,
    /// Scheduler-committed bytes. Settlement re-reads the content hash.
    /// A host commit can be absent from the workspace (an alias, a negative
    /// directory index) and still be the bytes the span indexes.
    HostCommit,
}

/// Retained evidence for one closed target.
#[derive(Clone, Debug)]
enum ClosedTargetProof {
    Workspace(Arc<str>),
    Committed(CommittedSourceContent),
}

/// Whether two canonical ids name one document.
fn same_document_path(left: &str, right: &str) -> bool {
    left == right || verter_span::path::fs_paths_equal(left, right)
}

/// A route answer whose edits are bound to the request's captured revisions
/// before delivery.
pub(crate) trait EditBearing {
    /// Bind every `WorkspaceEdit` the answer delivers through `bind`. `Err`
    /// refuses the whole answer; an answer made of independent alternatives
    /// may instead withdraw only the alternative whose edit cannot be bound.
    fn bind_each_edit(
        &mut self,
        bind: &mut dyn FnMut(&mut WorkspaceEdit) -> Result<(), EditRefusal>,
    ) -> Result<(), EditRefusal>;

    /// Visit every `WorkspaceEdit` the answer delivers, without binding it.
    fn visit_edits(&self, visit: &mut dyn FnMut(&WorkspaceEdit));
}

/// `ResourceOp::Create` targets in `edit`, in delivery order.
fn created_uris(edit: &WorkspaceEdit) -> Vec<&Uri> {
    let Some(tower_lsp_server::ls_types::DocumentChanges::Operations(operations)) =
        &edit.document_changes
    else {
        return Vec::new();
    };
    operations
        .iter()
        .filter_map(|operation| match operation {
            tower_lsp_server::ls_types::DocumentChangeOperation::Op(
                tower_lsp_server::ls_types::ResourceOp::Create(create),
            ) => Some(&create.uri),
            _ => None,
        })
        .collect()
}

impl EditBearing for WorkspaceEdit {
    fn bind_each_edit(
        &mut self,
        bind: &mut dyn FnMut(&mut WorkspaceEdit) -> Result<(), EditRefusal>,
    ) -> Result<(), EditRefusal> {
        bind(self)
    }

    fn visit_edits(&self, visit: &mut dyn FnMut(&WorkspaceEdit)) {
        visit(self);
    }
}

/// Code actions are independent alternatives: an action whose edit reaches an
/// open document the request never captured is withdrawn whole, and the
/// others are still offered. No action is ever delivered with part of its
/// edit.
impl EditBearing for Vec<CodeActionOrCommand> {
    fn bind_each_edit(
        &mut self,
        bind: &mut dyn FnMut(&mut WorkspaceEdit) -> Result<(), EditRefusal>,
    ) -> Result<(), EditRefusal> {
        self.retain_mut(|action| match action {
            CodeActionOrCommand::CodeAction(CodeAction {
                edit: Some(edit), ..
            }) => bind(edit).is_ok(),
            CodeActionOrCommand::CodeAction(_) | CodeActionOrCommand::Command(_) => true,
        });
        Ok(())
    }

    fn visit_edits(&self, visit: &mut dyn FnMut(&WorkspaceEdit)) {
        for action in self {
            if let CodeActionOrCommand::CodeAction(CodeAction {
                edit: Some(edit), ..
            }) = action
            {
                visit(edit);
            }
        }
    }
}

#[cfg(test)]
#[path = "foreground_tests.rs"]
mod tests;
