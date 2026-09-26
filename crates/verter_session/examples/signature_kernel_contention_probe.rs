//! Signature-kernel contention probe — where concurrent warm queries on ONE
//! host stop scaling, measured on the candidate tree alone.
//!
//! The comparative harness (`signature_kernel_bench`) builds the same source
//! against the pre-kernel baseline, so it can reach the public host API
//! only; its `concurrent_queries` section measures the whole audited
//! request. This probe isolates the parts of that request, through the
//! `test-support` seam this crate's examples are built with
//! (`verter_session::for_tests::signature_kernel_bench_support`), which no production
//! build compiles:
//!
//! * `A0` — the public audited query, identity built before timing.
//! * `A1` — one request context, store view and dispatch reused for every
//!   query of a caller's sample: each query builds its key and runs the
//!   dispatch's flow-return steps, with the per-request setup paid once
//!   per sample.
//! * `A2` — the flow-return memo read alone: every key built once, then
//!   the warm, carrier-validated read the dispatch's warm step takes, in
//!   one request per sample — no key construction or dispatch step per
//!   read.
//! * `A3` — the per-request setup and teardown alone, with no semantic
//!   work.
//!
//! Each variant runs at 1/2/4/8/16 callers on one warm host with a fixed
//! scheduler worker count. `A0` and `A2` also run with every caller on the
//! SAME witness (`same`) as well as on disjoint ones (`disjoint`, the
//! default for every variant): contention that appears only on one key is
//! single flight on that key; contention on disjoint keys is a point every
//! request shares.
//!
//! ```text
//! # full run
//! cargo run --release -p verter_session --example signature_kernel_contention_probe
//! # quick run
//! cargo run --release -p verter_session --example signature_kernel_contention_probe -- \
//!     --modules 4 --depth 4 --samples 5 --queries 200
//!
//! cargo run --release -p verter_session --example signature_kernel_contention_probe -- \
//!     [--modules N] [--depth N] [--samples N] [--queries N] [--host-workers N]
//!     [--only A3:disjoint] [--callers N]
//! ```
//!
//! One JSON document goes to stdout: per variant, key mode and caller
//! count, the wall samples, queries per second, and the host's
//! store-view reads per query (`store_view_reads_per_query`, from the
//! host's own provenance counter), which must not grow with the caller
//! count. The corpus is the comparative harness's module shape, written
//! here again because the harness must stay a single file the runner can
//! drop into the baseline tree.

use std::any::Any;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Barrier, Mutex};
use std::time::Instant;

use verter_scheduler::scheduler::SchedulerConfig;
use verter_session::for_tests::signature_kernel_bench_support as support;
use verter_session::{HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity, TopLevelOwnerId};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};

const CALLER_COUNTS: [usize; 5] = [1, 2, 4, 8, 16];
const KINDS: [&str; 6] = ["Chain", "Union", "Call", "New", "Await", "Global"];

/// The measured work runs on an explicit stack, as in the comparative
/// harness.
const WORK_STACK_BYTES: usize = 64 << 20;

fn shared_source() -> String {
    "export interface Box<T> {\n  value: T;\n}\n\
     export interface Pair<A, B> { left: A; right: B }\n\
     export type Opt<T> = T | undefined;\n"
        .to_owned()
}

fn augmentation_source() -> String {
    "declare global { interface SkGlobal { tag: string } }\nexport {};\n".to_owned()
}

fn module_path(i: usize) -> String {
    format!("/sk/m{i}.ts")
}

fn module_source(i: usize, depth: usize) -> String {
    let mut source = String::new();
    source.push_str("import { Box, Pair, Opt } from \"./shared\";\n");
    source.push_str(&format!(
        "export type U{i}<T> = Box<T> | Pair<T, T> | \"l{i}\" | \"r{i}\" | T | Box<T>;\n"
    ));
    source.push_str(&format!(
        "export function c0_{i}<T>(x: T) {{ return {{ v: x, tag: \"original\" as const }}; }}\n"
    ));
    for level in 1..depth {
        let previous = level - 1;
        source.push_str(&format!(
            "export function c{level}_{i}<T>(x: T) {{ return c{previous}_{i}(x); }}\n"
        ));
    }
    let last = depth - 1;
    source.push_str(&format!(
        "export function witnessChain{i}(v: number | string) {{ return c{last}_{i}(v); }}\n\
         export function witnessUnion{i}(u: U{i}<boolean>) {{ return u; }}\n\
         declare const call{i}: (() => Box<number>) & (() => Pair<string, string>);\n\
         export function witnessCall{i}() {{ return call{i}(); }}\n\
         declare const make{i}: (new () => Box<number>) & (new () => Pair<string, string>);\n\
         export function witnessNew{i}() {{ return new make{i}(); }}\n\
         export async function witnessAwait{i}(p: Promise<Opt<number>>) {{ return await p; }}\n\
         export function witnessGlobal{i}(g: SkGlobal) {{ return g.tag; }}\n"
    ));
    source
}

