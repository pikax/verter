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
//! Phases, timed separately and never overlapping, in this order:
//!
//! - `engineStart`: an empty engine, ready — the workspace and the host;
//! - `setup`: open the project — read the job's files, configure the
//!   project from its tsconfig, add the library and the scenario;
//! - `init`: the first request, for the trivial alias every scenario
//!   declares, which absorbs one-time lazy initialisation for both tools;
//! - per probe `cold` (its first request) and `warm` (the same request
//!   repeated on the same host);
//! - engine statistics: the host's retention counters and the process's OS
//!   memory, read with the host alive and before anything is observed, so
//!   the benchmark's own observation machinery is not charged to the engine;
//! - `observe`: materialising and rendering the answers (outside every
//!   timer), then the OS memory again (observation-inclusive, reported
//!   apart);
//! - `teardown`: dropping the host.
//!
//! The record is written as soon as the engine statistics are read and
//! rewritten after observation, and a phase marker (`<out>.phase`) names the
//! phase running, so an invocation stopped mid-way still says how far it got.

pub mod cli;
pub mod disk;
pub mod process_stats;
pub mod session;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_type_engine::semantic_query::SemanticNodeId;
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
    /// Registered as the project's ambient library.
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
pub const RESULT_SCHEMA: u32 = 2;

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
    /// Microseconds spent observing the answers (outside every timer);
    /// absent until observation ran.
    pub observe_micros: Option<u64>,
    /// The cold answer; absent until observation ran.
    pub observation: Option<Observation>,
    /// Allocations during the cold request (instrumented binary only).
    pub cold_allocations: Option<(u64, u64)>,
}

/// One timed request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRecord {
    pub micros: u64,
    pub outcome: RequestOutcome,
    /// For a warm request: whether it answered the cold request's answer —
    /// the same interned node, or (observed after the measurement) the same
    /// wire answer. Absent on a cold request and until observation ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub same_answer_as_cold: Option<bool>,
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
    pub engine_start: u64,
    pub setup: u64,
    pub init: u64,
    pub teardown: Option<u64>,
}

/// The job's full record.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobResult {
    pub schema: u32,
    pub tool: &'static str,
    pub instrumented: bool,
    pub observability: bool,
    /// `measured` once the requests and the engine statistics are recorded,
    /// `complete` once observation and teardown ran too.
    pub stage: &'static str,
    /// This process: every statistics reading must be of it.
    pub pid: u32,
    pub phases: PhaseTimes,
    pub init: RequestRecord,
    pub probes: Vec<ProbeRecord>,
    /// OS statistics after the requests, with the host alive and nothing
    /// observed yet: the engine's own figures.
    pub after_requests: Option<process_stats::ProcessStats>,
    /// OS statistics after observation (observation-inclusive).
    pub after_observe: Option<process_stats::ProcessStats>,
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
    disk::read_to_string(dir.join(name))
        .map_err(|err| JobError(format!("read {}: {err}", dir.join(name).display())))
}

fn micros(start: Instant) -> u64 {
    start.elapsed().as_micros() as u64
}

/// Where the record and the phase marker are written.
pub struct Sink<'a> {
    /// The record file.
    out: &'a Path,
    /// Every phase begun so far, with its wall-clock start (epoch ms).
    history: std::cell::RefCell<Vec<serde_json::Value>>,
}

impl<'a> Sink<'a> {
    /// A sink writing the record to `out` (and the marker beside it).
    pub fn new(out: &'a Path) -> Self {
        Sink {
            out,
            history: std::cell::RefCell::new(Vec::new()),
        }
    }

    fn sibling(&self, suffix: &str) -> PathBuf {
        let mut path = self.out.as_os_str().to_owned();
        path.push(suffix);
        PathBuf::from(path)
    }

    /// Replace `path` with `text` atomically (write aside, then rename), so
    /// a reader never sees a torn file.
    fn replace(path: &Path, text: &str) -> std::io::Result<()> {
        let mut aside = path.as_os_str().to_owned();
        aside.push(".tmp");
        let aside = PathBuf::from(aside);
        disk::write(&aside, text)?;
        disk::rename(&aside, path)
    }

