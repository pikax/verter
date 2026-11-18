use super::super::project_graph::DriverProject;
use super::*;
use verter_session::sfc_projection_diagnostics::DiagnosticAnchor;

fn project(name: &str, emits: bool, carriers: bool, references: &[&str]) -> DriverProject {
    DriverProject {
        config: PathBuf::from(name),
        root: PathBuf::new(),
        carriers: if carriers {
            vec![PathBuf::from("View.vue")]
        } else {
            vec![]
        },
        membership: verter_session_query::resolution::ConfiguredMembership {
            spec: verter_session_query::resolution::StaticMembershipSpec {
                files: Vec::new(),
                include: Vec::new(),
                exclude: Vec::new().into(),
            },
            materialized_files: Default::default(),
        },
        config_dependencies: Default::default(),
        missing_config_probes: Default::default(),
        references: references.iter().map(PathBuf::from).collect(),
        solution_only: false,
        custom_elements: vec![],
        emits,
    }
}

#[test]
fn partitions_only_complete_emitting_carrier_free_closures_in_graph_order() {
    let mut solution = project("solution", true, false, &["app", "b"]);
    solution.solution_only = true;
    let graph = ProjectGraph {
        projects: vec![
            project("a", true, false, &[]),
            project("b", true, false, &["a"]),
            project("app", false, true, &["b"]),
            project("check", false, false, &[]),
            project("uses-check", true, false, &["check"]),
            solution,
        ],
        problems: vec![],
    };
    let build = EngineBuild::partition(&graph);
    assert_eq!(build.configs, vec![PathBuf::from("a"), PathBuf::from("b")]);
    assert!(build.unsupported.is_empty());
    assert!(!build.is_empty());
    assert!(EngineBuild::partition(&ProjectGraph {
        projects: vec![],
        problems: vec![]
    })
    .is_empty());
}

#[test]
fn emitting_projects_reaching_carriers_are_reported_once_each() {
    let graph = ProjectGraph {
        projects: vec![
            project("carrier", false, true, &[]),
            project("middle", false, false, &["carrier"]),
            project("package", true, false, &["middle", "carrier"]),
            project("consumer", true, false, &["package"]),
        ],
        problems: vec![],
    };
    let build = EngineBuild::partition(&graph);
    assert!(build.configs.is_empty());
    assert_eq!(
        build.unsupported,
        vec![
            (PathBuf::from("package"), PathBuf::from("carrier")),
            (PathBuf::from("consumer"), PathBuf::from("carrier")),
        ]
    );
}

#[test]
fn incomplete_or_cyclic_reference_closures_are_never_sent_to_the_child() {
    let graph = ProjectGraph {
        projects: vec![
            project("missing", true, false, &["absent"]),
            project("a", true, false, &["b"]),
            project("b", true, false, &["a"]),
        ],
        problems: vec![],
    };
    let build = EngineBuild::partition(&graph);
    assert!(build.configs.is_empty());
    assert!(build.unsupported.is_empty());
}

#[test]
fn build_arguments_preserve_paths_and_forward_only_requested_flags() {
    let build = EngineBuild {
        configs: vec![
            PathBuf::from(r"\\?\D:\space here\tsconfig.json"),
            PathBuf::from("second.json"),
        ],
        unsupported: vec![],
    };
    for (force, no_emit, flags) in [
        (false, false, vec![]),
        (true, true, vec!["--force", "--noEmit"]),
    ] {
        let cmd = build_command(&build, Path::new("engine"), force, no_emit);
        let args: Vec<_> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let mut expected = vec![
            "-b",
            r"D:\space here\tsconfig.json",
            "second.json",
            "--pretty",
            "false",
            "--verbose",
        ];
        expected.extend(flags);
        assert_eq!(args, expected);
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    fs: verter_workspace::native_fs::NativeFs,
    ws: std::sync::Arc<verter_workspace::FilesystemWorkspace>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            fs: verter_workspace::native_fs::NativeFs::new(),
            ws: std::sync::Arc::new(verter_workspace::FilesystemWorkspace::new(
                verter_workspace::FilesystemOptions::default(),
            )),
        }
    }
    fn write(&self, name: &str, content: &str) {
        let path = self.dir.path().join(name);
        self.fs
            .create_dir_all(path.parent().unwrap().to_str().unwrap())
            .unwrap();
        self.fs.write_file(path.to_str().unwrap(), content).unwrap();
    }
    fn load(&self) -> super::super::project_graph::GraphLoad {
        ProjectGraph::load_published(
            &self.ws,
            super::super::BuildMode::Build,
            &[self.dir.path().join("tsconfig.json")],
            1,
        )
        .unwrap()
    }
}

