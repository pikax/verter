use super::super::{BuildMode, DriverRequest};
use super::*;
use std::path::PathBuf;
use verter_session::sfc_projection_mapper::ContentMapperProjectionAdapter;
use verter_workspace::native_fs::NativeFs;
use verter_workspace::{FilesystemOptions, FilesystemWorkspace};

pub(crate) struct Fixture {
    pub dir: tempfile::TempDir,
    pub fs: NativeFs,
}
impl Fixture {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            fs: NativeFs::new(),
        }
    }
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
    pub fn write(&self, name: &str, text: &str) {
        let path = self.path(name);
        self.fs
            .create_dir_all(path.parent().unwrap().to_str().unwrap())
            .unwrap();
        self.fs.write_file(path.to_str().unwrap(), text).unwrap();
    }
    pub fn request(&self, engine: PathBuf) -> DriverRequest {
        DriverRequest {
            mode: BuildMode::Build,
            configs: vec![self.path("tsconfig.json")],
            no_emit: true,
            declaration: false,
            declaration_dir: None,
            tsgo_bin: Some(engine),
            force: false,
            watch: false,
        }
    }
}

fn project(config: PathBuf) -> DriverProject {
    DriverProject {
        root: config.parent().unwrap().to_path_buf(),
        config,
        carriers: Vec::new(),
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
        references: Vec::new(),
        solution_only: false,
        custom_elements: Vec::new(),
        emits: false,
    }
}

#[test]
fn project_config_and_overlay_keep_the_authored_config_and_descriptor_paths() {
    let f = Fixture::new();
    let user = r#"{"extends":"./base.json","include":["*.vue"],"exclude":["dist"],"compilerOptions":{"rootDir":".","jsx":"preserve","paths":{"@/*":["./*"]},"types":[],"lib":["ES2022"]}}"#;
    f.write("tsconfig.json", user);
    f.write(
        "View.vue",
        r#"<script setup lang="ts">const value = 1;</script><template>{{ value }}</template>"#,
    );
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());
    let adapter = ContentMapperProjectionAdapter::new();
    let project = project(f.path("tsconfig.json"));
    let handle = MapperProjectHandle::of_config(project.config.to_str().unwrap());
    adapter
        .open_project(
            handle.clone(),
            MapperProjectSettings::new(handle.0.clone(), vec![], vec![], vec![]),
        )
        .unwrap();
    let carrier = project_carrier(&adapter, &handle, &f.path("View.vue"), &ws);
    assert!(!carrier.unreadable);
    assert!(carrier.result.module.is_some());
    let carriers = [carrier];
    let inputs = ProjectProgramInputs {
        project: &project,
        carriers: &carriers,
        surfaces: &[],
        no_emit: true,
    };
    let (path, bytes) = project_config_bytes(&inputs, &ws);
    assert_eq!(path, canonicalize_path(project.config.to_str().unwrap()));
    let mut expected: serde_json::Value = serde_json::from_str(user).unwrap();
    expected["files"] = serde_json::json!([carriers[0].result.virtual_file.as_ref()]);
    expected["compilerOptions"]["noEmit"] = true.into();
    expected["compilerOptions"]["composite"] = false.into();
    expected["compilerOptions"]["allowArbitraryExtensions"] = true.into();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&bytes).unwrap(),
        expected
    );
    let overlay = project_overlay(&inputs);
    assert_eq!(overlay.len(), 1);
    assert_eq!(overlay[0].path, carriers[0].result.virtual_file.as_ref());
    assert_eq!(overlay[0].content, carriers[0].result.text.as_ref());
    let missing = project_carrier(&adapter, &handle, &f.path("Missing.vue"), &ws);
    assert!(missing.unreadable);
    assert_eq!(missing.result.text.as_ref(), "export {};");
    assert_eq!(missing.result.diagnostics.len(), 1);
    assert!(!missing.result.complete);
}

pub(crate) fn fake_engine(f: &Fixture) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let root = exe.parent().unwrap().parent().unwrap();
    let binary = root.join(format!(
        "verter_tsc_fake_engine{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(
        binary.is_file(),
        "fake engine binary must be built: {}",
        binary.display()
    );
    let binary = verter_test_support::resolve_test_binary_path(
        "verter_tsc_fake_engine",
        Some(binary.into_os_string()),
        std::env::var_os("NEXTEST").is_some(),
        "",
    )
    .unwrap();
    let target = f.path(&format!(
        "verter-tsgo-fake-apiok{}",
        std::env::consts::EXE_SUFFIX
    ));
    f.fs.copy_file(binary.to_str().unwrap(), target.to_str().unwrap())
        .unwrap();
    target
}

