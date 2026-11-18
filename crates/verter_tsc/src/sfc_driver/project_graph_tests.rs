use super::*;
use verter_workspace::native_fs::NativeFs;

struct Fixture {
    dir: tempfile::TempDir,
    fs: NativeFs,
}

impl Fixture {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            fs: NativeFs::new(),
        }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
    fn write(&self, name: &str, text: &str) {
        let path = self.path(name);
        self.fs
            .create_dir_all(path.parent().unwrap().to_str().unwrap())
            .unwrap();
        self.fs.write_file(path.to_str().unwrap(), text).unwrap();
    }
    fn canonical(&self, name: &str) -> PathBuf {
        PathBuf::from(verter_span::path::canonicalize_path(
            &self.fs.realpath(self.path(name).to_str().unwrap()).unwrap(),
        ))
    }
    fn load(&self, mode: BuildMode) -> ProjectGraph {
        ProjectGraph::load(mode, &[self.path("tsconfig.json")]).unwrap()
    }
}

#[test]
fn solution_references_keep_authored_order_and_own_carriers() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        include_str!("../../tests/cases/fixtures/nuxt_references/tsconfig.json"),
    );
    for (name, config) in [
        (
            "app",
            include_str!("../../tests/cases/fixtures/nuxt_references/.nuxt/tsconfig.app.json"),
        ),
        (
            "server",
            include_str!("../../tests/cases/fixtures/nuxt_references/.nuxt/tsconfig.server.json"),
        ),
        (
            "shared",
            include_str!("../../tests/cases/fixtures/nuxt_references/.nuxt/tsconfig.shared.json"),
        ),
        (
            "node",
            include_str!("../../tests/cases/fixtures/nuxt_references/.nuxt/tsconfig.node.json"),
        ),
    ] {
        f.write(&format!(".nuxt/tsconfig.{name}.json"), config);
        f.write(&format!("{name}/View.vue"), "");
    }
    let graph = f.load(BuildMode::Build);
    assert!(graph.problems.is_empty());
    assert_eq!(graph.projects.len(), 5);
    for (project, name) in graph
        .projects
        .iter()
        .zip(["app", "server", "shared", "node"])
    {
        assert_eq!(
            project.config,
            f.canonical(&format!(".nuxt/tsconfig.{name}.json"))
        );
        assert_eq!(
            project.carriers,
            vec![f.canonical(&format!("{name}/View.vue"))]
        );
    }
    assert!(graph.projects[4].solution_only);
    assert_eq!(graph.carrier_count(), 4);
}

#[test]
fn inherited_membership_selects_sorted_carriers() {
    let f = Fixture::new();
    f.write(
        "base.json",
        r#"{"include":["src/**/*.vue"],"exclude":["src/Skip.vue"]}"#,
    );
    f.write(".nuxt/tsconfig.app.json", r#"{"extends":"../base.json"}"#);
    for name in ["Z.vue", "A.vue", "Skip.vue", "plain.ts"] {
        f.write(&format!("src/{name}"), "");
    }
    let graph =
        ProjectGraph::load(BuildMode::Project, &[f.path(".nuxt/tsconfig.app.json")]).unwrap();
    assert_eq!(
        graph.projects[0].carriers,
        vec![f.canonical("src/A.vue"), f.canonical("src/Z.vue")]
    );
    assert!(!graph.projects[0].solution_only);
}

#[test]
fn overlapping_projects_each_keep_the_shared_carrier() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"./one.json"},{"path":"./two.json"}]}"#,
    );
    for name in ["one.json", "two.json"] {
        f.write(name, r#"{"files":["View.vue"]}"#);
    }
    f.write("View.vue", "");
    let graph = f.load(BuildMode::Build);
    for project in &graph.projects[..2] {
        assert_eq!(project.carriers, vec![f.canonical("View.vue")]);
    }
    assert_eq!(graph.carrier_count(), 2);
}

