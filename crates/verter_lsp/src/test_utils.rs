//! Shared test utilities for verter_lsp tests.

use std::path::Path;
use std::sync::Arc;
use verter_host::{HostConfig, VerterHost};

/// Canonical test path: delegates to production `normalize_canonical_id`.
pub(crate) fn canonical_test_path(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    verter_vfs::resolver::normalize_canonical_id(&raw)
}

/// VerterHost backed by a real `FilesystemWorkspace`.
pub(crate) fn make_filesystem_test_host(workspace_path: &Path) -> Arc<VerterHost> {
    let workspace_id = canonical_test_path(workspace_path);
    let ws: Arc<dyn verter_vfs::WorkspaceAccess> = Arc::new(verter_vfs::FilesystemWorkspace::new(
        verter_vfs::FilesystemOptions {
            roots: vec![workspace_id],
            ..Default::default()
        },
    ));
    Arc::new(VerterHost::new(HostConfig::default(), ws))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_test_path_normalizes_backslashes() {
        let id = canonical_test_path(Path::new("/home/user/project"));
        assert!(!id.contains('\\'), "got: {id}");
    }

    #[test]
    fn canonical_test_path_strips_extended_prefix() {
        let id = canonical_test_path(Path::new("//?/C:/Users/dev/project"));
        assert!(!id.contains("//?/"), "got: {id}");
    }

    #[test]
    fn canonical_test_path_lowercases_drive_letter() {
        // Synthetic Windows path — works on any OS
        let id = canonical_test_path(Path::new("C:/Users/dev/project"));
        assert!(id.starts_with("c:/"), "got: {id}");
    }

    #[test]
    fn filesystem_host_ensure_loaded_reads_from_disk() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let ws = tmp.path().join("workspace");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("App.vue"), "<template><div/></template>").unwrap();
        let host = make_filesystem_test_host(&ws);
        let file_id = canonical_test_path(&ws.join("App.vue"));
        // Positive: can load real file
        assert!(
            host.ensure_loaded(&file_id),
            "filesystem-backed host should load files via ensure_loaded"
        );
        assert!(
            host.get_analysis(&file_id).is_some(),
            "loaded file should have analysis"
        );
        // Negative: non-existent file cannot be loaded
        let missing_id = canonical_test_path(&ws.join("Missing.vue"));
        assert!(
            !host.ensure_loaded(&missing_id),
            "non-existent file should not load"
        );
        assert!(
            host.get_analysis(&missing_id).is_none(),
            "non-existent file should have no analysis"
        );
    }

    #[test]
    fn filesystem_host_resolves_relative_imports() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let ws = tmp.path().join("workspace");
        let src = ws.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("App.vue"), "<template><div/></template>").unwrap();
        std::fs::write(src.join("Child.vue"), "<template><span/></template>").unwrap();
        let host = make_filesystem_test_host(&ws);
        let app_id = canonical_test_path(&src.join("App.vue"));
        let child_id = canonical_test_path(&src.join("Child.vue"));
        let resolved = host.resolve_import_via_workspace(&app_id, "./Child.vue");
        // Positive: resolves to Child.vue
        assert_eq!(
            resolved.as_deref(),
            Some(child_id.as_str()),
            "filesystem host should resolve relative imports to correct target"
        );
        // Negative: does not resolve to non-existent file
        let missing = host.resolve_import_via_workspace(&app_id, "./Missing.vue");
        assert!(
            missing.is_none(),
            "non-existent relative import should return None"
        );
    }
}
