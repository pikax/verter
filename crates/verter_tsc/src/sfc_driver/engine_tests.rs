//! Real engine coverage; prerequisites are required, so the gate cannot silently skip.

use super::*;
use std::path::{Path, PathBuf};
use verter_tsgo_api::toolchain::discovery::ResolutionRequest;
use verter_workspace::native_fs::NativeFs;

struct Fixture {
    dir: tempfile::TempDir,
    fs: NativeFs,
    engine: PathBuf,
}
fn example_package() -> PathBuf {
    let example = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("packages/example");
    assert!(
        example.join("node_modules/vue").is_dir(),
        "run the frozen pnpm install before the engine tests"
    );
    example
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("projection-program-")
            .tempdir_in(example_package())
            .unwrap();
        Self::at(dir)
    }
    /// A project in the system temporary directory, so the `node_modules`
    /// walk from its files reaches only the packages it installs itself.
    #[cfg(unix)]
    fn detached() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("projection-program-")
            .tempdir()
            .unwrap();
        Self::at(dir)
    }
    fn at(dir: tempfile::TempDir) -> Self {
        let request = ResolutionRequest::for_environment(Capability::Api, Some(example_package()));
        let engine = crate::checker::resolve_tsgo_engine_for(&request)
            .expect("the pinned API engine is required")
            .path;
        Self {
            dir,
            fs: NativeFs::new(),
            engine,
        }
    }
    fn write(&self, name: &str, text: &str) {
        let path = self.dir.path().join(name);
        self.fs
            .create_dir_all(path.parent().unwrap().to_str().unwrap())
            .unwrap();
        self.fs.write_file(path.to_str().unwrap(), text).unwrap();
    }
    fn request(&self) -> DriverRequest {
        DriverRequest {
            mode: BuildMode::Build,
            configs: vec![self.dir.path().join("tsconfig.json")],
            no_emit: true,
            declaration: false,
            declaration_dir: None,
            tsgo_bin: Some(self.engine.clone()),
            force: false,
            watch: false,
        }
    }
    fn run(&self) -> RunOutcome {
        let outcome = run(&self.request(), None).unwrap();
        assert!(
            outcome.graph_problems.is_empty(),
            "{:?}",
            outcome.graph_problems
        );
        for project in &outcome.projects {
            assert!(
                project.engine_failure.is_none(),
                "{}: {:?}",
                project.config,
                project.engine_failure
            );
        }
        outcome
    }
    fn base(&self) {
        self.write("base.json", r#"{"compilerOptions":{"strict":true,"target":"ES2022","module":"ESNext","moduleResolution":"bundler","skipLibCheck":true,"lib":["ES2022","DOM"],"types":[],"noEmit":true}}"#);
    }
}

#[test]
fn noemit_checks_a_shared_carrier_once_in_each_owning_project() {
    let f = Fixture::new();
    f.base();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"first.json"},{"path":"second.json"}]}"#,
    );
    f.write(
        "first.json",
        r#"{"extends":"./base.json","include":["*.vue"]}"#,
    );
    f.write(
        "second.json",
        r#"{"extends":"./base.json","include":["*.vue"]}"#,
    );
    f.write("View.vue", r#"<script setup lang="ts">const value: string = 123;</script><template>{{ value }}</template>"#);
    let outcome = f.run();
    assert_eq!(outcome.carriers_checked, 2);
    for project in &outcome.projects[..2] {
        assert_eq!(project.attributed.len(), 1, "{:?}", project.attributed);
        let diagnostic = &project.attributed[0];
        assert_eq!(diagnostic.code, 2322);
        assert!(diagnostic.carrier.ends_with("View.vue"));
        assert!(!diagnostic.message.contains(".vue.ts"));
        assert!(matches!(
            diagnostic.anchor,
            verter_session::sfc_projection_diagnostics::DiagnosticAnchor::Authored { .. }
        ));
    }
}

/// Two sibling projects: `a` lists its carriers and is not strict; `b` lists
/// only its TypeScript sources, is strict, and imports `a/C.vue`.
fn sibling_projects(f: &Fixture) {
    f.base();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"./a"},{"path":"./b"}]}"#,
    );
    f.write(
        "a/tsconfig.json",
        r#"{"extends":"../base.json","compilerOptions":{"strict":false},"include":["*.vue"]}"#,
    );
    f.write(
        "b/tsconfig.json",
        r#"{"extends":"../base.json","include":["*.ts"]}"#,
    );
    f.write(
        "b/x.ts",
        "import C from \"../a/C.vue\";\n\nexport const component = C;\n",
    );
}

fn project_named<'a>(outcome: &'a RunOutcome, config: &str) -> &'a ProjectCheck {
    outcome
        .projects
        .iter()
        .find(|project| project.config.ends_with(config))
        .unwrap_or_else(|| panic!("no project {config}"))
}

/// A carrier reached only by import from a project that does not list it
/// resolves there, as vue-tsc gives every carrier its program reaches a
/// virtual file. It is served, not checked as a member.
#[test]
fn a_carrier_imported_from_outside_the_membership_resolves() {
    let f = Fixture::new();
    sibling_projects(&f);
    f.write(
        "a/C.vue",
        r#"<script setup lang="ts">const label: string = "text";</script><template>{{ label }}</template>"#,
    );
    let outcome = f.run();
    for project in &outcome.projects {
        assert!(
            project.attributed.is_empty(),
            "{}: {:?}",
            project.config,
            project.attributed
        );
    }
    let b = project_named(&outcome, "b/tsconfig.json");
    assert!(b.carriers.is_empty());
    let [imported] = b.imported_carriers.as_slice() else {
        panic!("one imported carrier: {:?}", b.imported_carriers);
    };
    assert!(
        imported.carrier.ends_with("a/C.vue"),
        "{}",
        imported.carrier
    );
    assert!(project_named(&outcome, "a/tsconfig.json")
        .imported_carriers
        .is_empty());
    assert_eq!(outcome.carriers_checked, 1);
    assert_eq!(exit_status(&outcome), ExitStatus::Clean);
}

/// A carrier reached by import is checked under the importing project's
/// options, and its errors are reported at its authored position, as tsc
/// reports a non-root source file of the program.
#[test]
fn a_carrier_imported_from_outside_the_membership_is_checked_with_the_importers_options() {
    let f = Fixture::new();
    sibling_projects(&f);
    f.write(
        "a/C.vue",
        r#"<script setup lang="ts">const value: number = null;</script><template>{{ value }}</template>"#,
    );
    let outcome = f.run();
    assert!(project_named(&outcome, "a/tsconfig.json")
        .attributed
        .is_empty());
    let b = project_named(&outcome, "b/tsconfig.json");
    let [diagnostic] = b.attributed.as_slice() else {
        panic!("one diagnostic: {:?}", b.attributed);
    };
    assert_eq!(diagnostic.code, 2322);
    assert!(
        diagnostic.carrier.ends_with("a/C.vue"),
        "{}",
        diagnostic.carrier
    );
    assert!(!diagnostic.message.contains(".vue.ts"));
    assert!(matches!(
        diagnostic.anchor,
        verter_session::sfc_projection_diagnostics::DiagnosticAnchor::Authored { .. }
    ));
    let printed = outcome_diagnostics(&outcome);
    let [line] = printed.as_slice() else {
        panic!("one printed line: {printed:?}");
    };
    assert!(line.file.ends_with("a/C.vue"), "{}", line.file);
    assert_eq!((line.line, line.ts_code), (1, 2322));
    assert_eq!(exit_status(&outcome), ExitStatus::Diagnostics);
}

/// An emitting project with no carrier is built by the engine, which cannot
/// compile a carrier. When it imports a sibling's carrier it is refused once,
/// naming the carrier: never reported with a missing-module error for the
/// import and never clean. A missing module that is no carrier keeps the
/// engine's own error.
#[test]
fn an_emitting_project_that_imports_a_siblings_carrier_is_refused_naming_the_carrier() {
    let f = Fixture::new();
    sibling_projects(&f);
    let emitting = r#"{"extends":"../base.json","compilerOptions":{"noEmit":false,"outDir":"dist"},"include":["*.ts"]}"#;
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"./a"},{"path":"./b"},{"path":"./c"}]}"#,
    );
    f.write("b/tsconfig.json", emitting);
    f.write("c/tsconfig.json", emitting);
    f.write(
        "c/y.ts",
        "import { value } from \"./absent\";\n\nexport const copy = value;\n",
    );
    f.write(
        "a/C.vue",
        r#"<script setup lang="ts">const label: string = "text";</script><template>{{ label }}</template>"#,
    );
    let outcome = run(&f.request(), None).unwrap();
    assert!(
        outcome.graph_problems.is_empty(),
        "{:?}",
        outcome.graph_problems
    );
    let b = project_named(&outcome, "b/tsconfig.json");
    let refusal = b
        .engine_failure
        .as_deref()
        .expect("the project importing a carrier is refused");
    assert!(
        refusal.contains("b/tsconfig.json") && refusal.contains("a/C.vue"),
        "{refusal}"
    );
    assert!(b.raw.is_empty() && b.attributed.is_empty(), "{:?}", b.raw);
    let c = project_named(&outcome, "c/tsconfig.json");
    assert!(c.engine_failure.is_none(), "{:?}", c.engine_failure);
    assert_eq!(
        c.raw.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![2307],
        "{:?}",
        c.raw
    );
    let printed = outcome_diagnostics(&outcome);
    assert!(
        !printed.iter().any(|d| d.file.ends_with("b/x.ts")),
        "{printed:?}"
    );
    let failures = printed
        .iter()
        .filter(|d| d.verter_code == Some(crate::reporter::VerterCode::ProjectCheckFailed))
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0].file, &*b.config);
    assert_eq!(exit_status(&outcome), ExitStatus::Failed);
}

