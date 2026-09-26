//! Signature-kernel performance harness — the §12 work-accounting and
//! performance gates of `docs/arch/signature-kernel.md`.
//!
//! One synthesized, in-process corpus — order-heavy generic unions, long
//! descriptor-instantiation chains, callable and construct intersections,
//! and wrapper utilities — driven through the PUBLIC host API only. Nothing
//! here names a kernel internal, so the SAME source builds against the
//! pre-kernel baseline tree and the candidate tree, and the two arms differ
//! in nothing but the implementation under test.
//!
//! It records SAMPLES, never verdicts. One JSON document goes to stdout:
//! per-workload latency samples, per-workload allocation totals, the three
//! scalability benchmarks below, the edit/revert soak's live-heap series,
//! the completion census of every witness, and — untimed — every witness's
//! OUTCOME (completion, typed degradation or refusal, and the answered type
//! rendered structurally) in the original corpus, in each edited state an
//! edit workload reaches, and in the check corpus the scaling benchmarks
//! run. The runner reports a ratio for a workload only when both arms'
//! outcomes agree on every witness it queries. Statistics, the ABBA
//! interleaving, the control benchmark and the regression gate live in the
//! runner, `scripts/benchmark/signature-kernel-perf.mjs`.
//!
//! ```text
//! # full run (the runner's defaults)
//! cargo run --release -p verter_session --example signature_kernel_bench
//! # quick run (the runner's --quick corpus)
//! cargo run --release -p verter_session --example signature_kernel_bench -- \
//!     --modules 4 --depth 4 --samples 6 --cold-samples 3 --soak 20
//!
//! cargo run --release -p verter_session --example signature_kernel_bench -- \
//!     [--modules N] [--depth N] [--samples N] [--cold-samples N] [--soak N]
//!     [--exclude Kind,Kind] [--host-workers N] [--check-modules N]
//! ```
//!
//! Workloads, each a distribution:
//!
//! * `cold_load`         — fresh host, upsert the corpus, first query of every witness.
//! * `warm_query`        — warm-host queries, timed in batches of full sweeps, per query.
//! * `local_edit`        — edit one module's body, re-query that module.
//! * `declaration_edit`  — edit the shared declarations every module imports, re-query all.
//! * `augmentation_edit` — edit a `declare global` augmentation, re-query its readers.
//! * `restart`           — tear a warm host down and rebuild it on the same content.
//! * `soak`              — edit/revert one module repeatedly; live heap after each round.
//!
//! Scalability, each its own section, each point a distribution. Caller
//! threads are created once per point, outside timing, and every sample
//! is released by a start barrier and timed from the first caller's start
//! to the last caller's finish, each read by the caller itself:
//!
//! * `concurrent_queries` — ONE warm host with a fixed `--host-workers`
//!   scheduler CPU workers (default 4), queried by 1/2/4/8 callers at once,
//!   each running the same full sweeps; queries per second and per-query
//!   latency percentiles.
//! * `scheduler_scaling`  — ONE caller, hosts with 1/2/4/8 scheduler CPU
//!   workers; a fresh host loaded untimed per sample, the cold first query
//!   of every witness of the check corpus timed; wall time and speedup
//!   against one worker.
//! * `full_check`         — hosts with 1/2/4/8 scheduler CPU workers and as
//!   many callers, each loading and querying its share of the check
//!   corpus's files on a fresh host; files per second, wall time and CPU
//!   utilisation, `process CPU time / (wall time × workers)`.
//!
//! The check corpus is the same module shape, `--check-modules` modules
//! (default four times `--modules`).
//!
//! Cancellation is NOT a workload here: the baseline tree has no
//! caller-cancellable entry, so no matched workload exists. The candidate-only
//! `signature_kernel_cancel_probe` records cancellation/restart latency, and the
//! runner runs it inside the same session.
//!
//! Allocation is counted by a process-wide counting allocator that is OFF
//! during timing passes (relaxed loads only) and ON for a separate
//! accounting pass, so the counter never inflates a timing sample. Live
//! bytes are tracked only for the soak, from zero before its host exists.

use std::alloc::{GlobalAlloc, Layout, System};
use std::any::Any;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::Instant;

use verter_scheduler::scheduler::SchedulerConfig;
use verter_session::semantic_query::{ReturnProjectionDemand, SemanticNodeData, SemanticNodeId};
use verter_session::{HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity, TopLevelOwnerId};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};

// ─────────────────────────────────────────────────────────────────────────
// Counting allocator
// ─────────────────────────────────────────────────────────────────────────

struct Counting;

static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);
/// Whether live bytes are being tracked: ON only for the soak, so a timing
/// pass pays one relaxed load per allocation and free, never a shared
/// read-modify-write.
static TRACKING_LIVE: AtomicBool = AtomicBool::new(false);
/// Net bytes allocated since tracking started.
static LIVE_BYTES: AtomicI64 = AtomicI64::new(0);

// SAFETY: every method forwards to `System` unchanged; the counters are
// side effects that never touch the returned memory.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            note_alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        if TRACKING_LIVE.load(Ordering::Relaxed) {
            LIVE_BYTES.fetch_sub(layout.size() as i64, Ordering::Relaxed);
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            note_alloc(layout.size());
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let grown = unsafe { System.realloc(ptr, layout, new_size) };
        if !grown.is_null() {
            if TRACKING_LIVE.load(Ordering::Relaxed) {
                LIVE_BYTES.fetch_add(new_size as i64 - layout.size() as i64, Ordering::Relaxed);
            }
            if COUNTING.load(Ordering::Relaxed) {
                ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
                ALLOC_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
            }
        }
        grown
    }
}

