//! Execute a soak manifest against one production host, through its public
//! request routes, and write the run's summary.
//!
//! The host is built with the shipped [`HostConfig`] defaults (capture off);
//! only the component-meta isolation action also builds hosts with a tight
//! projection-operation allowance (see [`super::isolation`]).
//! Every action is recorded with its typed outcome and the digests of the
//! answers it observed; retention is sampled before the workload, every
//! [`SAMPLE_EVERY`] actions and after teardown. The runner judges nothing:
//! [`super::verify`] reads the written summary back and judges it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Barrier, Mutex};

use serde::{Deserialize, Serialize};
use verter_audit::soak::SoakCounts;
use verter_execution::cancellation::CancellationToken;
use verter_session::host_flow_return_audit::FlowReturnError;
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_session_query::retention::active_window::ActiveWindowReport;
use verter_type_engine::semantic_query::{
    BinderScopeId, ProjectionMode, ResolveDeclKey, ReturnProjectionDemand, ScopeId,
    SemanticQueryKey,
};
use verter_type_engine::semantic_query_memo::node_roots::NodeRoot;
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use super::fixtures;
use super::gates::{self, GateEvidence};
use super::isolation::{self, delivery_digest, IsolationEvidence};
use super::manifest::{
    big_file, canonical, churn_file, dep_file, module_file, surge_file, Action, ConfigVariant,
    LifecycleClass, Manifest, Mode, Target, PROJECT_ROOT,
};
use super::refusal::{self, RefusalEvidence};
use crate::semantic_perf::disk;

/// Retention is sampled at least this often (in actions).
pub const SAMPLE_EVERY: u32 = 25;
/// The summary file a run writes into its output directory.
pub const SUMMARY_FILE: &str = "soak-summary.json";
/// The summary schema this runner writes.
pub const SUMMARY_SCHEMA: u32 = 1;

/// A planted defect a negative control runs the soak with. Each must make
/// [`super::verify`] fail for its own reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "class")]
pub enum Mutation {
    /// Keep one held answer's root past teardown.
    RetainedKeyLeak,
    /// Drop every action of one class from the executed workload.
    OmitClass(LifecycleClass),
    /// Issue cancellation requests under a token nobody cancels.
    OmitCancellation,
    /// Keep an overlapping demand's callers concurrent but run their real
    /// demands one after another.
    FakeOverlap,
    /// Restore the alternate configuration instead of the original.
    WrongProjectReversion,
    /// Do not write the summary.
    MissingSummary,
    /// Skip one action without executing it.
    UnexpectedSkip,
    /// Publish unique-owner churn modules with bytes the manifest does not
    /// freeze.
    UnfrozenFixture,
}

/// Evidence that a mutation was applied: the actions it changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedMutation {
    pub mutation: Mutation,
    pub affected_actions: Vec<u32>,
}

/// What a run executes.
#[derive(Debug, Clone)]
pub struct RunOptions {
    pub mode: Mode,
    pub tranches: u32,
    pub out_dir: PathBuf,
    pub mutation: Option<Mutation>,
}

/// One action's typed outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum Outcome {
    /// A demand answered a value.
    Value,
    /// A well-formed demand that answered nothing.
    Miss,
    /// A typed request fault.
    Fault { detail: String },
    /// The request answered cancelled.
    Cancelled,
    /// A non-demand action completed.
    Done,
    /// The action did not run.
    Skipped { reason: String },
}

/// One executed action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRecord {
    pub index: u32,
    pub class: LifecycleClass,
    pub outcome: Outcome,
    /// Digests of the answers the action observed, in request order.
    pub digests: Vec<String>,
    /// For an overlapping demand: the most requests observed in flight at
    /// once (a request counts from just before it is issued until it
    /// returns).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_in_flight: Option<u32>,
    /// For an overlapping demand: joins onto an in-flight semantic
    /// computation the host recorded while the group ran (the witness that
    /// demands overlapped inside the host, not merely in their callers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joined_waits: Option<u64>,
    /// For an overlapping demand: what the flight rendezvous observed of the
    /// group's cold producers and of the subscribers registered on the held
    /// producer's flight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flight: Option<FlightEvidence>,
    /// For a cancellation: the uncancelled retry's outcome.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry: Option<Outcome>,
    /// For a surge: reservations the account refused for aggregate pressure
    /// while the surge ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusals_pressure: Option<u64>,
    /// For a surge: reservations the account refused as oversized while the
    /// surge ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusals_oversized: Option<u64>,
    /// For a component-meta isolation action: what it observed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<IsolationEvidence>,
    /// For a refusal or profile-healing action: what it observed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<RefusalEvidence>,
    /// For a publication-gate action: what the gate registry did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gates: Option<GateEvidence>,
    /// For a pressure action: what the account and memo did meanwhile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure: Option<PressureEvidence>,
    /// For a copy or release action: the ownership figures it observed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ownership: Vec<OwnerStep>,
    /// Digests of every source and configuration text the action published,
    /// in publication order (see [`publication_digest`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub published: Vec<String>,
}

impl ActionRecord {
    /// A record of a completed action that observed nothing yet.
    #[must_use]
    pub fn new(index: u32, class: LifecycleClass) -> Self {
        ActionRecord {
            index,
            class,
            outcome: Outcome::Done,
            digests: Vec::new(),
            max_in_flight: None,
            joined_waits: None,
            flight: None,
            retry: None,
            refusals_pressure: None,
            refusals_oversized: None,
            isolation: None,
            refusal: None,
            gates: None,
            pressure: None,
            ownership: Vec::new(),
            published: Vec::new(),
        }
    }
}