#[test]
fn generated_solution_projects_keep_ambient_alias_component_and_server_options() {
    let f = Fixture::new();
    f.base();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":".nuxt/app.json"},{"path":".nuxt/server.json"}]}"#,
    );
    f.write(".nuxt/app.json", r#"{"extends":"../base.json","include":["../app/**/*.vue","./*.d.ts"],"compilerOptions":{"paths":{"~/*":["../app/*"]},"rootDir":".."}}"#);
    f.write(".nuxt/server.json", r#"{"extends":"../base.json","include":["../server/**/*.ts"],"compilerOptions":{"strictNullChecks":false}}"#);
    f.write(".nuxt/imports.d.ts", "declare const computed: typeof import('vue')['computed']; declare function useRoute(): { path: string };
");
    f.write(
        "app/Child.vue",
        r#"<script setup lang="ts">defineProps<{ label: string }>();</script>"#,
    );
    f.write(".nuxt/components.d.ts", "export {}; declare module 'vue' { export interface GlobalComponents { GlobalChild: typeof import('../app/Child.vue')['default'] } }
");
    f.write("app/View.vue", r#"<script setup lang="ts">import Child from '~/Child.vue'; const value = computed(() => useRoute().path);</script><template><GlobalChild :label="value" /><Child :label="value" /></template>"#);
    f.write(
        "server/options.ts",
        "const allowedByServer: string = null; export {};
",
    );
    let clean = f.run();
    assert_eq!(clean.carriers_checked, 2);
    for project in &clean.projects {
        assert!(
            project.attributed.is_empty(),
            "{}: {:?}",
            project.config,
            project.attributed
        );
    }
    // The ambient component's props remain exact, and the carrier-free server
    // still reports its own errors under its own compiler options.
    f.write("app/View.vue", r#"<script setup lang="ts">const value = 42;</script><template><GlobalChild :label="value" /></template>"#);
    f.write(
        "server/options.ts",
        "const wrong: number = 'bad'; export {};
",
    );
    let errors = f.run();
    assert_eq!(
        errors.projects[0].attributed.len(),
        1,
        "{:?}",
        errors.projects[0].attributed
    );
    assert_eq!(errors.projects[0].attributed[0].code, 2322);
    assert!(errors.projects[0].attributed[0]
        .carrier
        .ends_with("View.vue"));
    assert_eq!(
        errors.projects[1].attributed.len(),
        1,
        "{:?}",
        errors.projects[1].attributed
    );
    assert_eq!(errors.projects[1].attributed[0].code, 2322);
    assert!(errors.projects[1].attributed[0]
        .carrier
        .ends_with("server/options.ts"));
}

/// A Nuxt app depends on the framework, not on `vue`; the generated config
/// maps the exact specifier `vue` to the framework's copy, and nothing maps
/// `vue/jsx-runtime`. vue-tsc reports only the authored error here: its own
/// `vue/jsx-runtime` read fails in generated code it never reports.
#[cfg(unix)]
#[test]
fn vue_reached_only_through_the_framework_package_reports_only_authored_errors() {
    let f = Fixture::detached();
    for dir in f.dir.path().ancestors() {
        assert!(
            !dir.join("node_modules/vue").exists(),
            "{} must not hold vue",
            dir.display()
        );
    }
    f.base();
    f.write(
        "node_modules/framework/package.json",
        r#"{"name":"framework","version":"1.0.0"}"#,
    );
    let framework_modules = f.dir.path().join("node_modules/framework/node_modules");
    std::fs::create_dir_all(&framework_modules).unwrap();
    std::os::unix::fs::symlink(
        example_package().join("node_modules/vue"),
        framework_modules.join("vue"),
    )
    .unwrap();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"./.nuxt/tsconfig.app.json"}]}"#,
    );
    f.write(".nuxt/tsconfig.app.json", r#"{"extends":"../base.json","compilerOptions":{"paths":{"vue":["../node_modules/framework/node_modules/vue"]}},"include":["../app/**/*.vue","./*.d.ts"]}"#);
    f.write(".nuxt/components.d.ts", "export {};\ndeclare module 'vue' { export interface GlobalComponents { GlobalChild: typeof import('../app/Child.vue')['default'] } }\n");
    f.write(
        "app/Child.vue",
        "<script setup lang=\"ts\">\ndefineProps<{ label: string }>();\n</script>\n<template><span>{{ label }}</span></template>\n",
    );
    let view = "<script setup lang=\"ts\">\nconst count: number = \"one\";\n</script>\n<template><div class=\"box\">{{ count }}<GlobalChild label=\"a\" /><global-child label=\"b\" /></div></template>\n";
    f.write("app/View.vue", view);
    let outcome = f.run();
    assert_eq!(outcome.carriers_checked, 2);
    let attributed = outcome
        .projects
        .iter()
        .flat_map(|project| &project.attributed)
        .collect::<Vec<_>>();
    assert_eq!(attributed.len(), 1, "{attributed:?}");
    let diagnostic = attributed[0];
    assert_eq!(diagnostic.code, 2322);
    assert!(diagnostic.carrier.ends_with("View.vue"));
    let count = view.find("count").unwrap() as u32;
    assert!(matches!(
        diagnostic.anchor,
        verter_session::sfc_projection_diagnostics::DiagnosticAnchor::Authored { start, .. } if start == count
    ));
}

#[test]
fn removed_base_url_option_matches_the_native_engine() {
    let f = Fixture::new();
    f.base();
    f.write(
        "tsconfig.json",
        r#"{"extends":"./base.json","include":["index.ts"],"compilerOptions":{"baseUrl":"."}}"#,
    );
    f.write("index.ts", "export {};");
    let mut command = crate::checker::checker_base_command(&f.engine);
    command
        .arg("--project")
        .arg(f.dir.path().join("tsconfig.json"))
        .args(["--pretty", "false", "--noEmit"]);
    let native = crate::checker::invoke_command_bounded(
        command,
        "baseUrl config check",
        std::time::Duration::from_secs(5),
    )
    .unwrap();
    let native_diagnostics = crate::reporter::parse_tsc_output(&native.output);
    let outcome = f.run();
    assert_eq!(outcome.projects.len(), 1);
    let diagnostics = &outcome.projects[0].attributed;
    assert_eq!(
        diagnostics
            .iter()
            .map(|d| (d.code, d.message.as_str()))
            .collect::<Vec<_>>(),
        native_diagnostics
            .iter()
            .map(|d| (d.ts_code, d.message.as_str()))
            .collect::<Vec<_>>(),
        "native output: {}",
        native.output
    );
    assert_eq!(
        native.success,
        !diagnostics
            .iter()
            .any(|d| d.category
                == verter_session::sfc_projection_diagnostics::DiagnosticCategory::Error),
        "native output: {}",
        native.output
    );
    assert!(diagnostics
        .iter()
        .all(|d| d.carrier.ends_with("tsconfig.json")));
}

#[test]
fn solution_root_with_references_has_no_false_errors() {
    let f = Fixture::new();
    f.base();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"app.json"}]}"#,
    );
    f.write(
        "app.json",
        r#"{"extends":"./base.json","include":["*.vue"]}"#,
    );
    f.write("View.vue", r#"<script setup lang="ts">const value = 'clean';</script><template><div>{{ value }}</div></template>"#);
    let outcome = f.run();
    assert_eq!(outcome.carriers_checked, 1);
    assert_eq!(outcome.projects.len(), 2);
    assert!(outcome.projects.iter().all(|p| p.attributed.is_empty()));
    assert!(outcome.projects[1].binding.is_none());
}

#[test]
fn ambient_global_helpers_stay_clean_with_unused_checks_enabled() {
    let f = Fixture::new();
    f.base();
    f.write("tsconfig.json", r#"{"extends":"./base.json","include":["*.vue","*.d.ts"],"compilerOptions":{"noUnusedLocals":true,"noUnusedParameters":true}}"#);
    f.write(
        "Child.vue",
        r#"<script setup lang="ts">defineProps<{ label: string }>();</script>"#,
    );
    f.write("components.d.ts", "export {}; declare module 'vue' { export interface GlobalComponents { GlobalChild: typeof import('./Child.vue')['default'] } }");
    f.write("View.vue", r#"<script setup lang="ts">const label = 'value';</script><template><GlobalChild :label="label" /></template>"#);
    let outcome = f.run();
    assert_eq!(outcome.carriers_checked, 2);
    assert!(
        outcome.projects[0].attributed.is_empty(),
        "{:?}",
        outcome.projects[0].attributed
    );
}

#[test]
fn implicit_roots_keep_unimported_typescript_errors_beside_carriers() {
    let f = Fixture::new();
    f.write("tsconfig.json", r#"{"compilerOptions":{"strict":true,"target":"ES2022","module":"ESNext","moduleResolution":"bundler","skipLibCheck":true,"types":[]}}"#);
    f.write("View.vue", "<template><div /></template>");
    f.write("broken.ts", "const value: string = 123; export {};");
    let outcome = f.run();
    assert_eq!(outcome.carriers_checked, 1);
    let diagnostics = &outcome.projects[0].attributed;
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2322);
    assert!(diagnostics[0].carrier.ends_with("broken.ts"));
}

#[test]
fn inherited_files_keep_ambient_inputs_when_companions_are_added() {
    let f = Fixture::new();
    f.base();
    f.write("shared/roots.json", r#"{"extends":"../base.json","files":["globals.d.ts","broken.ts"],"include":["../app/*.vue"]}"#);
    f.write("tsconfig.json", r#"{"extends":"./shared/roots.json"}"#);
    f.write("shared/globals.d.ts", "declare const ambientValue: string;");
    f.write("shared/broken.ts", "const value: string = 123; export {};");
    f.write("app/View.vue", "<script setup lang=\"ts\">const value: string = ambientValue;</script><template>{{ value }}</template>");
    let outcome = f.run();
    assert_eq!(outcome.carriers_checked, 1);
    let diagnostics = &outcome.projects[0].attributed;
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2322);
    assert!(diagnostics[0].carrier.ends_with("shared/broken.ts"));
}

#[test]
fn real_companion_shaped_source_keeps_its_authored_diagnostic() {
    let f = Fixture::new();
    f.base();
    f.write(
        "tsconfig.json",
        r#"{"extends":"./base.json","include":["*"]}"#,
    );
    f.write("View.vue", "<template><div /></template>");
    let source = "export const value: string = 123;";
    f.write("View.vue.ts", source);
    let outcome = f.run();
    assert_eq!(outcome.carriers_checked, 0);
    let project = &outcome.projects[0];
    assert_eq!(project.diagnostics.len(), 1, "{:?}", project.diagnostics);
    assert_eq!(
        project.diagnostics[0].severity,
        crate::reporter::Severity::Warning
    );
    assert_eq!(project.attributed.len(), 1, "{:?}", project.attributed);
    let diagnostic = &project.attributed[0];
    assert_eq!(diagnostic.code, 2322);
    assert!(diagnostic.carrier.ends_with("View.vue.ts"));
    let start = source.find("value").unwrap() as u32;
    assert_eq!(
        diagnostic.anchor,
        verter_session::sfc_projection_diagnostics::DiagnosticAnchor::Authored {
            start,
            end: start + 5
        }
    );
}

#[test]
fn empty_configured_project_reports_invalid_target_and_no_inputs() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"compilerOptions":{"target":"not-a-target","noEmit":true},"include":["src/**/*.ts"]}"#,
    );
    let outcome = f.run();
    assert_eq!(outcome.carriers_checked, 0);
    let project = &outcome.projects[0];
    assert!(project.binding.is_some());
    let codes = project
        .attributed
        .iter()
        .map(|d| d.code)
        .collect::<Vec<_>>();
    assert!(codes.contains(&6046), "{:?}", project.attributed);
    assert!(codes.contains(&18003), "{:?}", project.attributed);
    assert!(project
        .attributed
        .iter()
        .all(|d| d.carrier.ends_with("tsconfig.json")));
}

mod build {
    //! Real-engine build checks over a self-contained workspace fixture.

    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use super::super::{
        exit_status, outcome_diagnostics, run, run_engine_build, BuildMode, DriverRequest,
        EngineBuild, ExitStatus, ProjectGraph,
    };
    use crate::checker::{checker_base_command, invoke_command_bounded, resolve_tsgo_engine_for};
    use verter_tsgo_api::toolchain::discovery::ResolutionRequest;
    use verter_tsgo_api::toolchain::validation::Capability;
    use verter_workspace::{FilesystemOptions, FilesystemWorkspace};