    /// Mark the phase about to begin, with its wall-clock start and the
    /// history so far: the evidence for how long the engine had worked when
    /// a deadline expired.
    fn phase(&self, phase: &str) -> Result<(), JobError> {
        let at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut history = self.history.borrow_mut();
        history.push(serde_json::json!({ "phase": phase, "atMs": at_ms }));
        // A marker that cannot be published stops the job before the phase
        // begins: a stale marker would misattribute a later kill.
        Self::replace(
            &self.sibling(".phase"),
            &serde_json::json!({ "phase": phase, "atMs": at_ms, "history": *history }).to_string(),
        )
        .map_err(|err| JobError(format!("publish the {phase} phase marker: {err}")))
    }

    fn record<T: Serialize>(&self, result: &T) -> Result<(), JobError> {
        let text = serde_json::to_string_pretty(result).expect("a record serialises");
        Self::replace(self.out, &text)
            .map_err(|err| JobError(format!("write {}: {err}", self.out.display())))
    }
}

/// An empty engine, ready: the workspace and the host.
fn start_engine(observability: bool) -> (Arc<MemoryWorkspace>, Arc<VerterHost>) {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let access: Arc<dyn WorkspaceAccess> = workspace.clone();
    let config = if observability {
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
    (workspace, Arc::new(VerterHost::new(config, access)))
}

/// Configure the project from the tsconfig in `dir` and add the library
/// through `lib_mode`.
fn configure_project(
    dir: &Path,
    tsconfig: &str,
    lib: &str,
    lib_mode: LibMode,
    workspace: &MemoryWorkspace,
    host: &VerterHost,
) -> Result<(), JobError> {
    let tsconfig = read(dir, tsconfig)?;
    let lib_text = read(dir, lib)?;
    let tsconfig_path = format!("{PROJECT_ROOT}/tsconfig.json");
    workspace.inject_file(tsconfig_path.clone(), Arc::from(tsconfig.as_str()));
    let mut project = verter_workspace::ide_project_config(
        PROJECT_ROOT.to_string(),
        PROJECT_ROOT.to_string(),
        Some(tsconfig_path.clone()),
    );
    project.compiler_options = verter_workspace::load_compiler_options(workspace, &tsconfig_path);
    host.configure_projects(vec![project]);
    match lib_mode {
        LibMode::Ambient => workspace
            .register_ambient_lib(AmbientLibSpec {
                project_id: None,
                canonical_id: Arc::from(lib),
                source: Arc::from(lib_text.as_str()),
            })
            .map_err(|err| JobError(format!("register the library: {err}")))?,
        LibMode::RootFile => {
            let lib_id = format!("{PROJECT_ROOT}/{lib}");
            let language = verter_session::LanguageRegistry::global()
                .classify_static(&lib_id)
                .static_resolution();
            upsert(host, lib_id, &lib_text, language)
                .map_err(|err| JobError(format!("upsert the library: {err}")))?;
        }
    }
    Ok(())
}

/// Upsert one file's text into the host.
fn upsert(
    host: &VerterHost,
    canonical_id: String,
    text: &str,
    file_language: FileLanguage,
) -> Result<(), String> {
    let _update = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical_id.clone()),
            input_id: canonical_id,
            source: Arc::from(text),
            file_language,
            aliases: Vec::new(),
        })
        .map_err(|err| format!("{err:?}"))?;
    Ok(())
}

/// Open the project on the engine; returns the scenario's canonical id.
fn open_project(
    job: &Job,
    workspace: &MemoryWorkspace,
    host: &VerterHost,
) -> Result<String, JobError> {
    configure_project(
        &job.dir,
        &job.tsconfig,
        &job.lib,
        job.lib_mode,
        workspace,
        host,
    )?;
    let scenario = read(&job.dir, &job.scenario)?;
    let scenario_id = format!("{PROJECT_ROOT}/{}", job.scenario);
    upsert(
        host,
        scenario_id.clone(),
        &scenario,
        FileLanguage::script_ts(),
    )
    .map_err(|err| JobError(format!("upsert the scenario: {err}")))?;
    Ok(scenario_id)
}

