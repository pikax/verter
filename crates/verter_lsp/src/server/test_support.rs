use super::VerterLanguageServer;

impl VerterLanguageServer {
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
                            .filter(|path| verter_workspace::resolver::path_is_carrier(path)),
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
                self.documents.host(),
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