    fn stage(source: &Path, dest: &Path) {
        std::fs::create_dir_all(dest).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let target = dest.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                stage(&entry.path(), &target);
            } else {
                // A Gin source view can be read-only. Staged authored files belong
                // to this test and must remain editable for incremental scenarios.
                std::fs::write(target, std::fs::read(entry.path()).unwrap()).unwrap();
            }
        }
    }

    fn outputs(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut result = BTreeMap::new();
        for package in ["a", "b"] {
            let package_root = root.join("packages").join(package);
            for file in [
                "dist/index.js",
                "dist/index.js.map",
                "dist/index.d.ts",
                "dist/index.d.ts.map",
                "tsconfig.tsbuildinfo",
            ] {
                let path = package_root.join(file);
                if path.is_file() {
                    result.insert(
                        path.strip_prefix(root).unwrap().to_path_buf(),
                        std::fs::read(&path).unwrap(),
                    );
                }
            }
        }
        result
    }

    fn engine() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        resolve_tsgo_engine_for(&ResolutionRequest::for_environment(
            Capability::Api,
            Some(root),
        ))
        .expect("real engine required; run the Toki install prepare need")
        .path
    }

    #[test]
    fn engine_build_emits_the_engines_own_bytes_and_force_replaces_current_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        stage(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/cases/fixtures/workspace_references"),
            root,
        );
        let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
        let loaded =
            ProjectGraph::load_published(&ws, BuildMode::Build, &[root.join("tsconfig.json")], 1)
                .unwrap();
        let build = EngineBuild::partition(&loaded.graph);
        assert_eq!(build.configs.len(), 2);
        assert!(build.unsupported.is_empty());
        let engine = engine();
        let result = run_engine_build(&build, &loaded.graph, &*ws, &engine, false, false);
        assert!(result.failure.is_none(), "{:?}", result.failure);
        assert!(
            result.projects.iter().all(|project| project.raw.is_empty()),
            "{:?}",
            result.projects
        );
        let expected = outputs(root);
        assert_eq!(expected.len(), 6);
        let mut command = checker_base_command(&engine);
        command
            .arg("-b")
            .arg(root.join("packages/tsconfig.json"))
            .args(["--pretty", "false", "--force"]);
        let direct =
            invoke_command_bounded(command, "fixture build", std::time::Duration::from_secs(5))
                .unwrap();
        assert!(direct.success, "{}", direct.output);
        assert_eq!(outputs(root), expected);
        std::fs::write(root.join("packages/b/dist/index.d.ts"), "corrupted").unwrap();
        let result = run_engine_build(&build, &loaded.graph, &*ws, &engine, true, false);
        assert!(result.failure.is_none(), "{:?}", result.failure);
        assert_eq!(outputs(root), expected);
    }

    #[test]
    fn engine_build_no_emit_leaves_only_build_cache_and_package_errors_keep_their_owner() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        stage(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/cases/fixtures/workspace_references"),
            root,
        );
        let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
        let loaded = ProjectGraph::load_published(
            &ws,
            BuildMode::Build,
            &[root.join("packages/a/tsconfig.json")],
            1,
        )
        .unwrap();
        let build = EngineBuild::partition(&loaded.graph);
        let engine = engine();
        let result = run_engine_build(&build, &loaded.graph, &*ws, &engine, false, true);
        assert!(result.failure.is_none(), "{:?}", result.failure);
        assert!(
            result.projects.iter().all(|project| project.raw.is_empty()),
            "{:?}",
            result.projects
        );
        let no_emit_outputs = outputs(root);
        std::fs::write(
            root.join("packages/a/src/index.ts"),
            "export const initial: number = 'wrong';\n",
        )
        .unwrap();
        let fresh = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
        let loaded = ProjectGraph::load_published(
            &fresh,
            BuildMode::Build,
            &[root.join("tsconfig.json")],
            1,
        )
        .unwrap();
        let build = EngineBuild::partition(&loaded.graph);
        let result = run_engine_build(&build, &loaded.graph, &*fresh, &engine, false, false);
        assert!(result.failure.is_none(), "{:?}", result.failure);
        assert!(result.projects[0]
            .config
            .ends_with("packages/a/tsconfig.json"));
        assert_eq!(
            result.projects[0]
                .raw
                .iter()
                .filter(|diagnostic| diagnostic.code == 2322)
                .count(),
            1
        );
        assert!(
            no_emit_outputs
                .keys()
                .all(|path| path.extension().is_some_and(|ext| ext == "tsbuildinfo")),
            "native --noEmit wrote JS, declarations or maps: {:?}",
            no_emit_outputs.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn engine_build_no_emit_preserves_the_referencing_projects_native_error() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        stage(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/cases/fixtures/workspace_references"),
            root,
        );
        let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
        let loaded =
            ProjectGraph::load_published(&ws, BuildMode::Build, &[root.join("tsconfig.json")], 1)
                .unwrap();
        let build = EngineBuild::partition(&loaded.graph);
        let result = run_engine_build(&build, &loaded.graph, &*ws, &engine(), false, true);
        assert!(result.failure.is_none(), "{:?}", result.failure);
        let errors: Vec<_> = result
            .projects
            .iter()
            .flat_map(|project| {
                project
                    .raw
                    .iter()
                    .map(move |diagnostic| (&project.config, diagnostic))
            })
            .collect();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].0.ends_with("packages/b/tsconfig.json"));
        assert_eq!(errors[0].1.code, 6310);
        assert_eq!(
            errors[0].1.category,
            verter_session::sfc_projection_diagnostics::DiagnosticCategory::Error
        );
        let written = outputs(root);
        assert!(
            written
                .keys()
                .all(|path| path.extension().is_some_and(|ext| ext == "tsbuildinfo")),
            "native --noEmit with TS6310 wrote JS, declarations or maps: {:?}",
            written.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn engine_build_progress_attributes_shared_strict_null_errors_to_the_building_project() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(
            root.join("shared.ts"),
            "export const value: string = null;\n",
        )
        .unwrap();
        let configs: Vec<_> = [("a", false), ("b", true)].into_iter().map(|(name, strict)| {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("tsconfig.json");
        std::fs::write(&config, format!(r#"{{"compilerOptions":{{"composite":true,"strictNullChecks":{strict},"rootDir":"..","outDir":"dist"}},"files":["../shared.ts"]}}"#)).unwrap();
        config
    }).collect();
        let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
        let loaded = ProjectGraph::load_published(&ws, BuildMode::Build, &configs, 1).unwrap();
        let build = EngineBuild::partition(&loaded.graph);
        let result = run_engine_build(&build, &loaded.graph, &*ws, &engine(), false, false);
        assert!(result.failure.is_none(), "{:?}", result.failure);
        assert!(result.projects[0].raw.is_empty(), "{:?}", result.projects);
        assert_eq!(result.projects[1].raw.len(), 1, "{:?}", result.projects);
        assert_eq!(result.projects[1].raw[0].code, 2322);
    }

    #[test]
    fn engine_build_preserves_mixed_fileless_and_located_errors() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(
            root.join("index.ts"),
            "export const value: number = 'wrong';\n",
        )
        .unwrap();
        std::fs::write(
            root.join("tsconfig.json"),
            r#"{"compilerOptions":{"noLib":true},"files":["index.ts"]}"#,
        )
        .unwrap();
        // The engine suppresses semantic errors when globals are missing, and
        // suppresses global diagnostics when syntax is invalid. Independent
        // programs produce both categories in one real build's output.
        std::fs::write(root.join("checked.json"), r#"{"files":["index.ts"]}"#).unwrap();
        let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
        let loaded = ProjectGraph::load_published(
            &ws,
            BuildMode::Build,
            &[root.join("tsconfig.json"), root.join("checked.json")],
            1,
        )
        .unwrap();
        let build = EngineBuild::partition(&loaded.graph);
        let result = run_engine_build(&build, &loaded.graph, &*ws, &engine(), false, true);
        assert!(result.failure.is_none(), "{:?}", result.failure);
        let check = &result.projects[0];
        assert!(
            check
                .raw
                .iter()
                .any(|d| d.code == 2318 && d.file.is_empty()),
            "{check:?}"
        );
        let located = &result.projects[1];
        assert!(
            located
                .raw
                .iter()
                .any(|d| d.code == 2322 && d.file.ends_with("index.ts")),
            "{located:?}"
        );
        for project in &result.projects {
            assert_eq!(project.raw.len(), project.attributed.len());
        }
    }

    /// The fixture staged where `vue` resolves, with the app's package link
    /// to B's directory, so the app reads B through B's emitted declarations.
    fn linked_workspace(f: &super::Fixture) -> PathBuf {
        let root = f.dir.path().to_path_buf();
        stage(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/cases/fixtures/workspace_references"),
            &root,
        );
        std::fs::create_dir_all(root.join("app/node_modules")).unwrap();
        verter_test_support::link_directory(
            &root.join("packages/b"),
            &root.join("app/node_modules/fixture-library"),
        )
        .unwrap();
        root
    }

    fn request(config: PathBuf, engine: &Path, no_emit: bool, force: bool) -> DriverRequest {
        DriverRequest {
            mode: BuildMode::Build,
            configs: vec![config],
            no_emit,
            declaration: false,
            declaration_dir: None,
            tsgo_bin: Some(engine.to_path_buf()),
            force,
            watch: false,
        }
    }

    fn project_at(outcome: &super::RunOutcome, suffix: &str) -> usize {
        outcome
            .projects
            .iter()
            .position(|project| project.config.ends_with(suffix))
            .unwrap_or_else(|| panic!("no project {suffix} in {:?}", outcome.projects))
    }

    #[test]
    fn one_build_emits_the_packages_before_the_app_carrier_is_checked() {
        let f = super::Fixture::new();
        let root = linked_workspace(&f);
        assert!(!root.join("packages/b/dist").exists());
        let outcome = run(
            &request(root.join("tsconfig.json"), &f.engine, false, false),
            None,
        )
        .unwrap();
        assert!(
            outcome.graph_problems.is_empty(),
            "{:?}",
            outcome.graph_problems
        );
        // The carrier reads B's type through B's emitted declarations, so a
        // clean app proves B was emitted before the app's program opened.
        assert_eq!(
            exit_status(&outcome),
            ExitStatus::Clean,
            "{:?}",
            outcome_diagnostics(&outcome)
        );
        let (a, b, app) = (
            project_at(&outcome, "/packages/a/tsconfig.json"),
            project_at(&outcome, "/packages/b/tsconfig.json"),
            project_at(&outcome, "/app/tsconfig.json"),
        );
        assert!(a < b && b < app, "graph order: {:?}", outcome.projects);
        assert!(outcome.projects[b].binding.is_none());
        assert_eq!(outcome.projects[app].carriers.len(), 1);
        assert!(outcome.projects[app].binding.is_some());
        let emitted = outputs(&root);
        assert_eq!(emitted.len(), 6, "{:?}", emitted.keys());
        let mut command = checker_base_command(&f.engine);
        command
            .arg("-b")
            .arg(root.join("packages/tsconfig.json"))
            .args(["--pretty", "false", "--force"]);
        let direct =
            invoke_command_bounded(command, "fixture build", std::time::Duration::from_secs(5))
                .unwrap();
        assert!(direct.success, "{}", direct.output);
        assert_eq!(outputs(&root), emitted);
    }

    #[test]
    fn declaration_publication_leaves_the_engine_built_packages_output_alone() {
        let f = super::Fixture::new();
        let root = linked_workspace(&f);
        let outcome = run(
            &DriverRequest {
                declaration: true,
                ..request(root.join("tsconfig.json"), &f.engine, false, false)
            },
            None,
        )
        .unwrap();
        assert_eq!(
            exit_status(&outcome),
            ExitStatus::Clean,
            "{:?}",
            outcome_diagnostics(&outcome)
        );
        // The engine wrote the packages' output; the driver publishes no
        // declarations over it, so it stays the engine's own build.
        let emitted = outputs(&root);
        assert_eq!(emitted.len(), 6, "{:?}", emitted.keys());
        let mut command = checker_base_command(&f.engine);
        command
            .arg("-b")
            .arg(root.join("packages/tsconfig.json"))
            .args(["--pretty", "false", "--force"]);
        let direct =
            invoke_command_bounded(command, "fixture build", std::time::Duration::from_secs(5))
                .unwrap();
        assert!(direct.success, "{}", direct.output);
        assert_eq!(outputs(&root), emitted);
    }

    #[test]
    fn a_package_error_prints_once_under_its_package_and_a_changed_export_reaches_the_carrier() {
        let f = super::Fixture::new();
        let root = linked_workspace(&f);
        let package_source = root.join("packages/a/src/index.ts");
        let original = std::fs::read_to_string(&package_source).unwrap();
        std::fs::write(
            &package_source,
            format!("{original}const seeded: string = 1;\n"),
        )
        .unwrap();
        let outcome = run(
            &request(root.join("tsconfig.json"), &f.engine, false, false),
            None,
        )
        .unwrap();
        let seeded = outcome_diagnostics(&outcome)
            .into_iter()
            .filter(|d| d.file.ends_with("/packages/a/src/index.ts"))
            .collect::<Vec<_>>();
        assert_eq!(seeded.len(), 1, "{seeded:?}");
        assert_eq!(seeded[0].ts_code, 2322);
        let owners = outcome
            .projects
            .iter()
            .filter(|project| {
                project
                    .attributed
                    .iter()
                    .any(|d| d.carrier.ends_with("/packages/a/src/index.ts"))
            })
            .map(|project| project.config.to_string())
            .collect::<Vec<_>>();
        assert_eq!(owners.len(), 1, "{owners:?}");
        assert!(
            owners[0].ends_with("/packages/a/tsconfig.json"),
            "{owners:?}"
        );
        assert_eq!(exit_status(&outcome), ExitStatus::Diagnostics);

        std::fs::write(&package_source, &original).unwrap();
        std::fs::write(
            root.join("packages/b/src/index.ts"),
            "import { initial } from \"../../a/src/index\";\nexport const answer: string = String(initial.value);\n",
        )
        .unwrap();
        let outcome = run(
            &request(root.join("tsconfig.json"), &f.engine, false, false),
            None,
        )
        .unwrap();
        let diagnostics = outcome_diagnostics(&outcome);
        assert_eq!(
            diagnostics
                .iter()
                .map(|d| (d.ts_code, d.line, d.file.ends_with("/app/App.vue")))
                .collect::<Vec<_>>(),
            vec![(2322, 3, true)],
            "{diagnostics:?}"
        );
        let app = &outcome.projects[project_at(&outcome, "/app/tsconfig.json")];
        assert_eq!(app.attributed.len(), 1, "{:?}", app.attributed);
        assert_eq!(exit_status(&outcome), ExitStatus::Diagnostics);
    }

    #[test]
    fn watch_loop_a_package_edit_rebuilds_it_and_the_dependent_carrier_sees_the_new_export() {
        package_edit_in_watch(false);
    }

    #[test]
    fn watch_loop_a_package_emitting_outside_its_directory_still_reaches_the_dependent_carrier() {
        package_edit_in_watch(true);
    }

    /// Watch the linked workspace, change B's export so the carrier breaks,
    /// and compare with a fresh run. `outside` makes B's output directory a
    /// link to a directory outside B, so what the app reads resolves there.
    fn package_edit_in_watch(outside: bool) {
        use crate::sfc_driver::watch::loop_harness::{found, fresh, Watching, CHANGE_LINE};

        let f = super::Fixture::new();
        let root = linked_workspace(&f);
        if outside {
            std::fs::create_dir_all(root.join("out/b")).unwrap();
            verter_test_support::link_directory(&root.join("out/b"), &root.join("packages/b/dist"))
                .unwrap();
        }
        let request = request(root.join("tsconfig.json"), &f.engine, false, false);
        let mut watching = Watching::start(request.clone());
        watching.caught_up();

        let source = root.join("packages/b/src/index.ts");
        std::fs::write(
            &source,
            "import { initial } from \"../../a/src/index\";\nexport const answer: string = String(initial.value);\n",
        )
        .unwrap();
        watching.change(&PathBuf::from(verter_span::path::canonicalize_path(
            &f.fs.realpath(source.to_str().unwrap()).unwrap(),
        )));
        let (status, output, _) = watching.close(None);

        // Only a rebuilt package gives the carrier the new export's type.
        let (lines, errors) = fresh(&request);
        assert_eq!(errors, 1, "{lines:?}");
        assert!(lines[0].contains("TS2322"), "{lines:?}");
        let mut expected = vec![found(0), CHANGE_LINE.to_owned()];
        expected.extend(lines);
        expected.push(found(errors));
        assert_eq!(output, expected);
        assert_eq!(status, ExitStatus::Diagnostics);
    }

    #[test]
    fn the_requested_no_emit_and_force_reach_the_engine_build() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        stage(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/cases/fixtures/workspace_references"),
            root,
        );
        let engine = engine();
        let package = root.join("packages/a/tsconfig.json");
        let checked = run(&request(package.clone(), &engine, true, false), None).unwrap();
        assert_eq!(
            exit_status(&checked),
            ExitStatus::Clean,
            "{:?}",
            outcome_diagnostics(&checked)
        );
        assert!(
            outputs(root)
                .keys()
                .all(|path| path.extension().is_some_and(|ext| ext == "tsbuildinfo")),
            "--noEmit wrote JS, declarations or maps: {:?}",
            outputs(root).keys().collect::<Vec<_>>()
        );
        let emitted = run(&request(package.clone(), &engine, false, false), None).unwrap();
        assert_eq!(exit_status(&emitted), ExitStatus::Clean);
        let declaration = root.join("packages/a/dist/index.d.ts");
        let expected = std::fs::read(&declaration).expect("the build emits declarations");
        std::fs::write(&declaration, "corrupted").unwrap();
        run(&request(package.clone(), &engine, false, false), None).unwrap();
        assert_eq!(
            std::fs::read_to_string(&declaration).unwrap(),
            "corrupted",
            "an up-to-date project is not rebuilt without --force"
        );
        run(&request(package, &engine, false, true), None).unwrap();
        assert_eq!(std::fs::read(&declaration).unwrap(), expected);
    }
}