/// Live bytes are tracked only during the soak; the allocation totals only
/// while an accounting pass is running.
fn note_alloc(size: usize) {
    if TRACKING_LIVE.load(Ordering::Relaxed) {
        LIVE_BYTES.fetch_add(size as i64, Ordering::Relaxed);
    }
    if COUNTING.load(Ordering::Relaxed) {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(size as u64, Ordering::Relaxed);
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Run `work` once with the allocation totals counting, and return
/// `(allocations, bytes)` it performed.
fn account(work: impl FnOnce()) -> (u64, u64) {
    let count_before = ALLOC_COUNT.load(Ordering::Relaxed);
    let bytes_before = ALLOC_BYTES.load(Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    work();
    COUNTING.store(false, Ordering::Relaxed);
    (
        ALLOC_COUNT.load(Ordering::Relaxed) - count_before,
        ALLOC_BYTES.load(Ordering::Relaxed) - bytes_before,
    )
}

// ─────────────────────────────────────────────────────────────────────────
// Corpus
// ─────────────────────────────────────────────────────────────────────────

const SHARED: &str = "/sk/shared.ts";
const AUGMENTATION: &str = "/sk/augment.ts";
const WITNESSES: [&str; 6] = ["Chain", "Union", "Call", "New", "Await", "Global"];

struct Corpus {
    modules: usize,
    depth: usize,
    /// The witness kinds QUERIED (every kind is still written into each
    /// module, so the program under test never changes). Excluding a kind
    /// the arms answer differently makes every timing a matched workload.
    kinds: Vec<&'static str>,
}

impl Corpus {
    fn module_path(i: usize) -> String {
        format!("/sk/m{i}.ts")
    }

    /// The declarations every module imports. `variant` toggles one member,
    /// so a declaration edit changes a dependency of every module.
    fn shared(variant: bool) -> String {
        let extra = if variant { "  extra?: 1;\n" } else { "" };
        format!(
            "export interface Box<T> {{\n  value: T;\n{extra}}}\n\
             export interface Pair<A, B> {{ left: A; right: B }}\n\
             export type Opt<T> = T | undefined;\n"
        )
    }

    /// A global augmentation the `Global` witness reads through.
    fn augmentation(variant: bool) -> String {
        let tag = if variant { "number" } else { "string" };
        format!("declare global {{ interface SkGlobal {{ tag: {tag} }} }}\nexport {{}};\n")
    }

    /// One module: an order-heavy generic union with a duplicate arm, a
    /// descriptor-instantiation chain `depth` generic calls long, a call and
    /// a construction over intersections of signatures that differ only in
    /// their result, an awaited optional, and a global read. `variant`
    /// toggles a literal in the chain head (a local body edit).
    fn module(&self, i: usize, variant: bool) -> String {
        let mut source = String::new();
        source.push_str("import { Box, Pair, Opt } from \"./shared\";\n");
        source.push_str(&format!(
            "export type U{i}<T> = Box<T> | Pair<T, T> | \"l{i}\" | \"r{i}\" | T | Box<T>;\n"
        ));
        let marker = if variant { "edited" } else { "original" };
        source.push_str(&format!(
            "export function c0_{i}<T>(x: T) {{ return {{ v: x, tag: \"{marker}\" as const }}; }}\n"
        ));
        for level in 1..self.depth {
            let previous = level - 1;
            source.push_str(&format!(
                "export function c{level}_{i}<T>(x: T) {{ return c{previous}_{i}(x); }}\n"
            ));
        }
        let last = self.depth - 1;
        source.push_str(&format!(
            "export function witnessChain{i}(v: number | string) {{ return c{last}_{i}(v); }}\n"
        ));
        source.push_str(&format!(
            "export function witnessUnion{i}(u: U{i}<boolean>) {{ return u; }}\n"
        ));
        source.push_str(&format!(
            "declare const call{i}: (() => Box<number>) & (() => Pair<string, string>);\n\
             export function witnessCall{i}() {{ return call{i}(); }}\n"
        ));
        source.push_str(&format!(
            "declare const make{i}: (new () => Box<number>) & (new () => Pair<string, string>);\n\
             export function witnessNew{i}() {{ return new make{i}(); }}\n"
        ));
        source.push_str(&format!(
            "export async function witnessAwait{i}(p: Promise<Opt<number>>) {{ return await p; }}\n"
        ));
        source.push_str(&format!(
            "export function witnessGlobal{i}(g: SkGlobal) {{ return g.tag; }}\n"
        ));
        source
    }

    fn files(&self) -> Vec<(String, String)> {
        let mut files = vec![
            (SHARED.to_string(), Self::shared(false)),
            (AUGMENTATION.to_string(), Self::augmentation(false)),
        ];
        for i in 0..self.modules {
            files.push((Self::module_path(i), self.module(i, false)));
        }
        files
    }

    fn witnesses_of(&self, i: usize) -> Vec<(String, String)> {
        self.kinds
            .iter()
            .map(|kind| (Self::module_path(i), format!("witness{kind}{i}")))
            .collect()
    }

    fn all_witnesses(&self) -> Vec<(String, String)> {
        (0..self.modules)
            .flat_map(|i| self.witnesses_of(i))
            .collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Host driving
// ─────────────────────────────────────────────────────────────────────────

fn host_with_workers(cpu_threads: Option<usize>) -> Arc<VerterHost> {
    let scheduler = match cpu_threads {
        Some(cpu_threads) => SchedulerConfig {
            cpu_threads,
            ..SchedulerConfig::default()
        },
        None => SchedulerConfig::default(),
    };
    Arc::new(VerterHost::new_standalone_with_scheduler_config(
        HostConfig::default(),
        scheduler,
    ))
}

fn upsert(host: &VerterHost, canonical: &str, source: &str) {
    // The update summary is not a measured output; only success matters.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_owned()),
            input_id: canonical.to_owned(),
            source: Arc::from(source),
            file_language: verter_session::LanguageRegistry::global()
                .classify_static(canonical)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|err| panic!("upsert `{canonical}`: {err:?}"));
}

fn load(host: &VerterHost, corpus: &Corpus) {
    for (canonical, source) in corpus.files() {
        upsert(host, &canonical, &source);
    }
}

#[derive(Default, Clone, Copy)]
struct Census {
    complete: u64,
    degraded: u64,
    refused: u64,
}

impl Census {
    fn add(&mut self, other: Census) {
        self.complete += other.complete;
        self.degraded += other.degraded;
        self.refused += other.refused;
    }
}

fn census_json(census: Census) -> serde_json::Value {
    serde_json::json!({
        "complete": census.complete,
        "degraded": census.degraded,
        "refused": census.refused,
    })
}

fn query(host: &VerterHost, canonical: &str, symbol: &str) -> Census {
    let identity = FlowFunctionReturnIdentity {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            owner: TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(symbol),
            space: LocatorSymbolSpace::Value,
        },
        function_part: FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let carrier =
        host.get_flow_return_type_with_audit(&identity, ReturnProjectionDemand::whole_return());
    match carrier.as_result() {
        Ok(result) if result.degradation().is_none() => Census {
            complete: 1,
            ..Census::default()
        },
        Ok(_) => Census {
            degraded: 1,
            ..Census::default()
        },
        Err(_) => Census {
            refused: 1,
            ..Census::default()
        },
    }
}

fn query_all(host: &VerterHost, witnesses: &[(String, String)]) -> Census {
    let mut census = Census::default();
    for (canonical, symbol) in witnesses {
        census.add(query(host, canonical, symbol));
    }
    census
}

// ─────────────────────────────────────────────────────────────────────────
// Outcome fingerprints (untimed)
// ─────────────────────────────────────────────────────────────────────────

/// One witness's OUTCOME as the arms are compared on it: completion,
/// the typed degradation or refusal, and the answered type rendered
/// structurally — kinds, names, literals and shape, never node ids, spans
/// or scopes, so two implementations that answer the same type print the
/// same text. Union members are sorted (their order is presentation); every
/// other order is kept. A node kind this renderer does not spell prints as
/// its variant name, so an arm answering with a different kind still
/// differs.
fn outcome(host: &VerterHost, canonical: &str, symbol: &str) -> String {
    let identity = FlowFunctionReturnIdentity {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            owner: TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(symbol),
            space: LocatorSymbolSpace::Value,
        },
        function_part: FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let carrier =
        host.get_flow_return_type_with_audit(&identity, ReturnProjectionDemand::whole_return());
    match carrier.as_result() {
        Ok(result) => {
            let graph = host.project_type_store().semantic_graph();
            let mut rendered = String::new();
            render_type(
                &|id| graph.node_data(id),
                result.return_type(),
                0,
                &mut rendered,
            );
            match result.degradation() {
                None => format!("complete {rendered}"),
                Some(reason) => format!("degraded({reason:?}) {rendered}"),
            }
        }
        Err(error) => format!("refused({error:?})"),
    }
}

/// Render depth past which a type prints as `…`: deep enough for every
/// corpus witness, bounded so a recursive answer terminates.
const RENDER_DEPTH: usize = 12;

fn render_type(
    node_data: &dyn Fn(SemanticNodeId) -> Option<Arc<SemanticNodeData>>,
    node: SemanticNodeId,
    depth: usize,
    out: &mut String,
) {
    if depth > RENDER_DEPTH {
        out.push('…');
        return;
    }
    let Some(data) = node_data(node) else {
        out.push_str("<absent>");
        return;
    };
    let child = |id: SemanticNodeId| {
        let mut text = String::new();
        render_type(node_data, id, depth + 1, &mut text);
        text
    };
    match &*data {
        SemanticNodeData::Primitive(kind) => out.push_str(&format!("{kind:?}")),
        SemanticNodeData::Literal(value) => out.push_str(&format!("{value:?}")),
        SemanticNodeData::Alias(target) => out.push_str(&child(*target)),
        SemanticNodeData::Union(members) => {
            let mut arms: Vec<String> = members.iter().map(|member| child(*member)).collect();
            arms.sort_unstable();
            out.push_str(&format!("({})", arms.join(" | ")));
        }
        SemanticNodeData::Intersection(members) => {
            let parts: Vec<String> = members.iter().map(|member| child(*member)).collect();
            out.push_str(&format!("({})", parts.join(" & ")));
        }
        SemanticNodeData::Array { element, readonly } => {
            let prefix = if *readonly { "readonly " } else { "" };
            out.push_str(&format!("{prefix}{}[]", child(*element)));
        }
        SemanticNodeData::Tuple { elements, readonly } => {
            let parts: Vec<String> = elements
                .iter()
                .map(|element| {
                    let rest = if element.rest { "..." } else { "" };
                    let optional = if element.optional { "?" } else { "" };
                    format!("{rest}{}{optional}", child(element.value))
                })
                .collect();
            let prefix = if *readonly { "readonly " } else { "" };
            out.push_str(&format!("{prefix}[{}]", parts.join(", ")));
        }
        SemanticNodeData::Object(surface) => {
            let mut parts: Vec<String> = surface
                .positive_members()
                .iter()
                .map(|member| {
                    let readonly = if member.readonly { "readonly " } else { "" };
                    let optional = if member.optional { "?" } else { "" };
                    format!(
                        "{readonly}{:?}{optional}: {}",
                        member.key,
                        child(member.value)
                    )
                })
                .collect();
            parts.extend(
                surface
                    .call_signatures
                    .iter()
                    .map(|signature| format!("call {}", child(*signature))),
            );
            parts.extend(
                surface
                    .construct_signatures
                    .iter()
                    .map(|signature| format!("new {}", child(*signature))),
            );
            out.push_str(&format!("{{ {} }}", parts.join("; ")));
        }
        SemanticNodeData::Signature {
            kind,
            params,
            return_type,
            ..
        } => {
            let parts: Vec<String> = params
                .iter()
                .map(|param| {
                    let rest = if param.rest { "..." } else { "" };
                    let optional = if param.optional { "?" } else { "" };
                    format!("{rest}_{optional}: {}", child(param.ty))
                })
                .collect();
            out.push_str(&format!(
                "{kind:?}({}) => {}",
                parts.join(", "),
                child(*return_type)
            ));
        }
        SemanticNodeData::DeclRef { identity } => out.push_str(&identity.decl_name),
        SemanticNodeData::InstantiationRef { base, args } => {
            let parts: Vec<String> = args.iter().map(|arg| child(*arg)).collect();
            out.push_str(&format!("{}<{}>", base.decl_name, parts.join(", ")));
        }
        SemanticNodeData::TypeParam { decl, .. } => {
            out.push_str(&format!("param({})", decl.decl_name));
        }
        other => {
            // The variant name only: the rest of a `Debug` print carries
            // node ids and positions, which differ between implementations.
            let debug = format!("{other:?}");
            let name: String = debug
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            out.push_str(&format!("<{name}>"));
        }
    }
}

/// One outcome state: its name, the edit (canonical, source) applied to
/// the loaded corpus, and the witnesses re-queried in it.
type OutcomeState<'a> = (&'a str, Option<(&'a str, &'a str)>, Vec<(String, String)>);

/// The outcome of every witness in `witnesses`, keyed `canonical#symbol`.
fn outcomes(host: &VerterHost, witnesses: &[(String, String)]) -> serde_json::Value {
    let mut by_witness = serde_json::Map::new();
    for (canonical, symbol) in witnesses {
        by_witness.insert(
            format!("{canonical}#{symbol}"),
            serde_json::json!(outcome(host, canonical, symbol)),
        );
    }
    serde_json::Value::Object(by_witness)
}

fn nanos(work: impl FnOnce()) -> u64 {
    let started = Instant::now();
    work();
    started.elapsed().as_nanos() as u64
}

// ─────────────────────────────────────────────────────────────────────────
// Workloads
// ─────────────────────────────────────────────────────────────────────────

struct Workload {
    name: &'static str,
    samples_ns: Vec<u64>,
    alloc_count: u64,
    alloc_bytes: u64,
}

fn cold_load(corpus: &Corpus, samples: usize) -> Workload {
    let witnesses = corpus.all_witnesses();
    let mut samples_ns = Vec::with_capacity(samples);
    for _ in 0..samples {
        let host = host_with_workers(None);
        samples_ns.push(nanos(|| {
            load(&host, corpus);
            query_all(&host, &witnesses);
        }));
    }
    let (alloc_count, alloc_bytes) = account(|| {
        let host = host_with_workers(None);
        load(&host, corpus);
        query_all(&host, &witnesses);
    });
    Workload {
        name: "cold_load",
        samples_ns,
        alloc_count,
        alloc_bytes,
    }
}

/// Full sweeps over every witness in ONE warm-query sample. A single warm
/// query is a memo hit of tens of nanoseconds — below what a timer resolves
/// reliably, and its distribution is the mix of witness kinds rather than a
/// latency — so each sample times a fixed batch and is normalized per query.
const WARM_SWEEPS_PER_SAMPLE: usize = 10;

fn warm_query(host: &VerterHost, corpus: &Corpus, samples: usize) -> Workload {
    let witnesses = corpus.all_witnesses();
    query_all(host, &witnesses);
    let queries_per_sample = (WARM_SWEEPS_PER_SAMPLE * witnesses.len()).max(1) as u64;
    let mut samples_ns = Vec::with_capacity(samples);
    for _ in 0..samples {
        let batch = nanos(|| {
            for _ in 0..WARM_SWEEPS_PER_SAMPLE {
                query_all(host, &witnesses);
            }
        });
        samples_ns.push(batch / queries_per_sample);
    }
    let (alloc_count, alloc_bytes) = account(|| {
        query_all(host, &witnesses);
    });
    Workload {
        name: "warm_query",
        samples_ns,
        alloc_count,
        alloc_bytes,
    }
}

/// Toggle one input between two versions `samples` times, re-querying
/// `readers` after each upsert. The sample is the edit plus the re-query.
fn edit_workload(
    name: &'static str,
    host: &VerterHost,
    canonical: &str,
    versions: [&str; 2],
    readers: &[(String, String)],
    samples: usize,
) -> Workload {
    query_all(host, readers);
    let mut samples_ns = Vec::with_capacity(samples);
    for round in 0..samples {
        let version = versions[(round + 1) % 2];
        samples_ns.push(nanos(|| {
            upsert(host, canonical, version);
            query_all(host, readers);
        }));
    }
    // Leave the input on its original version.
    if samples % 2 == 1 {
        upsert(host, canonical, versions[0]);
        query_all(host, readers);
    }
    let (alloc_count, alloc_bytes) = account(|| {
        upsert(host, canonical, versions[1]);
        query_all(host, readers);
        upsert(host, canonical, versions[0]);
        query_all(host, readers);
    });
    Workload {
        name,
        samples_ns,
        alloc_count: alloc_count / 2,
        alloc_bytes: alloc_bytes / 2,
    }
}

fn restart(corpus: &Corpus, samples: usize) -> Workload {
    let witnesses = corpus.all_witnesses();
    let mut host = host_with_workers(None);
    load(&host, corpus);
    query_all(&host, &witnesses);
    let mut samples_ns = Vec::with_capacity(samples);
    for _ in 0..samples {
        samples_ns.push(nanos(|| {
            drop(std::mem::replace(&mut host, host_with_workers(None)));
            load(&host, corpus);
            query_all(&host, &witnesses);
        }));
    }
    let (alloc_count, alloc_bytes) = account(|| {
        drop(std::mem::replace(&mut host, host_with_workers(None)));
        load(&host, corpus);
        query_all(&host, &witnesses);
    });
    Workload {
        name: "restart",
        samples_ns,
        alloc_count,
        alloc_bytes,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Scalability
// ─────────────────────────────────────────────────────────────────────────

/// The caller counts of the concurrent-query benchmark and the host worker
/// counts of the scheduler-scaling and full-check benchmarks.
const SCALING_COUNTS: [usize; 4] = [1, 2, 4, 8];

/// One timed sample run by persistent caller threads.
struct CallerSample {
    /// From the earliest caller's start to the latest caller's finish, each
    /// read by the caller itself, so no wake-up latency is inside it.
    wall_ns: u64,
    /// Process CPU time (every thread, user and system) across the sample,
    /// read by the driving thread around the barriers; `None` where the
    /// platform has no reader.
    cpu_ns: Option<u64>,
}

/// What the callers ran: every sample's timing and, when latencies were
/// asked for, each query's latency pooled over every caller and sample.
struct CallerRun {
    samples: Vec<CallerSample>,
    query_latencies_ns: Vec<u64>,
}

/// Run `samples` timed samples (after one untimed warm-up) on `callers`
/// persistent threads, created once before the first sample and joined
/// after the last, so no thread is spawned or joined inside a timing.
///
/// Per sample, `prepare` runs untimed on the driving thread and hands every
/// caller the same state; each caller waits at a start barrier, reads its
/// own start instant, runs `work(caller, &state, latencies)`, reads its own
/// finish instant, drops its state and waits at an end barrier. The
/// driving thread drops the state after the end barrier, also untimed.
/// `work` pushes one latency per query into `latencies` only when
/// `record_latencies` is set; the vector is preallocated outside timing.
///
/// A panic in `prepare` or in any caller still reaches both barriers, so
/// every thread leaves together and the panic resumes on the driving
/// thread: a failing arm fails its invocation instead of hanging it.
fn run_callers<S: Send + Sync>(
    callers: usize,
    samples: usize,
    record_latencies: Option<usize>,
    mut prepare: impl FnMut() -> Arc<S>,
    work: impl Fn(usize, &S, &mut Vec<u64>) + Sync,
) -> CallerRun {
    let start = Barrier::new(callers + 1);
    let end = Barrier::new(callers + 1);
    let state: Mutex<Option<Arc<S>>> = Mutex::new(None);
    let spans: Vec<Mutex<Option<(Instant, Instant)>>> =
        (0..callers).map(|_| Mutex::new(None)).collect();
    // The first panic of any thread; read by every thread only after a
    // barrier, which orders it after the write.
    let failure: Mutex<Option<Box<dyn Any + Send>>> = Mutex::new(None);
    let failed = || failure.lock().expect("failure slot").is_some();
    let fail = |panic: Box<dyn Any + Send>| {
        failure.lock().expect("failure slot").get_or_insert(panic);
    };
    let rounds = samples + 1;
    let mut run = CallerRun {
        samples: Vec::with_capacity(samples),
        query_latencies_ns: Vec::new(),
    };
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..callers)
            .map(|caller| {
                let (start, end, state, spans, work, failed, fail) =
                    (&start, &end, &state, &spans, &work, &failed, &fail);
                std::thread::Builder::new()
                    .name(format!("signature-kernel-caller-{caller}"))
                    .stack_size(WORK_STACK_BYTES)
                    .spawn_scoped(scope, move || {
                        let capacity = record_latencies.map_or(0, |per_round| per_round * rounds);
                        let mut latencies = Vec::with_capacity(capacity);
                        for round in 0..rounds {
                            start.wait();
                            if failed() {
                                break;
                            }
                            let shared = state
                                .lock()
                                .expect("caller state")
                                .clone()
                                .expect("a prepared state");
                            let recorded = latencies.len();
                            let began = Instant::now();
                            let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
                                work(caller, &shared, &mut latencies)
                            }));
                            let finished = Instant::now();
                            drop(shared);
                            // The warm-up round's latencies are not samples.
                            if round == 0 {
                                latencies.truncate(recorded);
                            }
                            match outcome {
                                Ok(()) => {
                                    *spans[caller].lock().expect("caller span") =
                                        Some((began, finished));
                                }
                                Err(panic) => fail(panic),
                            }
                            end.wait();
                            if failed() {
                                break;
                            }
                        }
                        latencies
                    })
                    .expect("spawn a caller thread")
            })
            .collect();
        for round in 0..rounds {
            match std::panic::catch_unwind(AssertUnwindSafe(&mut prepare)) {
                Ok(prepared) => *state.lock().expect("caller state") = Some(prepared),
                Err(panic) => fail(panic),
            }
            let cpu_before = process_cpu_ns();
            start.wait();
            if failed() {
                break;
            }
            end.wait();
            let cpu_after = process_cpu_ns();
            drop(state.lock().expect("caller state").take());
            if failed() {
                break;
            }
            let spans: Vec<(Instant, Instant)> = spans
                .iter()
                .map(|span| {
                    span.lock()
                        .expect("caller span")
                        .take()
                        .expect("every caller ran")
                })
                .collect();
            let began = spans.iter().map(|span| span.0).min().expect("a caller");
            let finished = spans.iter().map(|span| span.1).max().expect("a caller");
            if round > 0 {
                let wall = finished.saturating_duration_since(began);
                run.samples.push(CallerSample {
                    wall_ns: wall.as_nanos() as u64,
                    cpu_ns: cpu_before
                        .zip(cpu_after)
                        .map(|(before, after)| after.saturating_sub(before)),
                });
            }
        }
        for handle in handles {
            let latencies = handle.join().expect("a caller thread finishes");
            run.query_latencies_ns.extend(latencies);
        }
    });
    drop(state.into_inner().expect("caller state"));
    if let Some(panic) = failure.into_inner().expect("failure slot") {
        std::panic::resume_unwind(panic);
    }
    run
}

