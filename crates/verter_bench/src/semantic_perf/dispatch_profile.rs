//! Per-request resolver-context call profile (measurement only).
//!
//! Counts how often each resolver-context port method is called while one
//! request runs, beside how many semantic nodes that request interned, so a
//! port method whose calls grow with the node count — a candidate for a
//! concrete, inlinable read instead of a virtual call — stands out from the
//! coarse per-miss operations.
//!
//! Lanes, each on a fresh host built with the shipped [`HostConfig`]
//! defaults:
//!
//! - every semantic benchmark scenario directory given on the command line
//!   (`target/semantic-perf/<run>/scenarios/<id>/<setting>`): cold project
//!   load, the init request, the cold probe, a warm hit, and a local edit (an
//!   unrelated declaration appended to the scenario) followed by the probe
//!   cold and warm again;
//! - a flow-return lane: unannotated functions whose inferred return types
//!   the probe demands;
//! - a component-meta lane over the session crate's vendored SFC fixtures,
//!   cold, warm, and after a local edit of an imported types module.
//!
//! The counters exist only under the `semantic-observe` feature; this module
//! is compiled only with it.
//!
//! ```text
//! cargo run --release -p verter_bench --features semantic-observe \
//!   --example resolver_dispatch_profile -- [--out <profile.json>] [<scenario-dir>...]
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_type_engine::resolver_core::dispatch_profile;
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use super::{open_project, Job, JobError, LibMode, JOB_SCHEMA, PROJECT_ROOT};

/// One phase of one lane.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseProfile {
    pub phase: &'static str,
    /// Semantic nodes the host retained after the phase minus before it.
    pub semantic_nodes_added: i64,
    /// Semantic nodes retained after the phase.
    pub semantic_nodes: usize,
    /// Memo entries retained after the phase.
    pub semantic_memo_entries: usize,
    /// Port method → calls during the phase.
    pub calls: BTreeMap<&'static str, u64>,
}

/// One lane's phases.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaneProfile {
    pub lane: String,
    pub phases: Vec<PhaseProfile>,
}

fn phase<R>(host: &VerterHost, name: &'static str, work: impl FnOnce() -> R) -> (R, PhaseProfile) {
    let before = host.retention_snapshot();
    dispatch_profile::reset();
    let result = work();
    let calls = dispatch_profile::snapshot()
        .into_iter()
        .map(|count| (count.method, count.calls))
        .collect();
    let after = host.retention_snapshot();
    (
        result,
        PhaseProfile {
            phase: name,
            semantic_nodes_added: after.semantic_nodes as i64 - before.semantic_nodes as i64,
            semantic_nodes: after.semantic_nodes,
            semantic_memo_entries: after.semantic_memo_entries,
            calls,
        },
    )
}

fn fresh_host() -> (Arc<MemoryWorkspace>, Arc<VerterHost>) {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let access: Arc<dyn WorkspaceAccess> = workspace.clone();
    (
        workspace,
        Arc::new(VerterHost::new(HostConfig::default(), access)),
    )
}

fn resolve(host: &VerterHost, canonical: &str, alias: &str) -> Result<(), String> {
    let (outcome, _audit) = host
        .resolve_named_symbol_with_audit(canonical, alias, None)
        .into_parts();
    match outcome {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(format!("`{alias}` resolved nothing")),
        Err(fault) => Err(format!("`{alias}`: {fault:?}")),
    }
}

fn upsert(host: &VerterHost, canonical: &str, source: &str, language: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: language,
            aliases: Vec::new(),
        })
        .unwrap_or_else(|err| panic!("upsert {canonical}: {err:?}"));
}

fn language_of(canonical: &str) -> FileLanguage {
    verter_session::LanguageRegistry::global()
        .classify_static(canonical)
        .static_resolution()
}