#[test]
fn shared_carrier_opens_two_distinct_certified_project_programs() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"./first.json"},{"path":"./second.json"}]}"#,
    );
    f.write(
        "first.json",
        r#"{"files":["View.vue"],"compilerOptions":{"noEmit":true,"paths":{"@/*":["first/*"]}}}"#,
    );
    f.write(
        "second.json",
        r#"{"files":["View.vue"],"compilerOptions":{"noEmit":true,"paths":{"@/*":["second/*"]}}}"#,
    );
    f.write(
        "View.vue",
        r#"<script setup lang="ts">const value = 1;</script>"#,
    );
    let engine = fake_engine(&f);
    let outcome = SfcTypeScriptDriverAdapter::new()
        .run(&f.request(engine), None)
        .unwrap();
    assert_eq!(outcome.projects.len(), 3);
    assert_eq!(outcome.carriers_checked, 2);
    let programs = &outcome.projects[..2];
    for p in programs {
        assert!(p.engine_failure.is_none(), "{:?}", p.engine_failure);
        assert_eq!(p.carriers.len(), 1);
        assert!(p.raw.is_empty());
        assert!(p.attributed.is_empty());
    }
    let first = programs[0].binding.as_ref().unwrap();
    let second = programs[1].binding.as_ref().unwrap();
    assert_ne!(first.engine, second.engine);
    assert_eq!(first.engine.project(), programs[0].config.as_ref());
    assert_eq!(second.engine.project(), programs[1].config.as_ref());
    assert!(outcome.projects[2].binding.is_none());
}

#[test]
fn occupied_checking_path_warns_and_leaves_the_real_member_in_the_program() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"include":["*"],"compilerOptions":{"noEmit":true}}"#,
    );
    f.write(
        "View.vue",
        r#"<script setup lang="ts">const value = 1;</script>"#,
    );
    f.write("View.vue.ts", "export const real = 1;");
    let outcome = SfcTypeScriptDriverAdapter::new()
        .run(&f.request(fake_engine(&f)), None)
        .unwrap();
    let project = &outcome.projects[0];
    assert!(
        project.engine_failure.is_none(),
        "{:?}",
        project.engine_failure
    );
    assert!(project.binding.is_some());
    assert!(project.carriers.is_empty());
    assert_eq!(project.diagnostics.len(), 1);
    assert_eq!(
        project.diagnostics[0].verter_code,
        Some(VerterCode::CarrierPathConflict)
    );
    assert_eq!(project.diagnostics[0].severity, Severity::Warning);
    assert_eq!(
        f.fs.read_file(f.path("View.vue.ts").to_str().unwrap())
            .unwrap()
            .as_ref(),
        "export const real = 1;"
    );
}

#[test]
fn a_project_reports_the_carriers_it_does_not_own() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"include":["*.vue","*.svelte"],"compilerOptions":{"noEmit":true}}"#,
    );
    f.write(
        "View.vue",
        r#"<script setup lang="ts">const value = 1;</script>"#,
    );
    // Not Vue: composed as Vue, either would be refused or misread.
    let svelte = "<script lang=\"ts\">let count: number = 1</script>\n<p>{count}</p>\n";
    f.write("Zed.svelte", svelte);
    f.write("Alpha.svelte", svelte);
    let outcome = SfcTypeScriptDriverAdapter::new()
        .run(&f.request(fake_engine(&f)), None)
        .unwrap();
    let project = &outcome.projects[0];
    assert!(
        project.engine_failure.is_none(),
        "{:?}",
        project.engine_failure
    );
    let names = |paths: Vec<&str>| paths.into_iter().map(str::to_string).collect::<Vec<_>>();
    assert_eq!(
        names(
            project
                .carriers
                .iter()
                .map(|c| c.carrier.rsplit('/').next().unwrap())
                .collect()
        ),
        ["View.vue"]
    );
    assert_eq!(
        names(
            project
                .unowned_carriers
                .iter()
                .map(|p| p.file_name().unwrap().to_str().unwrap())
                .collect()
        ),
        ["Alpha.svelte", "Zed.svelte"],
        "graph order"
    );
    assert_eq!(outcome.carriers_checked, 1);
    let diagnostics = crate::sfc_driver::outcome_diagnostics(&outcome);
    assert!(
        diagnostics.iter().all(|d| !d.file.ends_with(".svelte")),
        "an unowned carrier is not reported by this driver: {diagnostics:?}"
    );
}

