use std::sync::Arc;

use serde_json::json;

use crate::config::ProviderProjectPlan;
use crate::tsgo::protocol::TypeProviderError;
use crate::tsgo::traits::TypeProvider;
use crate::ProjectSyncMode;

/// Syncs project files to a `TypeProvider`.
///
/// In `FullProject` mode, `.vue` outputs and non-Vue source files are all sent.
#[derive(Clone)]
pub struct ProjectSync {
    provider: Arc<dyn TypeProvider>,
    mode: ProjectSyncMode,
}

impl ProjectSync {
    pub fn new(provider: Arc<dyn TypeProvider>, mode: ProjectSyncMode) -> Self {
        Self { provider, mode }
    }

    /// Load a Vue file's TSX into the type provider for import resolution only.
    /// Unlike `open_tsx`, this does NOT trigger diagnostics in providers that support it.
    pub async fn load_tsx(
        &self,
        tsx_path: &str,
        tsx_content: &str,
    ) -> Result<(), TypeProviderError> {
        self.provider.load_file(tsx_path, tsx_content).await
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

    /// Load a DTS file (.vue.ts) into the type provider for import resolution only.
    /// Unlike `open_dts`, this does NOT trigger diagnostics in providers that support it.
    pub async fn load_dts(
        &self,
        dts_path: &str,
        dts_content: &str,
    ) -> Result<(), TypeProviderError> {
        self.provider.load_file(dts_path, dts_content).await
    }

    /// Open a new DTS file (.vue.ts) in the type provider.
    pub async fn open_dts(
        &self,
        dts_path: &str,
        dts_content: &str,
    ) -> Result<(), TypeProviderError> {
        self.provider.open_file(dts_path, dts_content).await
    }

    /// Sync a Vue file's DTS representation (.vue.ts) to the type provider.
    pub async fn sync_dts(
        &self,
        dts_path: &str,
        dts_content: &str,
    ) -> Result<(), TypeProviderError> {
        self.provider.update_file(dts_path, dts_content).await
    }

    /// Close a DTS file in the type provider.
    pub async fn close_dts(&self, dts_path: &str) -> Result<(), TypeProviderError> {
        self.provider.close_file(dts_path).await
    }

    /// Sync a non-Vue file to the type provider.
    pub async fn sync_file(&self, path: &str, content: &str) -> Result<(), TypeProviderError> {
        self.provider.update_file(path, content).await
    }

    /// Close a non-Vue provider file.
    pub async fn close_file(&self, path: &str) -> Result<(), TypeProviderError> {
        self.provider.close_file(path).await
    }

    pub fn ensure_provider_project(&self, plan: &ProviderProjectPlan) -> std::io::Result<()> {
        std::fs::create_dir_all(&plan.provider_root)?;
        let config = render_provider_project_config(plan);
        let write = match std::fs::read_to_string(&plan.config_path) {
            Ok(existing) => existing != config,
            Err(_) => true,
        };
        if write {
            std::fs::write(&plan.config_path, config)?;
        }
        Ok(())
    }

    pub fn ensure_provider_projects(&self, plans: &[ProviderProjectPlan]) -> std::io::Result<()> {
        for plan in plans {
            self.ensure_provider_project(plan)?;
        }
        Ok(())
    }

    pub fn mode(&self) -> ProjectSyncMode {
        self.mode
    }
}

fn render_provider_project_config(plan: &ProviderProjectPlan) -> String {
    let config_dir = std::path::Path::new(&plan.config_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new(&plan.provider_root));

    let extends = plan
        .tsconfig_path
        .as_ref()
        .map(|path| relative_config_path(config_dir, std::path::Path::new(path)));
    let references = plan
        .reference_config_paths
        .iter()
        .map(|path| json!({ "path": relative_config_path(config_dir, std::path::Path::new(path)) }))
        .collect::<Vec<_>>();

    let compiler_options = if plan.tsconfig_path.is_some() {
        json!({ "rootDir": "." })
    } else {
        json!({
            "module": "esnext",
            "target": "esnext",
            "moduleResolution": "bundler",
            "jsx": "preserve",
            "jsxImportSource": "vue",
            "allowJs": true,
            "strict": true,
            "allowArbitraryExtensions": true,
            "allowSyntheticDefaultImports": true,
            "esModuleInterop": true,
            "skipLibCheck": true,
            "rootDir": "."
        })
    };

    let mut obj = serde_json::Map::new();
    if let Some(extends) = extends {
        obj.insert("extends".to_string(), serde_json::Value::String(extends));
    }
    obj.insert("compilerOptions".to_string(), compiler_options);
    if !references.is_empty() {
        obj.insert("references".to_string(), serde_json::Value::Array(references));
    }
    obj.insert("include".to_string(), json!(["./**/*"]));

    serde_json::to_string_pretty(&serde_json::Value::Object(obj)).unwrap() + "\n"
}

fn relative_config_path(from_dir: &std::path::Path, to_path: &std::path::Path) -> String {
    let from = from_dir
        .components()
        .collect::<Vec<std::path::Component<'_>>>();
    let to = to_path
        .components()
        .collect::<Vec<std::path::Component<'_>>>();

    if from.first() != to.first() {
        return to_path.to_string_lossy().replace('\\', "/");
    }

    let common = from
        .iter()
        .zip(to.iter())
        .take_while(|(left, right)| left == right)
        .count();

    let mut relative = Vec::new();
    for _ in common..from.len() {
        relative.push("..".to_string());
    }
    for component in &to[common..] {
        relative.push(component.as_os_str().to_string_lossy().to_string());
    }

    if relative.is_empty() {
        ".".to_string()
    } else {
        relative.join("/").replace('\\', "/")
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
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

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
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

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

    /// @ai-generated — load_tsx sends load_file (not open_file)
    #[tokio::test]
    async fn load_tsx_sends_load_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.load_tsx("App.vue.tsx", "const x = 1;").await.unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::LoadFile { path, content } => {
                assert_eq!(path, "App.vue.tsx");
                assert_eq!(content, "const x = 1;");
            }
            _ => panic!("expected LoadFile, got {:?}", calls[0]),
        }
    }