#[test]
fn recorded_diagnostics_use_snapshot_membership_and_exact_unicode_positions() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"b"},{"path":"a"}]}"#,
    );
    f.write("a/tsconfig.json", r#"{"files":["../shared.ts"]}"#);
    f.write("b/tsconfig.json", r#"{"files":["source.ts"]}"#);
    f.write("shared.ts", "// header\r\nlet x = '😀'; broken\n");
    f.write("b/source.ts", "bad\n");
    let loaded = f.load();
    let build = EngineBuild::partition(&loaded.graph);
    let root = f.dir.path().to_string_lossy().replace('\\', "/");
    let sample = format!("{root}/shared.ts(2,15): error TS2304: Cannot find name 'broken'.\n{root}/b/source.ts(1,1): warning TS9999: Recorded warning.\n");
    let checks = attribute_output(&build, &loaded.graph, &*f.ws, &sample).unwrap();
    assert!(checks[0].config.ends_with("b/tsconfig.json"));
    assert_eq!(checks[0].raw[0].code, 9999);
    assert_eq!(checks[0].raw[0].category, DiagnosticCategory::Warning);
    assert!(checks[1].config.ends_with("a/tsconfig.json"));
    let diagnostic = &checks[1].raw[0];
    assert_eq!(diagnostic.start, 27);
    assert_eq!(
        checks[1].attributed[0].anchor,
        DiagnosticAnchor::Authored { start: 27, end: 27 }
    );
    assert!(attribute_output(
        &build,
        &loaded.graph,
        &*f.ws,
        "unknown.ts(1,1): error TS1: Outside membership."
    )
    .is_err());
    assert!(attribute_output(
        &build,
        &loaded.graph,
        &*f.ws,
        &format!("{root}/shared.ts(2,11): error TS1: Inside surrogate.")
    )
    .is_err());
}

#[test]
fn loaded_missing_references_withhold_the_entire_referring_closure() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"consumer"},{"path":"clean"}]}"#,
    );
    f.write(
        "consumer/tsconfig.json",
        r#"{"files":["index.ts"],"references":[{"path":"../broken"}]}"#,
    );
    f.write(
        "broken/tsconfig.json",
        r#"{"files":["index.ts"],"references":[{"path":"../absent"}]}"#,
    );
    f.write("clean/tsconfig.json", r#"{"files":["index.ts"]}"#);
    for name in ["consumer", "broken", "clean"] {
        f.write(&format!("{name}/index.ts"), "export {};\n");
    }
    let loaded = f.load();
    assert!(loaded.graph.problems.iter().any(|problem| matches!(
        problem,
        super::super::project_graph::ProjectGraphError::MissingReference { .. }
    )));
    let build = EngineBuild::partition(&loaded.graph);
    assert_eq!(build.configs.len(), 1);
    assert!(build.configs[0].ends_with("clean/tsconfig.json"));
}

#[test]
fn build_progress_owns_shared_files_and_unlocated_errors_without_first_match() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"a"},{"path":"b"}]}"#,
    );
    f.write(
        "a/tsconfig.json",
        r#"{"files":["../shared.ts","../Foo.vue.ts"]}"#,
    );
    f.write("b/tsconfig.json", r#"{"files":["../shared.ts"]}"#);
    f.write("shared.ts", "export const value: string = null;\n");
    f.write("Foo.vue.ts", "bad\n");
    let loaded = f.load();
    let build = EngineBuild::partition(&loaded.graph);
    let root = f.dir.path().to_string_lossy().replace('\\', "/");
    let shared = format!(
        "{root}/shared.ts(1,14): error TS2322: Type 'null' is not assignable to type 'string'."
    );
    let refusal = attribute_output(&build, &loaded.graph, &*f.ws, &shared).unwrap_err();
    assert!(
        refusal.contains("shared.ts")
            && refusal.contains("a/tsconfig.json")
            && refusal.contains("b/tsconfig.json"),
        "{refusal}"
    );
    let sample = format!("[1:00:00 AM] Building project '{root}/a/tsconfig.json'...\n{root}/Foo.vue.ts(1,1): error TS2304: Cannot find name 'bad'.\n[1:00:01 AM] Building project '{root}/b/tsconfig.json'...\nerror TS2318: Cannot find global type 'Array'.\n{shared}\n");
    let checks = attribute_output(&build, &loaded.graph, &*f.ws, &sample).unwrap();
    assert_eq!(checks[0].raw.len(), 1);
    assert_eq!(
        checks[0].attributed[0].carrier.as_ref(),
        format!("{root}/Foo.vue.ts")
    );
    assert_eq!(
        checks[0].attributed[0].anchor,
        DiagnosticAnchor::Authored { start: 0, end: 0 }
    );
    assert_eq!(
        checks[1].raw.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![2318, 2322]
    );
    assert!(checks[1].raw[0].file.is_empty());
    assert!(matches!(
        checks[1].attributed[0].anchor,
        DiagnosticAnchor::Generated { .. }
    ));
}

