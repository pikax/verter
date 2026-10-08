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
//!   every imported source a native contribution was read from is still at
//!   the host revision it was read at, and every published child contract it
//!   was read through still validates against the producer read sets it was
//!   derived from;
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
use verter_session::carrier_publication_store::HostSourceRevisionToken;
use verter_session::framework::api_projector::ComponentApiProjectionWitness;

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
    /// settlement; the delivered edit names each captured version
    /// ([`ForegroundRequest::bind_edits`]).
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
    /// An imported source a native contribution was read from was re-registered,
    /// or a published child contract it was read through no longer validates
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
    /// The published workspace root the request was answered under.
    authority: Option<Arc<verter_workspace::PublishedRoot>>,
    /// Every provider surface a provider answer of this request was decoded
    /// through, in decode order. Each stays bracketed until settlement, so two
    /// contributions decoded through different epochs of one surface cannot
    /// both settle, and a surface that moves after its decode supersedes the
    /// answer built from it.
    decoded_surfaces: parking_lot::Mutex<Vec<Arc<ProviderSurfaceSnapshot>>>,
    /// The host source revision of every imported source a native
    /// contribution of this request was read from — the explicit dependency
    /// evidence that ties native child-contract enrichment to one basis with the
    /// provider answer it is delivered beside. Two reads of one source at
    /// different revisions cannot both settle.
    dependencies: parking_lot::Mutex<Vec<(Box<str>, HostSourceRevisionToken)>>,
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
            authority: documents.host().workspace_read().published_root(),
            decoded_surfaces: parking_lot::Mutex::new(Vec::new()),
            dependencies: parking_lot::Mutex::new(Vec::new()),
            contract_publications: parking_lot::Mutex::new(Vec::new()),
            targets: parking_lot::Mutex::new(Vec::new()),
            target_incoherent: AtomicBool::new(false),
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

    /// Record that the current task's foreground request read a native
    /// contribution from the imported source `canonical_id` at host revision
    /// `revision`. A no-op outside a foreground request.
    pub(crate) fn bracket_dependency(canonical_id: &str, revision: HostSourceRevisionToken) {
        let _ = ACTIVE_REQUEST.try_with(|request| {
            let mut dependencies = request.dependencies.lock();
            if !dependencies
                .iter()
                .any(|(known, at)| **known == *canonical_id && *at == revision)
            {
                dependencies.push((Box::from(canonical_id), revision));
            }
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

    /// The bytes a navigation location or edit of the current task's
    /// foreground request decodes positions in `path` through.
    ///
    /// For a navigation or edit request, a `path` open in the client resolves
    /// to the open revision's bytes, and that revision joins the request's
    /// captured targets: the first capture of a document is the one every later
    /// decode of it in this request reads and the one settlement validates.
    /// Anything else — a closed file, an informational route, work outside a
    /// foreground request — is read through `read`.
    pub(crate) fn target_source(
        documents: &DocumentRegistry,
        path: &str,
        read: impl FnOnce() -> Option<Arc<str>>,
    ) -> Option<Arc<str>> {
        match Self::capture_target(documents, path) {
            Some(Some(identity)) => Some(Arc::clone(&identity.source)),
            Some(None) | None => read(),
        }
    }

    /// [`Self::target_source`] for positions the host's semantic tables
    /// computed against `host_source`. When `path` is an open captured target
    /// holding other bytes, the positions cannot address the client's
    /// document: the request is marked incoherent (it settles
    /// `ContentModified`) and nothing decodes.
    pub(crate) fn host_target_source(
        documents: &DocumentRegistry,
        path: &str,
        host_source: Arc<str>,
    ) -> Option<Arc<str>> {
        match Self::capture_target(documents, path) {
            Some(Some(identity)) if *identity.source != *host_source => {
                Self::mark_target_incoherent();
                None
            }
            Some(_) | None => Some(host_source),
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
        let mut revision_of = |target: &Uri| self.edit_target_revision(documents, target);
        response.bind_each_edit(&mut |edit| {
            bind_workspace_edit(edit, self.edit_support, &mut revision_of)
        })
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
        } else {
            EditTargetRevision::Closed
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
        let authority_is_current = authority_is_equivalent(
            self.authority.as_ref(),
            documents.host().workspace_read().published_root().as_ref(),
        );
        if !authority_is_current {
            return Some(Superseded::Authority);
        }
        let surfaces = documents.provider_surfaces();
        let surfaces_are_current = self
            .decoded_surfaces
            .lock()
            .iter()
            .all(|surface| surfaces.captured_surface_is_current(surface));
        if !surfaces_are_current {
            return Some(Superseded::ProviderSurface);
        }
        let targets_are_current = !self.target_incoherent.load(Ordering::Acquire)
            && self
                .targets
                .lock()
                .iter()
                .all(|(uri, identity)| documents.snapshot_identity_is_current(uri, identity));
        if !targets_are_current {
            return Some(Superseded::Target);
        }
        let host = documents.host();
        let dependencies_are_current =
            self.dependencies
                .lock()
                .iter()
                .all(|(canonical_id, revision)| {
                    host.registered_source_revision_token(canonical_id) == Some(*revision)
                });
        let dependencies_are_current = dependencies_are_current
            && self
                .contract_publications
                .lock()
                .iter()
                .all(|witness| witness.is_current(&host));
        (!dependencies_are_current).then_some(Superseded::Dependency)
    }
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
}

impl EditBearing for WorkspaceEdit {
    fn bind_each_edit(
        &mut self,
        bind: &mut dyn FnMut(&mut WorkspaceEdit) -> Result<(), EditRefusal>,
    ) -> Result<(), EditRefusal> {
        bind(self)
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
}

/// Whether `current` is the admitted project authority or a republication of
/// the same workspace snapshot `Arc` with the same ownership readiness and
/// project environment tables, differing only in the consumer extension (the
/// LSP views derived from that snapshot).
///
/// Snapshot identity, not snapshot content, is the authority: a publication
/// that mints a new `WorkspaceSnapshot` — which is what every project-graph,
/// resolver and background-initialisation publication does — replaces the
/// authority even when its content and scalar generation repeat the admitted
/// one, because nothing proves a rebuilt snapshot resolves the same way. Such
/// a publication answers `ContentModified` conservatively.
fn authority_is_equivalent(
    admitted: Option<&Arc<verter_workspace::PublishedRoot>>,
    current: Option<&Arc<verter_workspace::PublishedRoot>>,
) -> bool {
    match (admitted, current) {
        (None, None) => true,
        (Some(admitted), Some(current)) => {
            Arc::ptr_eq(admitted, current)
                || (Arc::ptr_eq(&admitted.snapshot, &current.snapshot)
                    && admitted.ownership_ready == current.ownership_ready
                    && admitted.env_hashes_by_project == current.env_hashes_by_project
                    && admitted.project_identity_hashes == current.project_identity_hashes)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp_server::ls_types::Command;

    fn action(title: &str, target: &str) -> CodeActionOrCommand {
        let uri: Uri = target.parse().expect("uri");
        CodeActionOrCommand::CodeAction(CodeAction {
            title: title.to_string(),
            edit: Some(crate::features::action_utils::make_insert_edit(
                &uri,
                Default::default(),
                "x".to_string(),
            )),
            ..Default::default()
        })
    }

    /// An action whose edit cannot be bound is withdrawn whole; every other
    /// action — and a command, which carries no edit — is still offered.
    #[test]
    fn an_unbindable_code_action_is_withdrawn_alone() {
        let unbound: Uri = "file:///Unbound.vue".parse().expect("uri");
        let mut actions = vec![
            action("bound", "file:///Bound.vue"),
            action("unbound", unbound.as_str()),
            CodeActionOrCommand::Command(Command {
                title: "command".to_string(),
                command: "verter.command".to_string(),
                arguments: None,
            }),
        ];

        let refused = actions.bind_each_edit(&mut |edit| {
            let Some(tower_lsp_server::ls_types::DocumentChanges::Edits(edits)) =
                &edit.document_changes
            else {
                return Ok(());
            };
            if edits.iter().any(|edit| edit.text_document.uri == unbound) {
                Err(EditRefusal::UnboundTarget(unbound.clone()))
            } else {
                Ok(())
            }
        });

        assert_eq!(refused, Ok(()));
        let titles: Vec<&str> = actions
            .iter()
            .map(|action| match action {
                CodeActionOrCommand::CodeAction(action) => action.title.as_str(),
                CodeActionOrCommand::Command(command) => command.title.as_str(),
            })
            .collect();
        assert_eq!(titles, vec!["bound", "command"]);
    }

    /// A rename is one edit: an unbindable target refuses the whole answer.
    #[test]
    fn an_unbindable_rename_refuses_the_answer() {
        let unbound: Uri = "file:///Unbound.vue".parse().expect("uri");
        let mut edit = crate::features::action_utils::make_insert_edit(
            &unbound,
            Default::default(),
            "x".to_string(),
        );
        let refused =
            edit.bind_each_edit(&mut |_| Err(EditRefusal::UnboundTarget(unbound.clone())));
        assert_eq!(refused, Err(EditRefusal::UnboundTarget(unbound)));
    }
}
