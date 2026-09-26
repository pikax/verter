//! Signature-kernel cancellation probe — the §12 cancellation/restart
//! latency of `docs/arch/signature-kernel.md`, measured on ONE tree.
//!
//! The comparative harness (`signature_kernel_bench`) builds the same source
//! against the pre-kernel baseline, which has no caller-cancellable entry,
//! so cancellation cannot be one of its matched workloads. This probe runs
//! on the candidate alone and records absolute distributions (the V0
//! release-baseline form §12 asks for), never a ratio or a verdict.
//!
//! ```text
//! # full run (the defaults the performance runner uses)
//! cargo run --release -p verter_session --example signature_kernel_cancel_probe
//! # quick run
//! cargo run --release -p verter_session --example signature_kernel_cancel_probe -- \
//!     --modules 4 --depth 4 --cold-samples 3 --rounds 3
//!
//! cargo run --release -p verter_session --example signature_kernel_cancel_probe -- \
//!     [--modules N] [--depth N] [--cold-samples N] [--rounds N]
//! ```
//!
//! The request is one flow-return query whose answer reads every module's
//! descriptor-instantiation chain (breadth, so the connected-query depth
//! guard never ends it early). Distributions:
//!
//! * `cold_request` — the request on a freshly loaded host, uncancelled.
//! * `cancel_stop`  — from `cancel()` to the cancelled request returning
//!   `Cancelled`, with the cancellation landing at fixed fractions of the
//!   median cold request.
//! * `restart`      — the retry under a fresh token on the host the
//!   cancelled attempt ran on, until its complete answer; a request that
//!   completed before its cancellation landed gives no restart sample.
//!
//! Each distribution is recorded in aggregate (`workloads`) and per
//! injection point (`by_fraction`): the point is `fraction × the median
//! cold request` measured by this invocation, the same for every round.
//! The canceller thread exists and waits before the request starts, and
//! the request's own start instant is the origin it counts from, so thread
//! creation never shifts where a cancellation lands. Each point also
//! records where its cancellations actually landed (`landed_ns`, from the
//! request's start to `cancel()`).
//!
//! A cancellation that lands after the request completed is not a stop
//! sample; the probe counts those separately, per point, so a reader sees
//! how many fractions actually interrupted work.

use std::sync::{Arc, Barrier, OnceLock};
use std::time::{Duration, Instant};

use verter_scheduler::cancellation::CancellationToken;
use verter_session::host_flow_return_audit::FlowReturnError;
use verter_session::semantic_query::ReturnProjectionDemand;
use verter_session::{HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity, TopLevelOwnerId};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};

const ROOT: &str = "/sk/cancel.ts";

/// Where in the cold request each cancellation lands, as a fraction of the
/// median cold request time.
const FRACTIONS: [f64; 5] = [0.1, 0.3, 0.5, 0.7, 0.9];

/// The measured work runs on an explicit stack for the same reason as the
/// comparative harness: descriptor chains recurse through the
/// implementation, and a platform-sized main stack is not the variable
/// under test.
const WORK_STACK_BYTES: usize = 64 << 20;

fn module_path(i: usize) -> String {
    format!("/sk/m{i}.ts")
}

/// One module: a generic chain `depth` calls long and a witness over it.
fn module(i: usize, depth: usize) -> String {
    let mut source = format!(
        "export function c0_{i}<T>(x: T) {{ return {{ v: x, tag: \"m{i}\" as const }}; }}\n"
    );
    for level in 1..depth {
        let previous = level - 1;
        source.push_str(&format!(
            "export function c{level}_{i}<T>(x: T) {{ return c{previous}_{i}(x); }}\n"
        ));
    }
    let last = depth - 1;
    source.push_str(&format!(
        "export function witness{i}(v: number | string) {{ return c{last}_{i}(v); }}\n"
    ));
    source
}