/// A copy of this run's fake engine in `dir`, named for `scenario`.
fn fake_engine(dir: &Path, scenario: &str) -> PathBuf {
    // Unit targets have no CARGO_BIN_EXE binding. Discover only alongside
    // this run's test executable, including a relocated nextest target tree.
    let executable = std::env::current_exe().unwrap();
    let binary = executable.parent().unwrap().parent().unwrap().join(format!(
        "verter_tsc_fake_engine{}",
        std::env::consts::EXE_SUFFIX
    ));
    let source = verter_test_support::resolve_test_binary_path(
        "verter_tsc_fake_engine",
        Some(binary.into_os_string()),
        std::env::var_os("NEXTEST").is_some(),
        "",
    )
    .unwrap();
    assert!(
        source.is_file(),
        "missing current-run fake engine: {source:?}"
    );
    let engine = dir.join(format!(
        "verter-tsgo-fake-{scenario}{}",
        std::env::consts::EXE_SUFFIX
    ));
    std::fs::copy(&source, &engine).unwrap();
    engine
}

#[test]
fn failed_child_is_an_outcome_and_an_empty_partition_never_spawns() {
    let f = Fixture::new();
    f.write("tsconfig.json", r#"{"files":["index.ts"]}"#);
    f.write("index.ts", "export {};\n");
    let loaded = f.load();
    let build = EngineBuild::partition(&loaded.graph);
    for (scenario, failed) in [("exit", true), ("apiok", false)] {
        let engine = fake_engine(f.dir.path(), scenario);
        let outcome = run_engine_build(&build, &loaded.graph, &*f.ws, &engine, false, false);
        assert_eq!(outcome.failure.is_some(), failed, "{:?}", outcome.failure);
        assert_eq!(outcome.projects.len(), 1);
    }
    let empty = EngineBuild {
        configs: vec![],
        unsupported: vec![],
    };
    let outcome = run_engine_build(
        &empty,
        &loaded.graph,
        &*f.ws,
        Path::new("absent-engine"),
        false,
        false,
    );
    assert!(outcome.failure.is_none());
    assert!(outcome.projects.is_empty());
}

mod wiring {
    //! The run as a whole: which route checks each project, and in what order.

    use super::super::super::{
        exit_status, outcome_diagnostics, BuildMode, DriverRequest, ExitStatus, RunOutcome,
        SfcTypeScriptDriverAdapter,
    };
    use super::{fake_engine, Fixture};
    use crate::reporter::VerterCode;
    use std::path::{Path, PathBuf};

    /// A package that emits and has no carrier, and an app with one carrier,
    /// both referenced by the root. `package_references` is the package's own
    /// reference list.
    fn package_and_app(f: &Fixture, package_references: &str) {
        f.write(
            "tsconfig.json",
            r#"{"files":[],"references":[{"path":"package"},{"path":"app"}]}"#,
        );
        f.write(
            "package/tsconfig.json",
            &format!(
                r#"{{"compilerOptions":{{"composite":true}},"files":["index.ts"],"references":[{package_references}]}}"#
            ),
        );
        f.write("package/index.ts", "export const value = 1;\n");
        f.write(
            "app/tsconfig.json",
            r#"{"compilerOptions":{"noEmit":true},"include":["*.vue"]}"#,
        );
        f.write(
            "app/View.vue",
            r#"<script setup lang="ts">const value = 1;</script>"#,
        );
    }

    fn request(config: PathBuf, engine: &Path, mode: BuildMode) -> DriverRequest {
        DriverRequest {
            mode,
            configs: vec![config],
            no_emit: false,
            declaration: false,
            declaration_dir: None,
            tsgo_bin: Some(engine.to_path_buf()),
            force: false,
            watch: false,
        }
    }

    fn position(outcome: &RunOutcome, project: &str) -> usize {
        outcome
            .projects
            .iter()
            .position(|check| check.config.ends_with(&format!("/{project}/tsconfig.json")))
            .unwrap_or_else(|| panic!("no {project} project in {:?}", outcome.projects))
    }

    /// Make the fake engine record each process it starts, by mode.
    fn record_processes(engine: &Path) -> PathBuf {
        let mut dir = engine.as_os_str().to_owned();
        dir.push(".pids");
        let dir = PathBuf::from(dir);
        std::fs::create_dir(&dir).unwrap();
        dir
    }

    fn started(processes: &Path, mode: &str) -> usize {
        std::fs::read_dir(processes)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{mode}-"))
            })
            .count()
    }

    #[test]
    fn each_project_is_checked_once_by_the_engine_build_or_by_one_driver_program() {
        let f = Fixture::new();
        package_and_app(&f, "");
        let engine = fake_engine(f.dir.path(), "apiok");
        let processes = record_processes(&engine);
        let outcome = SfcTypeScriptDriverAdapter::new()
            .run(
                &request(
                    f.dir.path().join("tsconfig.json"),
                    &engine,
                    BuildMode::Build,
                ),
                None,
            )
            .unwrap();
        let (package, app) = (position(&outcome, "package"), position(&outcome, "app"));
        assert!(package < app, "graph order: {:?}", outcome.projects);
        let package = &outcome.projects[package];
        assert!(
            package.engine_failure.is_none(),
            "{:?}",
            package.engine_failure
        );
        assert!(
            package.binding.is_none(),
            "the engine build checks the package, not a driver program"
        );
        let app = &outcome.projects[app];
        assert!(app.engine_failure.is_none(), "{:?}", app.engine_failure);
        assert!(app.binding.is_some());
        assert_eq!(app.carriers.len(), 1);
        assert_eq!(
            (started(&processes, "build"), started(&processes, "api")),
            (1, 1),
            "one engine build and one program for the app"
        );
        assert_eq!(exit_status(&outcome), ExitStatus::Clean);

        // `-p` has no build mode: the same package is one driver program.
        let outcome = SfcTypeScriptDriverAdapter::new()
            .run(
                &request(
                    f.dir.path().join("package/tsconfig.json"),
                    &engine,
                    BuildMode::Project,
                ),
                None,
            )
            .unwrap();
        assert!(outcome.projects[0].binding.is_some());
        assert_eq!(
            (started(&processes, "build"), started(&processes, "api")),
            (1, 2)
        );
    }

    #[test]
    fn a_failed_engine_build_is_reported_once_and_the_driver_projects_still_run() {
        let f = Fixture::new();
        package_and_app(&f, "");
        let engine = fake_engine(f.dir.path(), "buildfail");
        let outcome = SfcTypeScriptDriverAdapter::new()
            .run(
                &request(
                    f.dir.path().join("tsconfig.json"),
                    &engine,
                    BuildMode::Build,
                ),
                None,
            )
            .unwrap();
        let package = &outcome.projects[position(&outcome, "package")];
        let failure = package
            .engine_failure
            .as_deref()
            .expect("the failed build is reported on the project it covered");
        assert!(
            failure.starts_with("The engine build of '")
                && failure.contains("/package/tsconfig.json"),
            "{failure}"
        );
        let app = &outcome.projects[position(&outcome, "app")];
        assert!(app.engine_failure.is_none(), "{:?}", app.engine_failure);
        assert!(app.binding.is_some());
        assert_eq!(app.carriers.len(), 1);
        let failures = outcome_diagnostics(&outcome)
            .into_iter()
            .filter(|d| d.verter_code == Some(VerterCode::ProjectCheckFailed))
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert_eq!(failures[0].file, &*package.config);
        assert_eq!(exit_status(&outcome), ExitStatus::Failed);
    }

    #[test]
    fn an_emitting_project_that_references_carriers_is_refused_once_and_never_built() {
        let f = Fixture::new();
        package_and_app(&f, r#"{"path":"../app"}"#);
        let engine = fake_engine(f.dir.path(), "apiok");
        let processes = record_processes(&engine);
        let outcome = SfcTypeScriptDriverAdapter::new()
            .run(
                &request(
                    f.dir.path().join("tsconfig.json"),
                    &engine,
                    BuildMode::Build,
                ),
                None,
            )
            .unwrap();
        let package = &outcome.projects[position(&outcome, "package")];
        let refusal = package
            .engine_failure
            .as_deref()
            .expect("the refused project is reported");
        assert!(
            refusal.contains("/package/tsconfig.json") && refusal.contains("/app/tsconfig.json"),
            "{refusal}"
        );
        assert!(package.binding.is_none());
        let app = &outcome.projects[position(&outcome, "app")];
        assert!(app.engine_failure.is_none(), "{:?}", app.engine_failure);
        assert!(app.binding.is_some());
        let failures = outcome_diagnostics(&outcome)
            .into_iter()
            .filter(|d| d.verter_code == Some(VerterCode::ProjectCheckFailed))
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert_eq!(failures[0].file, &*package.config);
        assert_eq!(started(&processes, "build"), 0);
        assert_eq!(exit_status(&outcome), ExitStatus::Failed);
    }
}