mod declaration_output {
    use super::*;
    use crate::reporter::VerterCode;

    const OPTIONS: &str = r#""strict":true,"target":"ES2022","module":"ESNext","moduleResolution":"bundler","skipLibCheck":true,"lib":["ES2022","DOM"],"types":[]"#;

    fn declare(f: &Fixture, mode: BuildMode, config: &str) -> RunOutcome {
        run(
            &DriverRequest {
                mode,
                configs: vec![f.dir.path().join(config)],
                no_emit: false,
                declaration: true,
                declaration_dir: None,
                tsgo_bin: Some(f.engine.clone()),
                force: false,
                watch: false,
            },
            None,
        )
        .unwrap()
    }

    fn files_under(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut pending = vec![dir.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    files.push(path);
                }
            }
        }
        files.sort();
        files
    }

    fn refusals(project: &ProjectCheck) -> Vec<&str> {
        project
            .diagnostics
            .iter()
            .filter(|d| d.verter_code == Some(VerterCode::PublicApiProjectionFailure))
            .map(|d| d.message.as_str())
            .collect()
    }

    fn project_config(out_dir: &str) -> String {
        format!(
            r#"{{"compilerOptions":{{{OPTIONS},"outDir":"{out_dir}"}},"include":["src/**/*"]}}"#
        )
    }

    #[test]
    fn emitted_declarations_hold_only_the_public_contract() {
        let f = Fixture::new();
        f.write("tsconfig.json", &project_config("dist"));
        f.write("src/types.ts", "export type Label = string;\n");
        f.write(
            "src/Counter.vue",
            concat!(
                "<script setup lang=\"ts\">\n",
                "import { ref, type Ref } from 'vue';\n",
                "import type { Label } from './types';\n",
                "const count: Ref<number> = ref(0);\n",
                "const secret = count.value * 2;\n",
                "defineProps<{ label: Label }>();\n",
                "defineExpose({ count });\n",
                "</script>\n",
            ),
        );
        let outcome = declare(&f, BuildMode::Project, "tsconfig.json");
        let project = &outcome.projects[0];
        assert!(
            project.engine_failure.is_none(),
            "{:?}",
            project.engine_failure
        );
        assert!(project.diagnostics.is_empty(), "{:?}", project.diagnostics);
        let dist = f.dir.path().join("dist");
        let written = files_under(&dist);
        assert_eq!(written.len(), outcome.emitted_files.len(), "{written:?}");
        for file in &written {
            assert!(file.to_string_lossy().ends_with(".d.ts"), "{file:?}");
        }
        assert!(dist.join("src/types.d.ts").is_file(), "{written:?}");
        let counter = std::fs::read_to_string(dist.join("src/Counter.vue.d.ts"))
            .unwrap_or_else(|_| panic!("the carrier declaration is published: {written:?}"));
        assert!(counter.contains("export default"), "{counter}");
        assert!(counter.contains("./types"), "{counter}");
        let fixture = f.dir.path().to_string_lossy().replace('\\', "/");
        // The product's own public names (`__VerterExpose`, the public
        // component) are the declaration; the checking scaffolding is not.
        for private in [
            "secret",
            "ref(0)",
            "__VerterSetupScope",
            "___VERTER",
            fixture.as_str(),
        ] {
            assert!(!counter.contains(private), "{private}:\n{counter}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_output_directory_reached_through_a_link_still_protects_the_sources() {
        let f = Fixture::new();
        let component =
            "<script setup lang=\"ts\">\ndefineProps<{ label: string }>();\n</script>\n";
        f.write("real-src/Comp.vue", component);
        std::os::unix::fs::symlink(f.dir.path().join("real-src"), f.dir.path().join("src"))
            .unwrap();
        f.write(
            "tsconfig.json",
            &format!(
                r#"{{"compilerOptions":{{{OPTIONS},"outDir":"src"}},"files":["src/Comp.vue"]}}"#
            ),
        );
        let outcome = declare(&f, BuildMode::Project, "tsconfig.json");
        let refused = refusals(&outcome.projects[0]);
        assert_eq!(refused.len(), 1, "{refused:?}");
        assert!(
            refused[0].contains("holds the project's own sources"),
            "{refused:?}"
        );
        assert!(outcome.emitted_files.is_empty());
        assert_eq!(
            std::fs::read_to_string(f.dir.path().join("real-src/Comp.vue")).unwrap(),
            component
        );
    }

    #[test]
    fn a_carrier_declaration_names_the_carriers_it_imports_by_their_authored_path() {
        let f = Fixture::new();
        f.write("tsconfig.json", &project_config("dist"));
        f.write(
            "src/Child.vue",
            "<script setup lang=\"ts\">\nconst count: number = 1;\ndefineExpose({ count });\n</script>\n",
        );
        let root = f.dir.path().to_string_lossy().replace('\\', "/");
        // A literal type that spells an import is a value, not a specifier.
        let marker = format!("import(\"{root}/src/model\")");
        f.write(
            "src/Parent.vue",
            &format!(
                concat!(
                    "<script setup lang=\"ts\">\n",
                    "import Child from './Child.vue';\n",
                    "const read: (c: InstanceType<typeof Child>) => number = (c) => c.count;\n",
                    "defineProps<{{ marker: '{}' }}>();\n",
                    "defineExpose({{ read }});\n",
                    "</script>\n",
                ),
                marker
            ),
        );
        let outcome = declare(&f, BuildMode::Project, "tsconfig.json");
        let project = &outcome.projects[0];
        assert!(
            project.engine_failure.is_none(),
            "{:?}",
            project.engine_failure
        );
        assert!(project.diagnostics.is_empty(), "{:?}", project.diagnostics);
        let dist = f.dir.path().join("dist/src");
        let parent = std::fs::read_to_string(dist.join("Parent.vue.d.ts"))
            .unwrap_or_else(|_| panic!("{:?}", files_under(&f.dir.path().join("dist"))));
        assert!(dist.join("Child.vue.d.ts").is_file());
        // The stage read the child through its staged product; the published
        // declaration names it as authored, beside its own declaration.
        assert!(parent.contains("./Child.vue"), "{parent}");
        assert!(!parent.contains(".tsc"), "{parent}");
        assert!(parent.contains(&marker), "{parent}");
    }

    /// A plain TypeScript root reads a carrier's declaration product, whether
    /// it imports the carrier by a relative path or a `paths` alias, so a
    /// carrier type it infers is published as the carrier declares it, never
    /// as the `*.vue` wildcard module's `any`. Each root keeps its authored
    /// specifier, and a member reached both ways is one module.
    #[test]
    fn ts_root_declarations_infer_carrier_types_from_the_carrier_products() {
        let f = Fixture::new();
        f.write(
            "tsconfig.json",
            &format!(
                r#"{{"compilerOptions":{{{OPTIONS},"outDir":"dist","paths":{{"@/*":["./src/*"]}}}},"include":["src/**/*"]}}"#
            ),
        );
        f.write("src/status.ts", "export enum Status { On, Off }\n");
        f.write(
            "src/Child.vue",
            concat!(
                "<script setup lang=\"ts\">\n",
                "import { Status } from './status';\n",
                "const count: number = 1;\n",
                "const status: Status = Status.On;\n",
                "defineExpose({ count, status });\n",
                "</script>\n",
            ),
        );
        // A root that references a declaration file by path still reads the
        // carrier's product.
        f.write("src/ambient.d.ts", "declare const ambientLimit: number;\n");
        for (name, specifier, reference) in [
            (
                "relative",
                "./Child.vue",
                "/// <reference path=\"./ambient.d.ts\" />\n",
            ),
            ("alias", "@/Child.vue", ""),
        ] {
            f.write(
                &format!("src/{name}.ts"),
                &format!(
                    concat!(
                        "{}",
                        "import Child from '{}';\n",
                        // The carrier reads `./status`; the alias must reach
                        // the same enum, or the two are not assignable.
                        "import {{ Status }} from '@/status';\n",
                        "export const read = (c: InstanceType<typeof Child>) => c.count;\n",
                        "export const state = (c: InstanceType<typeof Child>): Status => c.status;\n",
                    ),
                    reference, specifier
                ),
            );
        }
        let outcome = declare(&f, BuildMode::Project, "tsconfig.json");
        let project = &outcome.projects[0];
        assert!(
            project.engine_failure.is_none(),
            "{:?}",
            project.engine_failure
        );
        assert!(project.diagnostics.is_empty(), "{:?}", project.diagnostics);
        let dist = f.dir.path().join("dist/src");
        assert!(
            dist.join("Child.vue.d.ts").is_file(),
            "{:?}",
            files_under(&f.dir.path().join("dist"))
        );
        for (name, specifier) in [("relative", "./Child.vue"), ("alias", "@/Child.vue")] {
            let root = std::fs::read_to_string(dist.join(format!("{name}.d.ts")))
                .unwrap_or_else(|_| panic!("{:?}", files_under(&f.dir.path().join("dist"))));
            assert!(root.contains(specifier), "{name}:\n{root}");
            assert!(root.contains("'@/status'"), "{name}:\n{root}");
            assert!(root.contains("=> number"), "{name}:\n{root}");
            assert!(!root.contains("any"), "{name}:\n{root}");
            assert!(!root.contains(".tmp"), "{name}:\n{root}");
        }
        // Nothing of the stage reaches the output.
        for file in files_under(&f.dir.path().join("dist")) {
            let name = file.to_string_lossy().replace('\\', "/");
            assert!(name.ends_with(".d.ts"), "{name}");
            assert!(!name.contains("/.tmp"), "{name}");
        }
    }

    /// A package installed beside a member, where the stage's mirror does not
    /// look, is still the one the member's declaration reads, as a value and
    /// as a type, and the published declaration names it as authored.
    #[test]
    fn a_package_beside_a_member_is_read_from_the_members_own_directory() {
        let f = Fixture::new();
        f.write("tsconfig.json", &project_config("dist"));
        f.write(
            "src/lib/node_modules/dep/package.json",
            r#"{"name":"dep","version":"1.0.0","types":"index.d.ts"}"#,
        );
        f.write(
            "src/lib/node_modules/dep/index.d.ts",
            "export interface Dep { n: number }\nexport declare const dep: Dep;\n",
        );
        f.write(
            "src/lib/api.ts",
            "import { dep, type Dep } from 'dep';\nexport const value: Dep = dep;\n",
        );
        let outcome = declare(&f, BuildMode::Project, "tsconfig.json");
        let project = &outcome.projects[0];
        assert!(
            project.engine_failure.is_none(),
            "{:?}",
            project.engine_failure
        );
        assert!(project.diagnostics.is_empty(), "{:?}", project.diagnostics);
        let api = std::fs::read_to_string(f.dir.path().join("dist/src/lib/api.d.ts"))
            .unwrap_or_else(|_| panic!("{:?}", files_under(&f.dir.path().join("dist"))));
        assert!(api.contains("'dep'"), "{api}");
        assert!(!api.contains("node_modules"), "{api}");
    }

    /// A file two projects share reads, in each project's declarations, the
    /// carrier that project's own `paths` alias names.
    #[test]
    fn a_shared_root_binds_aliases_in_the_project_it_is_published_for() {
        let f = Fixture::new();
        f.write(
            "tsconfig.json",
            r#"{"files":[],"references":[{"path":"a.json"},{"path":"b.json"}]}"#,
        );
        for (name, count) in [("a", "number = 1"), ("b", "string = 'one'")] {
            f.write(
                &format!("{name}.json"),
                &format!(
                    r#"{{"compilerOptions":{{{OPTIONS},"outDir":"dist-{name}","paths":{{"@child":["./{name}/Child.vue"]}}}},"include":["src/**/*","{name}/**/*"]}}"#
                ),
            );
            f.write(
                &format!("{name}/Child.vue"),
                &format!(
                    "<script setup lang=\"ts\">\nconst count: {count};\ndefineExpose({{ count }});\n</script>\n"
                ),
            );
        }
        f.write(
            "src/api.ts",
            "import Child from '@child';\nexport const read = (c: InstanceType<typeof Child>) => c.count;\n",
        );
        let outcome = declare(&f, BuildMode::Build, "tsconfig.json");
        for project in &outcome.projects {
            assert!(
                project.engine_failure.is_none(),
                "{:?}",
                project.engine_failure
            );
            assert!(project.diagnostics.is_empty(), "{:?}", project.diagnostics);
        }
        for (name, count) in [("a", "=> number"), ("b", "=> string")] {
            let api =
                std::fs::read_to_string(f.dir.path().join(format!("dist-{name}/src/api.d.ts")))
                    .unwrap_or_else(|_| panic!("{:?}", files_under(f.dir.path())));
            assert!(api.contains("'@child'"), "{name}:\n{api}");
            assert!(api.contains(count), "{name}:\n{api}");
        }
    }

    #[test]
    fn a_declaration_diagnostic_publishes_nothing_and_keeps_the_output() {
        let f = Fixture::new();
        // Inherited build metadata settings must not reach the output either.
        f.write(
            "tsconfig.json",
            &format!(
                r#"{{"compilerOptions":{{{OPTIONS},"outDir":"dist","incremental":true,"tsBuildInfoFile":"dist/state.tsbuildinfo"}},"include":["src/**/*"]}}"#
            ),
        );
        f.write("dist/keep.d.ts", "export {};\n");
        let dist = f.dir.path().join("dist");
        let previous = files_under(&dist);

        // The public surface names a binding the declaration cannot carry.
        f.write(
            "src/Status.vue",
            concat!(
                "<script setup lang=\"ts\">\n",
                "const status: number = 1;\n",
                "defineExpose({ read: () => status });\n",
                "</script>\n",
            ),
        );
        let outcome = declare(&f, BuildMode::Project, "tsconfig.json");
        let project = &outcome.projects[0];
        assert!(
            project.engine_failure.is_none(),
            "{:?}",
            project.engine_failure
        );
        // One rendered diagnostic for the one refusal.
        let refused = refusals(project);
        assert_eq!(refused.len(), 1, "{:?}", project.diagnostics);
        assert!(
            refused[0].contains("the public types name 'status'"),
            "{refused:?}"
        );
        assert!(project
            .diagnostics
            .iter()
            .any(|d| d.file.ends_with("Status.vue")));
        assert!(outcome.emitted_files.is_empty());
        assert_eq!(files_under(&dist), previous);
        assert_eq!(
            std::fs::read_to_string(dist.join("keep.d.ts")).unwrap(),
            "export {};\n"
        );

        // A publishable carrier beside a source the declaration stage
        // reports on.
        f.write(
            "src/Status.vue",
            "<script setup lang=\"ts\">\ndefineProps<{ label: string }>();\n</script>\n",
        );
        f.write("src/broken.ts", "export const value: number = 'text';\n");
        let outcome = declare(&f, BuildMode::Project, "tsconfig.json");
        let project = &outcome.projects[0];
        assert!(
            project.engine_failure.is_none(),
            "{:?}",
            project.engine_failure
        );
        assert!(
            project
                .diagnostics
                .iter()
                .any(|d| d.ts_code == 2322 && d.file.ends_with("broken.ts")),
            "{:?}",
            project.diagnostics
        );
        assert!(outcome.emitted_files.is_empty());
        assert_eq!(files_under(&dist), previous);
        assert!(
            std::fs::read_dir(f.dir.path()).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".verter-declarations")),
            "a discarded stage leaves no staging directory"
        );
    }

    #[test]
    fn each_project_needs_its_own_output_directory() {
        let f = Fixture::new();
        f.write(
            "tsconfig.json",
            r#"{"files":[],"references":[{"path":"a.json"},{"path":"b.json"},{"path":"c.json"},{"path":"d.json"},{"path":"e.json"}]}"#,
        );
        for (name, options) in [
            ("a", r#","outDir":"shared""#),
            // Nested in the first: publishing either would erase the other.
            ("b", r#","outDir":"shared/nested""#),
            ("c", ""),
            // Spelled through a directory that is not there; it names the
            // project directory itself.
            ("d", r#","outDir":"lib/..""#),
            // A project whose own options set noEmit publishes nothing, so
            // it shares no directory with anyone.
            ("e", r#","outDir":"shared","noEmit":true"#),
        ] {
            // Each project holds a carrier, so the driver checks and
            // publishes it; a carrier-free emitting project would belong to
            // the engine build.
            f.write(
                &format!("{name}.json"),
                &format!(
                    r#"{{"compilerOptions":{{{OPTIONS}{options}}},"files":["{name}.ts","{name}.vue"]}}"#
                ),
            );
            f.write(&format!("{name}.ts"), "export const value = 1;\n");
            f.write(
                &format!("{name}.vue"),
                "<script setup lang=\"ts\">const value = 1;</script>\n",
            );
        }
        let outcome = declare(&f, BuildMode::Build, "tsconfig.json");
        let reasons: Vec<(String, Vec<&str>)> = outcome
            .projects
            .iter()
            .filter(|project| !project.config.ends_with("tsconfig.json"))
            .map(|project| (project.config.to_string(), refusals(project)))
            .collect();
        assert_eq!(reasons.len(), 5, "{reasons:?}");
        for (config, refused) in &reasons {
            let expected = if config.ends_with("a.json") || config.ends_with("b.json") {
                "overlaps the output directory of"
            } else if config.ends_with("c.json") {
                "sets no outDir"
            } else if config.ends_with("d.json") {
                "holds the project's own sources"
            } else {
                assert!(refused.is_empty(), "{config}: {refused:?}");
                continue;
            };
            assert_eq!(refused.len(), 1, "{config}: {refused:?}");
            assert!(refused[0].contains(expected), "{config}: {refused:?}");
            assert!(!refused[0].contains("e.json"), "{config}: {refused:?}");
        }
        assert!(outcome.emitted_files.is_empty());
        assert!(!f.dir.path().join("shared").exists());
        assert!(!f.dir.path().join("a.d.ts").exists());
    }
}

mod watch_loop {
    //! What the watch loop prints after a change equals a fresh run's lines.

    use super::*;
    use crate::sfc_driver::watch::loop_harness::{found, fresh, Watching, CHANGE_LINE};

    #[test]
    fn watch_loop_a_carrier_edit_that_fixes_the_error_prints_no_error() {
        let f = Fixture::new();
        f.base();
        f.write(
            "tsconfig.json",
            r#"{"extends":"./base.json","include":["*.vue"]}"#,
        );
        f.write(
            "View.vue",
            r#"<script setup lang="ts">const value: string = 123;</script><template>{{ value }}</template>"#,
        );
        let request = f.request();
        let (broken, errors) = fresh(&request);
        assert_eq!(errors, 1, "{broken:?}");
        assert!(broken[0].contains("TS2322"), "{broken:?}");
        let mut watching = Watching::start(request.clone());
        watching.caught_up();

        f.write(
            "View.vue",
            r#"<script setup lang="ts">const value: string = "text";</script><template>{{ value }}</template>"#,
        );
        let view = f.dir.path().join("View.vue");
        watching.change(&PathBuf::from(verter_span::path::canonicalize_path(
            &f.fs.realpath(view.to_str().unwrap()).unwrap(),
        )));
        let (status, output, _) = watching.close(None);

        assert_eq!(
            output,
            [
                broken,
                vec![found(1), CHANGE_LINE.to_owned()],
                fresh(&request).0,
                vec![found(0)],
            ]
            .concat()
        );
        assert_eq!(status, ExitStatus::Clean);
    }
}

mod watch_session {
    //! A watch session's outcome after each change equals a fresh run's.

    use super::super::{Recheck, WatchSession};
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use verter_session::sfc_projection_diagnostics::ProjectionDiagnosticAttribution;

    type Reported = (
        Arc<str>,
        Vec<Arc<str>>,
        Vec<Arc<str>>,
        Vec<ProjectionDiagnosticAttribution>,
        String,
        Option<String>,
    );

    /// Everything a run reports, in its order.
    fn reported(outcome: &RunOutcome) -> (Vec<Reported>, String) {
        let projects = outcome
            .projects
            .iter()
            .map(|project| {
                (
                    project.config.clone(),
                    project.carriers.iter().map(|c| c.carrier.clone()).collect(),
                    project
                        .imported_carriers
                        .iter()
                        .map(|c| c.carrier.clone())
                        .collect(),
                    project.attributed.clone(),
                    format!("{:?}", project.diagnostics),
                    project.engine_failure.clone(),
                )
            })
            .collect();
        (projects, format!("{:?}", outcome.graph_problems))
    }

    fn codes(outcome: &RunOutcome) -> Vec<u32> {
        outcome
            .projects
            .iter()
            .flat_map(|project| project.attributed.iter().map(|d| d.code))
            .collect()
    }

    struct Watched {
        f: Fixture,
        session: WatchSession,
    }

    impl Watched {
        fn open(f: Fixture) -> Self {
            let (session, first) = WatchSession::open(&f.request()).unwrap();
            assert_eq!(reported(&first), reported(&f.run()));
            Self { f, session }
        }

        /// The graph's identity for `name`, also once it was deleted.
        fn path(&self, name: &str) -> PathBuf {
            let dir = self
                .f
                .fs
                .realpath(self.f.dir.path().to_str().unwrap())
                .unwrap();
            PathBuf::from(verter_span::path::canonicalize_path(
                &Path::new(&dir).join(name).to_string_lossy(),
            ))
        }

        /// Recheck after `names` changed on disk; the outcome must equal a
        /// fresh run's. Returns it.
        fn changed(&mut self, names: &[&str]) -> RunOutcome {
            let paths = names.iter().map(|name| self.path(name)).collect::<Vec<_>>();
            let recheck: Recheck = self.session.recheck_for(&paths);
            let watched = self.session.recheck(&recheck);
            let fresh = self.f.run();
            assert_eq!(reported(&watched), reported(&fresh), "after {names:?}");
            watched
        }

        fn delete(&self, name: &str) {
            self.f
                .fs
                .delete_file(self.f.dir.path().join(name).to_str().unwrap())
                .unwrap();
        }
    }

    const CLEAN: &str = r#"<script setup lang="ts">const value: string = "text";</script><template>{{ value }}</template>"#;
    const BROKEN: &str = r#"<script setup lang="ts">const value: string = 123;</script><template>{{ value }}</template>"#;

    #[test]
    fn watch_session_carrier_edit_and_undo_match_fresh_runs() {
        let f = Fixture::new();
        f.base();
        f.write(
            "tsconfig.json",
            r#"{"extends":"./base.json","include":["*.vue"]}"#,
        );
        f.write("View.vue", CLEAN);
        f.write("Other.vue", CLEAN);
        let mut w = Watched::open(f);
        w.f.write("View.vue", BROKEN);
        assert_eq!(codes(&w.changed(&["View.vue"])), vec![2322]);
        w.f.write("View.vue", CLEAN);
        assert!(codes(&w.changed(&["View.vue"])).is_empty());
    }

    #[test]
    fn watch_session_added_and_deleted_carriers_match_fresh_runs() {
        let f = Fixture::new();
        f.base();
        f.write(
            "tsconfig.json",
            r#"{"extends":"./base.json","include":["*.vue"]}"#,
        );
        f.write("View.vue", CLEAN);
        let mut w = Watched::open(f);
        w.f.write("Added.vue", BROKEN);
        let added = w.changed(&["Added.vue"]);
        assert_eq!(added.carriers_checked, 2);
        assert_eq!(codes(&added), vec![2322]);
        w.delete("Added.vue");
        let deleted = w.changed(&["Added.vue"]);
        assert_eq!(deleted.carriers_checked, 1);
        assert!(codes(&deleted).is_empty());
    }

    #[test]
    fn watch_session_external_declaration_edits_match_fresh_runs() {
        let f = Fixture::new();
        f.base();
        f.write(
            "tsconfig.json",
            r#"{"extends":"./base.json","include":["*.vue","*.d.ts"]}"#,
        );
        f.write("globals.d.ts", "declare const injected: string;\n");
        f.write(
            "View.vue",
            r#"<script setup lang="ts">const value: string = injected; const other: number = extra;</script><template>{{ value }}{{ other }}</template>"#,
        );
        let mut w = Watched::open(f);
        let sorted = |outcome: RunOutcome| {
            let mut codes = codes(&outcome);
            codes.sort_unstable();
            codes
        };
        w.f.write("extra.d.ts", "declare const extra: number;\n");
        assert!(sorted(w.changed(&["extra.d.ts"])).is_empty());
        w.f.write("globals.d.ts", "declare const injected: number;\n");
        assert_eq!(sorted(w.changed(&["globals.d.ts"])), vec![2322]);
        w.delete("extra.d.ts");
        assert_eq!(sorted(w.changed(&["extra.d.ts"])), vec![2304, 2322]);
    }

    #[test]
    fn watch_session_config_option_and_new_reference_match_fresh_runs() {
        let f = Fixture::new();
        f.base();
        f.write(
            "tsconfig.json",
            r#"{"files":[],"references":[{"path":"./first.json"}]}"#,
        );
        f.write(
            "first.json",
            r#"{"extends":"./base.json","include":["first/*.vue"],"compilerOptions":{"strictNullChecks":false}}"#,
        );
        f.write(
            "first/View.vue",
            r#"<script setup lang="ts">const value: string = null;</script><template>{{ value }}</template>"#,
        );
        let mut w = Watched::open(f);
        w.f.write(
            "first.json",
            r#"{"extends":"./base.json","include":["first/*.vue"]}"#,
        );
        assert_eq!(codes(&w.changed(&["first.json"])), vec![2322]);
        w.f.write(
            "second.json",
            r#"{"extends":"./base.json","include":["second/*.vue"]}"#,
        );
        w.f.write("second/Other.vue", BROKEN);
        w.f.write(
            "tsconfig.json",
            r#"{"files":[],"references":[{"path":"./first.json"},{"path":"./second.json"}]}"#,
        );
        let referenced = w.changed(&["second.json", "second/Other.vue", "tsconfig.json"]);
        assert_eq!(referenced.projects.len(), 3);
        assert_eq!(codes(&referenced), vec![2322, 2322]);
    }

    const IMPORTS: &str = r#"<script setup lang="ts">import { injected } from "../shared/types"; const value: string = injected;</script><template>{{ value }}</template>"#;

    /// Sources a program imports from outside its project's root and
    /// membership are found through what the engine read, and stay found
    /// after incremental checks that read only the changed carrier.
    #[test]
    fn watch_session_sources_imported_from_outside_the_project_match_fresh_runs() {
        let f = Fixture::new();
        f.base();
        f.write(
            "tsconfig.json",
            r#"{"files":[],"references":[{"path":"./app"}]}"#,
        );
        f.write(
            "app/tsconfig.json",
            r#"{"extends":"../base.json","include":["*.vue"]}"#,
        );
        f.write(
            "shared/types.d.ts",
            "export declare const injected: string;\n",
        );
        f.write("app/View.vue", IMPORTS);
        let mut w = Watched::open(f);
        let shared = w.path("shared/types.d.ts");
        assert!(w.session.watched_paths().reports(&shared));
        // The engine reads only the edited carrier again.
        w.f.write(
            "app/View.vue",
            &IMPORTS.replace("{{ value }}", "{{ value }}!"),
        );
        assert!(codes(&w.changed(&["app/View.vue"])).is_empty());
        w.f.write(
            "shared/types.d.ts",
            "export declare const injected: number;\n",
        );
        assert_eq!(codes(&w.changed(&["shared/types.d.ts"])), vec![2322]);

        // Without the import, the engine still holds the file's text and is
        // told when it changes, so restoring the import sees the new text.
        w.f.write("app/View.vue", CLEAN);
        assert!(codes(&w.changed(&["app/View.vue"])).is_empty());
        w.f.write(
            "shared/types.d.ts",
            "export declare const injected: string;\n",
        );
        assert!(codes(&w.changed(&["shared/types.d.ts"])).is_empty());
        w.f.write("app/View.vue", IMPORTS);
        assert!(codes(&w.changed(&["app/View.vue"])).is_empty());
    }

    /// A carrier reached only by import from another project is watched for
    /// that project's program too, and an edit to it is checked there under
    /// that project's options.
    #[test]
    fn watch_session_carrier_imported_from_outside_the_membership_matches_fresh_runs() {
        let f = Fixture::new();
        sibling_projects(&f);
        f.write("a/C.vue", CLEAN);
        let mut w = Watched::open(f);
        w.f.write(
            "a/C.vue",
            r#"<script setup lang="ts">const value: number = null;</script><template>{{ value }}</template>"#,
        );
        assert_eq!(codes(&w.changed(&["a/C.vue"])), vec![2322]);
        w.f.write("a/C.vue", CLEAN);
        assert!(codes(&w.changed(&["a/C.vue"])).is_empty());
    }

    /// Once a project stops importing a carrier outside its membership, it
    /// no longer serves it or reports the carrier's own diagnostics, as a
    /// fresh run does not; restoring the import reports them again.
    #[test]
    fn watch_session_removed_import_of_an_outside_carrier_matches_fresh_runs() {
        const IMPORTING: &str = "import \"../a/C.vue\";\n\nexport const component = 1;\n";
        let f = Fixture::new();
        sibling_projects(&f);
        f.write("b/x.ts", IMPORTING);
        // Script blocks of different languages: the carrier's own diagnostic,
        // reported by every project that checks it.
        f.write(
            "a/C.vue",
            concat!(
                "<script lang=\"ts\">export const a = 1</script>\n",
                "<script setup lang=\"tsx\">const b = 2</script>\n",
            ),
        );
        let carrier_lines = |outcome: &RunOutcome| {
            outcome_diagnostics(outcome)
                .into_iter()
                .filter(|line| line.file.ends_with("a/C.vue"))
                .count()
        };
        let mut w = Watched::open(f);
        let importing = w.f.run();
        assert_eq!(
            project_named(&importing, "b/tsconfig.json")
                .imported_carriers
                .len(),
            1
        );

        w.f.write("b/x.ts", "export const component = 1;\n");
        let removed = w.changed(&["b/x.ts"]);
        assert!(project_named(&removed, "b/tsconfig.json")
            .imported_carriers
            .is_empty());
        let fresh = w.f.run();
        assert_eq!(carrier_lines(&removed), carrier_lines(&fresh));
        assert!(carrier_lines(&removed) < carrier_lines(&importing));
        assert_eq!(exit_status(&removed), exit_status(&fresh));

        w.f.write("b/x.ts", IMPORTING);
        let restored = w.changed(&["b/x.ts"]);
        assert_eq!(carrier_lines(&restored), carrier_lines(&importing));
    }

    /// A module created where the program's resolution probed is a change it
    /// read, as `tsc --watch` treats a failed lookup.
    #[test]
    fn watch_session_module_created_at_a_probed_path_matches_fresh_runs() {
        let f = Fixture::new();
        f.base();
        f.write(
            "tsconfig.json",
            r#"{"files":[],"references":[{"path":"./app"}]}"#,
        );
        f.write(
            "app/tsconfig.json",
            r#"{"extends":"../base.json","include":["*.vue"]}"#,
        );
        f.write(
            "shared/types.d.ts",
            "export declare const injected: string;\n",
        );
        f.write(
            "app/View.vue",
            &IMPORTS.replace("../shared/types", "../shared/later"),
        );
        let mut w = Watched::open(f);
        assert_eq!(codes(&w.f.run()), vec![2307]);
        w.f.write(
            "shared/later.d.ts",
            "export declare const injected: number;\n",
        );
        assert_eq!(codes(&w.changed(&["shared/later.d.ts"])), vec![2322]);
    }
}

#[test]
fn a_same_key_branch_chain_does_not_hide_another_carriers_error() {
    let f = Fixture::new();
    f.base();
    f.write(
        "tsconfig.json",
        r#"{"extends":"./base.json","include":["*.vue"]}"#,
    );
    // The same `:key` on both branches of a `v-if` / `v-else-if` chain with a
    // comment between them.
    f.write(
        "ItemDetail.vue",
        r#"<script setup lang="ts">
import MainRedDetail from './MainRedDetail.vue';
import QuestionDetail from './QuestionDetail.vue';
defineProps<{ tenant: string; item: { kind: string; id: string } }>();
</script>

<template>
  <article>
    <QuestionDetail v-if="item.kind === 'question'" :key="`${tenant}:${item.id}`" />

    <!-- A project its red main holds: why its starts wait. -->
    <MainRedDetail v-else-if="item.kind === 'main-red'" :key="`${tenant}:${item.id}`" />
  </article>
</template>
"#,
    );
    f.write("MainRedDetail.vue", "<template><p /></template>");
    f.write("QuestionDetail.vue", "<template><p /></template>");
    f.write(
        "Seeded.vue",
        r#"<script setup lang="ts">const seeded: string = 123;</script><template>{{ seeded }}</template>"#,
    );
    let outcome = f.run();
    assert_eq!(outcome.carriers_checked, 4);
    let diagnostics = outcome_diagnostics(&outcome);
    let seeded = diagnostics
        .iter()
        .filter(|d| d.file.ends_with("/Seeded.vue"))
        .collect::<Vec<_>>();
    assert_eq!(seeded.len(), 1, "{diagnostics:?}");
    assert_eq!((seeded[0].ts_code, seeded[0].line), (2322, 1));
    assert_ne!(exit_status(&outcome), ExitStatus::Clean);
}

#[test]
fn type_declared_events_reach_the_public_emit_and_listener_props() {
    let f = Fixture::new();
    f.base();
    f.write(
        "tsconfig.json",
        r#"{"extends":"./base.json","include":["*.vue"]}"#,
    );
    f.write(
        "Child.vue",
        r#"<script setup lang="ts">
defineEmits<{ (e: 'title', value: string): void; (e: 'close'): void }>();
</script>
<template><p /></template>
"#,
    );
    f.write(
        "Short.vue",
        r#"<script setup lang="ts">
interface Parameters { id: number }
defineEmits<{ save: [value: Parameters] }>();
</script>
<template><p /></template>
"#,
    );
    f.write(
        "Parent.vue",
        r#"<script setup lang="ts">
import Child from './Child.vue';
import Short from './Short.vue';
const child = null! as InstanceType<typeof Child>;
const short = null! as InstanceType<typeof Short>;
child.$emit('title', 'ok');
child.$emit('close');
short.$emit('save', { id: 2 });
const onTitle: NonNullable<InstanceType<typeof Child>['$props']['onTitle']> = (value) => value.toUpperCase();
const onSave: NonNullable<InstanceType<typeof Short>['$props']['onSave']> = (value) => value.id.toFixed();
child.$emit('title', 1);
short.$emit('save', 'x');
</script>
<template><p>{{ onTitle }}{{ onSave }}</p></template>
"#,
    );
    let outcome = f.run();
    assert_eq!(outcome.carriers_checked, 3);
    // vue-tsc reports exactly the two wrong payloads; the declared names and
    // payloads type every other emit and listener, including a payload type
    // that shares its name with a global utility type. Verter's own notes about
    // what it checked are not engine rows.
    let diagnostics = outcome_diagnostics(&outcome);
    let rows: Vec<(String, u32, u32)> = diagnostics
        .iter()
        .filter(|d| d.verter_code.is_none())
        .map(|d| {
            let file = Path::new(&d.file)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            (file, d.ts_code, d.line)
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            ("Parent.vue".to_owned(), 2345, 11),
            ("Parent.vue".to_owned(), 2345, 12)
        ],
        "{diagnostics:?}"
    );
}

/// One of the `diagnostics` fixture's own carriers, as authored.
fn diagnostics_fixture_carrier(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/cases/fixtures/diagnostics/src")
            .join(name),
    )
    .expect("the diagnostics fixture carrier")
}

/// A project of `diagnostics` fixture carriers under that fixture's own
/// unused-name options.
fn diagnostics_fixture_project(names: &[&str]) -> Fixture {
    let f = Fixture::new();
    f.base();
    f.write(
        "tsconfig.json",
        r#"{"extends":"./base.json","include":["*.vue"],"compilerOptions":{"noUnusedLocals":true,"noUnusedParameters":true}}"#,
    );
    for name in names {
        f.write(name, &diagnostics_fixture_carrier(name));
    }
    f
}

#[test]
fn generated_template_helpers_are_never_reported_as_unused() {
    // One carrier emits the attribute-payload contracts without normalizing
    // props; the other has an inline handler that never reads `$event`.
    let f = diagnostics_fixture_project(&["BaseButton.vue", "GenericList.vue"]);
    let outcome = f.run();
    assert_eq!(outcome.carriers_checked, 2);
    let diagnostics = outcome_diagnostics(&outcome);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn custom_directives_are_checked_without_a_verter_types_package() {
    // The project, like a user's, has no `@verter/types` to resolve.
    let f = diagnostics_fixture_project(&["DirectiveErrors.vue"]);
    let outcome = f.run();
    assert_eq!(outcome.carriers_checked, 1);
    let diagnostics = outcome_diagnostics(&outcome);
    assert!(
        diagnostics
            .iter()
            .all(|d| d.verter_code.is_none() && d.file.ends_with("/DirectiveErrors.vue")),
        "{diagnostics:?}"
    );
    let mut positions = diagnostics
        .iter()
        .map(|d| (d.line, d.col))
        .collect::<Vec<_>>();
    positions.sort_unstable();
    // vue-tsc's positions: an unknown modifier, a wrong value, an unknown
    // argument, and two unknown modifiers after an argument.
    assert_eq!(
        positions,
        [(13, 17), (14, 23), (18, 17), (19, 20), (20, 20)],
        "{diagnostics:?}"
    );
}

#[test]
fn template_diagnostics_print_where_vue_tsc_reports_them() {
    let f = Fixture::new();
    f.base();
    f.write(
        "tsconfig.json",
        r#"{"extends":"./base.json","include":["*.vue"]}"#,
    );
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cases/fixtures");
    for (dir, name) in [
        ("diagnostics/src", "TemplateExprErrors.vue"),
        (
            "fallthrough/src",
            "RejectDeclaredPropWrongTypeOnOptionsApiChild.vue",
        ),
        ("fallthrough/src", "OptionsApiAnchorRoot.vue"),
    ] {
        let text =
            f.fs.read_file(fixtures.join(dir).join(name).to_str().unwrap())
                .expect("the fixture is checked in");
        f.write(name, &text);
    }
    let outcome = f.run();
    let rows = outcome_diagnostics(&outcome)
        .into_iter()
        .filter(|d| d.verter_code.is_none())
        .map(|d| {
            let file = d.file.rsplit('/').next().unwrap_or_default().to_string();
            (file, d.line, d.col, d.ts_code)
        })
        .collect::<Vec<_>>();
    // vue-tsc's rows for the same fixtures: each template diagnostic at the
    // authored bytes it is about, a context read at its name, and a prop
    // that fails the child's contract at its attribute.
    let row = |file: &str, line: u32, col: u32, code: u32| (file.to_string(), line, col, code);
    assert_eq!(
        rows,
        [
            row(
                "RejectDeclaredPropWrongTypeOnOptionsApiChild.vue",
                4,
                33,
                2769
            ),
            row("TemplateExprErrors.vue", 7, 11, 2339),
            row("TemplateExprErrors.vue", 9, 17, 2339),
            row("TemplateExprErrors.vue", 11, 25, 2345),
            row("TemplateExprErrors.vue", 13, 11, 2362),
        ],
        "{:?}",
        outcome_diagnostics(&outcome)
    );
}

/// The `js_carrier` fixture's JavaScript SFCs and the TypeScript parent that
/// calls one of them, checked by the driver with or without `checkJs`. Rows
/// are `(file, line, column, code)`, sorted.
fn js_carrier_rows(check_js: bool) -> (RunOutcome, Vec<(String, u32, u32, u32)>) {
    let f = Fixture::new();
    let check_js = if check_js { r#","checkJs":true"# } else { "" };
    f.write(
        "tsconfig.json",
        &format!(
            r#"{{"compilerOptions":{{"strict":true,"target":"ES2020","module":"ESNext","moduleResolution":"bundler","jsx":"preserve","skipLibCheck":true,"types":[]{check_js}}},"include":["*.vue"]}}"#
        ),
    );
    for (name, text) in [
        (
            "JsDoc.vue",
            include_str!("../../tests/cases/fixtures/js_carrier/src/JsDoc.vue"),
        ),
        (
            "JsExpose.vue",
            include_str!("../../tests/cases/fixtures/js_carrier/src/JsExpose.vue"),
        ),
        (
            "JsOptions.vue",
            include_str!("../../tests/cases/fixtures/js_carrier/src/JsOptions.vue"),
        ),
        (
            "JsSetup.vue",
            include_str!("../../tests/cases/fixtures/js_carrier/src/JsSetup.vue"),
        ),
        (
            "JsxOptions.vue",
            include_str!("../../tests/cases/fixtures/js_carrier/src/JsxOptions.vue"),
        ),
        (
            "TsParentArity.vue",
            include_str!("../../tests/cases/fixtures/js_carrier/src/TsParentArity.vue"),
        ),
    ] {
        f.write(name, text);
    }
    let outcome = f.run();
    assert_eq!(outcome.carriers_checked, 6);
    let mut rows: Vec<(String, u32, u32, u32)> = outcome_diagnostics(&outcome)
        .into_iter()
        .filter(|d| d.verter_code.is_none())
        .map(|d| {
            let file = d
                .file
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or_default()
                .to_string();
            (file, d.line, d.col, d.ts_code)
        })
        .collect();
    rows.sort();
    (outcome, rows)
}

fn sorted_rows(rows: &[(&str, u32, u32, u32)]) -> Vec<(String, u32, u32, u32)> {
    let mut rows: Vec<_> = rows
        .iter()
        .map(|&(file, line, col, code)| (file.to_string(), line, col, code))
        .collect();
    rows.sort();
    rows
}

/// Under `checkJs` every row is one vue-tsc reports on the same files:
/// JavaScript carriers are checked as JavaScript, so an unannotated
/// parameter is an implicit `any`, a JSDoc `@param` types its call, Options
/// API `this` reads `data` and `props`, and an untyped parameter is optional
/// to its caller.
#[test]
fn javascript_carriers_are_checked_as_javascript_under_check_js() {
    let (outcome, rows) = js_carrier_rows(true);
    assert_eq!(
        rows,
        sorted_rows(&[
            ("JsDoc.vue", 7, 22, 2345),
            ("JsExpose.vue", 6, 15, 7006),
            ("JsExpose.vue", 15, 9, 7006),
            ("JsExpose.vue", 23, 28, 7006),
            ("JsExpose.vue", 29, 10, 7006),
            ("JsOptions.vue", 10, 10, 7006),
            ("JsOptions.vue", 13, 14, 7006),
            ("JsSetup.vue", 10, 15, 7006),
            ("JsSetup.vue", 15, 19, 7006),
            ("JsxOptions.vue", 7, 10, 7006),
            ("JsxOptions.vue", 11, 14, 7026),
            ("JsxOptions.vue", 11, 46, 7026),
            ("TsParentArity.vue", 9, 23, 2554),
        ])
    );
    // Each JavaScript carrier has a JavaScript checking module whose
    // template is checked too.
    use verter_compiler::framework_common::vue_projection_backend::{
        CheckingDialect, ObligationFamily,
    };
    for carrier in &outcome.projects[0].carriers {
        if carrier.carrier.ends_with("TsParentArity.vue") {
            continue;
        }
        let module = carrier.result.module.as_ref().expect("a checking module");
        assert!(
            matches!(module.dialect, CheckingDialect::Js | CheckingDialect::Jsx),
            "{}",
            carrier.carrier
        );
        assert!(
            !module
                .withheld
                .iter()
                .any(|product| product.family == ObligationFamily::Scaffold),
            "{}: {:?}",
            carrier.carrier,
            module.withheld
        );
    }
}

/// Without `checkJs` a JavaScript carrier reports nothing, and, as for
/// vue-tsc, a TypeScript importer cannot type a JavaScript module the project
/// does not allow.
#[test]
fn javascript_carriers_report_nothing_without_check_js() {
    let (_, rows) = js_carrier_rows(false);
    assert_eq!(rows, sorted_rows(&[("TsParentArity.vue", 2, 22, 7016)]));
}

/// A JavaScript carrier's `@ts-check` turns checking on and its
/// `@ts-nocheck` turns it off, in both script blocks, as vue-tsc reports them.
#[test]
fn javascript_carrier_pragmas_decide_checking_as_vue_tsc_does() {
    for check_js in [false, true] {
        let f = Fixture::new();
        let check_js_option = if check_js { r#","checkJs":true"# } else { "" };
        f.write(
            "tsconfig.json",
            &format!(
                r#"{{"compilerOptions":{{"strict":true,"target":"ES2020","module":"ESNext","moduleResolution":"bundler","jsx":"preserve","skipLibCheck":true,"types":[],"allowJs":true{check_js_option}}},"include":["*.vue"]}}"#
            ),
        );
        f.write("CheckNormal.vue", "<script>\n// @ts-check\nexport default { methods: { f(x) { return x } } }\n</script>\n<template><div /></template>\n");
        f.write("CheckSetup.vue", "<script setup>\n// @ts-check\nfunction f(x) { return x }\nf(1)\n</script>\n<template><div /></template>\n");
        f.write("NoCheckSetup.vue", "<script setup>\n// @ts-nocheck\nconst n = 1\nn.toUpperCase()\n</script>\n<template><div /></template>\n");
        f.write("NoCheckNormal.vue", "<script>\n// @ts-nocheck\nexport default { methods: { f(x) { return x.y.z } } }\n</script>\n<template><div /></template>\n");
        let outcome = f.run();
        let mut rows: Vec<(String, u32, u32, u32)> = outcome_diagnostics(&outcome)
            .into_iter()
            .filter(|d| d.verter_code.is_none())
            .map(|d| {
                let file = d
                    .file
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or_default()
                    .to_string();
                (file, d.line, d.col, d.ts_code)
            })
            .collect();
        rows.sort();
        assert_eq!(
            rows,
            sorted_rows(&[
                ("CheckNormal.vue", 3, 31, 7006),
                ("CheckSetup.vue", 3, 12, 7006)
            ]),
            "checkJs: {check_js}"
        );
    }
}

/// Writes each carrier beside a `tsconfig.json` that includes them, runs the
/// driver and returns the errors it reports.
fn carrier_errors(carriers: &[(&str, &str)]) -> Vec<crate::reporter::Diagnostic> {
    let f = Fixture::new();
    f.base();
    f.write(
        "tsconfig.json",
        r#"{"extends":"./base.json","include":["*.vue"]}"#,
    );
    for (name, text) in carriers {
        f.write(name, text);
    }
    let outcome = f.run();
    assert_eq!(outcome.carriers_checked, carriers.len());
    outcome_diagnostics(&outcome)
        .into_iter()
        .filter(|d| d.severity == crate::reporter::Severity::Error)
        .collect()
}

#[test]
fn loop_sources_read_setup_bindings_inside_callbacks_and_spreads() {
    let errors = carrier_errors(&[(
        "LoopSourceReads.vue",
        r#"<script setup lang="ts">
import { computed, ref } from 'vue';
const list = ref([{ id: 1 }, { id: 2 }]);
const selected = ref(1);
const a = ref([1, 2]);
const b = computed(() => [3]);
const limit = computed(() => 2);
</script>

<template>
  <ul>
    <li v-for="x in list.filter(i => i.id === selected)" :key="x.id">{{ x.id }}</li>
    <li v-for="n in [...a, ...b]" :key="n">{{ n }}</li>
    <li v-for="e in [{ a }]" :key="e.a.length">{{ e.a[0] }}</li>
    <li v-for="m in a.filter(limit => limit > 0).map(v => v < limit)" :key="String(m)">{{ m }}</li>
    <li v-for="row in [a]" :key="row.length">
      <b v-for="c in row.filter(x => x < limit)" :key="c">{{ c }}</b>
    </li>
  </ul>
</template>
"#,
    )]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn a_cast_model_target_is_written_as_an_assignable_target() {
    let errors = carrier_errors(&[
        (
            "ValueInput.vue",
            r#"<script setup lang="ts">
defineModel<string | string[]>();
</script>

<template><input /></template>
"#,
        ),
        (
            "CastModel.vue",
            r#"<script setup lang="ts">
import { reactive } from 'vue';
import ValueInput from './ValueInput.vue';
const filter = reactive<{ input: string | string[] | number }>({ input: '' });
</script>

<template>
  <ValueInput v-model="filter.input as string | string[]" />
</template>
"#,
        ),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
}