/// Nearest-rank quantile of `sorted`, which must be ascending and non-empty.
fn quantile(sorted: &[u64], q: f64) -> u64 {
    let rank = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

fn median_of(samples: &[u64]) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    quantile(&sorted, 0.5)
}

fn latency_json(latencies: &[u64]) -> serde_json::Value {
    if latencies.is_empty() {
        return serde_json::json!({ "n": 0 });
    }
    let mut sorted = latencies.to_vec();
    sorted.sort_unstable();
    serde_json::json!({
        "p50": quantile(&sorted, 0.5),
        "p95": quantile(&sorted, 0.95),
        "p99": quantile(&sorted, 0.99),
        "max": sorted[sorted.len() - 1],
        "n": sorted.len(),
    })
}

/// Concurrent query scalability: ONE warm host with a fixed
/// `host_workers` scheduler CPU workers, queried by `callers` persistent
/// threads at once. Every caller runs the same work per sample —
/// [`WARM_SWEEPS_PER_SAMPLE`] full sweeps over every witness, from its own
/// offset so the callers contend for different keys first — so with
/// perfect scaling the sample's wall time stays flat and queries per second
/// grow with the caller count.
fn concurrent_queries(
    corpus: &Corpus,
    host_workers: usize,
    callers: usize,
    samples: usize,
) -> serde_json::Value {
    let witnesses = corpus.all_witnesses();
    let host = host_with_workers(Some(host_workers));
    load(&host, corpus);
    query_all(&host, &witnesses);
    let per_caller = WARM_SWEEPS_PER_SAMPLE * witnesses.len();
    let run = run_callers(
        callers,
        samples,
        Some(per_caller),
        || Arc::clone(&host),
        |caller, host: &VerterHost, latencies| {
            let offset = caller * witnesses.len() / callers;
            for k in 0..per_caller {
                let (canonical, symbol) = &witnesses[(offset + k) % witnesses.len()];
                let started = Instant::now();
                query(host, canonical, symbol);
                latencies.push(started.elapsed().as_nanos() as u64);
            }
        },
    );
    let queries = (callers * per_caller) as f64;
    let samples_ns: Vec<u64> = run.samples.iter().map(|s| s.wall_ns).collect();
    let qps: Vec<u64> = samples_ns
        .iter()
        .map(|&ns| (queries / (ns.max(1) as f64 / 1e9)) as u64)
        .collect();
    serde_json::json!({
        "callers": callers,
        "queries_per_sample": callers * per_caller,
        "samples_ns": samples_ns,
        "qps": qps,
        "median_qps": median_of(&qps),
        "query_latency_ns": latency_json(&run.query_latencies_ns),
    })
}