/// The root files a scenario directory's tsconfig lists besides the library
/// and the scenario module, in its order.
fn companion_files(dir: &Path) -> Result<Vec<String>, JobError> {
    let path = dir.join("tsconfig.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|err| JobError(format!("read {}: {err}", path.display())))?;
    let config: serde_json::Value = serde_json::from_str(&text)
        .map_err(|err| JobError(format!("parse {}: {err}", path.display())))?;
    Ok(config["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| file.as_str())
        .filter(|file| !matches!(*file, "lib.bench.d.ts" | "scenario.ts"))
        .map(String::from)
        .collect())
}

/// Profile one semantic benchmark scenario directory.
pub fn profile_scenario(dir: &Path) -> Result<LaneProfile, JobError> {
    let job = Job {
        schema: JOB_SCHEMA,
        dir: dir.to_path_buf(),
        tsconfig: "tsconfig.json".into(),
        lib: "lib.bench.d.ts".into(),
        lib_mode: LibMode::RootFile,
        files: companion_files(dir)?,
        scenario: "scenario.ts".into(),
        init_alias: "__BenchInit".into(),
        probes: vec!["__Probe".into()],
        warm_repeats: 1,
        observability: false,
    };
    let (workspace, host) = fresh_host();
    let mut phases = Vec::new();
    let (scenario_id, p) = phase(&host, "setup", || open_project(&job, &workspace, &host));
    let scenario_id = scenario_id?;
    phases.push(p);
    let (init, p) = phase(&host, "init", || {
        resolve(&host, &scenario_id, "__BenchInit")
    });
    init.map_err(JobError)?;
    phases.push(p);
    for name in ["cold", "warm"] {
        let (answer, p) = phase(&host, name, || resolve(&host, &scenario_id, "__Probe"));
        answer.map_err(JobError)?;
        phases.push(p);
    }
    let source = super::read(dir, "scenario.ts")?;
    let edited = format!("{source}\ntype __BenchEdit = 1;\n");
    let ((), p) = phase(&host, "edit", || {
        upsert(&host, &scenario_id, &edited, FileLanguage::script_ts())
    });
    phases.push(p);
    for name in ["edit-cold", "edit-warm"] {
        let (answer, p) = phase(&host, name, || resolve(&host, &scenario_id, "__Probe"));
        answer.map_err(JobError)?;
        phases.push(p);
    }
    let lane = dir
        .ancestors()
        .nth(1)
        .and_then(Path::file_name)
        .map_or_else(
            || dir.display().to_string(),
            |id| id.to_string_lossy().into_owned(),
        );
    Ok(LaneProfile { lane, phases })
}

const FLOW_RETURN_SOURCE: &str = r#"
type __BenchInit = 0;
function point(x: number, y: number) {
  const origin = { x, y, tag: "point" as const };
  if (x > y) {
    return { ...origin, larger: "x" as const };
  }
  return { ...origin, larger: "y" as const };
}
function pair(n: number) {
  const p = point(n, n + 1);
  return [p, point(p.y, p.x)] as const;
}
function wrap(n: number) {
  const list = [pair(n), pair(n * 2)];
  return { list, first: list[0], count: list.length };
}
type __Probe = ReturnType<typeof wrap>;
export {};
"#;

/// Profile demands answered through unannotated function return types.
///
/// `Err` when any phase's probe resolves nothing: an unanswered lane is a
/// failed measurement run, never a cheap-looking near-zero row.
pub fn profile_flow_return() -> Result<LaneProfile, JobError> {
    let (_workspace, host) = fresh_host();
    let canonical = format!("{PROJECT_ROOT}/flow.ts");
    let mut phases = Vec::new();
    let ((), p) = phase(&host, "setup", || {
        host.configure_projects(vec![verter_workspace::ide_project_config(
            PROJECT_ROOT.to_string(),
            PROJECT_ROOT.to_string(),
            None,
        )]);
        upsert(
            &host,
            &canonical,
            FLOW_RETURN_SOURCE,
            FileLanguage::script_ts(),
        );
    });
    phases.push(p);
    for (name, alias) in [
        ("init", "__BenchInit"),
        ("cold", "__Probe"),
        ("warm", "__Probe"),
    ] {
        let (answer, p) = phase(&host, name, || resolve(&host, &canonical, alias));
        answer.map_err(JobError)?;
        phases.push(p);
    }
    Ok(LaneProfile {
        lane: "flow-return".into(),
        phases,
    })
}

const SFC_FIXTURES: &[(&str, &str)] = &[
    (
        "table.vue",
        include_str!("../../../verter_session/test_fixtures/table.vue"),
    ),
    (
        "table_types.ts",
        include_str!("../../../verter_session/test_fixtures/table_types.ts"),
    ),
    (
        "tabs.vue",
        include_str!("../../../verter_session/test_fixtures/tabs.vue"),
    ),
    (
        "tabs_types.ts",
        include_str!("../../../verter_session/test_fixtures/tabs_types.ts"),
    ),
    (
        "tabs_helper.ts",
        include_str!("../../../verter_session/test_fixtures/tabs_helper.ts"),
    ),
    (
        "editor_toolbar.vue",
        include_str!("../../../verter_session/test_fixtures/editor_toolbar.vue"),
    ),
    (
        "editor_toolbar_types.ts",
        include_str!("../../../verter_session/test_fixtures/editor_toolbar_types.ts"),
    ),
];

/// The fixtures whose component metadata the lane demands.
const SFC_COMPONENTS: &[&str] = &["table.vue", "tabs.vue", "editor_toolbar.vue"];

/// Profile component metadata over the vendored SFC fixtures.
///
/// `Err` when any demanded component yields no metadata: an unanswered lane
/// is a failed measurement run, never a cheap-looking near-zero row.
pub fn profile_component_meta() -> Result<LaneProfile, JobError> {
    let (_workspace, host) = fresh_host();
    let id = |name: &str| format!("{PROJECT_ROOT}/{name}");
    let components: Vec<String> = SFC_COMPONENTS.iter().map(|name| id(name)).collect();
    let mut phases = Vec::new();
    let ((), p) = phase(&host, "setup", || {
        host.configure_projects(vec![verter_workspace::ide_project_config(
            PROJECT_ROOT.to_string(),
            PROJECT_ROOT.to_string(),
            None,
        )]);
        for (name, source) in SFC_FIXTURES {
            let canonical = id(name);
            upsert(&host, &canonical, source, language_of(&canonical));
        }
    });
    phases.push(p);
    let all_meta = |host: &VerterHost| -> Result<(), String> {
        for component in &components {
            if host.get_component_meta(component).is_none() {
                return Err(format!("no component metadata for `{component}`"));
            }
        }
        Ok(())
    };
    for name in ["cold", "warm"] {
        let (answer, p) = phase(&host, name, || all_meta(&host));
        answer.map_err(JobError)?;
        phases.push(p);
    }
    let (types_name, types_source) = SFC_FIXTURES[1];
    let edited = format!("{types_source}\nexport type BenchEdit = 1;\n");
    let ((), p) = phase(&host, "edit", || {
        upsert(
            &host,
            &id(types_name),
            &edited,
            language_of(&id(types_name)),
        )
    });
    phases.push(p);
    for name in ["edit-cold", "edit-warm"] {
        let (answer, p) = phase(&host, name, || all_meta(&host));
        answer.map_err(JobError)?;
        phases.push(p);
    }
    Ok(LaneProfile {
        lane: "component-meta".into(),
        phases,
    })
}

/// Entry point of the `resolver_dispatch_profile` example.
pub fn main() -> std::process::ExitCode {
    let mut args = std::env::args().skip(1);
    let mut out: Option<PathBuf> = None;
    let mut dirs = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--out" {
            out = args.next().map(PathBuf::from);
        } else {
            dirs.push(PathBuf::from(arg));
        }
    }
    let mut lanes = Vec::new();
    for dir in &dirs {
        match profile_scenario(dir) {
            Ok(lane) => lanes.push(lane),
            Err(err) => {
                eprintln!("{}: {err}", dir.display());
                return std::process::ExitCode::FAILURE;
            }
        }
    }
    for lane in [profile_flow_return(), profile_component_meta()] {
        match lane {
            Ok(lane) => lanes.push(lane),
            Err(err) => {
                eprintln!("{err}");
                return std::process::ExitCode::FAILURE;
            }
        }
    }
    let json = serde_json::to_string_pretty(&lanes).expect("profile serialises");
    match out {
        Some(path) => {
            if let Err(err) = super::disk::write(&path, &json) {
                eprintln!("write {}: {err}", path.display());
                return std::process::ExitCode::FAILURE;
            }
        }
        None => println!("{json}"),
    }
    std::process::ExitCode::SUCCESS
}
