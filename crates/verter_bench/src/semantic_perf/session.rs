//! Session workloads of the semantic benchmark: one live engine answering a
//! script of edits and demands over a multi-file project, the way an editor
//! drives it.
//!
//! A session job names the project's files on disk (the same files the tsc
//! session driver opens) and an ordered script:
//!
//! - `demand`: request the declared type of one or more aliases. A
//!   `concurrent` demand issues every request at once, one thread each, on
//!   the shared host;
//! - `edit`: replace one file's text (the workspace and the host both see the
//!   new text, as an editor's change notification delivers it);
//! - `meta`: request a Vue component's metadata (props and events), a demand
//!   with no tsc counterpart.
//!
//! Every request is timed alone; each step's answers are observed right
//! after the step, outside every timer. Engine statistics are read once,
//! after the last step, so in a session they include observation (both
//! tools observe the same way). The record is written when the script
//! completes; the phase marker names the step running.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use verter_session::VerterHost;

use super::{
    configure_project, micros, observe, process_stats, read, request, retention, start_engine,
    upsert, wire_bytes, JobError, LibMode, Observation, RequestOutcome, RetentionCounts, Sink,
    PROJECT_ROOT,
};

/// The session job schema this runner reads.
pub const SESSION_JOB_SCHEMA: u32 = 1;
/// The session record schema this runner writes.
pub const SESSION_RESULT_SCHEMA: u32 = 1;

/// One aliased type to demand.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Demand {
    /// The module declaring the alias (relative to the job's `dir`).
    pub file: String,
    pub alias: String,
}

/// One step of a session script.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", deny_unknown_fields)]
pub enum Step {
    Demand {
        requests: Vec<Demand>,
        #[serde(default)]
        concurrent: bool,
    },
    Edit {
        file: String,
        text: String,
    },
    Meta {
        file: String,
    },
}

/// A session job.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionJob {
    /// Must be [`SESSION_JOB_SCHEMA`].
    pub schema: u32,
    pub dir: PathBuf,
    pub tsconfig: String,
    pub lib: String,
    pub lib_mode: LibMode,
    /// The project's files (relative to `dir`), opened in this order.
    pub files: Vec<String>,
    /// The module declaring the init alias.
    pub init_file: String,
    pub init_alias: String,
    pub steps: Vec<Step>,
    /// Host observability bookkeeping, as for a probe job.
    #[serde(default)]
    pub observability: bool,
}

/// One timed request of a demand step and its answer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DemandRecord {
    pub file: String,
    pub alias: String,
    pub micros: u64,
    pub outcome: RequestOutcome,
    pub observation: Observation,
}

/// One prop of a component's published surface.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaProp {
    pub name: String,
    pub required: bool,
}

/// What a `meta` step observed: the published prop and event names.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaSurface {
    pub props: Vec<MetaProp>,
    pub events: Vec<String>,
}

/// One step's record.
#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum StepRecord {
    Demand {
        concurrent: bool,
        /// Threads the step ran its requests on (1 when sequential).
        threads: usize,
        /// The whole step, first request issued to last answered.
        wall_micros: u64,
        requests: Vec<DemandRecord>,
    },
    Edit {
        file: String,
        /// SHA-256 of the exact text the edit installed.
        text_sha256: String,
        micros: u64,
    },
    Meta {
        file: String,
        micros: u64,
        outcome: RequestOutcome,
        surface: Option<MetaSurface>,
    },
}

/// The session's phase times, in microseconds.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPhases {
    pub engine_start: u64,
    pub setup: u64,
    pub init: u64,
}

/// A session job's record.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResult {
    pub schema: u32,
    pub tool: &'static str,
    pub kind: &'static str,
    pub observability: bool,
    /// Whether this binary has optional semantic capture compiled in.
    pub capture_available: bool,
    pub stage: &'static str,
    pub pid: u32,
    pub phases: SessionPhases,
    pub init_outcome: RequestOutcome,
    pub steps: Vec<StepRecord>,
    /// OS statistics after the last step (observation-inclusive).
    pub after_steps: Option<process_stats::ProcessStats>,
    pub stats_errors: Vec<String>,
    pub retention: RetentionCounts,
}

fn canonical(file: &str) -> String {
    format!("{PROJECT_ROOT}/{file}")
}

fn language_of(canonical_id: &str) -> verter_session::FileLanguage {
    verter_session::LanguageRegistry::global()
        .classify_static(canonical_id)
        .static_resolution()
}

/// The record of one answered demand, its answer observed now (outside
/// every timer).
fn demand_record(
    host: &VerterHost,
    demand: &Demand,
    record: super::RequestRecord,
    node: Option<super::SemanticNodeId>,
) -> DemandRecord {
    let bytes = wire_bytes(host, node);
    DemandRecord {
        file: demand.file.clone(),
        alias: demand.alias.clone(),
        micros: record.micros,
        outcome: record.outcome,
        observation: observe(bytes.as_deref().map_err(Clone::clone)),
    }
}