    /// @ai-generated — close_tsx sends close_file
    #[tokio::test]
    async fn close_tsx_sends_close_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

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

    /// @ai-generated — open_dts sends open_file
    #[tokio::test]
    async fn open_dts_sends_open_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.open_dts("App.vue.ts", "export default App;")
            .await
            .unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::OpenFile { path, content } => {
                assert_eq!(path, "App.vue.ts");
                assert_eq!(content, "export default App;");
            }
            _ => panic!("expected OpenFile"),
        }
    }

    /// @ai-generated — load_dts sends load_file (not open_file)
    #[tokio::test]
    async fn load_dts_sends_load_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.load_dts("App.vue.ts", "export default App;")
            .await
            .unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::LoadFile { path, content } => {
                assert_eq!(path, "App.vue.ts");
                assert_eq!(content, "export default App;");
            }
            _ => panic!("expected LoadFile, got {:?}", calls[0]),
        }
    }

    /// @ai-generated — sync_dts sends update_file
    #[tokio::test]
    async fn sync_dts_sends_update_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.sync_dts("App.vue.ts", "export default App;")
            .await
            .unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::UpdateFile { path, content } => {
                assert_eq!(path, "App.vue.ts");
                assert_eq!(content, "export default App;");
            }
            _ => panic!("expected UpdateFile"),
        }
    }

    /// @ai-generated — close_dts sends close_file
    #[tokio::test]
    async fn close_dts_sends_close_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.close_dts("App.vue.ts").await.unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::CloseFile { path } => {
                assert_eq!(path, "App.vue.ts");
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

    #[tokio::test]
    async fn non_vue_file_close_sends_close_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.close_file("utils.__verter__.ts").await.unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::CloseFile { path } => {
                assert_eq!(path, "utils.__verter__.ts");
            }
            _ => panic!("expected CloseFile"),
        }
    }

    #[test]
    fn project_sync_loads_provider_project_config_before_owner_files() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = ProviderProjectPlan {
            root: "/workspace/pkg-a".to_string(),
            workspace_root: "/workspace".to_string(),
            tsconfig_path: Some("/workspace/pkg-a/tsconfig.json".to_string()),
            provider_root: tmp.path().join(".verter/ide/pkg-a").to_string_lossy().replace('\\', "/"),
            config_path: tmp
                .path()
                .join(".verter/ide/pkg-a/tsconfig.json")
                .to_string_lossy()
                .replace('\\', "/"),
            reference_config_paths: vec![
                tmp.path()
                    .join(".verter/ide/shared/tsconfig.json")
                    .to_string_lossy()
                    .replace('\\', "/"),
            ],
        };
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.ensure_provider_project(&plan)
            .expect("provider project config should materialize");

        let written = std::fs::read_to_string(&plan.config_path)
            .expect("provider project config should be readable from disk");
        assert!(written.contains("\"extends\":"));
        assert!(written.contains("\"references\":"));
        assert!(written.contains("\"./**/*\""));
    }

    /// @ai-generated — Vue TSX is sent in resolver-managed mode
    #[tokio::test]
    async fn vue_tsx_sent_in_full_project_mode() {
        for mode in [ProjectSyncMode::FullProject] {
            let mock = MockTypeProvider::new();
            let sync = make_sync(&mock, mode);

            sync.sync_tsx("Comp.vue.tsx", "tsx content").await.unwrap();

            let calls = mock.file_sync_calls();
            assert_eq!(calls.len(), 1, "TSX should be sent in {:?} mode", mode);
        }
    }

    // ── Load vs Open contract tests ───────────

    /// @ai-generated — load_tsx uses load_file, open_tsx uses open_file — they must NOT overlap.
    /// This is the key contract: background-loaded files should not trigger diagnostics.
    #[tokio::test]
    async fn load_and_open_use_different_provider_methods() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.load_tsx("A.vue.tsx", "load content").await.unwrap();
        sync.open_tsx("B.vue.tsx", "open content").await.unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 2);

        // First call must be LoadFile (not OpenFile)
        assert!(
            matches!(&calls[0], MockCall::LoadFile { path, .. } if path == "A.vue.tsx"),
            "load_tsx should use LoadFile, got {:?}",
            calls[0]
        );
        // Second call must be OpenFile (not LoadFile)
        assert!(
            matches!(&calls[1], MockCall::OpenFile { path, .. } if path == "B.vue.tsx"),
            "open_tsx should use OpenFile, got {:?}",
            calls[1]
        );
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
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

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
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        let result = sync.open_tsx("App.vue.tsx", "const x = 1;").await;
        assert!(result.is_err(), "open_tsx should propagate provider error");
    }

    /// @ai-generated — Regression: load_tsx propagates provider errors (dead pipe scenario).
    #[tokio::test]
    async fn load_tsx_propagates_provider_errors() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        let result = sync.load_tsx("App.vue.tsx", "const x = 1;").await;
        assert!(result.is_err(), "load_tsx should propagate provider error");
    }

    /// @ai-generated — Regression: load_dts propagates provider errors.
    #[tokio::test]
    async fn load_dts_propagates_provider_errors() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        let result = sync.load_dts("App.vue.ts", "export default App;").await;
        assert!(result.is_err(), "load_dts should propagate provider error");
    }

    /// @ai-generated — Regression: close_tsx propagates provider errors.
    #[tokio::test]
    async fn close_tsx_propagates_provider_errors() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        let result = sync.close_tsx("App.vue.tsx").await;
        assert!(result.is_err(), "close_tsx should propagate provider error");
    }

    /// @ai-generated — Regression: sync_file in FullProject mode propagates provider errors.
    #[tokio::test]
    async fn sync_file_full_project_propagates_provider_errors_duplicate() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        let result = sync.sync_file("utils.ts", "export const x = 1;").await;
        assert!(
            result.is_err(),
            "sync_file in FullProject mode should propagate error"
        );
    }

    /// @ai-generated — sync_file propagates provider errors for non-Vue files.
    #[tokio::test]
    async fn sync_file_full_project_propagates_provider_errors() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        let result = sync.sync_file("utils.ts", "export const x = 1;").await;
        assert!(
            result.is_err(),
            "sync_file should propagate provider errors"
        );
    }

    /// @ai-generated — Multiple consecutive errors don't cause panic or unexpected state.
    /// Simulates repeated operations after tsgo crashes.
    #[tokio::test]
    async fn repeated_operations_after_provider_death() {
        let failing = FailingTypeProvider::new("write error: broken pipe");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

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