fn request(
    host: &VerterHost,
    scenario_id: &str,
    alias: &str,
    keep_audit: bool,
) -> (RequestRecord, Option<SemanticNodeId>) {
    let start = Instant::now();
    let carrier = host.resolve_named_symbol_with_audit(scenario_id, alias, None);
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
            same_answer_as_cold: None,
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

/// The wire bytes of `node`'s answer, or why there are none.
fn wire_bytes(host: &VerterHost, node: Option<SemanticNodeId>) -> Result<Vec<u8>, String> {
    let node = node.ok_or_else(|| "no value to observe".to_string())?;
    host.project_node_to_type_expr_json_bytes(node)
        .ok_or_else(|| "the answer did not materialise".to_string())
}

fn observe(bytes: Result<&[u8], String>) -> Observation {
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => {
            return Observation {
                error: Some(error),
                ..Observation::empty()
            }
        }
    };
    let decoded = serde_json::from_slice::<serde_json::Value>(bytes)
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

/// Answer `job`, writing its record through `sink`, and return the record.
/// `alloc` is present only in the instrumented binary.
pub fn run_job(
    job: &Job,
    alloc: Option<AllocHooks>,
    sink: &Sink<'_>,
) -> Result<JobResult, JobError> {
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

    sink.phase("engine-start")?;
    let start = Instant::now();
    let (workspace, host) = start_engine(job.observability);
    let engine_start = micros(start);

    sink.phase("setup")?;
    let start = Instant::now();
    let scenario_id = open_project(job, &workspace, &host)?;
    let setup = micros(start);
    drop(workspace);

    sink.phase("init")?;
    let (init, _) = request(&host, &scenario_id, &job.init_alias, false);

    let mut probes = Vec::with_capacity(job.probes.len());
    let mut nodes = Vec::with_capacity(job.probes.len());
    for alias in &job.probes {
        sink.phase("cold")?;
        if let Some(hooks) = alloc {
            (hooks.reset)();
        }
        let (cold, node) = request(&host, &scenario_id, alias, job.observability);
        let cold_allocations = alloc.map(|hooks| (hooks.read)());
        sink.phase("warm")?;
        let mut warm = Vec::with_capacity(job.warm_repeats as usize);
        let mut warm_nodes = Vec::with_capacity(job.warm_repeats as usize);
        for _ in 0..job.warm_repeats {
            let (record, warm_node) = request(&host, &scenario_id, alias, false);
            warm.push(record);
            warm_nodes.push(warm_node);
        }
        probes.push(ProbeRecord {
            alias: alias.clone(),
            cold,
            warm,
            observe_micros: None,
            observation: None,
            cold_allocations,
        });
        nodes.push((node, warm_nodes));
    }

    sink.phase("stats")?;
    let retention = retention(&host);
    let after_requests = process_stats::current_process()
        .map_err(|err| stats_errors.push(format!("after requests: {err}")))
        .ok();
    let mut result = JobResult {
        schema: RESULT_SCHEMA,
        tool: "verter",
        instrumented: alloc.is_some(),
        observability: job.observability,
        stage: "measured",
        pid: std::process::id(),
        phases: PhaseTimes {
            engine_start,
            setup,
            init: init.micros,
            teardown: None,
        },
        init,
        probes,
        after_requests,
        after_observe: None,
        after_teardown: None,
        stats_errors,
        retention,
    };
    sink.record(&result)?;

    sink.phase("observe")?;
    for (probe, (node, warm_nodes)) in result.probes.iter_mut().zip(&nodes) {
        let start = Instant::now();
        let cold_bytes = wire_bytes(&host, *node);
        probe.observation = Some(observe(cold_bytes.as_deref().map_err(Clone::clone)));
        for (record, warm_node) in probe.warm.iter_mut().zip(warm_nodes) {
            record.same_answer_as_cold = Some(match (node, warm_node) {
                (Some(cold), Some(warm)) if cold == warm => true,
                (Some(_), Some(_)) => {
                    matches!((&cold_bytes, wire_bytes(&host, *warm_node)), (Ok(a), Ok(b)) if *a == b)
                }
                _ => false,
            });
        }
        probe.observe_micros = Some(micros(start));
    }
    result.after_observe = process_stats::current_process()
        .map_err(|err| result.stats_errors.push(format!("after observe: {err}")))
        .ok();

    sink.phase("teardown")?;
    let start = Instant::now();
    drop(host);
    result.phases.teardown = Some(micros(start));
    result.after_teardown = process_stats::current_process()
        .map_err(|err| result.stats_errors.push(format!("after teardown: {err}")))
        .ok();
    result.stage = "complete";
    sink.record(&result)?;
    sink.phase("done")?;
    Ok(result)
}