#[test]
fn missing_reference_suppression_uses_the_attribution_key() {
    let problems = [ProjectGraphError::MissingReference {
        from: PathBuf::from("/project/tsconfig.json"),
        to: PathBuf::from("/project/missing.json"),
    }];
    let graph = missing_reference_diagnostics(&problems);
    assert_eq!(graph.len(), 1);
    let raw = EngineDiagnostic {
        file: Arc::from("/project/tsconfig.json"),
        start: 0,
        length: 0,
        code: 6053,
        category: DiagnosticCategory::Error,
        message: "File '/project/missing.json' not found.".into(),
        related: vec![],
    };
    let mut another = raw.clone();
    another.message = "File '/project/another.json' not found.".into();
    let engine = attribute_diagnostics(&[raw, another], &ProjectTransforms(&[], None));
    assert_eq!(engine[0].dedup, graph[0].dedup);
    assert_ne!(engine[1].dedup, graph[0].dedup);
}

#[test]
fn solution_only_project_does_not_resolve_an_engine() {
    let f = Fixture::new();
    f.write("tsconfig.json", r#"{"files":[],"references":[]}"#);
    let mut request = f.request(f.path("missing-engine"));
    request.mode = BuildMode::Project;
    let outcome = SfcTypeScriptDriverAdapter::new()
        .run(&request, None)
        .unwrap();
    assert_eq!(outcome.projects.len(), 1);
    assert!(outcome.projects[0].binding.is_none());
    assert!(outcome.projects[0].engine_failure.is_none());
    assert!(outcome.projects[0].diagnostics.is_empty());
    assert!(outcome.projects[0].attributed.is_empty());
    assert_eq!(outcome.carriers_checked, 0);
}

#[test]
fn unreadable_root_uses_the_typescript_config_read_error_before_engine_resolution() {
    let f = Fixture::new();
    let request = f.request(f.path("missing-engine"));
    let error = SfcTypeScriptDriverAdapter::new()
        .run(&request, None)
        .unwrap_err();
    assert!(error
        .message
        .starts_with("error TS5082: Cannot read tsconfig file '"));
    assert!(error.message.ends_with("tsconfig.json'."));
}

#[test]
fn missing_explicit_engine_uses_the_shared_actionable_error() {
    let f = Fixture::new();
    f.write("tsconfig.json", r#"{"include":["*.ts"]}"#);
    let request = f.request(f.path("missing-engine"));
    let error = SfcTypeScriptDriverAdapter::new()
        .run(&request, None)
        .unwrap_err();
    assert!(error.message.contains("--tsgo-bin"), "{}", error.message);
    assert!(
        error.message.contains("missing-engine"),
        "{}",
        error.message
    );
}

#[test]
fn an_unreadable_carrier_reports_once_and_the_other_carriers_still_print() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"include":["*.vue"],"compilerOptions":{"noEmit":true}}"#,
    );
    // Not UTF-8, so the workspace cannot read it as text.
    std::fs::write(f.path("Bad.vue"), [0xff, 0xfe, 0xfd]).unwrap();
    f.write(
        "Other.vue",
        r#"<script setup lang="ts">const a = </script>"#,
    );
    let outcome = SfcTypeScriptDriverAdapter::new()
        .run(&f.request(fake_engine(&f)), None)
        .unwrap();
    let project = &outcome.projects[0];
    assert!(
        project.engine_failure.is_none(),
        "{:?}",
        project.engine_failure
    );
    assert_eq!(project.carriers.len(), 2);
    let diagnostics = crate::sfc_driver::outcome_diagnostics(&outcome);
    let on = |name: &str| {
        diagnostics
            .iter()
            .filter(|d| d.file.ends_with(&format!("/{name}")))
            .collect::<Vec<_>>()
    };
    let bad = on("Bad.vue");
    assert_eq!(bad.len(), 1, "{diagnostics:?}");
    assert_eq!(bad[0].verter_code, Some(VerterCode::CarrierUnreadable));
    assert!(!on("Other.vue").is_empty(), "{diagnostics:?}");
    assert_eq!(
        crate::sfc_driver::exit_status(&outcome),
        crate::sfc_driver::ExitStatus::Failed
    );
}