#[test]
fn missing_reference_is_recorded_and_other_projects_load() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"./missing"},{"path":"./ok.json"}]}"#,
    );
    f.write("ok.json", r#"{"files":[]}"#);
    let graph = f.load(BuildMode::Build);
    assert_eq!(
        graph
            .projects
            .iter()
            .map(|p| p.config.clone())
            .collect::<Vec<_>>(),
        vec![f.canonical("ok.json"), f.canonical("tsconfig.json")]
    );
    assert!(
        matches!(&graph.problems[..], [ProjectGraphError::MissingReference { from, to }] if from == &f.canonical("tsconfig.json") && to.ends_with("missing"))
    );
}

#[test]
fn reference_cycle_is_recorded_and_each_project_is_loaded_once() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"./other.json"}]}"#,
    );
    f.write(
        "other.json",
        r#"{"files":[],"references":[{"path":"./tsconfig.json"}]}"#,
    );
    let graph = f.load(BuildMode::Build);
    assert_eq!(graph.projects.len(), 2);
    assert!(
        matches!(&graph.problems[..], [ProjectGraphError::Cycle { path }] if path == &vec![f.canonical("tsconfig.json"), f.canonical("other.json"), f.canonical("tsconfig.json")])
    );
}

#[test]
fn project_mode_selects_only_the_first_normal_config() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"files":["View.vue"],"references":[{"path":"./missing"}]}"#,
    );
    f.write("View.vue", "");
    let graph = ProjectGraph::load(
        BuildMode::Project,
        &[f.path("tsconfig.json"), f.path("ignored.json")],
    )
    .unwrap();
    assert_eq!(graph.projects.len(), 1);
    assert_eq!(graph.projects[0].carriers, vec![f.canonical("View.vue")]);
    assert!(
        matches!(&graph.problems[..], [ProjectGraphError::MissingReference { from, .. }] if from == &f.canonical("tsconfig.json"))
    );
}

#[test]
fn project_mode_keeps_a_solution_config_with_nothing_to_check() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"./missing"},{"path":"./child.json"}]}"#,
    );
    f.write(
        "child.json",
        r#"{"files":[],"references":[{"path":"./nested-missing"}]}"#,
    );
    let graph = f.load(BuildMode::Project);
    assert_eq!(graph.projects.len(), 1);
    assert!(graph.projects[0].solution_only);
    assert_eq!(graph.carrier_count(), 0);
    assert!(
        matches!(&graph.problems[..], [ProjectGraphError::MissingReference { from, to }] if from == &f.canonical("tsconfig.json") && to.ends_with("missing"))
    );
}

