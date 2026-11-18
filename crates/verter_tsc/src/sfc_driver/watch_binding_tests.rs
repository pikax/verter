use super::super::project_graph::BuildMode;
use super::*;
use verter_workspace::native_fs::NativeFs;

#[test]
fn creating_a_missing_external_reference_rechecks_referrers_and_clears_the_graph_error() {
    use super::super::project_graph::ProjectGraphError;

    for (mode, reference) in [
        (BuildMode::Project, "../../shared/tsconfig.json"),
        (BuildMode::Build, "../../shared/tsconfig.json"),
        (BuildMode::Build, "../../shared"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let fs = NativeFs::new();
        let base = PathBuf::from(verter_span::path::canonicalize_path(
            &fs.realpath(temp.path().to_str().unwrap()).unwrap(),
        ));
        let app = base.join("workspace/app/tsconfig.json");
        let root = base.join("workspace/tsconfig.json");
        let missing = base.join("shared/tsconfig.json");
        fs.write_file(
            app.to_str().unwrap(),
            &format!(r#"{{"files":[],"references":[{{"path":"{reference}"}}]}}"#),
        )
        .unwrap();
        fs.write_file(
            root.to_str().unwrap(),
            r#"{"files":[],"references":[{"path":"./app"}]}"#,
        )
        .unwrap();
        let selected = if mode == BuildMode::Build {
            &root
        } else {
            &app
        };
        let graph = ProjectGraph::load(mode, std::slice::from_ref(selected)).unwrap();
        assert!(
            matches!(&graph.problems[..], [ProjectGraphError::MissingReference { from, .. }] if from == &app)
        );
        let watch = ProjectWatchProjectionBinding::new(&graph);
        assert_eq!(
            watch.recheck_for(&[base.join("unrelated/config.json")]),
            Recheck::default()
        );

        fs.write_file(missing.to_str().unwrap(), r#"{"files":[]}"#)
            .unwrap();
        assert_eq!(
            watch.recheck_for(&[missing.clone(), missing.clone()]),
            Recheck {
                projects: graph.projects.iter().map(|p| p.config.clone()).collect(),
                carriers: vec![],
                reload_graph: true,
                changed: vec![missing.clone()],
            }
        );
        let reloaded = ProjectGraph::load(mode, std::slice::from_ref(selected)).unwrap();
        assert!(reloaded.problems.is_empty());
        let app_project = reloaded.projects.iter().find(|p| p.config == app).unwrap();
        assert_eq!(app_project.references, vec![missing.clone()]);
        assert!(app_project.missing_config_probes.is_empty());
        assert!(app_project.config_dependencies.contains(&missing));
    }
}

#[test]
fn watch_tracks_carriers_external_reads_configs_and_reference_closure() {
    let temp = tempfile::tempdir().unwrap();
    let fs = NativeFs::new();
    for (name, text) in [
        (
            "workspace/tsconfig.json",
            r#"{"files":[],"references":[{"path":"./app"}]}"#,
        ),
        (
            "workspace/app/tsconfig.json",
            r#"{"extends":["../../settings/base.json","cfg"],"include":["*.vue"]}"#,
        ),
        (
            "settings/base.json",
            r#"{"compilerOptions":{"strict":true}}"#,
        ),
        ("workspace/app/View.vue", ""),
        ("external/old.ts", ""),
        ("external/new.ts", ""),
        (
            "node_modules/cfg/package.json",
            r#"{"tsconfig":"./base.json"}"#,
        ),
        (
            "node_modules/cfg/base.json",
            r#"{"compilerOptions":{"strict":true}}"#,
        ),
    ] {
        fs.write_file(temp.path().join(name).to_str().unwrap(), text)
            .unwrap();
    }
    let path = |name: &str| {
        PathBuf::from(verter_span::path::canonicalize_path(
            &fs.realpath(temp.path().join(name).to_str().unwrap())
                .unwrap(),
        ))
    };
    let graph = ProjectGraph::load(BuildMode::Build, &[path("workspace/tsconfig.json")]).unwrap();
    let mut watch = ProjectWatchProjectionBinding::new(&graph);
    let app = path("workspace/app/tsconfig.json");
    let root = path("workspace/tsconfig.json");
    let carrier = path("workspace/app/View.vue");
    watch.record_read(&app, &carrier, &[path("external/old.ts")]);
    watch.record_read(&app, &carrier, &[path("external/new.ts")]);
    assert_eq!(
        watch.recheck_for(&[path("external/old.ts")]),
        Recheck::default()
    );
    for changed in [carrier.clone(), path("external/new.ts")] {
        assert_eq!(
            watch.recheck_for(&[changed.clone(), changed.clone()]),
            Recheck {
                projects: vec![app.clone(), root.clone()],
                carriers: vec![(app.clone(), carrier.clone())],
                reload_graph: false,
                changed: vec![changed],
            }
        );
    }
    assert!(
        watch
            .recheck_for(&[path("settings/base.json")])
            .reload_graph
    );
    assert!(watch.recheck_for(&[root]).reload_graph);
    assert!(
        watch
            .recheck_for(&[path("node_modules/cfg/package.json")])
            .reload_graph
    );
    assert_eq!(
        watch.recheck_for(&[temp.path().join("unrelated/file.ts")]),
        Recheck::default()
    );
}

#[test]
fn watch_matches_new_members_outside_config_root_and_root_descendants() {
    let temp = tempfile::tempdir().unwrap();
    let fs = NativeFs::new();
    let config = temp.path().join(".nuxt/tsconfig.app.json");
    fs.write_file(
        config.to_str().unwrap(),
        r#"{"include":["../app/**/*.ts","../app/**/*.vue"],"exclude":["../app/excluded/**"]}"#,
    )
    .unwrap();
    let graph = ProjectGraph::load(BuildMode::Project, &[config]).unwrap();
    let watch = ProjectWatchProjectionBinding::new(&graph);
    let root = &graph.projects[0].root;
    let sibling = root.parent().unwrap().join("app/new.ts");
    assert_eq!(
        watch.recheck_for(&[sibling]).projects,
        vec![graph.projects[0].config.clone()]
    );
    assert_eq!(
        watch.recheck_for(&[root.parent().unwrap().join("app/excluded/new.ts")]),
        Recheck::default()
    );
    assert_eq!(
        watch.recheck_for(&[root.join("generated.d.ts")]).projects,
        vec![graph.projects[0].config.clone()]
    );
    let carrier = root.parent().unwrap().join("app/New.vue");
    fs.write_file(carrier.to_str().unwrap(), "").unwrap();
    assert_eq!(
        watch.recheck_for(std::slice::from_ref(&carrier)),
        Recheck {
            projects: vec![graph.projects[0].config.clone()],
            carriers: vec![(graph.projects[0].config.clone(), carrier.clone())],
            reload_graph: true,
            changed: vec![carrier],
        }
    );
}

#[test]
fn watch_normalizes_windows_event_paths_and_read_set_keys_without_disk_reads() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("tsconfig.json");
    NativeFs::new()
        .write_file(config.to_str().unwrap(), r#"{"files":[]}"#)
        .unwrap();
    let mut graph = ProjectGraph::load(BuildMode::Project, &[config]).unwrap();
    let project = &mut graph.projects[0];
    project.config = PathBuf::from("c:/repo/app/tsconfig.json");
    project.root = PathBuf::from("c:/repo/app");
    project.carriers = vec![PathBuf::from("c:/repo/app/View.vue")];
    project.config_dependencies = [PathBuf::from("c:/repo/base.json")].into_iter().collect();
    let config = project.config.clone();
    let carrier = project.carriers[0].clone();
    let mut watch = ProjectWatchProjectionBinding::new(&graph);
    watch.record_read(
        Path::new(r"\\?\C:\repo\app\tsconfig.json"),
        Path::new(r"C:\repo\app\View.vue"),
        &[PathBuf::from(r"C:\types\old.ts")],
    );
    assert_eq!(
        watch
            .recheck_for(&[PathBuf::from(r"C:\types\old.ts")])
            .carriers,
        vec![(config.clone(), carrier.clone())]
    );
    watch.record_read(&config, &carrier, &[PathBuf::from("c:/types/new.ts")]);
    assert_eq!(
        watch.recheck_for(&[PathBuf::from(r"C:\types\old.ts")]),
        Recheck::default()
    );
    assert_eq!(
        watch.recheck_for(&[PathBuf::from(r"\\?\C:\types\new.ts")]),
        Recheck {
            projects: vec![config.clone()],
            carriers: vec![(config.clone(), carrier.clone())],
            reload_graph: false,
            changed: vec![PathBuf::from(verter_span::path::canonicalize_path(
                "c:/types/new.ts"
            ))],
        }
    );
    assert!(
        watch
            .recheck_for(&[PathBuf::from(r"\\?\C:\repo\app\tsconfig.json")])
            .reload_graph
    );
    assert!(
        watch
            .recheck_for(&[PathBuf::from(r"C:\repo\base.json")])
            .reload_graph
    );
    assert_eq!(
        watch
            .recheck_for(&[PathBuf::from(r"C:\repo\app\View.vue")])
            .carriers,
        vec![(config, carrier)]
    );
}

#[test]
fn program_reads_outside_membership_make_the_project_and_its_referrers_stale() {
    let temp = tempfile::tempdir().unwrap();
    let fs = NativeFs::new();
    for (name, text) in [
        (
            "workspace/tsconfig.json",
            r#"{"files":[],"references":[{"path":"./app"}]}"#,
        ),
        ("workspace/app/tsconfig.json", r#"{"include":["*.vue"]}"#),
        ("workspace/app/View.vue", ""),
        ("shared/types.d.ts", ""),
        ("shared/old.d.ts", ""),
    ] {
        fs.write_file(temp.path().join(name).to_str().unwrap(), text)
            .unwrap();
    }
    let path = |name: &str| {
        PathBuf::from(verter_span::path::canonicalize_path(
            &fs.realpath(temp.path().join(name).to_str().unwrap())
                .unwrap(),
        ))
    };
    let graph = ProjectGraph::load(BuildMode::Build, &[path("workspace/tsconfig.json")]).unwrap();
    let mut watch = ProjectWatchProjectionBinding::new(&graph);
    let app = path("workspace/app/tsconfig.json");
    let root = path("workspace/tsconfig.json");
    let shared = path("shared/types.d.ts");
    let old = path("shared/old.d.ts");
    // Outside the root and the membership, nothing reads it yet.
    assert_eq!(
        watch.recheck_for(std::slice::from_ref(&shared)),
        Recheck::default()
    );

    watch.record_program_reads(&app, std::slice::from_ref(&old));
    // The engine spells paths with forward slashes and may name a probe
    // that does not exist yet.
    let probe = PathBuf::from(shared.to_string_lossy().replace(".d.ts", ".ts"));
    watch.record_program_reads(&app, &[shared.clone(), probe.clone()]);
    assert_eq!(
        watch.recheck_for(&[old]),
        Recheck::default(),
        "a replaced read set no longer names the path"
    );
    for changed in [shared.clone(), probe.clone()] {
        assert_eq!(
            watch.recheck_for(&[changed.clone(), changed.clone()]),
            Recheck {
                projects: vec![app.clone(), root.clone()],
                carriers: vec![],
                reload_graph: false,
                changed: vec![changed],
            }
        );
    }
    assert_eq!(
        watch.program_reads().cloned().collect::<BTreeSet<_>>(),
        [shared.clone(), probe.clone()].into_iter().collect()
    );
    // The change source reports them, watching their directory without its
    // subtree: engine reads reach ancestor directories up to the drive root.
    let watched = watch.watched_paths();
    assert!(watched.reports(&shared) && watched.reports(&probe));
    let directories: BTreeMap<&Path, bool> = watched.directories().collect();
    assert_eq!(directories.get(shared.parent().unwrap()), Some(&false));
}

#[test]
fn watched_paths_name_graph_inputs_members_and_reads_but_not_excluded_directories() {
    let temp = tempfile::tempdir().unwrap();
    let fs = NativeFs::new();
    for (name, text) in [
        (
            "workspace/app/tsconfig.json",
            r#"{"extends":"../base.json","exclude":["dist"],"references":[{"path":"../missing"}]}"#,
        ),
        (
            "workspace/base.json",
            r#"{"compilerOptions":{"strict":true}}"#,
        ),
        ("workspace/app/src/View.vue", ""),
        ("workspace/app/src/util.ts", ""),
        ("external/shared.ts", ""),
    ] {
        fs.write_file(temp.path().join(name).to_str().unwrap(), text)
            .unwrap();
    }
    let base = PathBuf::from(verter_span::path::canonicalize_path(
        &fs.realpath(temp.path().to_str().unwrap()).unwrap(),
    ));
    let at = |name: &str| base.join(name);
    let config = at("workspace/app/tsconfig.json");
    let graph = ProjectGraph::load(BuildMode::Project, std::slice::from_ref(&config)).unwrap();
    let mut watch = ProjectWatchProjectionBinding::new(&graph);
    let carrier = at("workspace/app/src/View.vue");
    watch.record_read(&config, &carrier, &[at("external/shared.ts")]);
    let watched = watch.watched_paths();

    for reported in [
        config.clone(),
        at("workspace/base.json"),
        carrier.clone(),
        at("workspace/app/src/util.ts"),
        // Created after the graph loaded, claimed by the include spec.
        at("workspace/app/src/New.vue"),
        at("external/shared.ts"),
        at("workspace/missing/tsconfig.json"),
    ] {
        assert!(watched.reports(&reported), "{reported:?}");
    }
    for ignored in [
        at("workspace/app/node_modules/pkg/index.d.ts"),
        at("workspace/app/dist/index.d.ts"),
        at("workspace/app/notes.txt"),
        at("external/other.ts"),
        at("workspace/app/src"),
    ] {
        assert!(!watched.reports(&ignored), "{ignored:?}");
    }
    let below = |dir: &str| -> Vec<PathBuf> {
        watched
            .named_below(&at(dir))
            .map(Path::to_path_buf)
            .collect()
    };
    assert_eq!(
        below("workspace/app/src"),
        vec![carrier.clone(), at("workspace/app/src/util.ts")]
    );
    assert_eq!(below("external"), vec![at("external/shared.ts")]);
    assert!(below("workspace/app/dist").is_empty());
    assert!(below("workspace/app/src/View.vue").is_empty());

    let reaching = |dir: &str| -> Vec<PathBuf> {
        watched
            .configs_reaching(&at(dir))
            .map(Path::to_path_buf)
            .collect()
    };
    for dir in ["workspace/app/src/new", "workspace/app", "workspace"] {
        assert_eq!(reaching(dir), vec![config.clone()], "{dir}");
    }
    for dir in [
        "workspace/app/dist/out",
        "workspace/app/node_modules/pkg",
        "workspace/app/.cache",
        "external",
    ] {
        assert!(reaching(dir).is_empty(), "{dir}");
    }

    assert!(watched.needs_directory_within(&at("workspace/app/src")));
    assert!(watched.needs_directory_within(&at("external")));
    assert!(!watched.needs_directory_within(&at("workspace/app/node_modules")));

    let directories: BTreeMap<&Path, bool> = watched.directories().collect();
    assert_eq!(directories.get(at("workspace/app").as_path()), Some(&true));
    assert_eq!(directories.get(at("external").as_path()), Some(&false));
    assert_eq!(
        directories.get(at("workspace/missing").as_path()),
        Some(&true)
    );
    assert_eq!(directories.get(at("workspace").as_path()), Some(&false));
}

#[test]
fn a_directory_arriving_at_an_excluded_include_root_reaches_no_walk() {
    let temp = tempfile::tempdir().unwrap();
    let fs = NativeFs::new();
    for (name, text) in [
        (
            "app/tsconfig.json",
            r#"{"include":["src/**/*","dist/**/*"],"exclude":["dist"]}"#,
        ),
        ("app/src/View.vue", ""),
    ] {
        fs.write_file(temp.path().join(name).to_str().unwrap(), text)
            .unwrap();
    }
    let base = PathBuf::from(verter_span::path::canonicalize_path(
        &fs.realpath(temp.path().to_str().unwrap()).unwrap(),
    ));
    let config = base.join("app/tsconfig.json");
    let graph = ProjectGraph::load(BuildMode::Project, std::slice::from_ref(&config)).unwrap();
    let watched = ProjectWatchProjectionBinding::new(&graph).watched_paths();
    let reaching = |dir: &str| -> Vec<PathBuf> {
        watched
            .configs_reaching(&base.join(dir))
            .map(Path::to_path_buf)
            .collect()
    };
    assert!(reaching("app/dist").is_empty());
    assert!(reaching("app/dist/types").is_empty());
    assert_eq!(reaching("app/src"), vec![config.clone()]);
    // An ancestor of both roots reaches the walk through the included one.
    assert_eq!(reaching("app"), vec![config]);
}