/// What a flight rendezvous observed of one overlap group.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlightEvidence {
    /// Joinable cold producers of the group's key that claimed a flight.
    pub producer_claims: u32,
    /// Subscribers that registered a wait on the held producer's flight.
    pub subscribers: u32,
}

/// Bounds every rendezvous wait: it only stops a broken schedule from
/// hanging the run, and never decides whether the work overlapped.
const HANG_BOUND: std::time::Duration = std::time::Duration::from_secs(30);

/// The memo key a public named-symbol demand of `alias` in `file` claims
/// first.
fn resolve_decl_key(file: &str, alias: &str) -> SemanticQueryKey {
    let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
    SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: verter_type_expr::OwnedStr::from(canonical(file).as_str()),
            owner,
            local_scope: None,
            binder_scope_id: BinderScopeId::file_scope(owner),
        },
        name: verter_type_expr::OwnedStr::from(alias),
    })
}

/// What a pressure action observed of the account and the memo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PressureEvidence {
    /// Retained memo candidates before the action ran.
    pub memo_candidates_before: u64,
    /// Retained memo candidates when it finished.
    pub memo_candidates_after: u64,
    /// Reservations refused because the active class was exhausted while
    /// the action ran.
    pub refusals_active: u64,
    /// For a surge: the component-meta results its host still retained once
    /// every requested result was delivered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_results: Option<u64>,
    /// For a surge: the ACTIVE-occupancy window that covered it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<ActiveEvidence>,
}

/// What an ACTIVE-occupancy window observed while one workload ran alone on
/// the process account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveEvidence {
    /// ACTIVE bytes charged when the window opened.
    pub active_at_open: u64,
    /// ACTIVE bytes charged when it finished.
    pub active_at_close: u64,
    /// The most ACTIVE bytes charged at once inside the window.
    pub peak_active_bytes: u64,
    /// ACTIVE transitions inside the window.
    pub transitions: u64,
    /// The peak, when the window attributes it to the workload alone.
    pub attributed_peak: Option<u64>,
    /// Why the window does not attribute its peak, when it does not.
    pub rejection: Option<String>,
}

impl ActiveEvidence {
    fn of(report: &ActiveWindowReport) -> Self {
        let attributed = report.attributed();
        ActiveEvidence {
            active_at_open: report.active_at_open as u64,
            active_at_close: report.active_at_close as u64,
            peak_active_bytes: report.peak_active_bytes as u64,
            transitions: report.transitions,
            attributed_peak: attributed
                .as_ref()
                .ok()
                .map(|peak| peak.peak_active_bytes as u64),
            rejection: attributed.err().map(|why| format!("{why:?}")),
        }
    }
}

/// A step of a held answer's ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OwnerStepKind {
    /// Before the public copy was made.
    BeforeCopy,
    /// Before either handle was released.
    BeforeRelease,
    /// After the answer's cache entries were retired, both handles live.
    AfterRetirement,
    /// After the public copy was made.
    AfterCopy,
    /// After the first handle was released.
    AfterFirstRelease,
    /// After the last handle was released.
    AfterFinalRelease,
}

/// The ownership figures at one step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerStep {
    pub step: OwnerStepKind,
    /// Independently acquired semantic roots live at this step.
    pub roots: u64,
    /// The process account's charges at this step, read after the
    /// surviving handle (if any) was projected.
    pub charges: Charges,
    /// The digest of the answer as projected through the handle still held
    /// at this step, when one is.
    pub projected: Option<String>,
}

/// One retention sample.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    /// `baseline`, `action` or `final`.
    pub at: String,
    /// Actions completed when it was read.
    pub actions_done: u32,
    pub counts: SoakCounts,
}

/// A run's summary, as written to [`SUMMARY_FILE`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoakSummary {
    pub schema: u32,
    pub mode: Mode,
    pub tranches: u32,
    pub manifest_digest: String,
    /// The sentinel's answer under the original configuration, at setup.
    pub sentinel_digest: String,
    /// Digests of the texts the setup published before the first action.
    pub setup_published: Vec<String>,
    pub records: Vec<ActionRecord>,
    pub samples: Vec<Sample>,
    pub mutation: Option<AppliedMutation>,
    /// The process's OS statistics after teardown (reported; judged only on
    /// the benchmark machine).
    pub process: Option<serde_json::Value>,
}

/// A run that could not start or finish.
#[derive(Debug)]
pub struct SoakError(pub String);

impl std::fmt::Display for SoakError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn language_of(canonical_id: &str) -> FileLanguage {
    verter_session::LanguageRegistry::global()
        .classify_static(canonical_id)
        .static_resolution()
}