/// The fields every scaling point records: wall and CPU samples and, where
/// the platform reads process CPU time, the utilisation of `workers`
/// workers, `cpu / (wall × workers)` — per sample, and over every sample
/// at once (`Σ cpu / (Σ wall × workers)`), which stays meaningful where the
/// process CPU clock ticks coarsely (Windows' advances in scheduler ticks,
/// about 15.6 ms).
fn scaling_samples_json(
    run: &CallerRun,
    workers: usize,
) -> serde_json::Map<String, serde_json::Value> {
    let samples_ns: Vec<u64> = run.samples.iter().map(|s| s.wall_ns).collect();
    let cpu_ns: Option<Vec<u64>> = run.samples.iter().map(|s| s.cpu_ns).collect();
    let utilisation: Option<Vec<f64>> = cpu_ns.as_ref().map(|cpu| {
        cpu.iter()
            .zip(&samples_ns)
            .map(|(&cpu, &wall)| cpu as f64 / (wall.max(1) as f64 * workers as f64))
            .collect()
    });
    let utilisation_total = cpu_ns.as_ref().map(|cpu| {
        let wall: u64 = samples_ns.iter().sum();
        cpu.iter().sum::<u64>() as f64 / (wall.max(1) as f64 * workers as f64)
    });
    let mut fields = serde_json::Map::new();
    fields.insert("workers".into(), serde_json::json!(workers));
    fields.insert(
        "median_ns".into(),
        serde_json::json!(median_of(&samples_ns)),
    );
    fields.insert("samples_ns".into(), serde_json::json!(samples_ns));
    fields.insert("cpu_ns".into(), serde_json::json!(cpu_ns));
    fields.insert("cpu_utilisation".into(), serde_json::json!(utilisation));
    fields.insert(
        "cpu_utilisation_total".into(),
        serde_json::json!(utilisation_total),
    );
    fields
}