fn upsert(host: &VerterHost, canonical: &str, source: String) {
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

fn witness(canonical: &str, symbol: &str) -> FlowFunctionReturnIdentity {
    FlowFunctionReturnIdentity {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            owner: TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(symbol),
            space: LocatorSymbolSpace::Value,
        },
        function_part: FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    }
}

/// A warm host over `modules` modules and every witness in it, each
/// answered once.
fn warm_host(
    modules: usize,
    depth: usize,
    host_workers: usize,
) -> (Arc<VerterHost>, Vec<FlowFunctionReturnIdentity>) {
    let host = Arc::new(VerterHost::new_standalone_with_scheduler_config(
        HostConfig::default(),
        SchedulerConfig {
            cpu_threads: host_workers,
            ..SchedulerConfig::default()
        },
    ));
    upsert(&host, "/sk/shared.ts", shared_source());
    upsert(&host, "/sk/augment.ts", augmentation_source());
    let mut witnesses = Vec::new();
    for i in 0..modules {
        upsert(&host, &module_path(i), module_source(i, depth));
        for kind in KINDS {
            witnesses.push(witness(&module_path(i), &format!("witness{kind}{i}")));
        }
    }
    for witness in &witnesses {
        assert!(
            support::flow_return_query(&host, witness),
            "every witness answers completely"
        );
    }
    (host, witnesses)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Variant {
    A0,
    A1,
    A2,
    A3,
}

impl Variant {
    fn name(self) -> &'static str {
        match self {
            Variant::A0 => "A0",
            Variant::A1 => "A1",
            Variant::A2 => "A2",
            Variant::A3 => "A3",
        }
    }
}

/// One caller's sample: `queries` operations of `variant`, walking
/// `witnesses` from `offset` (or always the first witness when `same`).
fn caller_sample(
    host: &VerterHost,
    variant: Variant,
    witnesses: &[FlowFunctionReturnIdentity],
    offset: usize,
    same: bool,
    queries: usize,
) {
    let pick = |k: usize| {
        if same {
            &witnesses[0]
        } else {
            &witnesses[(offset + k) % witnesses.len()]
        }
    };
    match variant {
        Variant::A0 => {
            for k in 0..queries {
                assert!(
                    support::flow_return_query(host, pick(k)),
                    "a warm query answers"
                );
            }
        }
        Variant::A1 => support::flow_returns_in_one_request(host, (0..queries).map(pick)),
        Variant::A2 => {
            if same {
                support::flow_return_memo_reads(host, &witnesses[..1], 0..queries);
            } else {
                support::flow_return_memo_reads(host, witnesses, offset..offset + queries);
            }
        }
        Variant::A3 => {
            for k in 0..queries {
                support::flow_return_request_setup_only(host, pick(k));
            }
        }
    }
}

struct Point {
    samples_ns: Vec<u64>,
    store_view_reads: u64,
}