/// The host's retention snapshot as a soak sample.
#[must_use]
pub fn counts(host: &VerterHost) -> SoakCounts {
    let s = host.retention_snapshot();
    let w = host.workspace_read().resource_snapshot();
    let storage = s.semantic_graph_storage_bytes + s.semantic_bookkeeping_storage_bytes;
    let n = |v: usize| v as u64;
    let d = host.scheduler().dependency_occupancy();
    SoakCounts {
        live_artifacts: n(s.live_artifacts),
        retired_versions: n(s.retained_retired_versions),
        live_roots: n(s.live_roots),
        snapshot_leases: n(s.snapshot_leases),
        semantic_nodes: n(s.semantic_nodes),
        semantic_roots: n(s.semantic_node_ownership.roots),
        semantic_internal_edges: n(s.semantic_node_ownership.internal_edges),
        memo_entries: n(s.semantic_memo_entries),
        memo_candidates: n(s.semantic_memo_candidates),
        union_views: n(s.union_views),
        shape_cache_entries: n(s.shape_cache_entries),
        pending_destroyed_nodes: n(s.pending_destroyed_nodes),
        resolved_import_facts: n(s.resolved_import_facts),
        component_meta_states: n(s.component_meta_states),
        registered_sources: n(s.registered_sources),
        overlay_resolution_slots: n(s.overlay_resolution_slots),
        overlay_value_versions: n(s.overlay_value_versions),
        flow_graphs: n(s.flow_graphs),
        flow_hash_entries: n(s.flow_hash_entries),
        flow_lowered_entries: n(s.flow_lowered_entries),
        carrier_candidates: n(s.carrier_candidates),
        publication_lanes: n(s.publication_lanes),
        unresolved_reach: n(s.unresolved_reach),
        stable_key_classes: n(s.stable_key_classes),
        identity_index_records: n(s.identity_indexes.records()),
        skeleton_names: n(s.skeleton_name_indexes.names),
        capture_frames: n(s.capture_summaries.frames),
        mapper_fingerprints: n(s.mapper_fingerprints),
        framework_surface_entries: n(s.framework_surface_entries),
        fallthrough_nodes: n(s.fallthrough_nodes),
        resolution_slots: n(w.resolution.slots),
        resolution_candidates: n(w.resolution.candidates),
        resolution_owners: n(w.resolution.owners),
        resolution_derived_nodes: n(w.resolution.derived_nodes),
        resolution_retired_decisions: n(w.resolution.retired_decisions),
        resolution_flights: n(w.resolution_flights),
        freshness_exact_entries: n(w.freshness_history.exact_entries),
        freshness_subtree_entries: n(w.freshness_history.subtree_entries),
        freshness_leased_entries: n(w.freshness_history.leased_entries),
        freshness_view_leases: n(w.freshness_history.view_leases),
        freshness_queued_entries: n(w.freshness_history.queued_entries),
        scheduler_dep_edges: n(d.dep_edges.edges),
        scheduler_gated_deps: n(d.dep_edges.gated_deps),
        scheduler_file_deps: n(d.dep_edges.file_deps),
        scheduler_blocker_refs: n(d.blocker_ref_entries),
        active_bytes: n(s.active_bytes),
        retained_bytes: n(s.retained_bytes),
        pinned_bytes: n(s.pinned_bytes),
        content_pinned_bytes: n(s.pinned_bytes.saturating_sub(storage)),
        storage_bytes: n(storage),
        peak_total_bytes: n(s.peak_total_bytes),
        refusals_pressure: s.refusals_pressure,
        refusals_oversized: s.refusals_oversized,
        refusals_active: s.refusals_active,
    }
}

struct Engine {
    mode: Mode,
    workspace: Arc<MemoryWorkspace>,
    host: Arc<VerterHost>,
    /// Held roots per pin slot: the original, then its public copy.
    pins: HashMap<u32, Vec<NodeRoot>>,
    leaked: Vec<NodeRoot>,
    /// Actions that kept a root past their release.
    leaked_at: Vec<u32>,
    /// Publications since the last action finished.
    publications: Mutex<Vec<String>>,
}

/// The process account's charges by class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Charges {
    pub active: u64,
    pub retained: u64,
    pub pinned: u64,
    /// Of the pinned bytes, the semantic stores' own backing storage.
    pub storage: u64,
}

impl Charges {
    /// Every charged byte.
    #[must_use]
    pub fn total(self) -> u64 {
        self.active + self.retained + self.pinned
    }
}

/// One ownership step: the held handle (if any) is projected first, so a
/// projection's own work lands before the charges and roots are read.
fn owner_step(host: &VerterHost, held: Option<&NodeRoot>) -> OwnerStep {
    let projected = held.map(|node| digest(host, Some(node)));
    let s = host.retention_snapshot();
    OwnerStep {
        step: OwnerStepKind::BeforeCopy,
        roots: s.semantic_node_ownership.roots as u64,
        charges: Charges {
            active: s.active_bytes as u64,
            retained: s.retained_bytes as u64,
            pinned: s.pinned_bytes as u64,
            storage: (s.semantic_graph_storage_bytes + s.semantic_bookkeeping_storage_bytes) as u64,
        },
        projected,
    }
}

/// A demand's outcome and, for a value, its root. An answer whose wire
/// form is a top-level `unknown` (the host's typed semantic-miss sentinel)
/// is a miss: the request resolved nothing.
fn demand(
    host: &VerterHost,
    file: &str,
    alias: &str,
    mode: Option<ProjectionMode>,
) -> (Outcome, Option<NodeRoot>) {
    let (outcome, _audit) = host
        .resolve_named_symbol_with_audit(&canonical(file), alias, mode)
        .into_parts();
    match outcome {
        Ok(Some(node)) if is_unknown_answer(host, &node) => (Outcome::Miss, None),
        Ok(Some(node)) => (Outcome::Value, Some(node)),
        Ok(None) => (Outcome::Miss, None),
        Err(fault) => (
            Outcome::Fault {
                detail: format!("{fault:?}"),
            },
            None,
        ),
    }
}

fn is_unknown_answer(host: &VerterHost, node: &NodeRoot) -> bool {
    host.project_node_to_type_expr_json_bytes(node)
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|wire| wire["kind"] == "unknown")
}

