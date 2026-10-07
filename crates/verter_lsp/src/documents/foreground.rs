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
use std::sync::Arc;

use tower_lsp_server::ls_types::Uri;

use super::{DocumentRegistry, DocumentSnapshotIdentity};
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
#[cfg(feature = "semantic-observe")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResponseClass {
    /// Describes the admitted revision (hover, signature help, inlay hints,
    /// highlights, semantic tokens, binding types). It stays coherent across background
    /// churn that changes no input.
    Informational,
    /// Locations into the admitted revision; each foreign location is decoded
    /// by the route through the map captured for its own target.
    Navigation,
    /// Edits a client applies. The route validates every target it edits
    /// before returning; the request gate below covers the requested document.
    Edit,
}

#[cfg(feature = "semantic-observe")]
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
    #[cfg(feature = "semantic-observe")]
    route: ForegroundRoute,
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
}

impl ForegroundRequest {
    /// Admit a request for `uri` on `route`.
    pub(crate) fn admit(
        documents: &DocumentRegistry,
        route: ForegroundRoute,
        uri: &Uri,
    ) -> Arc<Self> {
        #[cfg(not(feature = "semantic-observe"))]
        let _ = route;
        Arc::new(Self {
            #[cfg(feature = "semantic-observe")]
            route,
            uri: uri.clone(),
            document: documents.snapshot_identity(uri),
            authority: documents.host().workspace_read().published_root(),
            decoded_surfaces: parking_lot::Mutex::new(Vec::new()),
            dependencies: parking_lot::Mutex::new(Vec::new()),
            contract_publications: parking_lot::Mutex::new(Vec::new()),
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