/// Internal scheduler scalability: ONE caller, a host with `workers`
/// scheduler CPU workers. Each sample gets a fresh host, built and loaded
/// with the check corpus untimed; the sample is the cold first query of
/// every witness — independent roots across every module, each demanding
/// its module's declarations, chain and imports cold. Any speedup over one
/// worker is work the host spreads over its scheduler by itself.
fn scheduler_scaling(corpus: &Corpus, workers: usize, samples: usize) -> serde_json::Value {
    let witnesses = corpus.all_witnesses();
    let run = run_callers(
        1,
        samples,
        None,
        || {
            let host = host_with_workers(Some(workers));
            load(&host, corpus);
            host
        },
        |_, host: &VerterHost, _| {
            query_all(host, &witnesses);
        },
    );
    serde_json::Value::Object(scaling_samples_json(&run, workers))
}

/// Full-check throughput: every file of the check corpus loaded and every
/// witness in it answered, on a fresh host with `workers` scheduler CPU
/// workers, by as many caller threads as workers — the files dealt out
/// round-robin, each caller upserting its file and then querying it, as a
/// project checker drives one host with one checking thread per worker.
/// The two shared inputs every module imports are loaded untimed with the
/// fresh host; the sample is the rest of the check.
fn full_check(corpus: &Corpus, workers: usize, samples: usize) -> serde_json::Value {
    let run = run_callers(
        workers,
        samples,
        None,
        || {
            let host = host_with_workers(Some(workers));
            upsert(&host, SHARED, &Corpus::shared(false));
            upsert(&host, AUGMENTATION, &Corpus::augmentation(false));
            host
        },
        |caller, host: &VerterHost, _| {
            for i in (caller..corpus.modules).step_by(workers) {
                upsert(host, &Corpus::module_path(i), &corpus.module(i, false));
                query_all(host, &corpus.witnesses_of(i));
            }
        },
    );
    let mut fields = scaling_samples_json(&run, workers);
    let files_per_second: Vec<f64> = run
        .samples
        .iter()
        .map(|s| corpus.modules as f64 / (s.wall_ns.max(1) as f64 / 1e9))
        .collect();
    fields.insert("callers".into(), serde_json::json!(workers));
    fields.insert(
        "files_per_second".into(),
        serde_json::json!(files_per_second),
    );
    serde_json::Value::Object(fields)
}

