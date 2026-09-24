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
//! per-workload latency samples, per-workload allocation totals,
//! worker-count throughput, the edit/revert soak's live-heap series, the
//! completion census of every witness, and — untimed — every witness's
//! OUTCOME (completion, typed degradation or refusal, and the answered type
//! rendered structurally) in the original corpus and in each edited state
//! an edit workload reaches. The runner reports a ratio for a workload only
//! when both arms' outcomes agree on every witness it queries. Statistics, the ABBA
//! interleaving, the control benchmark and the regression gate live in the
//! runner, `scripts/benchmark/signature-kernel-perf.mjs`.
//!
//! ```text
//! cargo run --release -p verter_session --example signature_kernel_bench -- \
//!     [--modules N] [--depth N] [--samples N] [--cold-samples N] [--soak N]
//!     [--exclude Kind,Kind]
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
//! * `throughput_wN`     — N threads querying one warm host whose scheduler has N workers.
//! * `soak`              — edit/revert one module repeatedly; live heap after each round.
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
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
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

/// `workers` threads each query the whole corpus against ONE warm host
/// whose scheduler has `workers` CPU workers. Each thread walks the witness
/// list from a different offset, so the threads contend for different keys
/// first. A sample is the queries completed per second across all threads.
fn throughput(corpus: &Corpus, workers: usize, samples: usize) -> (Workload, u64) {
    let witnesses = Arc::new(corpus.all_witnesses());
    let host = host_with_workers(Some(workers));
    load(&host, corpus);
    query_all(&host, &witnesses);
    let per_thread = witnesses.len();
    let mut samples_ns = Vec::with_capacity(samples);
    let mut qps = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        std::thread::scope(|scope| {
            for thread in 0..workers {
                let host = Arc::clone(&host);
                let witnesses = Arc::clone(&witnesses);
                scope.spawn(move || {
                    let offset = thread * witnesses.len() / workers;
                    for k in 0..witnesses.len() {
                        let (canonical, symbol) = &witnesses[(offset + k) % witnesses.len()];
                        query(&host, canonical, symbol);
                    }
                });
            }
        });
        let elapsed = started.elapsed();
        samples_ns.push(elapsed.as_nanos() as u64);
        qps.push(((workers * per_thread) as f64 / elapsed.as_secs_f64()) as u64);
    }
    qps.sort_unstable();
    let median_qps = qps[qps.len() / 2];
    (
        Workload {
            name: match workers {
                1 => "throughput_w1",
                2 => "throughput_w2",
                4 => "throughput_w4",
                _ => "throughput_w8",
            },
            samples_ns,
            alloc_count: 0,
            alloc_bytes: 0,
        },
        median_qps,
    )
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
/// pre-kernel baseline overflows a 1 MiB stack on a depth-12 chain where
/// the candidate reaches its typed depth refusal. Both arms run on one explicit, generous stack
/// so the timing compares the same completed workload on every platform;
/// the depth each arm tolerates on a default stack is a separate finding,
/// not something to let a platform default decide inside a timing run.
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
    // same answered work. Past eleven levels the connected-query depth
    // guard refuses it in both arms (two nested queries per level), and
    // the baseline's chain work doubles per level, so a deeper default
    // times refusals or spends the session on one witness.
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

    let mut throughput_qps = serde_json::Map::new();
    for workers in [1, 2, 4, 8] {
        let (workload, median_qps) = throughput(&corpus, workers, cold_samples);
        throughput_qps.insert(workload.name.to_string(), serde_json::json!(median_qps));
        workloads.push(workload);
    }

    let live = soak(&corpus, soak_rounds);

    let mut by_name = serde_json::Map::new();
    for workload in &workloads {
        by_name.insert(workload.name.to_string(), workload_json(workload));
    }
    let document = serde_json::json!({
        "harness": "signature_kernel_bench",
        "harness_version": 2,
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
        "throughput_qps": throughput_qps,
        "soak_live_bytes": live,
        "warm_query_sweeps_per_sample": WARM_SWEEPS_PER_SAMPLE,
    });
    println!("{document}");
}