/// The request's function: one field per module, each reading that
/// module's chain.
fn root(modules: usize) -> String {
    let mut source = String::new();
    for i in 0..modules {
        source.push_str(&format!("import {{ witness{i} }} from \"./m{i}\";\n"));
    }
    source.push_str("export function all(v: number | string) {\n  return {\n");
    for i in 0..modules {
        source.push_str(&format!("    m{i}: witness{i}(v),\n"));
    }
    source.push_str("  };\n}\n");
    source
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

fn loaded_host(modules: usize, depth: usize) -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    for i in 0..modules {
        upsert(&host, &module_path(i), &module(i, depth));
    }
    upsert(&host, ROOT, &root(modules));
    host
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Complete,
    Cancelled,
}

fn request(host: &VerterHost, token: CancellationToken) -> Outcome {
    let identity = FlowFunctionReturnIdentity {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(ROOT),
            owner: TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from("all"),
            space: LocatorSymbolSpace::Value,
        },
        function_part: FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let carrier = host.get_flow_return_type_with_audit_cancellable(
        &identity,
        ReturnProjectionDemand::whole_return(),
        token,
    );
    match carrier.as_result() {
        Ok(result) if result.degradation().is_none() => Outcome::Complete,
        Err(FlowReturnError::Cancelled) => Outcome::Cancelled,
        // A probe that timed a degraded answer or another failure would
        // publish a latency for work nobody asked for.
        Ok(result) => panic!("the request degraded: {:?}", result.degradation()),
        Err(other) => panic!("the request failed: {other:?}"),
    }
}

fn arg(args: &[String], flag: &str, default: usize) -> usize {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(|v| {
            v.parse()
                .unwrap_or_else(|_| panic!("{flag} takes a number"))
        })
        .unwrap_or(default)
}

/// One injection point: where its cancellations aim, and what each landed
/// cancellation measured.
#[derive(Default)]
struct Point {
    fraction: f64,
    /// From the request's start to `cancel()`: `fraction` of the median
    /// cold request.
    delay: Duration,
    landed_ns: Vec<u64>,
    cancel_stop_ns: Vec<u64>,
    restart_ns: Vec<u64>,
    completed_before_cancel: u64,
}

/// One cancelled request and the instants its latencies are read from.
struct Landing {
    outcome: Outcome,
    started: Instant,
    cancelled_at: Instant,
    returned_at: Instant,
}

/// Run the request on `host` and cancel it `delay` after it starts.
///
/// The canceller thread is created and parked BEFORE the request starts;
/// the request thread publishes its own start instant as it enters the
/// request, and the canceller spins from there to the landing instant — a
/// sleep's granularity is coarser than the request on some platforms. So
/// neither thread creation nor a wake-up is inside the delay.
fn cancelled_request(host: &VerterHost, delay: Duration) -> Landing {
    let token = CancellationToken::new();
    let origin: Arc<OnceLock<Instant>> = Arc::new(OnceLock::new());
    let ready = Arc::new(Barrier::new(2));
    let canceller = {
        let token = token.clone();
        let origin = Arc::clone(&origin);
        let ready = Arc::clone(&ready);
        std::thread::spawn(move || {
            ready.wait();
            let started = loop {
                if let Some(started) = origin.get() {
                    break *started;
                }
                std::hint::spin_loop();
            };
            let at = started + delay;
            while Instant::now() < at {
                std::hint::spin_loop();
            }
            let cancelled_at = Instant::now();
            token.cancel();
            cancelled_at
        })
    };
    ready.wait();
    let started = Instant::now();
    origin.set(started).expect("the origin is published once");
    let outcome = request(host, token);
    let returned_at = Instant::now();
    let cancelled_at = canceller.join().expect("the canceller finishes");
    Landing {
        outcome,
        started,
        cancelled_at,
        returned_at,
    }
}

fn median(samples: &[u64]) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

fn main() {
    let worker = std::thread::Builder::new()
        .name("signature-kernel-cancel-probe".into())
        .stack_size(WORK_STACK_BYTES)
        .spawn(run)
        .expect("spawn the measured-work thread");
    if let Err(panic) = worker.join() {
        std::panic::resume_unwind(panic);
    }
}