// ─────────────────────────────────────────────────────────────────────────
// Process CPU time
// ─────────────────────────────────────────────────────────────────────────

/// Where [`process_cpu_ns`] reads from on this platform, or `None`.
const CPU_TIME_SOURCE: Option<&str> = cpu_clock::SOURCE;

/// The process's CPU time so far (every thread, user plus system), in
/// nanoseconds; `None` on a platform this harness has no reader for.
fn process_cpu_ns() -> Option<u64> {
    cpu_clock::read()
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
))]
mod cpu_clock {
    use std::os::raw::{c_int, c_long};

    /// `struct timespec`: `time_t` and the nanoseconds are both `long` on
    /// these platforms.
    #[repr(C)]
    struct Timespec {
        tv_sec: c_long,
        tv_nsec: c_long,
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    const CLOCK_PROCESS_CPUTIME_ID: c_int = 2;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const CLOCK_PROCESS_CPUTIME_ID: c_int = 12;

    extern "C" {
        fn clock_gettime(clock: c_int, time: *mut Timespec) -> c_int;
    }

    pub(super) const SOURCE: Option<&str> = Some("clock_gettime(CLOCK_PROCESS_CPUTIME_ID)");

    pub(super) fn read() -> Option<u64> {
        let mut time = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `time` is a valid, writable `struct timespec`; the call
        // writes it and reads nothing else.
        let status = unsafe { clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &mut time) };
        (status == 0).then(|| time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64)
    }
}

