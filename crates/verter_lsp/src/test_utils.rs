//! Shared test utilities for verter_lsp tests.

use std::path::Path;
use std::sync::Arc;
use verter_session::{HostConfig, VerterHost};

/// Canonical test path: delegates to production `normalize_canonical_id`.
pub(crate) fn canonical_test_path(path: &Path) -> String {
    let raw = std::fs::canonicalize(path)
        .ok()
        .unwrap_or_else(|| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    verter_workspace::resolver::normalize_canonical_id(&raw)
}

/// Create a test VFS workspace with a published resolver snapshot wrapped in RwLock.
///
/// Used by tests that call functions expecting a
/// `&parking_lot::RwLock<Option<Arc<FilesystemWorkspace>>>`.
pub(crate) fn make_test_vfs_workspace_with_resolver(
    root: &str,
    tsconfig: Option<&str>,
) -> parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>> {
    let vfs_ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));

    let root_cp = verter_workspace::CanonicalPath::new(root);
    // A `tsconfig` ⇒ a CONFIGURED owner (the only ownership state that binds a carrier
    // through the shared `WorkspaceProjectResolver`; spec-bridge `include: {root}/**/*`
    // matches every file under `root`). `None` ⇒ an inferred Fallback project (never a
    // carrier owner for external-TS).
    let payload = match tsconfig {
        Some(tsconfig) => {
            let spec = verter_workspace::StaticMembershipSpec {
                files: Vec::new(),
                include: vec![verter_workspace::CompiledGlob::new(
                    verter_workspace::NormalizedGlob::from_root_and_pattern(&root_cp, "**/*"),
                )],
                exclude: vec![verter_workspace::CompiledGlob::new(
                    verter_workspace::NormalizedGlob::from_root_and_pattern(
                        &root_cp,
                        "node_modules/**",
                    ),
                )]
                .into(),
            };
            verter_workspace::workspace_snapshot::ProjectPayload::Configured {
                tsconfig_path: verter_workspace::CanonicalPath::new(tsconfig),
                membership: verter_workspace::ConfiguredMembership {
                    spec,
                    materialized_files: Default::default(),
                },
                compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
                references: Vec::new(),
                workspace_aliases: Vec::new(),
            }
        }
        None => verter_workspace::workspace_snapshot::ProjectPayload::Fallback {
            membership: verter_workspace::FallbackMembership {
                root: root_cp.clone(),
                exclude: vec![verter_workspace::CompiledGlob::new(
                    verter_workspace::NormalizedGlob::new(&format!("{}/node_modules/**", root)),
                )]
                .into(),
            },
        },
    };
    let projects = vec![verter_workspace::workspace_snapshot::OwnershipProject {
        id: verter_workspace::workspace_snapshot::ProjectId(0),
        root: root_cp.clone(),
        workspace_root: root_cp.clone(),
        payload,
    }];

    let resolver = verter_workspace::ProjectResolver::new(vec![
        crate::project_resolver::IdeProjectConfig::new(
            root.to_string(),
            root.to_string(),
            tsconfig.map(|s| s.to_string()),
        ),
    ]);

    let snapshot = Arc::new(verter_workspace::WorkspaceSnapshot {
        projects,
        resolver,
        generation: verter_workspace::workspace_snapshot::SnapshotGeneration(1),
    });

    let views = crate::workspace_state::build_lsp_views(&*vfs_ws, &snapshot, vec![]);
    vfs_ws.publish_snapshot(verter_workspace::PublishedRoot::with_ext(
        snapshot,
        Box::new(views),
    ));
    parking_lot::RwLock::new(Some(vfs_ws))
}