#[test]
fn one_projects_engine_failure_does_not_stop_the_next_project() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"./broken.json"},{"path":"./good.json"}]}"#,
    );
    f.write(
        "broken.json",
        r#"{"files":["Broken.vue"],"compilerOptions":{"noEmit":true}}"#,
    );
    f.write(
        "good.json",
        r#"{"files":["Good.vue"],"compilerOptions":{"noEmit":true}}"#,
    );
    f.write(
        "Broken.vue",
        r#"<script setup lang="ts">const a = 1;</script>"#,
    );
    f.write(
        "Good.vue",
        r#"<script setup lang="ts">const b = 1;</script>"#,
    );
    let engine = fake_engine(&f);
    // The engine answers no project for this config only.
    let mut hollow = engine.clone().into_os_string();
    hollow.push(".hollow");
    std::fs::write(PathBuf::from(hollow), "broken.json\n").unwrap();
    let outcome = SfcTypeScriptDriverAdapter::new()
        .run(&f.request(engine), None)
        .unwrap();
    let named = |name: &str| {
        outcome
            .projects
            .iter()
            .find(|p| p.config.ends_with(&format!("/{name}")))
            .unwrap()
    };
    let (broken, good) = (named("broken.json"), named("good.json"));
    assert!(broken.engine_failure.is_some());
    assert_eq!(broken.carriers.len(), 1, "carriers are projected first");
    assert!(good.engine_failure.is_none(), "{:?}", good.engine_failure);
    assert!(good.binding.is_some());
    let failures = crate::sfc_driver::outcome_diagnostics(&outcome)
        .into_iter()
        .filter(|d| d.verter_code == Some(VerterCode::ProjectCheckFailed))
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0].file, &*broken.config);
    assert_eq!(
        crate::sfc_driver::exit_status(&outcome),
        crate::sfc_driver::ExitStatus::Failed
    );
}

/// A scripted lane: every surface it renders names the carrier and how many
/// times it was asked, and a carrier listed in `fails` has no surface.
#[derive(Default)]
struct ScriptedSurfaces {
    asked: std::cell::RefCell<Vec<Vec<PathBuf>>>,
    fails: Vec<&'static str>,
}

impl ForeignSurfaceSource for ScriptedSurfaces {
    fn surfaces(&self, carriers: &[PathBuf]) -> Vec<ForeignSurface> {
        self.asked.borrow_mut().push(carriers.to_vec());
        let round = self.asked.borrow().len();
        carriers
            .iter()
            .map(|carrier| {
                let name = carrier.file_name().unwrap().to_str().unwrap();
                let stem = name.strip_suffix(".svelte").unwrap();
                ForeignSurface {
                    carrier: carrier.clone(),
                    path: canonicalize_path(
                        &carrier
                            .with_file_name(format!("{stem}.d.svelte.ts"))
                            .to_string_lossy(),
                    ),
                    text: (!self.fails.contains(&name)).then(|| {
                        Arc::from(format!("export declare const {stem}: {round};").as_str())
                    }),
                }
            })
            .collect()
    }
}

/// One Vue carrier and two carriers the driver does not own.
fn mixed_project(f: &Fixture) {
    f.write(
        "tsconfig.json",
        r#"{"include":["*.vue","*.svelte"],"compilerOptions":{"noEmit":true}}"#,
    );
    f.write(
        "View.vue",
        r#"<script setup lang="ts">const value = 1;</script>"#,
    );
    let svelte = "<script lang=\"ts\">let count: number = 1</script>\n<p>{count}</p>\n";
    f.write("Zed.svelte", svelte);
    f.write("Alpha.svelte", svelte);
}

fn names(paths: &[PathBuf]) -> Vec<&str> {
    paths
        .iter()
        .map(|path| path.file_name().unwrap().to_str().unwrap())
        .collect()
}

/// The overlay the project's program was served, rebuilt from its check.
fn served(project: &DriverProject, check: &ProjectCheck) -> Vec<OverlayFile> {
    project_overlay(&ProjectProgramInputs {
        project,
        carriers: &check.carriers,
        surfaces: &check.foreign_surfaces,
        no_emit: true,
    })
}