#[cfg(windows)]
mod cpu_clock {
    use std::ffi::c_void;

    /// `FILETIME`: a count of 100-nanosecond intervals, split in halves.
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    impl FileTime {
        fn nanos(&self) -> u64 {
            ((u64::from(self.high) << 32) | u64::from(self.low)) * 100
        }
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn GetProcessTimes(
            process: *mut c_void,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }

    pub(super) const SOURCE: Option<&str> = Some("GetProcessTimes");

    pub(super) fn read() -> Option<u64> {
        let (mut creation, mut exit) = (FileTime::default(), FileTime::default());
        let (mut kernel, mut user) = (FileTime::default(), FileTime::default());
        // SAFETY: the pseudo-handle of the current process needs no
        // closing, and the call writes the four `FILETIME`s it is given.
        let ok = unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        };
        (ok != 0).then(|| kernel.nanos() + user.nanos())
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    windows
)))]
mod cpu_clock {
    pub(super) const SOURCE: Option<&str> = None;

    pub(super) fn read() -> Option<u64> {
        None
    }
}

/// Edit/revert one module `rounds` times on one host and record the live
/// heap after every round. A plateau is the retirement policy working;
/// steady growth is retained state that nothing releases.
fn soak(corpus: &Corpus, rounds: usize) -> Vec<i64> {
    // Track from zero BEFORE the soak's host exists, so the series is the
    // soak host's own net heap (the earlier workloads' hosts are dropped).
    LIVE_BYTES.store(0, Ordering::Relaxed);
    TRACKING_LIVE.store(true, Ordering::Relaxed);
    let host = host_with_workers(None);
    load(&host, corpus);
    let readers = corpus.witnesses_of(0);
    let path = Corpus::module_path(0);
    let versions = [corpus.module(0, false), corpus.module(0, true)];
    query_all(&host, &corpus.all_witnesses());
    let mut live = Vec::with_capacity(rounds);
    for round in 0..rounds {
        upsert(&host, &path, &versions[(round + 1) % 2]);
        query_all(&host, &readers);
        live.push(LIVE_BYTES.load(Ordering::Relaxed));
    }
    TRACKING_LIVE.store(false, Ordering::Relaxed);
    live
}

// ─────────────────────────────────────────────────────────────────────────
// Driver
// ─────────────────────────────────────────────────────────────────────────

fn arg(args: &[String], flag: &str, default: usize) -> usize {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(|v| {
            v.parse()
                .unwrap_or_else(|_| panic!("{flag} expects a number, got `{v}`"))
        })
        .unwrap_or(default)
}

/// The witness kinds left after `--exclude Kind,Kind`; an unknown kind is
/// refused rather than silently ignored.
fn queried_kinds(args: &[String]) -> Vec<&'static str> {
    let excluded: Vec<&str> = args
        .iter()
        .position(|a| a == "--exclude")
        .and_then(|i| args.get(i + 1))
        .map(|list| {
            list.split(',')
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .collect()
        })
        .unwrap_or_default();
    for kind in &excluded {
        assert!(
            WITNESSES.contains(kind),
            "--exclude names unknown witness kind `{kind}`; known: {WITNESSES:?}"
        );
    }
    WITNESSES
        .into_iter()
        .filter(|kind| !excluded.contains(kind))
        .collect()
}

fn workload_json(workload: &Workload) -> serde_json::Value {
    serde_json::json!({
        "samples_ns": workload.samples_ns,
        "alloc_count": workload.alloc_count,
        "alloc_bytes": workload.alloc_bytes,
    })
}

/// The stack the measured work runs on. Deep descriptor-instantiation chains
/// recurse through the implementation, and a process main thread's stack is
/// platform-sized (1 MiB on Windows, 8 MiB on macOS and Linux): the
/// pre-kernel baseline overflows a 1 MiB stack on a depth-12 chain the
/// candidate evaluates from an explicit schedule on a fraction of that.
/// Both arms run on one explicit, generous stack so the timing compares
/// the same completed workload on every platform; the depth each arm
/// tolerates on a default stack is a separate finding, not something to
/// let a platform default decide inside a timing run.
const WORK_STACK_BYTES: usize = 64 << 20;

fn main() {
    let worker = std::thread::Builder::new()
        .name("signature-kernel-bench".into())
        .stack_size(WORK_STACK_BYTES)
        .spawn(run)
        .expect("spawn the measured-work thread");
    if let Err(panic) = worker.join() {
        std::panic::resume_unwind(panic);
    }
}