/// The digest of an answer's wire bytes, or of why it has none.
fn digest(host: &VerterHost, node: Option<&NodeRoot>) -> String {
    let bytes = match node.and_then(|n| host.project_node_to_type_expr_json_bytes(n)) {
        Some(bytes) => bytes,
        None => return "unmaterialised".into(),
    };
    super::manifest::sha256_hex(&bytes)
}

impl Engine {
    fn start(mode: Mode) -> Self {
        let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
        let access: Arc<dyn WorkspaceAccess> = workspace.clone();
        let host = Arc::new(VerterHost::new(HostConfig::default(), access));
        let engine = Engine {
            mode,
            workspace,
            host,
            pins: HashMap::new(),
            leaked: Vec::new(),
            leaked_at: Vec::new(),
            publications: Mutex::new(Vec::new()),
        };
        engine.configure(ConfigVariant::Original);
        engine
    }

    fn note_publication(&self, path: &str, text: &str) {
        self.publications
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(publication_digest(path, text));
    }

    fn take_publications(&self) -> Vec<String> {
        std::mem::take(&mut *self.publications.lock().unwrap_or_else(|p| p.into_inner()))
    }

    fn configure(&self, variant: ConfigVariant) {
        let path = canonical("tsconfig.json");
        let text = fixtures::tsconfig(variant);
        self.note_publication(&path, &text);
        self.workspace
            .inject_file(path.clone(), Arc::from(text.as_str()));
        let project = verter_workspace::configured_ide_project(
            self.workspace.as_ref(),
            PROJECT_ROOT.to_string(),
            PROJECT_ROOT.to_string(),
            &path,
        );
        self.host.configure_projects(vec![project]);
    }

    fn upsert(&self, file: &str, text: &str) -> Outcome {
        let id = canonical(file);
        self.note_publication(&id, text);
        match self.host.upsert(UpsertRequest {
            canonical_id: Some(id.clone()),
            input_id: id.clone(),
            source: Arc::from(text),
            file_language: language_of(&id),
            aliases: Vec::new(),
        }) {
            Ok(_) => Outcome::Done,
            Err(err) => Outcome::Fault {
                detail: format!("upsert {file}: {err:?}"),
            },
        }
    }

    /// Close a module as an editor does (release its document state), then
    /// delete it.
    fn remove(&self, file: &str) -> Outcome {
        let id = canonical(file);
        self.host.evict(&id);
        match self.host.remove(&id) {
            Some(_) => Outcome::Done,
            None => Outcome::Fault {
                detail: format!("remove {file}: not open"),
            },
        }
    }

    fn source_of(&self, file: &str, tranche: u32, revision: u32) -> String {
        if file == dep_file(tranche) {
            fixtures::dep_source(tranche)
        } else if file == big_file(tranche) {
            fixtures::big_source(tranche)
        } else {
            fixtures::module_source(self.mode, tranche, revision)
        }
    }

    fn sentinel(&self) -> (Outcome, String) {
        let (outcome, node) = demand(
            &self.host,
            fixtures::SENTINEL_FILE,
            fixtures::SENTINEL_ALIAS,
            None,
        );
        (outcome, digest(&self.host, node.as_ref()))
    }

    fn open_sentinel(&self) -> Result<String, SoakError> {
        for (file, text) in fixtures::PICK_FILES {
            if let Outcome::Fault { detail } = self.upsert(file, text) {
                return Err(SoakError(detail));
            }
        }
        if let Outcome::Fault { detail } =
            self.upsert(fixtures::SENTINEL_FILE, &fixtures::sentinel_source())
        {
            return Err(SoakError(detail));
        }
        match self.sentinel() {
            (Outcome::Value, digest) => Ok(digest),
            (other, _) => Err(SoakError(format!(
                "the configuration sentinel does not answer: {other:?}"
            ))),
        }
    }

    fn close_sentinel(&self) {
        let _ = self.remove(fixtures::SENTINEL_FILE);
        for (file, _) in fixtures::PICK_FILES {
            let _ = self.remove(file);
        }
    }