fn demand_step(host: &Arc<VerterHost>, requests: &[Demand], concurrent: bool) -> StepRecord {
    if !concurrent || requests.len() < 2 {
        let mut wall_micros = 0;
        let answered: Vec<_> = requests
            .iter()
            .map(|demand| {
                let (record, node) = request(host, &canonical(&demand.file), &demand.alias, false);
                wall_micros += record.micros;
                (record, node)
            })
            .collect();
        // Observed after the step, outside every timer (as the tsc arm does).
        let requests = requests
            .iter()
            .zip(answered)
            .map(|(demand, (record, node))| demand_record(host, demand, record, node))
            .collect();
        return StepRecord::Demand {
            concurrent: false,
            threads: 1,
            wall_micros,
            requests,
        };
    }
    // Every request is issued before any is answered: one thread each,
    // released together.
    let barrier = std::sync::Barrier::new(requests.len());
    let start = Instant::now();
    let answered: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = requests
            .iter()
            .map(|demand| {
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    request(host, &canonical(&demand.file), &demand.alias, false)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("a demand thread completes"))
            .collect()
    });
    let wall_micros = micros(start);
    StepRecord::Demand {
        concurrent: true,
        threads: requests.len(),
        wall_micros,
        requests: requests
            .iter()
            .zip(answered)
            .map(|(demand, (record, node))| demand_record(host, demand, record, node))
            .collect(),
    }
}

fn meta_step(host: &VerterHost, file: &str) -> StepRecord {
    let start = Instant::now();
    let meta = host.try_get_component_meta(&canonical(file));
    let micros = micros(start);
    let (outcome, surface) = match meta {
        Ok(Some(meta)) => (
            RequestOutcome::Value,
            Some(MetaSurface {
                props: meta
                    .props
                    .iter()
                    .map(|prop| MetaProp {
                        name: prop.name.clone(),
                        required: prop.required,
                    })
                    .collect(),
                events: meta.events.iter().map(|event| event.name.clone()).collect(),
            }),
        ),
        Ok(None) => (RequestOutcome::Miss, None),
        Err(abort) => (
            RequestOutcome::Fault {
                detail: format!("{abort:?}"),
            },
            None,
        ),
    };
    StepRecord::Meta {
        file: file.to_string(),
        micros,
        outcome,
        surface,
    }
}

/// Run `job`'s script, writing its record through `sink`.
pub fn run_session(job: &SessionJob, sink: &Sink<'_>) -> Result<SessionResult, JobError> {
    if job.schema != SESSION_JOB_SCHEMA {
        return Err(JobError(format!(
            "session job schema {} is not the supported {SESSION_JOB_SCHEMA}",
            job.schema
        )));
    }
    if job.steps.is_empty() {
        return Err(JobError("a session runs at least one step".into()));
    }
    let mut stats_errors = Vec::new();

    sink.phase("engine-start")?;
    let start = Instant::now();
    let (workspace, host) = start_engine(job.observability);
    let engine_start = micros(start);

    sink.phase("setup")?;
    let start = Instant::now();
    configure_project(
        &job.dir,
        &job.tsconfig,
        &job.lib,
        job.lib_mode,
        &workspace,
        &host,
    )?;
    for file in &job.files {
        let text = read(&job.dir, file)?;
        let id = canonical(file);
        workspace.inject_file(id.clone(), Arc::from(text.as_str()));
        upsert(&host, id.clone(), &text, language_of(&id))
            .map_err(|err| JobError(format!("upsert {file}: {err}")))?;
    }
    let setup = micros(start);

    sink.phase("init")?;
    let (init, _) = request(&host, &canonical(&job.init_file), &job.init_alias, false);

    let mut steps = Vec::with_capacity(job.steps.len());
    for (index, step) in job.steps.iter().enumerate() {
        sink.phase(&format!("step-{index}"))?;
        steps.push(match step {
            Step::Demand {
                requests,
                concurrent,
            } => demand_step(&host, requests, *concurrent),
            Step::Edit { file, text } => {
                let id = canonical(file);
                let start = Instant::now();
                workspace.inject_file(id.clone(), Arc::from(text.as_str()));
                upsert(&host, id.clone(), text, language_of(&id))
                    .map_err(|err| JobError(format!("edit {file}: {err}")))?;
                StepRecord::Edit {
                    file: file.clone(),
                    text_sha256: sha256_hex(text.as_bytes()),
                    micros: micros(start),
                }
            }
            Step::Meta { file } => meta_step(&host, file),
        });
    }

    sink.phase("stats")?;
    let retention = retention(&host);
    let after_steps = process_stats::current_process()
        .map_err(|err| stats_errors.push(format!("after steps: {err}")))
        .ok();
    let result = SessionResult {
        schema: SESSION_RESULT_SCHEMA,
        tool: "verter",
        kind: "session",
        observability: job.observability,
        capture_available: capture_available(),
        stage: "complete",
        pid: std::process::id(),
        phases: SessionPhases {
            engine_start,
            setup,
            init: init.micros,
        },
        init_outcome: init.outcome,
        steps,
        after_steps,
        stats_errors,
        retention,
    };
    sink.record(&result)?;
    sink.phase("done")?;
    Ok(result)
}

/// Whether this binary has optional semantic capture compiled in.
pub fn capture_available() -> bool {
    verter_audit::observe::CaptureAvailability::compiled()
        == verter_audit::observe::CaptureAvailability::Available
}

/// Read a session job from `path`.
pub fn read_job(path: &Path) -> Result<SessionJob, String> {
    super::disk::read_to_string(path)
        .map_err(|err| err.to_string())
        .and_then(|text| serde_json::from_str(&text).map_err(|err| err.to_string()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}