#[test]
fn a_project_overlays_the_surfaces_of_carriers_it_does_not_own() {
    let f = Fixture::new();
    mixed_project(&f);
    let source = ScriptedSurfaces::default();
    let outcome = SfcTypeScriptDriverAdapter::new()
        .run(&f.request(fake_engine(&f)), Some(&source))
        .unwrap();
    let check = &outcome.projects[0];
    assert!(check.engine_failure.is_none(), "{:?}", check.engine_failure);
    let asked = source.asked.borrow();
    assert_eq!(asked.len(), 1, "asked once per project");
    assert_eq!(names(&asked[0]), ["Alpha.svelte", "Zed.svelte"]);
    assert_eq!(asked[0], check.unowned_carriers);
    let expected = source_surfaces(&asked[0], 1);
    assert_eq!(check.foreign_surfaces, expected);
    let overlay = served(&project(f.path("tsconfig.json")), check);
    assert_eq!(overlay.len(), 3, "one carrier and two surfaces");
    for (file, surface) in overlay[1..].iter().zip(&expected) {
        assert_eq!(file.path, surface.path);
        assert_eq!(file.content, surface.text.as_deref().unwrap());
    }
}

/// What `ScriptedSurfaces` renders for `carriers` in its `round`th answer.
fn source_surfaces(carriers: &[PathBuf], round: usize) -> Vec<ForeignSurface> {
    let source = ScriptedSurfaces::default();
    for _ in 1..round {
        source.surfaces(&[]);
    }
    source.surfaces(carriers)
}

#[test]
fn a_project_that_owns_every_carrier_asks_for_no_surface() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"include":["*.vue"],"compilerOptions":{"noEmit":true}}"#,
    );
    f.write(
        "View.vue",
        r#"<script setup lang="ts">const value = 1;</script>"#,
    );
    let source = ScriptedSurfaces::default();
    let outcome = SfcTypeScriptDriverAdapter::new()
        .run(&f.request(fake_engine(&f)), Some(&source))
        .unwrap();
    let check = &outcome.projects[0];
    assert!(check.engine_failure.is_none(), "{:?}", check.engine_failure);
    assert_eq!(check.carriers.len(), 1);
    assert!(source.asked.borrow().is_empty());
    assert!(check.foreign_surfaces.is_empty());
}

#[test]
fn a_failed_surface_is_served_as_an_empty_module() {
    let f = Fixture::new();
    mixed_project(&f);
    let source = ScriptedSurfaces {
        fails: vec!["Zed.svelte"],
        ..ScriptedSurfaces::default()
    };
    let outcome = SfcTypeScriptDriverAdapter::new()
        .run(&f.request(fake_engine(&f)), Some(&source))
        .unwrap();
    let check = &outcome.projects[0];
    assert!(check.engine_failure.is_none(), "{:?}", check.engine_failure);
    let overlay = served(&project(f.path("tsconfig.json")), check);
    let zed = &check.foreign_surfaces[1];
    assert!(zed.text.is_none());
    let file = overlay.iter().find(|file| file.path == zed.path).unwrap();
    assert_eq!(file.content, "export {};");
    let alpha = &check.foreign_surfaces[0];
    let file = overlay.iter().find(|file| file.path == alpha.path).unwrap();
    assert_eq!(file.content, alpha.text.as_deref().unwrap());
}

#[test]
fn a_changed_foreign_carrier_refreshes_only_its_own_surface() {
    use super::super::project_graph::ProjectGraph;
    use super::super::watch_binding::ProjectWatchProjectionBinding;

    let f = Fixture::new();
    mixed_project(&f);
    let source = ScriptedSurfaces::default();
    let outcome = SfcTypeScriptDriverAdapter::new()
        .run(&f.request(fake_engine(&f)), Some(&source))
        .unwrap();
    let check = &outcome.projects[0];
    let (alpha, zed) = (&check.unowned_carriers[0], &check.unowned_carriers[1]);
    let graph = ProjectGraph::load(BuildMode::Build, &[f.path("tsconfig.json")]).unwrap();
    let recheck = ProjectWatchProjectionBinding::new(&graph).recheck_for(std::slice::from_ref(zed));
    let changed = recheck
        .carriers
        .iter()
        .map(|(_, carrier)| carrier.clone())
        .collect::<Vec<_>>();
    assert_eq!(changed, std::slice::from_ref(zed));

    let refreshed = foreign_surfaces(
        Some(&source),
        &check.unowned_carriers,
        &check.foreign_surfaces,
        &changed,
    );
    let asked = source.asked.borrow();
    assert_eq!(asked.len(), 2);
    assert_eq!(
        asked[1],
        std::slice::from_ref(zed),
        "only the changed carrier is asked"
    );
    assert_eq!(refreshed[0], check.foreign_surfaces[0], "{alpha:?} is kept");
    assert_eq!(
        refreshed[1],
        source_surfaces(std::slice::from_ref(zed), 2)[0]
    );
    assert_ne!(refreshed[1], check.foreign_surfaces[1]);
}
