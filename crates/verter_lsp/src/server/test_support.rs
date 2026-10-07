use std::collections::HashMap;
use std::sync::Arc;

use futures_util::future::BoxFuture;

use super::VerterLanguageServer;

/// A named point inside one foreground request at which a test moves state.
///
/// The server reaches [`Self::Capture`] and [`Self::Settlement`]; a test type
/// provider reaches the two provider points, so every route that queries the
/// provider passes all four without a per-route hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RequestBarrier {
    /// A settlement basis was just captured; the attempt has not yet looked up
    /// the surface it will query (the generation-read/snapshot-lookup interval).
    Capture,
    /// The provider received the query and has not produced its answer.
    ProviderDispatch,
    /// The provider's answer is ready and the handler has not mapped it.
    ProviderDecode,
    /// An attempt's answer is computed and its settlement has not yet read the
    /// basis's generation; where the settlement first checks the document and
    /// workspace, this is the interval between its two currency reads.
    Settlement,
}

/// What a barrier runs when a request reaches it. The action receives the
/// zero-based arrival count at that barrier, so a schedule can act on a chosen
/// arrival or on every one.
pub(crate) type BarrierAction = Arc<dyn Fn(usize) -> BoxFuture<'static, ()> + Send + Sync>;

/// The barrier table one server and its test provider share.
///
/// A request awaits the armed action in line, so a schedule interleaves with the
/// request in program order: no sleep, no timer, no second task.
#[derive(Default)]
pub(crate) struct RequestBarriers {
    actions: parking_lot::Mutex<HashMap<RequestBarrier, BarrierAction>>,
    arrivals: parking_lot::Mutex<HashMap<RequestBarrier, usize>>,
}

impl RequestBarriers {
    /// Run `action` at every arrival at `barrier` until [`Self::clear`].
    pub(crate) fn arm(&self, barrier: RequestBarrier, action: BarrierAction) {
        self.actions.lock().insert(barrier, action);
    }

    /// Disarm every barrier and reset the arrival counts.
    pub(crate) fn clear(&self) {
        self.actions.lock().clear();
        self.arrivals.lock().clear();
    }

    /// How many times requests reached `barrier` since the last [`Self::clear`].
    pub(crate) fn arrivals(&self, barrier: RequestBarrier) -> usize {
        self.arrivals.lock().get(&barrier).copied().unwrap_or(0)
    }

    pub(crate) async fn reach(&self, barrier: RequestBarrier) {
        let arrival = {
            let mut arrivals = self.arrivals.lock();
            let count = arrivals.entry(barrier).or_insert(0);
            *count += 1;
            *count - 1
        };
        let action = self.actions.lock().get(&barrier).cloned();
        if let Some(action) = action {
            action(arrival).await;
        }
    }
}

impl VerterLanguageServer {
    /// The barrier table this server's foreground requests reach.
    pub(crate) fn request_barriers(&self) -> Arc<RequestBarriers> {
        Arc::clone(&self.request_barriers)
    }

    /// The IDE surface the open carrier `uri` currently serves.
    pub(crate) fn test_current_ide_surface(
        &self,
        uri: &tower_lsp_server::ls_types::Uri,
    ) -> Arc<crate::provider_surface_store::ProviderSurfaceSnapshot> {
        let path = self
            .active_ide_path_for_uri(uri)
            .expect("the open carrier has a live IDE path");
        self.documents
            .provider_surfaces()
            .current_snapshot(&path)
            .expect("the open carrier has a current IDE surface")
    }

    /// Record the open carrier's current IDE surface again through the store's
    /// own `record`: every input identical, except `provider_content` and
    /// `map_hash` when given.
    pub(crate) fn test_record_ide_surface(
        &self,
        uri: &tower_lsp_server::ls_types::Uri,
        provider_content: Option<Arc<str>>,
        map_hash: Option<verter_session_query::analysis::types::Hash16>,
    ) {
        let current = self.test_current_ide_surface(uri);
        self.documents
            .provider_surfaces()
            .record(crate::provider_surface_store::RecordSurface {
                provider_path: current.stamp.provider_path.to_string(),
                kind: current.kind,
                source_canonical: current.source_canonical.to_string(),
                provider_content: provider_content
                    .unwrap_or_else(|| Arc::clone(&current.provider_content)),
                source_map: current.source_map.as_deref().cloned(),
                carrier_source: Arc::clone(&current.carrier_source),
                map_hash: map_hash.unwrap_or(current.stamp.map_hash),
                project_owner: current.project_owner.clone(),
                regen_key: current.regen_key,
                engine_recheck: current.engine_recheck,
            });
    }

    /// Publish the installed workspace's current snapshot again with freshly
    /// built LSP views: a root publication that changes no input a request
    /// answers from.
    pub(crate) fn test_republish_equivalent_root(&self) {
        let workspace = self
            .vfs_workspace
            .read()
            .clone()
            .expect("a workspace is installed");
        let published = workspace
            .load_published()
            .expect("the installed workspace published a snapshot");
        let snapshot = Arc::clone(&published.snapshot);
        let views = crate::workspace_state::build_lsp_views(&*workspace, &snapshot, Vec::new());
        workspace.publish_snapshot(verter_workspace::PublishedRoot::with_ext(
            snapshot,
            Box::new(views),
        ));
    }

    /// Re-arm the open carrier `uri` as an importer whose dependency changed on
    /// disk, through the server's own disk-change path.
    pub(crate) fn test_rearm_open_importer(&self, uri: &tower_lsp_server::ls_types::Uri) {
        let canonical = crate::documents::uri_to_canonical_id(uri);
        self.refresh_open_importers_after_disk_change(std::slice::from_ref(&canonical));
    }

    /// Run the production carrier-publication pass to completion for a real-
    /// provider harness that intentionally does not call `initialized()`. This
    /// mirrors the workspace scanner's carrier phase so workspace-symbol tests
    /// exercise a complete configured-project Program instead of relying on a
    /// partial set of manually opened fixture files.
    pub(crate) async fn test_settle_workspace_carriers(&self) {
        let sources = {
            let Some(workspace) = self.vfs_workspace.read().clone() else {
                return;
            };
            let Some(published) = workspace.load_published() else {
                return;
            };
            let mut sources = Vec::new();
            for project in &published.snapshot.projects {
                if let verter_workspace::workspace_snapshot::ProjectPayload::Configured {
                    membership,
                    ..
                } = &project.payload
                {
                    sources.extend(
                        membership
                            .materialized_files
                            .iter()
                            .map(|path| path.as_str().to_string())
                            .filter(|path| verter_session_query::resolution::path_is_carrier(path)),
                    );
                }
            }
            sources.sort_unstable();
            sources.dedup();
            sources
        };
        let profile = self.documents.tsx_profile.read().clone();
        let mut published_companions = Vec::new();
        for source in sources {
            crate::workspace_scanner::sync_file_to_provider(
                &source,
                &self.documents.host(),
                Some(&self.documents),
                &profile,
                self.project_sync.as_ref(),
                self.documents.provider_surfaces(),
                &self.vfs_workspace,
                matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo),
                &self.provider_sync_states,
                self.carrier_publish_coordinator.as_ref(),
                &self.carrier_transaction_coordinator,
                Some(&self.pending_snapshot_provider_sync),
                Some(&mut published_companions),
            )
            .await;
        }
        if let Some(coordinator) = &self.carrier_publish_coordinator {
            if !published_companions.is_empty() {
                coordinator
                    .refresh_published_companions(&published_companions)
                    .await
                    .expect("test carrier batch refresh must succeed");
            }
        }
    }
}
