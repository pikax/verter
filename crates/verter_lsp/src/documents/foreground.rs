//! The foreground request context: one immutable capture of the inputs a
//! request answers from, taken at admission, and the one disposition that
//! decides whether the answer computed against it reaches the client.
//!
//! Three questions are kept apart:
//! - coherence: the answer was computed against one admitted revision of the
//!   requested document and one project authority, and every provider answer
//!   inside it was mapped through a provider surface whose content epoch,
//!   incarnation and owner held from its capture to the answer's decode (the
//!   provider-surface bracket each route closes before it maps a provider
//!   answer; a failed bracket drops that provider contribution);
//! - applicability: the admitted revision and authority still describe what
//!   the client holds when the answer is delivered — the disposition below;
//! - publication freshness: whether background diagnostics are current. That
//!   is the diagnostics publication's question (`ReadinessBasis`), never a
//!   foreground one, so a diagnostics-generation move cannot cost a request its
//!   answer.

use std::sync::Arc;

use tower_lsp_server::ls_types::Uri;

use super::{DocumentRegistry, DocumentSnapshotIdentity};

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
}

/// The contract a route's answer carries to the client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResponseClass {
    /// Describes the admitted revision (hover, signature help, inlay hints,
    /// highlights, semantic tokens). It stays coherent across background
    /// churn that changes no input.
    Informational,
    /// Locations into the admitted revision; each foreign location is decoded
    /// by the route through the map captured for its own target.
    Navigation,
    /// Edits a client applies. The route validates every target it edits
    /// before returning; the request gate below covers the requested document.
    Edit,
}

impl ForegroundRoute {
    pub(crate) const fn class(self) -> ResponseClass {
        match self {
            Self::Hover
            | Self::SignatureHelp
            | Self::DocumentHighlight
            | Self::InlayHint
            | Self::SemanticTokens => ResponseClass::Informational,
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

/// One foreground request's admitted inputs. Every field is an `Arc` or a
/// copy, so a warm admission allocates nothing beyond the URI clone, and the
/// pins are released when the request ends, however it ends.
pub(crate) struct ForegroundRequest {
    route: ForegroundRoute,
    uri: Uri,
    /// Open incarnation, edit generation, client version and source bytes of
    /// the requested document; `None` when it was not open.
    document: Option<DocumentSnapshotIdentity>,
    /// The published workspace root the request was answered under.
    authority: Option<Arc<verter_workspace::PublishedRoot>>,
}

impl ForegroundRequest {
    /// Admit a request for `uri` on `route`.
    pub(crate) fn admit(documents: &DocumentRegistry, route: ForegroundRoute, uri: &Uri) -> Self {
        Self {
            route,
            uri: uri.clone(),
            document: documents.snapshot_identity(uri),
            authority: documents.host().workspace_read().published_root(),
        }
    }

    /// Whether the requested document was open at admission.
    pub(crate) fn document_was_open(&self) -> bool {
        self.document.is_some()
    }

    /// The single disposition of a computed answer. An empty answer claims
    /// nothing and is delivered as is. Otherwise the answer is delivered
    /// exactly when the requested revision and its project authority are still
    /// the admitted ones; the diagnostics generation is never consulted.
    pub(crate) fn settle<T>(
        &self,
        documents: &DocumentRegistry,
        response: Option<T>,
    ) -> Settled<T> {
        let Some(response) = response else {
            return Settled::Answer(None);
        };
        match self.superseded(documents) {
            None => Settled::Answer(Some(response)),
            Some(superseded) => {
                tracing::debug!(
                    uri = self.uri.as_str(),
                    route = ?self.route,
                    class = ?self.route.class(),
                    ?superseded,
                    "foreground answer superseded"
                );
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
        (!authority_is_current).then_some(Superseded::Authority)
    }
}

/// Whether `current` is the admitted project authority or an equivalent
/// republication of it: the same workspace snapshot with the same ownership
/// readiness and project environment tables. Only the consumer extension — the
/// LSP views derived from that snapshot — may differ. A replaced snapshot is a
/// different authority even when it repeats the scalar generation.
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