    /// One initially cold overlap group: `threads` ordinary demands of one
    /// identity. A flight rendezvous armed on the identity's first cold key
    /// holds the first request after it claims its flight; the others are
    /// issued while it is held, and the producer is released only once a
    /// subscriber registered on that flight. The fake-overlap control keeps
    /// every caller live but releases the producer and waits for its answer
    /// before the others issue their requests.
    fn overlap(&self, file: &str, alias: &str, threads: u32, fake: bool) -> ActionRecord {
        let graph = self.host.project_type_store().semantic_graph();
        let in_flight = AtomicU32::new(0);
        let max_in_flight = AtomicU32::new(0);
        let joins_before = graph.stats_snapshot().joined_waits;
        let issue = || {
            let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            max_in_flight.fetch_max(now, Ordering::SeqCst);
            let (outcome, node) = demand(&self.host, file, alias, None);
            in_flight.fetch_sub(1, Ordering::SeqCst);
            (outcome, digest(&self.host, node.as_ref()))
        };
        let (go, start) = std::sync::mpsc::channel::<()>();
        let start = Mutex::new(start);
        // The control's callers are all live (counted) before the producer
        // is released.
        let counted = Barrier::new(threads as usize);
        let (answers, observed, schedule) = std::thread::scope(|scope| {
            let rendezvous = graph.arm_flight_rendezvous(resolve_decl_key(file, alias));
            let producer = scope.spawn(issue);
            let mut schedule = Ok(());
            if rendezvous.wait_producer_parked(HANG_BOUND).is_err() {
                schedule = Err("the first request never parked at its cold build".to_string());
            }
            let subscribers: Vec<_> = (1..threads)
                .map(|_| {
                    let start = &start;
                    let counted = &counted;
                    let issue = &issue;
                    let count = &in_flight;
                    let max = &max_in_flight;
                    scope.spawn(move || {
                        if fake {
                            // Counted live as a caller, held back from the
                            // host until the producer answered.
                            let now = count.fetch_add(1, Ordering::SeqCst) + 1;
                            max.fetch_max(now, Ordering::SeqCst);
                            counted.wait();
                            let _ = start.lock().expect("the overlap start gate").recv();
                            count.fetch_sub(1, Ordering::SeqCst);
                        }
                        issue()
                    })
                })
                .collect();
            let mut answers = Vec::with_capacity(threads as usize);
            if fake {
                counted.wait();
                rendezvous.release();
                answers.push(producer.join().expect("a demand thread panicked"));
                for _ in 1..threads {
                    let _ = go.send(());
                }
            } else {
                if schedule.is_ok() && rendezvous.wait_subscriber_registered(HANG_BOUND).is_err() {
                    schedule = Err("no request subscribed to the held flight".to_string());
                }
                rendezvous.release();
                answers.push(producer.join().expect("a demand thread panicked"));
            }
            answers.extend(
                subscribers
                    .into_iter()
                    .map(|h| h.join().expect("a demand thread panicked")),
            );
            (answers, rendezvous.observe(), schedule)
        });
        let joined_waits = graph
            .stats_snapshot()
            .joined_waits
            .saturating_sub(joins_before);
        let outcome = match schedule {
            Err(detail) => Outcome::Fault { detail },
            Ok(()) => answers
                .iter()
                .map(|(o, _)| o.clone())
                .find(|o| *o != Outcome::Value)
                .unwrap_or(Outcome::Value),
        };
        ActionRecord {
            outcome,
            digests: answers.into_iter().map(|(_, d)| d).collect(),
            max_in_flight: Some(max_in_flight.load(Ordering::SeqCst)),
            joined_waits: Some(joined_waits),
            flight: Some(FlightEvidence {
                producer_claims: observed.producer_claims as u32,
                subscribers: observed.subscribers as u32,
            }),
            ..ActionRecord::new(0, LifecycleClass::OverlappingCold)
        }
    }