/// `samples` timed samples (after one untimed warm-up) of `callers`
/// persistent threads, each released by a start barrier and timed from the
/// first caller's start to the last caller's finish. A caller's panic
/// reaches both barriers and resumes on the driving thread.
fn run(
    host: &VerterHost,
    variant: Variant,
    witnesses: &[FlowFunctionReturnIdentity],
    same: bool,
    callers: usize,
    samples: usize,
    queries: usize,
) -> Point {
    let start = Barrier::new(callers + 1);
    let end = Barrier::new(callers + 1);
    let spans: Vec<Mutex<Option<(Instant, Instant)>>> =
        (0..callers).map(|_| Mutex::new(None)).collect();
    let failure: Mutex<Option<Box<dyn Any + Send>>> = Mutex::new(None);
    let failed = || failure.lock().expect("failure slot").is_some();
    let mut samples_ns = Vec::with_capacity(samples);
    let reads_before = host.provenance_snapshot().store_view_from_host_reads;
    std::thread::scope(|scope| {
        for caller in 0..callers {
            let (start, end, spans, failure, failed) = (&start, &end, &spans, &failure, &failed);
            std::thread::Builder::new()
                .name(format!("contention-caller-{caller}"))
                .stack_size(WORK_STACK_BYTES)
                .spawn_scoped(scope, move || {
                    let offset = caller * witnesses.len() / callers;
                    for _ in 0..=samples {
                        start.wait();
                        if failed() {
                            break;
                        }
                        let began = Instant::now();
                        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
                            caller_sample(host, variant, witnesses, offset, same, queries)
                        }));
                        let finished = Instant::now();
                        match outcome {
                            Ok(()) => {
                                *spans[caller].lock().expect("span") = Some((began, finished));
                            }
                            Err(panic) => {
                                failure.lock().expect("failure slot").get_or_insert(panic);
                            }
                        }
                        end.wait();
                        if failed() {
                            break;
                        }
                    }
                })
                .expect("spawn a caller");
        }
        for round in 0..=samples {
            start.wait();
            end.wait();
            if failed() {
                break;
            }
            let spans: Vec<(Instant, Instant)> = spans
                .iter()
                .map(|span| span.lock().expect("span").take().expect("every caller ran"))
                .collect();
            let began = spans.iter().map(|s| s.0).min().expect("a caller");
            let finished = spans.iter().map(|s| s.1).max().expect("a caller");
            if round > 0 {
                samples_ns.push(finished.saturating_duration_since(began).as_nanos() as u64);
            }
        }
    });
    if let Some(panic) = failure.into_inner().expect("failure slot") {
        std::panic::resume_unwind(panic);
    }
    Point {
        samples_ns,
        store_view_reads: host.provenance_snapshot().store_view_from_host_reads - reads_before,
    }
}

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

fn text_arg(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn median(samples: &[u64]) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

fn main() {
    let worker = std::thread::Builder::new()
        .name("signature-kernel-contention-probe".into())
        .stack_size(WORK_STACK_BYTES)
        .spawn(probe)
        .expect("spawn the measured-work thread");
    if let Err(panic) = worker.join() {
        std::panic::resume_unwind(panic);
    }
}

fn probe() {
    let args: Vec<String> = std::env::args().collect();
    let modules = arg(&args, "--modules", 24).max(1);
    let depth = arg(&args, "--depth", 8).max(1);
    let samples = arg(&args, "--samples", 15).max(1);
    // Per caller per sample; the default is ten sweeps of the default
    // corpus, as in the comparative harness's concurrent queries.
    let queries = arg(&args, "--queries", 1440).max(1);
    let host_workers = arg(&args, "--host-workers", 4).max(1);
    let (host, witnesses) = warm_host(modules, depth, host_workers);

    let plan: [(Variant, bool); 6] = [
        (Variant::A0, false),
        (Variant::A0, true),
        (Variant::A1, false),
        (Variant::A2, false),
        (Variant::A2, true),
        (Variant::A3, false),
    ];
    // `--only A3:disjoint` and `--callers 8` narrow the run to one point,
    // for a profiler to record.
    let only = text_arg(&args, "--only");
    let only_callers = args
        .iter()
        .any(|a| a == "--callers")
        .then(|| arg(&args, "--callers", 1));
    let mut points = Vec::new();
    for (variant, same) in plan {
        let keys = if same { "same" } else { "disjoint" };
        if only
            .as_deref()
            .is_some_and(|only| only != format!("{}:{keys}", variant.name()))
        {
            continue;
        }
        let mut by_callers = serde_json::Map::new();
        for callers in CALLER_COUNTS {
            if only_callers.is_some_and(|only| only != callers) {
                continue;
            }
            let point = run(&host, variant, &witnesses, same, callers, samples, queries);
            let operations = (callers * queries) as f64;
            let qps: Vec<u64> = point
                .samples_ns
                .iter()
                .map(|&ns| (operations / (ns.max(1) as f64 / 1e9)) as u64)
                .collect();
            // Every sample plus the warm-up ran `callers × queries`
            // operations.
            let performed = (callers * queries * (samples + 1)) as f64;
            by_callers.insert(
                callers.to_string(),
                serde_json::json!({
                    "callers": callers,
                    "samples_ns": point.samples_ns,
                    "qps": qps,
                    "median_qps": median(&qps),
                    "store_view_reads_per_query": point.store_view_reads as f64 / performed,
                }),
            );
        }
        points.push(serde_json::json!({
            "variant": variant.name(),
            "keys": keys,
            "by_callers": by_callers,
        }));
    }

    let document = serde_json::json!({
        "harness": "signature_kernel_contention_probe",
        "harness_version": 1,
        "rev": std::env::var("SK_BENCH_REV").unwrap_or_default(),
        "machine": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "logical_cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
        },
        "corpus": { "modules": modules, "depth": depth, "witnesses": witnesses.len() },
        "host_workers": host_workers,
        "queries_per_caller": queries,
        "points": points,
    });
    println!("{document}");
}
