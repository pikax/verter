//! State movers. Each one goes through the producer that owns the state in
//! production — the document registry, the host, the provider-surface store,
//! the workspace publication, the background drain — never through a counter.

use std::sync::Arc;

use futures_util::future::BoxFuture;
use tower_lsp_server::ls_types::{TextDocumentItem, Uri};

use super::super::test_support::{BarrierAction, RequestBarrier};
use super::super::VerterLanguageServer;
use super::{Fixture, APP};
use crate::type_provider::mock::MockTypeProvider;

/// Owned handles a barrier action moves state through.
#[derive(Clone)]
pub(super) struct Handles {
    pub(super) server: VerterLanguageServer,
    pub(super) provider: Arc<MockTypeProvider>,
    pub(super) uri: Uri,
    pub(super) canonical: String,
    pub(super) workspace_id: String,
}

impl Handles {
    pub(super) fn of(fixture: &Fixture) -> Self {
        Self {
            server: fixture.server().clone(),
            provider: Arc::clone(&fixture.provider),
            uri: fixture.uri.clone(),
            canonical: fixture.canonical.clone(),
            workspace_id: fixture.workspace_id.clone(),
        }
    }

    fn tsconfig(&self) -> String {
        format!("{}/tsconfig.json", self.workspace_id)
    }

    /// The provider surface the carrier's IDE path currently serves.
    pub(super) fn current_surface(
        &self,
    ) -> Arc<crate::provider_surface_store::ProviderSurfaceSnapshot> {
        self.server.test_current_ide_surface(&self.uri)
    }

    /// Replace the document text through the registry, as a client edit does.
    pub(super) fn edit(&self, version: i32, text: &str) {
        let _ = self.server.documents.did_change(&self.uri, version, text);
    }

    /// Close the document and open it again with `text`.
    pub(super) fn close_and_reopen(&self, text: &str) {
        self.server.documents.did_close(&self.uri);
        let _ = self.server.documents.did_open(&TextDocumentItem {
            uri: self.uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: text.to_string(),
        });
    }

    /// Install a freshly built workspace for the same root, as a workspace
    /// re-initialization does. Its snapshot carries the same scalar generation
    /// as the one it replaces.
    pub(super) fn replace_workspace(&self, configured: bool) {
        let tsconfig = self.tsconfig();
        super::super::server_tests::install_test_resolver_for_root(
            &self.server,
            &self.workspace_id,
            configured.then_some(tsconfig.as_str()),
        );
    }

    /// Record the current IDE surface again, identical except, when given, its
    /// map identity.
    pub(super) fn record_current_surface(&self, map_hash: Option<[u8; 16]>) {
        self.server
            .test_record_ide_surface(&self.uri, None, map_hash);
    }

    /// Record a surface with different provider bytes at the IDE path.
    pub(super) fn record_drifted_surface(&self) {
        let current = self.current_surface();
        self.server.test_record_ide_surface(
            &self.uri,
            Some(Arc::from(format!(
                "{}\n// drifted",
                current.provider_content
            ))),
            None,
        );
    }

    /// Publish the workspace root again over the unchanged snapshot.
    pub(super) fn republish_equivalent_root(&self) {
        self.server.test_republish_equivalent_root();
    }

    /// One diagnostics-generation advance from the producers that move it in
    /// production without changing the document, chosen by `arrival` so a
    /// continuous schedule rotates through all of them.
    pub(super) async fn republish_diagnostics(&self, arrival: usize) {
        let host = self.server.documents.host();
        match arrival % 4 {
            // An open importer re-armed by a dependency's settled change.
            0 => self.server.test_rearm_open_importer(&self.uri),
            // The background drain re-syncing the carrier with identical bytes:
            // it rehydrates, recompiles and re-records the same surface.
            1 => self.resync_identical_carrier().await,
            // An identical recompile writing the same diagnostics again.
            2 => {
                host.invalidate_compile_slots(&self.canonical);
                let _ = self.server.documents.get_ide(&self.uri);
            }
            // A host eviction followed by the byte-identical reload.
            _ => {
                host.evict(&self.canonical);
                host.ensure_loaded(&self.canonical);
            }
        }
    }

    async fn resync_identical_carrier(&self) {
        let server = &self.server;
        let workspace = server
            .vfs_workspace
            .read()
            .clone()
            .expect("the fixture installed a workspace");
        let snapshot = server
            .published_resolver()
            .expect("the fixture published a resolver snapshot");
        let publish = super::super::background_drain::CarrierPublishCtx {
            coordinator: None,
            provider_delivery: crate::external_ts::CarrierProviderDelivery::DirectOpen,
            vfs: workspace,
            ownership_ready: true,
        };
        let _ = super::super::background_drain::sync_pending_snapshot_provider_file(
            server.project_sync.as_ref(),
            &server.documents,
            &snapshot,
            &server.provider_sync_states,
            &self.canonical,
            Some(&publish),
            &server.carrier_transaction_coordinator,
            true,
            &server.pending_snapshot_provider_sync,
        )
        .await;
    }
}

/// A barrier action that runs `mover` on every arrival.
pub(super) fn every<F>(handles: Handles, mover: F) -> BarrierAction
where
    F: Fn(Handles, usize) -> BoxFuture<'static, ()> + Send + Sync + 'static,
{
    Arc::new(move |arrival| mover(handles.clone(), arrival))
}

/// The carrier after an in-place edit: the literal grows, so every later
/// offset shifts.
pub(super) fn edited_app() -> String {
    APP.replace("'hello'", "'hello there'")
}

/// The carrier with authored text inserted before every block, so every
/// carrier offset moves while the script and template bodies are unchanged.
pub(super) fn shifted_app() -> String {
    format!("<!-- moved -->\n{APP}")
}

/// Every barrier a request reaches, for a continuous schedule.
pub(super) const ALL_BARRIERS: [RequestBarrier; 4] = super::BARRIERS;