    fn concurrent(&self, queries: &[(String, String)]) -> (Outcome, Vec<String>) {
        let answers: Vec<(Outcome, String)> = std::thread::scope(|scope| {
            let handles: Vec<_> = queries
                .iter()
                .map(|(file, alias)| {
                    scope.spawn(move || {
                        let (outcome, node) = demand(&self.host, file, alias, None);
                        (outcome, digest(&self.host, node.as_ref()))
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("a demand thread panicked"))
                .collect()
        });
        let outcome = answers
            .iter()
            .map(|(o, _)| o.clone())
            .find(|o| *o != Outcome::Value)
            .unwrap_or(Outcome::Value);
        (outcome, answers.into_iter().map(|(_, d)| d).collect())
    }

    fn cancellation(&self, file: &str, symbol: &str, cancel: bool) -> (Outcome, Outcome) {
        let identity = FlowFunctionReturnIdentity {
            anchor: AuthoredAnchor {
                canonical_id: canonical(file).as_str().into(),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                symbol: symbol.into(),
                space: LocatorSymbolSpace::Value,
            },
            function_part: FunctionPartIdentity::DeclarationBody,
            overload_ordinal: 0,
        };
        let classify = |result: Result<_, &FlowReturnError>| match result {
            Ok(_) => Outcome::Value,
            Err(FlowReturnError::Cancelled) => Outcome::Cancelled,
            Err(other) => Outcome::Fault {
                detail: format!("{other:?}"),
            },
        };
        let token = CancellationToken::new();
        if cancel {
            token.cancel();
        }
        let whole = ReturnProjectionDemand::whole_return;
        let first =
            self.host
                .get_flow_return_type_with_audit_cancellable(&identity, whole(), token);
        let first = classify(first.as_result());
        let retry = self.host.get_flow_return_type_with_audit_cancellable(
            &identity,
            whole(),
            CancellationToken::new(),
        );
        (first, classify(retry.as_result()))
    }

    /// Demand the component-meta result of every surge component on a fresh
    /// host while all of them stay open, so their retained results are live
    /// together in the process-wide account, then drop the host. The refusal
    /// deltas are read while every result is still retained. A refused
    /// retention still delivers a complete result.
    fn surge(&self, tranche: u32, shape: (u32, u32, u32), record: &mut ActionRecord) {
        let (modules, props, oversized_props) = shape;
        let workspace: Arc<dyn WorkspaceAccess> =
            Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
        let host = VerterHost::new(HostConfig::default(), workspace);
        let before = host.retention_snapshot();
        // The ACTIVE witness covers the surge alone: the soak's own host is
        // idle while it runs, so detectable foreign work rejects it.
        let owner = host.mint_active_occupancy_owner();
        let window = host.open_active_occupancy_window(&owner);
        record.outcome = Outcome::Value;
        for ordinal in 0..modules {
            let file = surge_file(tranche, ordinal);
            let id = canonical(&file);
            let count = if ordinal == 0 { oversized_props } else { props };
            let text = fixtures::surge_source(tranche, ordinal, count);
            record.published.push(publication_digest(&id, &text));
            if let Err(err) = host.upsert(UpsertRequest {
                canonical_id: Some(id.clone()),
                input_id: id.clone(),
                source: Arc::from(text.as_str()),
                file_language: language_of(&id),
                aliases: Vec::new(),
            }) {
                record.outcome = Outcome::Fault {
                    detail: format!("upsert {file}: {err:?}"),
                };
                break;
            }
            let delivery = host.get_component_meta(&id);
            record.digests.push(match delivery.as_ref() {
                Some(delivery) => delivery_digest(delivery),
                None => "absent".into(),
            });
            if !delivery.is_some_and(|d| d.is_complete()) {
                record.outcome = Outcome::Miss;
                break;
            }
        }
        // Read while every delivered result the account admitted is still
        // retained.
        let after = host.retention_snapshot();
        let retained_results = host.project_type_store().component_meta_results().len() as u64;
        let report = window.finish();
        record.refusals_pressure = Some(
            after
                .refusals_pressure
                .saturating_sub(before.refusals_pressure),
        );
        record.refusals_oversized = Some(
            after
                .refusals_oversized
                .saturating_sub(before.refusals_oversized),
        );
        record.pressure = Some(PressureEvidence {
            memo_candidates_before: before.semantic_memo_candidates as u64,
            memo_candidates_after: after.semantic_memo_candidates as u64,
            refusals_active: after.refusals_active.saturating_sub(before.refusals_active),
            retained_results: Some(retained_results),
            active: Some(ActiveEvidence::of(&report)),
        });
        host.close();
        // The surge host's parse releases complete before it is dropped, so
        // none of its pins is still pending when the soak host is sampled.
        if let Err(err) = host.release_completion().wait() {
            record.outcome = Outcome::Fault {
                detail: format!("surge release completion: {err:?}"),
            };
        }
    }

    fn execute(&mut self, action: &Action, mutation: Option<Mutation>) -> ActionRecord {
        use LifecycleClass::*;
        let mut record = ActionRecord::new(action.index, action.class);
        let host = Arc::clone(&self.host);
        let observe = |record: &mut ActionRecord, (outcome, node): (Outcome, Option<NodeRoot>)| {
            record.digests.push(digest(&host, node.as_ref()));
            record.outcome = outcome;
            node
        };
        match (&action.class, &action.target) {
            (Open | Edit, Target::File { file, revision }) => {
                let text = self.source_of(file, action.tranche, *revision);
                record.outcome = self.upsert(file, &text);
            }
            (RevertSource, Target::File { file, revision }) => {
                let text = self.source_of(file, action.tranche, *revision);
                record.outcome = self.upsert(file, &text);
                if record.outcome == Outcome::Done {
                    observe(&mut record, demand(&self.host, file, "A0", None));
                }
            }
            (Close, Target::File { file, .. }) => record.outcome = self.remove(file),
            (
                ColdDemand | WarmDemand | PostEditDemand | LargeDemand,
                Target::Query { file, alias },
            ) => {
                observe(&mut record, demand(&self.host, file, alias, None));
            }
            (
                RefusalRepeat | ProfileHealing,
                Target::Refusal {
                    variant,
                    op_budget,
                    props,
                },
            ) => {
                if action.class == ProfileHealing {
                    // The narrower named-symbol profile, healed by the
                    // default one.
                    let file = module_file(action.tranche);
                    observe(
                        &mut record,
                        demand(&self.host, &file, "A12", Some(ProjectionMode::Navigate)),
                    );
                    observe(&mut record, demand(&self.host, &file, "A12", None));
                }
                let (evidence, published) =
                    refusal::run(*variant, *op_budget, action.tranche, *props);
                record.refusal = Some(evidence);
                record.published = published;
            }
            (
                OverlappingCold,
                Target::Overlap {
                    file,
                    alias,
                    threads,
                },
            ) => {
                let fake = mutation == Some(Mutation::FakeOverlap);
                let overlapped = self.overlap(file, alias, *threads, fake);
                record = ActionRecord {
                    index: action.index,
                    ..overlapped
                };
            }
            (MixedConcurrent, Target::Queries { queries }) => {
                let (outcome, digests) = self.concurrent(queries);
                record.outcome = outcome;
                record.digests = digests;
            }
            (HoldPin, Target::Hold { slot, file, alias }) => {
                let node = observe(&mut record, demand(&self.host, file, alias, None));
                self.pins.insert(*slot, node.into_iter().collect());
            }
            (PublicCopy, Target::Slot { slot }) => match self.pins.get_mut(slot) {
                Some(handles) if handles.len() == 1 => {
                    record.ownership.push(OwnerStep {
                        step: OwnerStepKind::BeforeCopy,
                        ..owner_step(&host, handles.first())
                    });
                    let copy = handles[0].clone();
                    record.ownership.push(OwnerStep {
                        step: OwnerStepKind::AfterCopy,
                        ..owner_step(&host, Some(&copy))
                    });
                    handles.push(copy);
                }
                _ => {
                    record.outcome = Outcome::Fault {
                        detail: format!("pin slot {slot} holds no answer"),
                    }
                }
            },
            (
                ReleaseCopyThenOriginal | ReleaseOriginalThenCopy,
                Target::Release {
                    slot,
                    file,
                    revision,
                },
            ) => match self.pins.remove(slot) {
                Some(mut handles) if handles.len() == 2 => {
                    if mutation == Some(Mutation::RetainedKeyLeak) && self.leaked.is_empty() {
                        self.leaked.push(handles[0].clone());
                        self.leaked_at.push(action.index);
                    }
                    record.ownership.push(OwnerStep {
                        step: OwnerStepKind::BeforeRelease,
                        ..owner_step(&host, handles.first())
                    });
                    // Retire the cache entries the answer came from while
                    // both handles still read it: the document closes as an
                    // editor closes it, and the store reclaims what it
                    // retired.
                    self.host.evict(&canonical(file));
                    self.host.reclaim_retired_artifacts();
                    record.ownership.push(OwnerStep {
                        step: OwnerStepKind::AfterRetirement,
                        ..owner_step(&host, handles.first())
                    });
                    // The copy goes first, or the original does: the other
                    // is the final reader.
                    let first = usize::from(action.class == ReleaseCopyThenOriginal);
                    drop(handles.remove(first));
                    record.ownership.push(OwnerStep {
                        step: OwnerStepKind::AfterFirstRelease,
                        ..owner_step(&host, handles.first())
                    });
                    drop(handles);
                    record.ownership.push(OwnerStep {
                        step: OwnerStepKind::AfterFinalRelease,
                        ..owner_step(&host, None)
                    });
                    // The document reopens for the actions that follow.
                    let text = self.source_of(file, action.tranche, *revision);
                    record.outcome = self.upsert(file, &text);
                }
                _ => {
                    record.outcome = Outcome::Fault {
                        detail: format!("pin slot {slot} holds no answer and copy"),
                    }
                }
            },
            (Cancellation, Target::Function { file, symbol }) => {
                let cancel = mutation != Some(Mutation::OmitCancellation);
                let (first, retry) = self.cancellation(file, symbol, cancel);
                record.outcome = first;
                record.retry = Some(retry);
            }
            (ConfigChange | ConfigRestore, Target::Config { variant }) => {
                let variant = if action.class == ConfigRestore
                    && mutation == Some(Mutation::WrongProjectReversion)
                {
                    ConfigVariant::Alternate
                } else {
                    *variant
                };
                self.configure(variant);
                let (outcome, digest) = self.sentinel();
                record.outcome = outcome;
                record.digests.push(digest);
            }
            (UniqueOwnerChurn, Target::Query { file, alias }) => {
                let ordinal = (0..)
                    .find(|k| churn_file(action.tranche, *k) == *file)
                    .expect("a churn module of the tranche");
                let mut text = fixtures::churn_source(action.tranche, ordinal);
                if mutation == Some(Mutation::UnfrozenFixture) {
                    text.push_str(
                        "// unfrozen
",
                    );
                }
                record.outcome = self.upsert(file, &text);
                if record.outcome == Outcome::Done {
                    observe(&mut record, demand(&self.host, file, alias, None));
                    if let Outcome::Fault { detail } = self.remove(file) {
                        record.outcome = Outcome::Fault { detail };
                    }
                }
            }
            (CandidateChurn, Target::Churn { file, alias, edits }) => {
                let before = host.retention_snapshot();
                'churn: {
                    for edit in 0..*edits {
                        let text = fixtures::module_source(self.mode, action.tranche, 100 + edit);
                        if let Outcome::Fault { detail } = self.upsert(file, &text) {
                            record.outcome = Outcome::Fault { detail };
                            break 'churn;
                        }
                        observe(&mut record, demand(&self.host, file, alias, None));
                        if record.outcome != Outcome::Value {
                            break 'churn;
                        }
                    }
                    let text = fixtures::module_source(self.mode, action.tranche, 0);
                    record.outcome = self.upsert(file, &text);
                    if record.outcome == Outcome::Done {
                        observe(&mut record, demand(&self.host, file, alias, None));
                    }
                }
                let after = host.retention_snapshot();
                record.pressure = Some(PressureEvidence {
                    memo_candidates_before: before.semantic_memo_candidates as u64,
                    memo_candidates_after: after.semantic_memo_candidates as u64,
                    refusals_active: after.refusals_active.saturating_sub(before.refusals_active),
                    retained_results: None,
                    active: None,
                });
            }
            (PublicationGates, Target::Gates { sources }) => {
                record.gates = Some(gates::run(action.tranche, *sources));
            }
            (
                MetaIsolation,
                Target::Isolation {
                    op_budget,
                    spend_props,
                    refuse_props,
                },
            ) => {
                let (evidence, published) =
                    isolation::run(*op_budget, action.tranche, *spend_props, *refuse_props);
                record.isolation = Some(evidence);
                record.published = published;
                record.outcome = Outcome::Done;
            }
            (
                PressureSurge,
                Target::Surge {
                    modules,
                    props,
                    oversized_props,
                },
            ) => {
                self.surge(
                    action.tranche,
                    (*modules, *props, *oversized_props),
                    &mut record,
                );
            }
            (Reclaim, Target::Store) => {
                self.host.reclaim_retired_artifacts();
            }
            (class, target) => {
                record.outcome = Outcome::Fault {
                    detail: format!("class {class:?} has no route for target {target:?}"),
                }
            }
        }
        let mut published = self.take_publications();
        published.append(&mut record.published);
        record.published = published;
        record
    }

    /// Release every pin, close the configuration modules, tear the host's
    /// state down through its close route and wait on the host's release
    /// boundary, so the closed sample reads only what a live owner still
    /// holds. Returns the roots a planted leak keeps.
    fn teardown(&mut self) -> Result<Vec<NodeRoot>, SoakError> {
        self.pins.clear();
        self.close_sentinel();
        self.host.close();
        self.host
            .release_completion()
            .wait()
            .map_err(|err| SoakError(format!("release completion after close: {err:?}")))?;
        Ok(std::mem::take(&mut self.leaked))
    }
}

/// The actions a mutation leaves to execute, and the actions it changed.
fn planned(manifest: &Manifest, mutation: Option<Mutation>) -> (Vec<Action>, Vec<u32>) {
    let affected_by = |class: LifecycleClass| {
        manifest
            .actions
            .iter()
            .filter(|a| a.class == class)
            .map(|a| a.index)
            .collect::<Vec<_>>()
    };
    match mutation {
        Some(Mutation::OmitClass(class)) => (
            manifest
                .actions
                .iter()
                .filter(|a| a.class != class)
                .cloned()
                .collect(),
            affected_by(class),
        ),
        Some(Mutation::OmitCancellation) => (
            manifest.actions.clone(),
            affected_by(LifecycleClass::Cancellation),
        ),
        Some(Mutation::FakeOverlap) => (
            manifest.actions.clone(),
            affected_by(LifecycleClass::OverlappingCold),
        ),
        Some(Mutation::WrongProjectReversion) => (
            manifest.actions.clone(),
            affected_by(LifecycleClass::ConfigRestore),
        ),
        Some(Mutation::UnexpectedSkip) => (
            manifest.actions.clone(),
            affected_by(LifecycleClass::WarmDemand)
                .into_iter()
                .take(1)
                .collect(),
        ),
        Some(Mutation::UnfrozenFixture) => (
            manifest.actions.clone(),
            affected_by(LifecycleClass::UniqueOwnerChurn),
        ),
        Some(Mutation::RetainedKeyLeak) => (manifest.actions.clone(), Vec::new()),
        Some(Mutation::MissingSummary) | None => (manifest.actions.clone(), Vec::new()),
    }
}

/// Run the soak: execute the manifest of `options`, sample retention and
/// write the summary into `options.out_dir`. Returns the summary.
pub fn run(options: &RunOptions) -> Result<SoakSummary, SoakError> {
    let manifest = Manifest::generate(options.mode, options.tranches);
    let (actions, affected) = planned(&manifest, options.mutation);
    let mut engine = Engine::start(options.mode);
    let mut samples = vec![Sample {
        at: "baseline".into(),
        actions_done: 0,
        counts: counts(&engine.host),
    }];
    let sentinel_digest = engine.open_sentinel()?;
    let setup_published = engine.take_publications();
    let mut records = Vec::with_capacity(actions.len());
    for (done, action) in actions.iter().enumerate() {
        let record = if options.mutation == Some(Mutation::UnexpectedSkip)
            && affected.first() == Some(&action.index)
        {
            ActionRecord {
                outcome: Outcome::Skipped {
                    reason: "planted skip".into(),
                },
                ..ActionRecord::new(action.index, action.class)
            }
        } else {
            engine.execute(action, options.mutation)
        };
        records.push(record);
        let done = done as u32 + 1;
        if done.is_multiple_of(SAMPLE_EVERY) {
            samples.push(Sample {
                at: "action".into(),
                actions_done: done,
                counts: counts(&engine.host),
            });
        }
    }
    let leaked = engine.teardown()?;
    let affected = if options.mutation == Some(Mutation::RetainedKeyLeak) {
        std::mem::take(&mut engine.leaked_at)
    } else {
        affected
    };
    samples.push(Sample {
        at: "closed".into(),
        actions_done: records.len() as u32,
        counts: counts(&engine.host),
    });
    // Teardown ends with the host dropped. Its own figures go with it; the
    // process-wide retention account outlives it, and a fresh empty host
    // reads it. A root the runner still holds keeps its charges alive.
    drop(engine);
    let probe = Engine::start(options.mode);
    samples.push(Sample {
        at: "final".into(),
        actions_done: records.len() as u32,
        counts: counts(&probe.host),
    });
    drop(probe);
    drop(leaked);
    let summary = SoakSummary {
        schema: SUMMARY_SCHEMA,
        mode: options.mode,
        tranches: options.tranches,
        manifest_digest: manifest.digest(),
        sentinel_digest,
        setup_published,
        records,
        samples,
        mutation: options.mutation.map(|mutation| AppliedMutation {
            mutation,
            affected_actions: affected,
        }),
        process: crate::semantic_perf::process_stats::current_process()
            .ok()
            .and_then(|stats| serde_json::to_value(stats).ok()),
    };
    if options.mutation != Some(Mutation::MissingSummary) {
        write_summary(&options.out_dir, &summary)?;
    }
    Ok(summary)
}

fn write_summary(dir: &Path, summary: &SoakSummary) -> Result<(), SoakError> {
    disk::create_dir_all(dir)
        .map_err(|err| SoakError(format!("create {}: {err}", dir.display())))?;
    let path = dir.join(SUMMARY_FILE);
    let text = serde_json::to_string_pretty(summary).expect("a summary serialises");
    disk::write(&path, &text).map_err(|err| SoakError(format!("write {}: {err}", path.display())))
}

/// The digest one publication of `text` at `path` contributes: the bytes the
/// runner handed the host, bound to where it published them.
#[must_use]
pub fn publication_digest(path: &str, text: &str) -> String {
    let mut bytes = Vec::with_capacity(path.len() + text.len() + 1);
    bytes.extend_from_slice(path.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(text.as_bytes());
    super::manifest::sha256_hex(&bytes)
}
