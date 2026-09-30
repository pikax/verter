//! The Verter arm of the equivalent-demand semantic benchmark
//! (`scripts/benchmark/semantic-perf.mjs`).
//!
//! A job names the files of one scenario project on disk — the same files,
//! byte for byte, the tsc arm hands to TypeScript's native API — and the
//! type aliases whose declared type is demanded. The runner answers them
//! through the production typeinfo surface a shipped consumer calls
//! (`VerterHost::resolve_named_symbol_with_audit`, then
//! `project_node_to_type_expr_json_bytes` for the answer), on a host built
//! with the production [`HostConfig`] defaults, and writes one JSON record.
//!
//! Phases are timed separately and never overlap:
//!
//! - `setup`: read the job's files, build the workspace and host, configure
//!   the project, register the library and upsert the scenario;
//! - `init`: the first request, for the trivial alias every scenario
//!   declares, which absorbs one-time lazy initialisation for both tools;
//! - per probe `cold` (its first request), `observe` (materialising and
//!   rendering the answer — outside the request timer) and `warm` (the same
//!   request repeated on the same host);
//! - `teardown`: dropping the host and workspace.
//!
//! Memory is read from the operating system ([`process_stats`]) after the
//! requests with the host still alive (what the process retains) and after
//! teardown. The host's own retention counters are read after the requests,
//! outside every timer.

pub mod cli;
pub mod process_stats;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::TypeExpr;
use verter_workspace::{AmbientLibSpec, MemoryOptions, MemoryWorkspace, WorkspaceAccess};

/// Allocation counters a counting global allocator supplies. The plain
/// probe binary has none; the separately labelled instrumented binary does.
#[derive(Clone, Copy)]
pub struct AllocHooks {
    /// Reset the process-wide counters.
    pub reset: fn(),
    /// Read `(allocations, allocated bytes)` since the last reset.
    pub read: fn() -> (u64, u64),
}

/// How the job's library reaches the Verter host.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LibMode {
    /// Registered as the project's ambient library — the channel through
    /// which a Verter project reads the declarations TypeScript reads from
    /// its lib files.
    Ambient,
    /// Upserted as an ordinary `.d.ts` file of the project — the channel
    /// through which tsc reads it (a root file under `noLib`).
    RootFile,
}

/// One scenario project to answer.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Job {
    /// Must be [`JOB_SCHEMA`].
    pub schema: u32,
    /// Directory holding the files below.
    pub dir: PathBuf,
    /// The tsconfig file (relative to `dir`) whose `compilerOptions` the
    /// project is configured with.
    pub tsconfig: String,
    /// The library file (relative to `dir`).
    pub lib: String,
    /// How the library reaches the host.
    pub lib_mode: LibMode,
    /// The scenario module (relative to `dir`).
    pub scenario: String,
    /// The alias every scenario declares for the init request.
    pub init_alias: String,
    /// The demanded aliases, in request order.
    pub probes: Vec<String>,
    /// Warm repetitions of each probe after its cold request.
    pub warm_repeats: u32,
    /// Turn on the host's observability bookkeeping (audit records with
    /// timing and footprint capture, metrics). Off in the production
    /// configuration the head-to-head measures; on only for the separately
    /// labelled arm that shows what the bookkeeping costs.
    #[serde(default)]
    pub observability: bool,
}

/// The job schema this runner reads.
pub const JOB_SCHEMA: u32 = 1;
/// The result schema this runner writes.
pub const RESULT_SCHEMA: u32 = 1;

/// The canonical root the scenario project lives at inside the host.
const PROJECT_ROOT: &str = "/bench";

/// Typed outcome of one request.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum RequestOutcome {
    /// The request resolved a type.
    Value,
    /// A well-formed request that resolved nothing.
    Miss,
    /// A typed request fault (budget, unstable state, cycle, …).
    Fault { detail: String },
}

/// The observed answer of one probe.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    /// The rendered TypeScript text of the answer, when it rendered.
    pub text: Option<String>,
    /// Why the answer could not be materialised or rendered.
    pub error: Option<String>,
    /// Top-level shape of the answer (`literal`, `union`, `primitive:any`, …).
    pub shape: Option<String>,
    /// Member count when the answer is a union.
    pub union_members: Option<usize>,
    /// `unknown` nodes in the answer's wire form: raw source or an
    /// unmaterialised sentinel the terminal projection spelled as text — the
    /// answer is not complete.
    pub unknown_leaves: usize,
    /// The first few `unknown` spellings, for the report.
    pub unknown_samples: Vec<String>,
    /// `conditional` / `infer` nodes in the answer: a conditional left
    /// unevaluated.
    pub conditional_nodes: usize,
}