fn run() {
    let args: Vec<String> = std::env::args().collect();
    let modules = arg(&args, "--modules", 24).max(1);
    let depth = arg(&args, "--depth", 8).max(1);
    let cold_samples = arg(&args, "--cold-samples", 10).max(1);
    // Twenty rounds: every injection point gets twenty stop samples per
    // invocation, enough for its own p95 once the runner pools invocations.
    let rounds = arg(&args, "--rounds", 20).max(1);

    let mut cold_request_ns = Vec::with_capacity(cold_samples);
    for _ in 0..cold_samples {
        let host = loaded_host(modules, depth);
        let started = Instant::now();
        let outcome = request(&host, CancellationToken::new());
        cold_request_ns.push(started.elapsed().as_nanos() as u64);
        assert_eq!(
            outcome,
            Outcome::Complete,
            "an uncancelled request completes"
        );
    }
    let cold_median = median(&cold_request_ns);

    let mut points: Vec<Point> = FRACTIONS
        .iter()
        .map(|&fraction| Point {
            fraction,
            delay: Duration::from_nanos((cold_median as f64 * fraction) as u64),
            ..Point::default()
        })
        .collect();
    for _ in 0..rounds {
        for point in &mut points {
            let host = loaded_host(modules, depth);
            let landing = cancelled_request(&host, point.delay);
            point.landed_ns.push(
                landing
                    .cancelled_at
                    .saturating_duration_since(landing.started)
                    .as_nanos() as u64,
            );
            match landing.outcome {
                Outcome::Cancelled => point.cancel_stop_ns.push(
                    landing
                        .returned_at
                        .saturating_duration_since(landing.cancelled_at)
                        .as_nanos() as u64,
                ),
                Outcome::Complete => point.completed_before_cancel += 1,
            }
            // A retry after a request that completed is a warm read, not a
            // restart: it still runs, but only a cancelled attempt's retry
            // is a restart sample.
            let retried = Instant::now();
            let retry = request(&host, CancellationToken::new());
            if landing.outcome == Outcome::Cancelled {
                point.restart_ns.push(retried.elapsed().as_nanos() as u64);
            }
            assert_eq!(retry, Outcome::Complete, "the retry completes");
        }
    }

    // The aggregate distributions pool every point.
    let mut cancel_stop_ns = Vec::new();
    let mut restart_ns = Vec::new();
    for point in &points {
        cancel_stop_ns.extend_from_slice(&point.cancel_stop_ns);
        restart_ns.extend_from_slice(&point.restart_ns);
    }
    let completed_before_cancel: u64 = points.iter().map(|p| p.completed_before_cancel).sum();
    let by_fraction: Vec<serde_json::Value> = points
        .iter()
        .map(|point| {
            serde_json::json!({
                "fraction": point.fraction,
                "delay_ns": point.delay.as_nanos() as u64,
                "completed_before_cancel": point.completed_before_cancel,
                "landed_ns": { "samples_ns": point.landed_ns },
                "cancel_stop": { "samples_ns": point.cancel_stop_ns },
                "restart": { "samples_ns": point.restart_ns },
            })
        })
        .collect();

    let document = serde_json::json!({
        "harness": "signature_kernel_cancel_probe",
        "harness_version": 2,
        "rev": std::env::var("SK_BENCH_REV").unwrap_or_default(),
        "machine": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "logical_cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
            "work_stack_bytes": WORK_STACK_BYTES,
        },
        "corpus": { "modules": modules, "depth": depth },
        "fractions": FRACTIONS,
        "rounds": rounds,
        "cold_request_median_ns": cold_median,
        "completed_before_cancel": completed_before_cancel,
        "workloads": {
            "cold_request": { "samples_ns": cold_request_ns },
            "cancel_stop": { "samples_ns": cancel_stop_ns },
            "restart": { "samples_ns": restart_ns },
        },
        "by_fraction": by_fraction,
    });
    println!("{document}");
}
