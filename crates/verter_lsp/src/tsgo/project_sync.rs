use std::sync::Arc;

use crate::tsgo::protocol::TypeProviderError;
use crate::tsgo::traits::TypeProvider;
use crate::ProjectSyncMode;

/// Syncs project files to a `TypeProvider`.
///
/// In `TsxOnly` mode, only `.vue` -> TSX files are sent.
/// In `FullProject` mode, all files are sent (for environments without file system access).
pub struct ProjectSync {
    provider: Arc<dyn TypeProvider>,
    mode: ProjectSyncMode,
}

impl ProjectSync {
    pub fn new(provider: Arc<dyn TypeProvider>, mode: ProjectSyncMode) -> Self {
        Self { provider, mode }
    }

    /// Sync a Vue file's TSX representation to the type provider.
    pub async fn sync_tsx(
        &self,
        tsx_path: &str,
        tsx_content: &str,
    ) -> Result<(), TypeProviderError> {
        self.provider.update_file(tsx_path, tsx_content).await
    }

    /// Open a new TSX file in the type provider.
    pub async fn open_tsx(
        &self,
        tsx_path: &str,
        tsx_content: &str,
    ) -> Result<(), TypeProviderError> {
        self.provider.open_file(tsx_path, tsx_content).await
    }

    /// Close a TSX file in the type provider.
    pub async fn close_tsx(&self, tsx_path: &str) -> Result<(), TypeProviderError> {
        self.provider.close_file(tsx_path).await
    }

    /// Sync a non-Vue file to the type provider (only in FullProject mode).
    pub async fn sync_file(&self, path: &str, content: &str) -> Result<(), TypeProviderError> {
        match self.mode {
            ProjectSyncMode::FullProject => self.provider.update_file(path, content).await,
            ProjectSyncMode::TsxOnly => Ok(()), // Type provider reads from disk
        }
    }

    pub fn mode(&self) -> ProjectSyncMode {
        self.mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsgo::mock::{MockCall, MockTypeProvider};

    fn make_sync(mock: &MockTypeProvider, mode: ProjectSyncMode) -> ProjectSync {
        ProjectSync::new(Arc::new(mock.clone()), mode)
    }

    /// @ai-generated — TSX sync sends update_file in both modes
    #[tokio::test]
    async fn tsx_sync_sends_update_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::TsxOnly);

        sync.sync_tsx("App.vue.tsx", "export default {}")
            .await
            .unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::UpdateFile { path, content } => {
                assert_eq!(path, "App.vue.tsx");
                assert_eq!(content, "export default {}");
            }
            _ => panic!("expected UpdateFile"),
        }
    }

    /// @ai-generated — open_tsx sends open_file
    #[tokio::test]
    async fn open_tsx_sends_open_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::TsxOnly);

        sync.open_tsx("App.vue.tsx", "const x = 1;").await.unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::OpenFile { path, content } => {
                assert_eq!(path, "App.vue.tsx");
                assert_eq!(content, "const x = 1;");
            }
            _ => panic!("expected OpenFile"),
        }
    }

    /// @ai-generated — close_tsx sends close_file
    #[tokio::test]
    async fn close_tsx_sends_close_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::TsxOnly);

        sync.close_tsx("App.vue.tsx").await.unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::CloseFile { path } => {
                assert_eq!(path, "App.vue.tsx");
            }
            _ => panic!("expected CloseFile"),
        }
    }

    /// @ai-generated — Non-Vue files are synced only in FullProject mode
    #[tokio::test]
    async fn non_vue_file_synced_in_full_project_mode() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.sync_file("utils.ts", "export const x = 1;")
            .await
            .unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::UpdateFile { path, content } => {
                assert_eq!(path, "utils.ts");
                assert_eq!(content, "export const x = 1;");
            }
            _ => panic!("expected UpdateFile"),
        }
    }

    /// @ai-generated — Non-Vue files are skipped in TsxOnly mode
    #[tokio::test]
    async fn non_vue_file_skipped_in_tsx_only_mode() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::TsxOnly);

        sync.sync_file("utils.ts", "export const x = 1;")
            .await
            .unwrap();

        let calls = mock.file_sync_calls();
        assert!(
            calls.is_empty(),
            "TsxOnly mode should not sync non-Vue files"
        );
    }

    /// @ai-generated — Vue TSX is sent regardless of mode
    #[tokio::test]
    async fn vue_tsx_sent_in_both_modes() {
        for mode in [ProjectSyncMode::TsxOnly, ProjectSyncMode::FullProject] {
            let mock = MockTypeProvider::new();
            let sync = make_sync(&mock, mode);

            sync.sync_tsx("Comp.vue.tsx", "tsx content").await.unwrap();

            let calls = mock.file_sync_calls();
            assert_eq!(calls.len(), 1, "TSX should be sent in {:?} mode", mode);
        }
    }
}