fn run() {
    let args: Vec<String> = std::env::args().collect();
    // Depth 8: both arms COMPLETE the chain, so its timing compares the
    // same answered work. Past eleven levels the baseline's connected-query
    // depth guard refuses it (two nested queries per level) while the
    // candidate completes it at the same work per level, and the
    // baseline's chain work doubles per level, so a deeper default times
    // a refusal against an answer or spends the session on one witness.
    let corpus = Corpus {
        modules: arg(&args, "--modules", 24),
        depth: arg(&args, "--depth", 8).max(1),
        kinds: queried_kinds(&args),
    };
    let samples = arg(&args, "--samples", 30);
    let cold_samples = arg(&args, "--cold-samples", 10);
    // 400 rounds: a short soak only sees the warm-up climb; retention is
    // bounded and reclaimed in bursts, so the plateau shows after it.
    let soak_rounds = arg(&args, "--soak", 400);
    // The concurrent-query benchmark's fixed scheduler worker count.
    let host_workers = arg(&args, "--host-workers", 4).max(1);
    // The scheduler-scaling and full-check corpus: the same modules, four
    // times as many by default, so eight workers each have many
    // independent roots and files.
    let check = Corpus {
        modules: arg(&args, "--check-modules", corpus.modules * 4).max(1),
        depth: corpus.depth,
        kinds: corpus.kinds.clone(),
    };

    // Completion census on a fresh host: what the implementation ANSWERS,
    // so the runner can separate "faster" from "answering differently" —
    // per witness kind, so a difference names the shape that moved.
    let (census, census_by_witness) = {
        let host = host_with_workers(None);
        load(&host, &corpus);
        let mut total = Census::default();
        let mut by_witness = serde_json::Map::new();
        for &kind in &corpus.kinds {
            let of_kind: Vec<(String, String)> = (0..corpus.modules)
                .map(|i| (Corpus::module_path(i), format!("witness{kind}{i}")))
                .collect();
            let census = query_all(&host, &of_kind);
            total.add(census);
            by_witness.insert(kind.to_owned(), census_json(census));
        }
        (total, by_witness)
    };

    // Outcome fingerprints, untimed, each state on a fresh host: the
    // original corpus, and every edited state an edit workload reaches,
    // over exactly the witnesses that workload re-queries. The runner
    // compares them per witness across the arms before it reports a ratio.
    let outcomes_by_state = {
        let module0 = corpus.module(0, true);
        let shared = Corpus::shared(true);
        let augmentation = Corpus::augmentation(true);
        let global_readers: Vec<(String, String)> = (0..corpus.modules)
            .filter(|_| corpus.kinds.contains(&"Global"))
            .map(|i| (Corpus::module_path(i), format!("witnessGlobal{i}")))
            .collect();
        let states: [OutcomeState<'_>; 4] = [
            ("original", None, corpus.all_witnesses()),
            (
                "local_edit",
                Some((&Corpus::module_path(0), module0.as_str())),
                corpus.witnesses_of(0),
            ),
            (
                "declaration_edit",
                Some((SHARED, shared.as_str())),
                corpus.all_witnesses(),
            ),
            (
                "augmentation_edit",
                Some((AUGMENTATION, augmentation.as_str())),
                global_readers,
            ),
        ];
        let mut by_state = serde_json::Map::new();
        for (state, edit, witnesses) in states {
            let host = host_with_workers(None);
            load(&host, &corpus);
            if let Some((canonical, source)) = edit {
                upsert(&host, canonical, source);
            }
            by_state.insert(state.to_owned(), outcomes(&host, &witnesses));
        }
        // The check corpus the scheduler-scaling and full-check
        // benchmarks answer.
        let host = host_with_workers(None);
        load(&host, &check);
        by_state.insert("check".to_owned(), outcomes(&host, &check.all_witnesses()));
        by_state
    };

    let mut workloads = vec![cold_load(&corpus, cold_samples)];
    let host = host_with_workers(None);
    load(&host, &corpus);
    workloads.push(warm_query(&host, &corpus, samples));
    let module0 = [corpus.module(0, false), corpus.module(0, true)];
    workloads.push(edit_workload(
        "local_edit",
        &host,
        &Corpus::module_path(0),
        [module0[0].as_str(), module0[1].as_str()],
        &corpus.witnesses_of(0),
        samples,
    ));
    let shared = [Corpus::shared(false), Corpus::shared(true)];
    workloads.push(edit_workload(
        "declaration_edit",
        &host,
        SHARED,
        [shared[0].as_str(), shared[1].as_str()],
        &corpus.all_witnesses(),
        samples,
    ));
    let augmentation = [Corpus::augmentation(false), Corpus::augmentation(true)];
    // The augmentation's readers are the Global witnesses — none when that
    // kind is excluded, and the workload then times the edit alone.
    let global_readers: Vec<(String, String)> = (0..corpus.modules)
        .filter(|_| corpus.kinds.contains(&"Global"))
        .map(|i| (Corpus::module_path(i), format!("witnessGlobal{i}")))
        .collect();
    workloads.push(edit_workload(
        "augmentation_edit",
        &host,
        AUGMENTATION,
        [augmentation[0].as_str(), augmentation[1].as_str()],
        &global_readers,
        samples,
    ));
    drop(host);
    workloads.push(restart(&corpus, cold_samples));

    let mut by_callers = serde_json::Map::new();
    for callers in SCALING_COUNTS {
        by_callers.insert(
            callers.to_string(),
            concurrent_queries(&corpus, host_workers, callers, samples),
        );
    }
    let mut scheduler_by_workers = serde_json::Map::new();
    let mut full_check_by_workers = serde_json::Map::new();
    for workers in SCALING_COUNTS {
        scheduler_by_workers.insert(
            workers.to_string(),
            scheduler_scaling(&check, workers, cold_samples),
        );
    }
    for workers in SCALING_COUNTS {
        full_check_by_workers.insert(
            workers.to_string(),
            full_check(&check, workers, cold_samples),
        );
    }
    // Speedup against one worker, from the medians.
    let one_worker = scheduler_by_workers["1"]["median_ns"].as_u64().unwrap_or(0) as f64;
    for point in scheduler_by_workers.values_mut() {
        let median = point["median_ns"].as_u64().unwrap_or(0).max(1) as f64;
        point["speedup_vs_1"] = serde_json::json!(one_worker / median);
    }

    let live = soak(&corpus, soak_rounds);

    let mut by_name = serde_json::Map::new();
    for workload in &workloads {
        by_name.insert(workload.name.to_string(), workload_json(workload));
    }
    let document = serde_json::json!({
        "harness": "signature_kernel_bench",
        "harness_version": 3,
        "rev": std::env::var("SK_BENCH_REV").unwrap_or_default(),
        "machine": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "logical_cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
            "work_stack_bytes": WORK_STACK_BYTES,
        },
        "corpus": {
            "modules": corpus.modules,
            "depth": corpus.depth,
            "witnesses": corpus.all_witnesses().len(),
            "kinds": corpus.kinds,
        },
        "census": census_json(census),
        "census_by_witness": census_by_witness,
        "outcomes": outcomes_by_state,
        "workloads": by_name,
        "concurrent_queries": {
            "host_workers": host_workers,
            "sweeps_per_sample": WARM_SWEEPS_PER_SAMPLE,
            "witnesses": corpus.all_witnesses().len(),
            "by_callers": by_callers,
        },
        "scheduler_scaling": {
            "callers": 1,
            "modules": check.modules,
            "witnesses": check.all_witnesses().len(),
            "by_workers": scheduler_by_workers,
        },
        "full_check": {
            "files": check.modules,
            "witnesses": check.all_witnesses().len(),
            "by_workers": full_check_by_workers,
        },
        "cpu_time_source": CPU_TIME_SOURCE,
        "soak_live_bytes": live,
        "warm_query_sweeps_per_sample": WARM_SWEEPS_PER_SAMPLE,
    });
    println!("{document}");
}