#[test]
fn directory_roots_and_file_roots_select_the_same_projects() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"./packages/app"}]}"#,
    );
    f.write("packages/app/tsconfig.json", r#"{"files":["View.vue"]}"#);
    f.write("packages/app/View.vue", "");
    for mode in [BuildMode::Project, BuildMode::Build] {
        for directory in ["", "packages/app"] {
            assert_eq!(
                ProjectGraph::load(mode, &[f.path(directory)]).unwrap(),
                ProjectGraph::load(mode, &[f.path(directory).join("tsconfig.json")]).unwrap()
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn symlinked_references_use_the_traversal_identity_in_the_publication() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"./alias"},{"path":"./app"}]}"#,
    );
    f.write("app/tsconfig.json", r#"{"files":["View.vue"]}"#);
    f.write("app/View.vue", "");
    std::os::unix::fs::symlink(f.path("app"), f.path("alias")).unwrap();
    let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
    let load = ProjectGraph::load_published(
        &ws,
        BuildMode::Build,
        &[f.path("tsconfig.json"), f.path("alias")],
        1,
    )
    .unwrap();
    assert!(load.graph.problems.is_empty());
    assert_eq!(load.graph.projects.len(), 2);
    let app = f.canonical("app/tsconfig.json");
    assert_eq!(load.graph.projects[0].config, app);
    assert_eq!(
        load.graph.projects[1].references,
        vec![app.clone(), app.clone()]
    );
    let root_id = load.project_id(&load.graph.projects[1].config).unwrap();
    let ProjectPayload::Configured { references, .. } =
        &load.published.snapshot.projects[root_id.0 as usize].payload
    else {
        panic!("configured root")
    };
    for reference in references {
        assert_eq!(Path::new(reference.as_str()), app);
        assert_eq!(
            load.project_id(Path::new(reference.as_str())),
            load.project_id(&app)
        );
    }
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn case_variant_references_use_the_traversal_identity() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"./APP"}]}"#,
    );
    f.write("app/tsconfig.json", r#"{"files":[]}"#);
    let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
    let load =
        ProjectGraph::load_published(&ws, BuildMode::Build, &[f.path("tsconfig.json")], 1).unwrap();
    assert!(load.graph.problems.is_empty());
    assert_eq!(load.graph.projects.len(), 2);
    assert_eq!(
        load.graph.projects[1].references,
        vec![load.graph.projects[0].config.clone()]
    );
    let id = load.project_id(&load.graph.projects[1].config).unwrap();
    let ProjectPayload::Configured { references, .. } =
        &load.published.snapshot.projects[id.0 as usize].payload
    else {
        panic!("configured root")
    };
    assert_eq!(
        Path::new(references[0].as_str()),
        load.graph.projects[0].config
    );
}

#[test]
fn retained_workspace_loads_overlay_only_roots_and_references() {
    use verter_workspace::WorkspaceChange;
    let f = Fixture::new();
    let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
    let root = PathBuf::from(canonicalize_path(
        &f.path("tsconfig.json").to_string_lossy(),
    ));
    let child = PathBuf::from(canonicalize_path(&f.path("child.json").to_string_lossy()));
    let base = PathBuf::from(canonicalize_path(&f.path("base.json").to_string_lossy()));
    ws.apply_changes(vec![
        WorkspaceChange::OverlaySet {
            canonical_id: root.to_str().unwrap().into(),
            source: Arc::from(r#"{"files":[],"references":[{"path":"./child.json"}]}"#),
        },
        WorkspaceChange::OverlaySet {
            canonical_id: child.to_str().unwrap().into(),
            source: Arc::from(r#"{"extends":"./base.json","files":[]}"#),
        },
        WorkspaceChange::OverlaySet {
            canonical_id: base.to_str().unwrap().into(),
            source: Arc::from(r#"{"compilerOptions":{"strict":true}}"#),
        },
    ]);
    let load = ProjectGraph::load_published(&ws, BuildMode::Build, std::slice::from_ref(&root), 1)
        .unwrap();
    assert!(load.graph.problems.is_empty());
    assert_eq!(load.graph.projects.len(), 2);
    assert_eq!(load.graph.projects[0].config, child);
    assert!(load.graph.projects[0].config_dependencies.contains(&base));
    assert!(load.graph.projects[1].config_dependencies.contains(&child));
    assert!(
        super::super::watch_binding::ProjectWatchProjectionBinding::new(&load.graph)
            .recheck_for(&[base])
            .reload_graph
    );
    assert_eq!(load.graph.projects[1].references, vec![child]);
    ws.apply_changes(vec![WorkspaceChange::OverlaySet {
        canonical_id: root.to_str().unwrap().into(),
        source: Arc::from(r#"{"files":[],"references":[{"path":"./missing.json"}]}"#),
    }]);
    let reload =
        ProjectGraph::load_published(&ws, BuildMode::Project, std::slice::from_ref(&root), 2)
            .unwrap();
    assert_eq!(reload.graph.projects.len(), 1);
    assert!(
        matches!(&reload.graph.problems[..], [ProjectGraphError::MissingReference { from, .. }] if from == &root)
    );
    assert_eq!(reload.lease.value(), 2);
}

#[test]
fn nested_solution_and_multiple_roots_visit_each_dependency_once() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"./solution.json"},{"path":"./leaf.json"}]}"#,
    );
    f.write(
        "solution.json",
        r#"{"files":[],"references":[{"path":"./leaf.json"}]}"#,
    );
    f.write("leaf.json", r#"{"files":[]}"#);
    let graph = ProjectGraph::load(
        BuildMode::Build,
        &[f.path("tsconfig.json"), f.path("leaf.json")],
    )
    .unwrap();
    assert_eq!(
        graph
            .projects
            .iter()
            .map(|p| p.config.clone())
            .collect::<Vec<_>>(),
        vec![
            f.canonical("leaf.json"),
            f.canonical("solution.json"),
            f.canonical("tsconfig.json")
        ]
    );
}