impl Observation {
    fn empty() -> Self {
        Self {
            text: None,
            error: None,
            shape: None,
            union_members: None,
            unknown_leaves: 0,
            unknown_samples: Vec::new(),
            conditional_nodes: 0,
        }
    }
}

/// One probe's record.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeRecord {
    pub alias: String,
    pub cold: RequestRecord,
    pub warm: Vec<RequestRecord>,
    pub observe_micros: u64,
    pub observation: Observation,
    /// Allocations during the cold request (instrumented binary only).
    pub cold_allocations: Option<(u64, u64)>,
}

/// One timed request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRecord {
    pub micros: u64,
    pub outcome: RequestOutcome,
    /// The production audit record of the request (work counts: hops,
    /// expansions, projection operations, …), kept only on the
    /// observability-on arm, where the host produces it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audit: Option<serde_json::Value>,
}

/// Every phase's time, in microseconds.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseTimes {
    pub setup: u64,
    pub init: u64,
    pub teardown: u64,
}

/// The job's full record.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobResult {
    pub schema: u32,
    pub tool: &'static str,
    pub instrumented: bool,
    pub observability: bool,
    pub phases: PhaseTimes,
    pub init: RequestRecord,
    pub probes: Vec<ProbeRecord>,
    /// OS statistics after the requests, with the host alive.
    pub after_requests: Option<process_stats::ProcessStats>,
    /// OS statistics after teardown.
    pub after_teardown: Option<process_stats::ProcessStats>,
    pub stats_errors: Vec<String>,
    /// The host's retention counters after the requests.
    pub retention: RetentionCounts,
}

/// The host's production retention counters, read after the requests.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionCounts {
    pub semantic_nodes: usize,
    pub semantic_memo_entries: usize,
    pub relation_proofs: usize,
    pub relate_keys: usize,
    pub union_views: usize,
    pub shape_cache_entries: usize,
    pub active_bytes: usize,
    pub retained_bytes: usize,
    pub pinned_bytes: usize,
    pub peak_total_bytes: usize,
    pub refusals_pressure: u64,
    pub refusals_oversized: u64,
    pub refusals_active: u64,
}

/// A job that could not run at all.
#[derive(Debug)]
pub struct JobError(pub String);

impl std::fmt::Display for JobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn read(dir: &Path, name: &str) -> Result<String, JobError> {
    std::fs::read_to_string(dir.join(name))
        .map_err(|err| JobError(format!("read {}: {err}", dir.join(name).display())))
}

fn micros(start: Instant) -> u64 {
    start.elapsed().as_micros() as u64
}

/// The host one job is answered on, with the workspace it reads.
struct ScenarioHost {
    host: Arc<VerterHost>,
    scenario_id: String,
}

