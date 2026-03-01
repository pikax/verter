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
    use crate::tsgo::mock::{FailingTypeProvider, MockCall, MockTypeProvider};

    fn make_sync(mock: &MockTypeProvider, mode: ProjectSyncMode) -> ProjectSync {
        ProjectSync::new(Arc::new(mock.clone()), mode)
    }

    fn make_sync_failing(provider: &FailingTypeProvider, mode: ProjectSyncMode) -> ProjectSync {
        // FailingTypeProvider is not Clone, so wrap directly
        ProjectSync {
            provider: Arc::new(FailingTypeProvider::new(&provider.error_message)),
            mode,
        }
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

    // ── Dead pipe / error propagation regression tests ───────────

    /// @ai-generated — Regression: sync_tsx propagates provider errors (dead pipe scenario).
    ///
    /// When tsgo crashes (e.g., OS error 232 "The pipe is being closed"),
    /// `sync_tsx` must return `Err`, not silently succeed or panic.
    #[tokio::test]
    async fn sync_tsx_propagates_provider_errors() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::TsxOnly);

        let result = sync.sync_tsx("App.vue.tsx", "export default {}").await;
        assert!(result.is_err(), "sync_tsx should propagate provider error");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("pipe"),
            "error should mention pipe: {err}"
        );
    }

    /// @ai-generated — Regression: open_tsx propagates provider errors.
    #[tokio::test]
    async fn open_tsx_propagates_provider_errors() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::TsxOnly);

        let result = sync.open_tsx("App.vue.tsx", "const x = 1;").await;
        assert!(result.is_err(), "open_tsx should propagate provider error");
    }

    /// @ai-generated — Regression: close_tsx propagates provider errors.
    #[tokio::test]
    async fn close_tsx_propagates_provider_errors() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::TsxOnly);

        let result = sync.close_tsx("App.vue.tsx").await;
        assert!(result.is_err(), "close_tsx should propagate provider error");
    }

    /// @ai-generated — Regression: sync_file in FullProject mode propagates provider errors.
    #[tokio::test]
    async fn sync_file_full_project_propagates_provider_errors() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        let result = sync.sync_file("utils.ts", "export const x = 1;").await;
        assert!(
            result.is_err(),
            "sync_file in FullProject mode should propagate error"
        );
    }

    /// @ai-generated — sync_file in TsxOnly mode succeeds even with failing provider
    /// because non-Vue files are skipped entirely (no provider call made).
    #[tokio::test]
    async fn sync_file_tsx_only_succeeds_with_failing_provider() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::TsxOnly);

        let result = sync.sync_file("utils.ts", "export const x = 1;").await;
        assert!(
            result.is_ok(),
            "sync_file in TsxOnly mode should skip provider entirely"
        );
    }

    /// @ai-generated — Multiple consecutive errors don't cause panic or unexpected state.
    /// Simulates repeated operations after tsgo crashes.
    #[tokio::test]
    async fn repeated_operations_after_provider_death() {
        let failing = FailingTypeProvider::new("write error: broken pipe");
        let sync = make_sync_failing(&failing, ProjectSyncMode::TsxOnly);

        // All operations should return errors, none should panic
        for i in 0..5 {
            let result = sync.sync_tsx("App.vue.tsx", &format!("version {i}")).await;
            assert!(
                result.is_err(),
                "operation {i} should still return error, not panic"
            );
        }
    }
}