#[test]
fn each_project_uses_its_own_inherited_emit_options() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"./check.json"},{"path":"./emit.json"}]}"#,
    );
    f.write(
        "base.json",
        r#"{"compilerOptions":{"noEmit":true,"composite":true,"outDir":"dist"}}"#,
    );
    f.write("check.json", r#"{"extends":"./base.json","files":[]}"#);
    f.write(
        "emit.json",
        r#"{"extends":"./base.json","compilerOptions":{"noEmit":false},"files":[]}"#,
    );
    let graph = f.load(BuildMode::Build);
    assert!(!graph.projects[0].emits);
    assert!(graph.projects[1].emits);
    assert!(graph.projects[2].emits);
}

#[test]
fn missing_and_malformed_roots_are_rejected() {
    let f = Fixture::new();
    f.write("bad.json", "{broken");
    for name in ["missing.json", "bad.json"] {
        assert!(matches!(
            ProjectGraph::load(BuildMode::Build, &[f.path(name)]),
            Err(ProjectGraphError::UnreadableRoot { .. })
        ));
    }
}

#[test]
fn published_graph_ids_and_lease_come_from_one_load_ordinal() {
    let f = Fixture::new();
    f.write("left/tsconfig.json", r#"{"files":["View.vue"]}"#);
    f.write("right/tsconfig.json", r#"{"files":["View.vue"]}"#);
    f.write("left/View.vue", "");
    f.write("right/View.vue", "");
    let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
    let load = ProjectGraph::load_published(
        &ws,
        BuildMode::Build,
        &[
            f.path("left/./tsconfig.json"),
            f.path("right/tsconfig.json"),
        ],
        9,
    )
    .unwrap();
    assert!(load.published.ownership_ready);
    assert!(Arc::ptr_eq(&load.published, &ws.load_published().unwrap()));
    assert_eq!(load.published.snapshot.generation, SnapshotGeneration(9));
    assert_eq!(load.lease.value(), 9);
    assert_eq!(load.published.snapshot.projects.len(), 3);
    let root = f.canonical("");
    for project in &load.graph.projects {
        let id = load.project_id(&project.config).unwrap();
        let owner = &load.published.snapshot.projects[id.0 as usize];
        let ProjectPayload::Configured { membership, .. } = &owner.payload else {
            panic!("expected a configured project")
        };
        assert_eq!(&project.membership, membership);
        assert!(project.config_dependencies.contains(&project.config));
        assert_eq!(Path::new(owner.workspace_root.as_str()), root);
        assert!(load.published.env_hashes_by_project.contains_key(&id));
        assert!(load.published.project_identity_hashes.contains_key(&id));
        assert_eq!(
            load.published.snapshot.tsconfig_path(id).unwrap().as_str(),
            project.config.to_str().unwrap()
        );
    }
    assert_ne!(
        load.project_id(&load.graph.projects[0].config),
        load.project_id(&load.graph.projects[1].config)
    );
    assert_eq!(load.project_id(&f.path("unselected.json")), None);
}