fn build_host(job: &Job) -> Result<ScenarioHost, JobError> {
    let tsconfig = read(&job.dir, &job.tsconfig)?;
    let lib = read(&job.dir, &job.lib)?;
    let scenario = read(&job.dir, &job.scenario)?;

    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let tsconfig_path = format!("{PROJECT_ROOT}/tsconfig.json");
    workspace.inject_file(tsconfig_path.clone(), Arc::from(tsconfig.as_str()));
    let mut project = verter_workspace::ide_project_config(
        PROJECT_ROOT.to_string(),
        PROJECT_ROOT.to_string(),
        Some(tsconfig_path.clone()),
    );
    project.compiler_options = verter_workspace::load_compiler_options(&*workspace, &tsconfig_path);
    let access: Arc<dyn WorkspaceAccess> = workspace.clone();
    let config = if job.observability {
        HostConfig {
            audit_enabled: true,
            audit_timing_capture: true,
            footprint_capture: true,
            metrics_enabled: true,
            ..HostConfig::default()
        }
    } else {
        // The shipped defaults, untouched.
        HostConfig::default()
    };
    let host = Arc::new(VerterHost::new(config, access));
    host.configure_projects(vec![project]);
    match job.lib_mode {
        LibMode::Ambient => workspace
            .register_ambient_lib(AmbientLibSpec {
                project_id: None,
                canonical_id: Arc::from(job.lib.as_str()),
                source: Arc::from(lib.as_str()),
            })
            .map_err(|err| JobError(format!("register the library: {err}")))?,
        LibMode::RootFile => {
            let lib_id = format!("{PROJECT_ROOT}/{}", job.lib);
            let language = verter_session::LanguageRegistry::global()
                .classify_static(&lib_id)
                .static_resolution();
            let _update = host
                .upsert(UpsertRequest {
                    canonical_id: Some(lib_id.clone()),
                    input_id: lib_id,
                    source: Arc::from(lib.as_str()),
                    file_language: language,
                    aliases: Vec::new(),
                })
                .map_err(|err| JobError(format!("upsert the library: {err:?}")))?;
        }
    }
    let scenario_id = format!("{PROJECT_ROOT}/{}", job.scenario);
    let _update = host
        .upsert(UpsertRequest {
            canonical_id: Some(scenario_id.clone()),
            input_id: scenario_id.clone(),
            source: Arc::from(scenario.as_str()),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .map_err(|err| JobError(format!("upsert the scenario: {err:?}")))?;
    Ok(ScenarioHost { host, scenario_id })
}

fn request(
    scenario: &ScenarioHost,
    alias: &str,
    keep_audit: bool,
) -> (
    RequestRecord,
    Option<verter_session::semantic_query::SemanticNodeId>,
) {
    let start = Instant::now();
    let carrier = scenario
        .host
        .resolve_named_symbol_with_audit(&scenario.scenario_id, alias, None);
    let micros = micros(start);
    // Everything below is outside the timer, including dropping the audit
    // record the carrier holds.
    let (outcome, audit) = carrier.into_parts();
    let (outcome, node) = match outcome {
        Ok(Some(node)) => (RequestOutcome::Value, Some(node)),
        Ok(None) => (RequestOutcome::Miss, None),
        Err(fault) => (
            RequestOutcome::Fault {
                detail: format!("{fault:?}"),
            },
            None,
        ),
    };
    let audit = keep_audit
        .then(|| serde_json::to_value(&audit).ok())
        .flatten();
    (
        RequestRecord {
            micros,
            outcome,
            audit,
        },
        node,
    )
}

fn shape_of(expr: &TypeExpr) -> (String, Option<usize>) {
    match expr {
        TypeExpr::Primitive(name) => (format!("primitive:{name:?}").to_lowercase(), None),
        TypeExpr::Literal(_) => ("literal".into(), None),
        TypeExpr::Union(members) => ("union".into(), Some(members.len())),
        TypeExpr::Intersection(_) => ("intersection".into(), None),
        TypeExpr::Array { .. } => ("array".into(), None),
        TypeExpr::Tuple { .. } => ("tuple".into(), None),
        TypeExpr::Object(_) => ("object".into(), None),
        TypeExpr::Function(_) => ("function".into(), None),
        TypeExpr::Ref { .. } => ("ref".into(), None),
        TypeExpr::TemplateLiteral { .. } => ("template".into(), None),
        other => {
            let debug = format!("{other:?}");
            let head = debug
                .split(|c: char| !c.is_alphanumeric())
                .next()
                .unwrap_or("other")
                .to_lowercase();
            (head, None)
        }
    }
}

/// Count the `unknown` and `conditional`/`infer` nodes of a wire
/// `TypeExpr` (iteratively: answers can be deep).
fn wire_node_counts(root: &serde_json::Value) -> (usize, Vec<String>, usize) {
    let mut unknown = 0;
    let mut samples = Vec::new();
    let mut conditional = 0;
    let mut stack = vec![root];
    while let Some(value) = stack.pop() {
        match value {
            serde_json::Value::Object(map) => {
                match map.get("kind").and_then(serde_json::Value::as_str) {
                    Some("unknown") => {
                        unknown += 1;
                        if samples.len() < 4 {
                            let raw = map
                                .get("raw")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("");
                            samples.push(raw.chars().take(80).collect());
                        }
                    }
                    Some("conditional") | Some("infer") => conditional += 1,
                    _ => {}
                }
                stack.extend(map.values());
            }
            serde_json::Value::Array(items) => stack.extend(items.iter()),
            _ => {}
        }
    }
    (unknown, samples, conditional)
}

fn observe(
    scenario: &ScenarioHost,
    node: Option<verter_session::semantic_query::SemanticNodeId>,
) -> Observation {
    let Some(node) = node else {
        return Observation {
            error: Some("no value to observe".into()),
            ..Observation::empty()
        };
    };
    let Some(bytes) = scenario.host.project_node_to_type_expr_json_bytes(node) else {
        return Observation {
            error: Some("the answer did not materialise".into()),
            ..Observation::empty()
        };
    };
    let decoded = serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|err| err.to_string())
        .and_then(|value| {
            let expr =
                serde_json::from_value::<TypeExpr>(value.clone()).map_err(|err| err.to_string())?;
            Ok((value, expr))
        });
    let (wire, expr) = match decoded {
        Ok(decoded) => decoded,
        Err(err) => {
            return Observation {
                error: Some(format!("decode the answer: {err}")),
                ..Observation::empty()
            }
        }
    };
    let (unknown_leaves, unknown_samples, conditional_nodes) = wire_node_counts(&wire);
    let (shape, union_members) = shape_of(&expr);
    match verter_type_expr::render_type_expr_display(&expr) {
        Ok(rendered) => Observation {
            text: Some(rendered.text),
            error: None,
            shape: Some(shape),
            union_members,
            unknown_leaves,
            unknown_samples,
            conditional_nodes,
        },
        Err(err) => Observation {
            error: Some(format!("render the answer: {err:?}")),
            shape: Some(shape),
            union_members,
            unknown_leaves,
            unknown_samples,
            conditional_nodes,
            ..Observation::empty()
        },
    }
}

fn retention(host: &VerterHost) -> RetentionCounts {
    let snapshot = host.retention_snapshot();
    RetentionCounts {
        semantic_nodes: snapshot.semantic_nodes,
        semantic_memo_entries: snapshot.semantic_memo_entries,
        relation_proofs: snapshot.relation_proofs,
        relate_keys: snapshot.relate_keys,
        union_views: snapshot.union_views,
        shape_cache_entries: snapshot.shape_cache_entries,
        active_bytes: snapshot.active_bytes,
        retained_bytes: snapshot.retained_bytes,
        pinned_bytes: snapshot.pinned_bytes,
        peak_total_bytes: snapshot.peak_total_bytes,
        refusals_pressure: snapshot.refusals_pressure,
        refusals_oversized: snapshot.refusals_oversized,
        refusals_active: snapshot.refusals_active,
    }
}

/// Answer `job` and return its record. `alloc` is present only in the
/// instrumented binary.
pub fn run_job(job: &Job, alloc: Option<AllocHooks>) -> Result<JobResult, JobError> {
    if job.schema != JOB_SCHEMA {
        return Err(JobError(format!(
            "job schema {} is not the supported {JOB_SCHEMA}",
            job.schema
        )));
    }
    if job.probes.is_empty() {
        return Err(JobError("a job demands at least one probe".into()));
    }
    let mut stats_errors = Vec::new();

    let start = Instant::now();
    let scenario = build_host(job)?;
    let setup = micros(start);

    let (init, _) = request(&scenario, &job.init_alias, false);

    let mut probes = Vec::with_capacity(job.probes.len());
    for alias in &job.probes {
        if let Some(hooks) = alloc {
            (hooks.reset)();
        }
        let (cold, node) = request(&scenario, alias, job.observability);
        let cold_allocations = alloc.map(|hooks| (hooks.read)());
        let observe_start = Instant::now();
        let observation = observe(&scenario, node);
        let observe_micros = micros(observe_start);
        let warm = (0..job.warm_repeats)
            .map(|_| request(&scenario, alias, false).0)
            .collect();
        probes.push(ProbeRecord {
            alias: alias.clone(),
            cold,
            warm,
            observe_micros,
            observation,
            cold_allocations,
        });
    }

    let retention = retention(&scenario.host);
    let after_requests = process_stats::current_process()
        .map_err(|err| stats_errors.push(format!("after requests: {err}")))
        .ok();

    let start = Instant::now();
    drop(scenario);
    let teardown = micros(start);
    let after_teardown = process_stats::current_process()
        .map_err(|err| stats_errors.push(format!("after teardown: {err}")))
        .ok();

    Ok(JobResult {
        schema: RESULT_SCHEMA,
        tool: "verter",
        instrumented: alloc.is_some(),
        observability: job.observability,
        phases: PhaseTimes {
            setup,
            init: init.micros,
            teardown,
        },
        init,
        probes,
        after_requests,
        after_teardown,
        stats_errors,
        retention,
    })
}
