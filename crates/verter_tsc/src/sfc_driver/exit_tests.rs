use std::path::{Path, PathBuf};
use std::sync::Arc;

use verter_compiler::framework_common::vue_projection_backend::{WithheldProduct, WithheldReason};
use verter_session::sfc_projection_diagnostics::{
    DiagnosticAnchor, DiagnosticCategory, DiagnosticDeduplicationKey,
    ProjectionDiagnosticAttribution,
};
use verter_session::sfc_projection_mapper::{
    ContentMapperProjectionAdapter, MapperDiagnostic, MapperDiagnosticCode, MapperProjectHandle,
    MapperProjectSettings,
};
use verter_workspace::native_fs::NativeFs;
use verter_workspace::{FilesystemOptions, FilesystemWorkspace};

use super::*;
use crate::position::ColumnEncoding;
use crate::sfc_driver::per_file::{project_carrier, CarrierProjection};

const CLEAN: &str = "<script setup lang=\"ts\">\nconst ok = 1;\n</script>\n";

struct Workspace {
    dir: tempfile::TempDir,
    fs: NativeFs,
    adapter: ContentMapperProjectionAdapter,
    handle: MapperProjectHandle,
}

impl Workspace {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("tsconfig.json");
        let handle = MapperProjectHandle::of_config(config.to_str().unwrap());
        let adapter = ContentMapperProjectionAdapter::new();
        adapter
            .open_project(
                handle.clone(),
                MapperProjectSettings::new(handle.0.clone(), vec![], vec![], vec![]),
            )
            .unwrap();
        Self {
            dir,
            fs: NativeFs::new(),
            adapter,
            handle,
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn write(&self, name: &str, text: &str) {
        self.fs
            .write_file(self.path(name).to_str().unwrap(), text)
            .unwrap();
    }

    /// A carrier projected by the real mapper from the file written here.
    fn carrier(&self, name: &str, text: &str) -> CarrierProjection {
        self.write(name, text);
        self.project(name)
    }

    fn project(&self, name: &str) -> CarrierProjection {
        let workspace = FilesystemWorkspace::new(FilesystemOptions::default());
        project_carrier(&self.adapter, &self.handle, &self.path(name), &workspace)
    }

    /// A projected carrier whose result carries `diagnostics` instead.
    fn with_diagnostics(
        &self,
        name: &str,
        diagnostics: Vec<MapperDiagnostic>,
    ) -> CarrierProjection {
        let mut carrier = self.carrier(name, CLEAN);
        let mut result = (*carrier.result).clone();
        result.diagnostics = diagnostics;
        carrier.result = Arc::new(result);
        carrier
    }
}

fn mapper(code: MapperDiagnosticCode, message: &str) -> MapperDiagnostic {
    MapperDiagnostic {
        start: 0,
        end: 1,
        code,
        message: message.to_string(),
    }
}

fn engine(
    file: &Path,
    start: u32,
    code: u32,
    category: DiagnosticCategory,
) -> ProjectionDiagnosticAttribution {
    let carrier: Arc<str> = Arc::from(verter_span::path::canonicalize_path(
        &file.to_string_lossy(),
    ));
    ProjectionDiagnosticAttribution {
        carrier: carrier.clone(),
        anchor: DiagnosticAnchor::Authored {
            start,
            end: start + 1,
        },
        family: None,
        host: None,
        code,
        category,
        message: format!("engine {code}"),
        related: vec![],
        dedup: DiagnosticDeduplicationKey {
            carrier,
            start,
            end: start + 1,
            code,
            message_digest: [0; 16],
        },
    }
}

fn project(config: &Path) -> ProjectCheck {
    ProjectCheck {
        config: Arc::from(config.to_string_lossy().replace('\\', "/")),
        carriers: vec![],
        unowned_carriers: vec![],
        foreign_surfaces: vec![],
        diagnostics: vec![],
        binding: None,
        raw: vec![],
        attributed: vec![],
        engine_failure: None,
    }
}

fn outcome(projects: Vec<ProjectCheck>) -> RunOutcome {
    RunOutcome {
        carriers_checked: projects.iter().map(|p| p.carriers.len()).sum(),
        projects,
        graph_problems: vec![],
        emitted_files: vec![],
    }
}

fn rendered(outcome: &RunOutcome) -> Vec<String> {
    let mut out = Vec::new();
    render_outcome(
        outcome,
        &outcome_renderer(outcome, ColumnEncoding::Utf16),
        &mut out,
    )
    .unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn exit_status_follows_the_worst_outcome() {
    let ws = Workspace::new();
    let config = ws.path("tsconfig.json");
    let clean = || {
        let mut p = project(&config);
        p.carriers.push(ws.carrier("Clean.vue", CLEAN));
        p
    };
    let with = |edit: &dyn Fn(&mut ProjectCheck)| {
        let mut p = clean();
        edit(&mut p);
        exit_status(&outcome(vec![p]))
    };
    let warning = |p: &mut ProjectCheck| {
        let mut d = Diagnostic::verter(
            "View.vue".into(),
            1,
            1,
            VerterCode::CarrierPathConflict,
            "skipped".into(),
        );
        d.severity = Severity::Warning;
        p.diagnostics.push(d);
        p.attributed.push(engine(
            &ws.path("Clean.vue"),
            0,
            6133,
            DiagnosticCategory::Warning,
        ));
    };
    type Case<'a> = (&'a str, &'a dyn Fn(&mut ProjectCheck), ExitStatus);
    let cases: [Case<'_>; 7] = [
        ("clean", &|_: &mut ProjectCheck| {}, ExitStatus::Clean),
        ("warnings only", &warning, ExitStatus::Clean),
        (
            "an engine error",
            &|p: &mut ProjectCheck| {
                p.attributed.push(engine(
                    &ws.path("Clean.vue"),
                    0,
                    2322,
                    DiagnosticCategory::Error,
                ))
            },
            ExitStatus::Diagnostics,
        ),
        (
            "a refused carrier",
            &|p: &mut ProjectCheck| {
                p.carriers.push(ws.with_diagnostics(
                    "Refused.vue",
                    vec![mapper(MapperDiagnosticCode::ProjectionRefused, "refused")],
                ))
            },
            ExitStatus::Diagnostics,
        ),
        (
            "a projection-internal failure",
            &|p: &mut ProjectCheck| {
                p.carriers.push(ws.with_diagnostics(
                    "Internal.vue",
                    vec![mapper(MapperDiagnosticCode::ProjectionInternal, "panic")],
                ))
            },
            ExitStatus::Failed,
        ),
        (
            "an unreadable carrier",
            &|p: &mut ProjectCheck| p.carriers.push(ws.project("Missing.vue")),
            ExitStatus::Failed,
        ),
        (
            "an engine failure",
            &|p: &mut ProjectCheck| p.engine_failure = Some("engine died".into()),
            ExitStatus::Failed,
        ),
    ];
    for (name, edit, expected) in cases {
        assert_eq!(with(edit), expected, "{name}");
    }
    let mut graph = outcome(vec![clean()]);
    graph
        .graph_problems
        .push(ProjectGraphError::MissingReference {
            from: config.clone(),
            to: ws.path("missing.json"),
        });
    assert_eq!(exit_status(&graph), ExitStatus::Failed, "a graph problem");
}

#[test]
fn a_failed_composition_fails_the_run_and_every_other_carrier_still_prints() {
    let ws = Workspace::new();
    let config = ws.path("tsconfig.json");
    let mut first = project(&config);
    first.carriers.push(ws.with_diagnostics(
        "Broken.vue",
        vec![mapper(
            MapperDiagnosticCode::ProjectionInternal,
            "the projection of this carrier failed: boom",
        )],
    ));
    first.carriers.push(ws.carrier("Good.vue", CLEAN));
    first.attributed.push(engine(
        &ws.path("Good.vue"),
        CLEAN.find("ok").unwrap() as u32,
        2322,
        DiagnosticCategory::Error,
    ));
    let mut second = project(&ws.path("other.json"));
    second.engine_failure = Some("Cannot ensure project: engine died".into());
    let run = outcome(vec![first, second]);
    // Printing positions in the checked source, not in what is on disk now.
    ws.fs
        .delete_file(ws.path("Good.vue").to_str().unwrap())
        .unwrap();
    assert_eq!(exit_status(&run), ExitStatus::Failed);
    let lines = rendered(&run);
    let broken = canonical(&ws.path("Broken.vue"));
    let good = canonical(&ws.path("Good.vue"));
    assert_eq!(
        lines,
        [
            format!("{broken}(1,1): error VTER1006: the projection of this carrier failed: boom"),
            format!("{good}(2,7): error TS2322: engine 2322"),
            format!(
                "{}: error VTER1008: Cannot ensure project: engine died",
                run.projects[1].config
            ),
        ]
    );
}

#[test]
fn outcome_diagnostics_are_graph_problems_then_projects_in_order() {
    let ws = Workspace::new();
    let config = ws.path("tsconfig.json");
    ws.write("tsconfig.json", "{}");
    // A non-ASCII character before the error: the column counts UTF-16 units.
    let wide = "<script setup lang=\"ts\">\nconst é = 1; const bad = 2;\n</script>\n";
    let mut p = project(&config);
    let mut layout = Diagnostic::verter(
        canonical(&ws.path("View.vue")),
        1,
        1,
        VerterCode::CarrierPathConflict,
        "skipped".into(),
    );
    layout.severity = Severity::Warning;
    p.diagnostics.push(layout);
    p.carriers.push(ws.carrier("Wide.vue", wide));
    p.attributed.push(engine(
        &ws.path("Wide.vue"),
        wide.find("bad").unwrap() as u32,
        6133,
        DiagnosticCategory::Warning,
    ));
    let mut run = outcome(vec![p]);
    run.graph_problems = vec![
        ProjectGraphError::Cycle {
            path: vec![config.clone(), ws.path("b.json"), config.clone()],
        },
        ProjectGraphError::MissingReference {
            from: config.clone(),
            to: ws.path("missing.json"),
        },
    ];
    let diagnostics = outcome_diagnostics(&run);
    let summary = diagnostics
        .iter()
        .map(|d| {
            (
                d.verter_code
                    .map_or(d.ts_code.to_string(), |c| c.to_string()),
                d.line,
                d.col,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        summary,
        [
            ("6202".to_string(), 0, 0),
            ("6053".to_string(), 1, 1),
            ("VTER1003".to_string(), 1, 1),
            ("6133".to_string(), 2, 20),
        ]
    );
    assert!(diagnostics[0].file.is_empty());
    assert_eq!(diagnostics[1].file, canonical(&config));
    let lines = rendered(&run);
    assert!(lines[0].starts_with("error TS6202: Project references may not form"));
    let missing = format!(
        "{}(1,1): error TS6053: File '{}' not found.",
        canonical(&config),
        canonical(&ws.path("missing.json"))
    );
    assert!(lines.contains(&missing), "{lines:?}");
    assert_eq!(
        lines.last().unwrap(),
        &format!(
            "{}(2,20): warning TS6133: engine 6133",
            canonical(&ws.path("Wide.vue"))
        )
    );
    assert_eq!(exit_status(&run), ExitStatus::Failed);
}

#[test]
fn a_script_lang_mismatch_prints_vter1002_on_the_carrier() {
    let ws = Workspace::new();
    let mixed = concat!(
        "<script lang=\"ts\">export const a = 1</script>\n",
        "<script setup>const b = 2</script>\n",
    );
    let mut p = project(&ws.path("tsconfig.json"));
    p.carriers.push(ws.carrier("MixedLang.vue", mixed));
    let run = outcome(vec![p]);
    let file = canonical(&ws.path("MixedLang.vue"));
    // One fact, one line, under the registered code: no second refusal line.
    assert_eq!(
        rendered(&run),
        [format!(
            "{file}(2,1): error VTER1002: <script> and <script setup> must have the same language type."
        )]
    );
    assert_eq!(exit_status(&run), ExitStatus::Diagnostics);
}

/// The `v-if` chain the repeated-key cases share: three branches, one key.
const REPEATED_BRANCH_KEY: &str = concat!(
    "<script setup lang=\"ts\">\n",
    "const kind = 'a' as string;\n",
    "const id = 1;\n",
    "</script>\n",
    "\n",
    "<template>\n",
    "  <div>\n",
    "    <p v-if=\"kind === 'a'\" :key=\"`k:${id}`\">a</p>\n",
    "    <p v-else-if=\"kind === 'b'\" :key=\"`k:${id}`\">b</p>\n",
    "    <p v-else :key=\"`k:${id}`\">c</p>\n",
    "  </div>\n",
    "</template>\n",
);

#[test]
fn a_key_repeated_across_a_v_if_chain_warns_without_withholding_the_carrier() {
    let ws = Workspace::new();
    let mut p = project(&ws.path("tsconfig.json"));
    p.carriers
        .push(ws.carrier("Repeated.vue", REPEATED_BRANCH_KEY));
    let run = outcome(vec![p]);
    let file = canonical(&ws.path("Repeated.vue"));
    // One warning on each branch that repeats an earlier branch's key, under
    // its own code: no syntax error, no partial-check line.
    assert_eq!(
        rendered(&run),
        [
            format!("{file}(9,5): warning VTER1010: v-if/else branches must use unique keys."),
            format!("{file}(10,5): warning VTER1010: v-if/else branches must use unique keys."),
        ]
    );
    let result = &run.projects[0].carriers[0].result;
    let module = result.module.as_ref().expect("the carrier composes");
    assert!(module.withheld.is_empty(), "{:?}", module.withheld);
    assert!(result.complete, "a warning leaves the result complete");
    assert_eq!(exit_status(&run), ExitStatus::Clean, "a warning only");
}

#[test]
fn an_unreadable_carrier_prints_one_diagnostic() {
    let ws = Workspace::new();
    let mut p = project(&ws.path("tsconfig.json"));
    p.carriers.push(ws.project("Missing.vue"));
    let run = outcome(vec![p]);
    let diagnostics = outcome_diagnostics(&run);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(
        diagnostics[0].verter_code,
        Some(VerterCode::CarrierUnreadable)
    );
    assert_eq!(
        rendered(&run),
        [format!(
            "{}: error VTER1007: Cannot read carrier '{}'",
            canonical(&ws.path("Missing.vue")),
            canonical(&ws.path("Missing.vue"))
        )]
    );
}

#[test]
fn a_load_failure_prints_the_typescript_config_read_error() {
    let problem = ProjectGraphError::UnreadableRoot {
        config: PathBuf::from("/project/tsconfig.json"),
        detail: "missing".into(),
    };
    assert_eq!(
        load_failure(&problem),
        "error TS5082: Cannot read tsconfig file '/project/tsconfig.json'."
    );
}

fn canonical(path: &Path) -> String {
    verter_span::path::canonicalize_path(&path.to_string_lossy())
}

/// A carrier that composed with `families` withheld, as the mapper returns it.
fn partly_checked(ws: &Workspace, name: &str, families: &[ObligationFamily]) -> CarrierProjection {
    let mut carrier = ws.carrier(name, CLEAN);
    let mut result = (*carrier.result).clone();
    let mut module = (**result.module.as_ref().expect("a clean carrier composes")).clone();
    module.withheld = families
        .iter()
        .map(|&family| WithheldProduct {
            family,
            reason: WithheldReason::Pending,
        })
        .collect();
    result.complete = module.is_complete() && result.diagnostics.is_empty();
    result.module = Some(Arc::new(module));
    carrier.result = Arc::new(result);
    carrier
}

#[test]
fn a_carrier_with_a_withheld_product_is_reported_once() {
    let ws = Workspace::new();
    let mut p = project(&ws.path("tsconfig.json"));
    p.carriers.push(partly_checked(
        &ws,
        "Partial.vue",
        &[ObligationFamily::Model, ObligationFamily::ProvidedSlot],
    ));
    p.carriers.push(ws.carrier("Clean.vue", CLEAN));
    let run = outcome(vec![p]);
    assert!(!run.projects[0].carriers[0].result.complete);
    let partial = rendered(&run)
        .into_iter()
        .filter(|line| line.contains(VerterCode::PartialCheck.code()))
        .collect::<Vec<_>>();
    assert_eq!(
        partial,
        [format!(
            "{}(1,1): warning VTER1009: this file was only partly checked: \
             v-model, slot content checks were not run",
            canonical(&ws.path("Partial.vue"))
        )]
    );
    assert_eq!(exit_status(&run), ExitStatus::Clean, "a warning only");
}

#[test]
fn a_projects_lines_print_by_file_then_position_like_tsc() {
    let ws = Workspace::new();
    let mut p = project(&ws.path("tsconfig.json"));
    // Listed first but named last: its warning and its error print after
    // every line of `Early.vue`, and together.
    p.carriers.push(partly_checked(
        &ws,
        "Late.vue",
        &[ObligationFamily::Listener],
    ));
    p.carriers.push(ws.carrier("Early.vue", CLEAN));
    let ok = CLEAN.find("ok").unwrap() as u32;
    p.attributed.push(engine(
        &ws.path("Late.vue"),
        ok,
        2322,
        DiagnosticCategory::Error,
    ));
    p.attributed.push(engine(
        &ws.path("Early.vue"),
        ok,
        2322,
        DiagnosticCategory::Error,
    ));
    let run = outcome(vec![p]);
    let early = canonical(&ws.path("Early.vue"));
    let late = canonical(&ws.path("Late.vue"));
    assert_eq!(
        rendered(&run),
        [
            format!("{early}(2,7): error TS2322: engine 2322"),
            format!(
                "{late}(1,1): warning VTER1009: this file was only partly checked: \
                 event listener checks were not run"
            ),
            format!("{late}(2,7): error TS2322: engine 2322"),
        ]
    );
}

#[test]
fn every_incomplete_carrier_has_a_line_that_explains_it() {
    let ws = Workspace::new();
    let mut p = project(&ws.path("tsconfig.json"));
    p.carriers.push(ws.carrier("Clean.vue", CLEAN));
    p.carriers.push(ws.carrier(
        "Syntax.vue",
        "<script setup lang=\"ts\">const a = </script>\n",
    ));
    p.carriers.push(ws.carrier(
        "Refused.vue",
        concat!(
            "<script lang=\"ts\">export const a = 1</script>\n",
            "<script setup>const b = 2</script>\n",
        ),
    ));
    let mut internal = ws.with_diagnostics(
        "Internal.vue",
        vec![mapper(MapperDiagnosticCode::ProjectionInternal, "panic")],
    );
    let mut failed = (*internal.result).clone();
    failed.complete = false;
    internal.result = Arc::new(failed);
    p.carriers.push(internal);
    p.carriers.push(ws.project("Missing.vue"));
    p.carriers.push(partly_checked(
        &ws,
        "Partial.vue",
        &[ObligationFamily::Scaffold],
    ));
    let run = outcome(vec![p]);
    let diagnostics = outcome_diagnostics(&run);
    let explains = [
        VerterCode::CarrierSyntax,
        VerterCode::ScriptLangMismatch,
        VerterCode::CarrierProjectionRefused,
        VerterCode::CarrierProjectionInternal,
        VerterCode::CarrierUnreadable,
        VerterCode::PartialCheck,
    ];
    let incomplete = run.projects[0]
        .carriers
        .iter()
        .filter(|carrier| !carrier.result.complete)
        .collect::<Vec<_>>();
    assert_eq!(incomplete.len(), 5, "every carrier but the clean one");
    for carrier in incomplete {
        assert!(
            diagnostics.iter().any(|d| d.file == *carrier.carrier
                && d.verter_code.is_some_and(|code| explains.contains(&code))),
            "{} is incomplete and no line says why: {diagnostics:?}",
            carrier.carrier
        );
    }
}