/// Create a test VFS workspace from a `ProjectRegistry`.
///
/// Builds a published snapshot from the registry's resolver and projects,
/// then wraps it in `RwLock<Option<Arc<FilesystemWorkspace>>>`.
pub(crate) fn make_test_vfs_workspace_from_registry(
    registry: &crate::config::ProjectRegistry,
) -> parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>> {
    let vfs_ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));

    let resolver = registry.to_native_project_resolver();
    let projects: Vec<verter_workspace::workspace_snapshot::OwnershipProject> = registry
        .projects()
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let root_cp = verter_workspace::CanonicalPath::new(&p.root);
            // A registry project with a `tsconfig_path` ⇒ a CONFIGURED owner (the only
            // ownership state that binds a carrier through the shared resolver;
            // spec-bridge `include: {root}/**/*`). Otherwise an inferred Fallback.
            let payload = match &p.tsconfig_path {
                Some(tsconfig) => {
                    let spec = verter_workspace::StaticMembershipSpec {
                        files: Vec::new(),
                        include: vec![verter_workspace::CompiledGlob::new(
                            verter_workspace::NormalizedGlob::from_root_and_pattern(
                                &root_cp, "**/*",
                            ),
                        )],
                        exclude: vec![verter_workspace::CompiledGlob::new(
                            verter_workspace::NormalizedGlob::from_root_and_pattern(
                                &root_cp,
                                "node_modules/**",
                            ),
                        )]
                        .into(),
                    };
                    verter_workspace::workspace_snapshot::ProjectPayload::Configured {
                        tsconfig_path: verter_workspace::CanonicalPath::new(tsconfig),
                        membership: verter_workspace::ConfiguredMembership {
                            spec,
                            materialized_files: Default::default(),
                        },
                        compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
                        references: Vec::new(),
                        workspace_aliases: Vec::new(),
                    }
                }
                None => verter_workspace::workspace_snapshot::ProjectPayload::Fallback {
                    membership: verter_workspace::FallbackMembership {
                        root: root_cp.clone(),
                        exclude: vec![verter_workspace::CompiledGlob::new(
                            verter_workspace::NormalizedGlob::new(&format!(
                                "{}/node_modules/**",
                                p.root
                            )),
                        )]
                        .into(),
                    },
                },
            };
            verter_workspace::workspace_snapshot::OwnershipProject {
                id: verter_workspace::workspace_snapshot::ProjectId(i as u32),
                root: root_cp.clone(),
                workspace_root: verter_workspace::CanonicalPath::new(&p.workspace_root),
                payload,
            }
        })
        .collect();

    let snapshot = Arc::new(verter_workspace::WorkspaceSnapshot {
        projects,
        resolver,
        generation: verter_workspace::workspace_snapshot::SnapshotGeneration(1),
    });

    let views = crate::workspace_state::build_lsp_views(&*vfs_ws, &snapshot, vec![]);
    vfs_ws.publish_snapshot(verter_workspace::PublishedRoot::with_ext(
        snapshot,
        Box::new(views),
    ));
    parking_lot::RwLock::new(Some(vfs_ws))
}

/// Create a test VFS workspace from a pre-built `NativeProjectResolver`.
///
/// Creates fallback projects for each workspace root in the resolver.
pub(crate) fn make_test_vfs_workspace_with_resolver_and_projects(
    resolver: verter_workspace::ProjectResolver,
    project_roots: &[(&str, &str, Option<&str>)], // (root, workspace_root, tsconfig)
) -> parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>> {
    let vfs_ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));

    let projects: Vec<verter_workspace::workspace_snapshot::OwnershipProject> = project_roots
        .iter()
        .enumerate()
        .map(|(i, (root, ws_root, tsconfig))| {
            let root_cp = verter_workspace::CanonicalPath::new(root);
            // A `tsconfig` ⇒ a CONFIGURED owner (the only ownership state that binds a
            // carrier through the shared `WorkspaceProjectResolver`; the spec-bridge
            // `include: {root}/**/*` matches every file under `root`). `None` ⇒ an
            // inferred Fallback project (never a carrier owner for external-TS).
            let payload = match tsconfig {
                Some(tsconfig) => {
                    let spec = verter_workspace::StaticMembershipSpec {
                        files: Vec::new(),
                        include: vec![verter_workspace::CompiledGlob::new(
                            verter_workspace::NormalizedGlob::from_root_and_pattern(
                                &root_cp, "**/*",
                            ),
                        )],
                        exclude: vec![verter_workspace::CompiledGlob::new(
                            verter_workspace::NormalizedGlob::from_root_and_pattern(
                                &root_cp,
                                "node_modules/**",
                            ),
                        )]
                        .into(),
                    };
                    verter_workspace::workspace_snapshot::ProjectPayload::Configured {
                        tsconfig_path: verter_workspace::CanonicalPath::new(tsconfig),
                        membership: verter_workspace::ConfiguredMembership {
                            spec,
                            materialized_files: Default::default(),
                        },
                        compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
                        references: Vec::new(),
                        workspace_aliases: Vec::new(),
                    }
                }
                None => verter_workspace::workspace_snapshot::ProjectPayload::Fallback {
                    membership: verter_workspace::FallbackMembership {
                        root: root_cp.clone(),
                        exclude: vec![verter_workspace::CompiledGlob::new(
                            verter_workspace::NormalizedGlob::new(&format!(
                                "{}/node_modules/**",
                                root
                            )),
                        )]
                        .into(),
                    },
                },
            };
            verter_workspace::workspace_snapshot::OwnershipProject {
                id: verter_workspace::workspace_snapshot::ProjectId(i as u32),
                root: root_cp,
                workspace_root: verter_workspace::CanonicalPath::new(ws_root),
                payload,
            }
        })
        .collect();

    let snapshot = Arc::new(verter_workspace::WorkspaceSnapshot {
        projects,
        resolver,
        generation: verter_workspace::workspace_snapshot::SnapshotGeneration(1),
    });

    let views = crate::workspace_state::build_lsp_views(&*vfs_ws, &snapshot, vec![]);
    vfs_ws.publish_snapshot(verter_workspace::PublishedRoot::with_ext(
        snapshot,
        Box::new(views),
    ));
    parking_lot::RwLock::new(Some(vfs_ws))
}

/// VerterHost backed by a real `FilesystemWorkspace`.
pub(crate) fn make_filesystem_test_host(workspace_path: &Path) -> Arc<VerterHost> {
    let workspace_id = canonical_test_path(workspace_path);
    let ws: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions {
            roots: vec![workspace_id],
            ..Default::default()
        }),
    );
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
