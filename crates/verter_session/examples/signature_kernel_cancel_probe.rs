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
//!   cancelled attempt ran on, until its complete answer.
//!
//! A cancellation that lands after the request completed is not a stop
//! sample; the probe counts those separately so a reader sees how many
//! fractions actually interrupted work.

use std::sync::Arc;
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
    let rounds = arg(&args, "--rounds", 6).max(1);

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

    let mut cancel_stop_ns = Vec::new();
    let mut restart_ns = Vec::new();
    let mut completed_before_cancel = 0_u64;
    for _ in 0..rounds {
        for fraction in FRACTIONS {
            let host = loaded_host(modules, depth);
            let token = CancellationToken::new();
            let delay = Duration::from_nanos((cold_median as f64 * fraction) as u64);
            let started = Instant::now();
            let canceller = {
                let token = token.clone();
                std::thread::spawn(move || {
                    // Spin to the instant: a sleep's granularity is coarser
                    // than the request on some platforms.
                    let at = started + delay;
                    while Instant::now() < at {
                        std::hint::spin_loop();
                    }
                    let cancelled_at = Instant::now();
                    token.cancel();
                    cancelled_at
                })
            };
            let outcome = request(&host, token);
            let returned_at = Instant::now();
            let cancelled_at = canceller.join().expect("the canceller finishes");
            match outcome {
                Outcome::Cancelled => cancel_stop_ns.push(
                    returned_at
                        .saturating_duration_since(cancelled_at)
                        .as_nanos() as u64,
                ),
                Outcome::Complete => completed_before_cancel += 1,
            }
            let retried = Instant::now();
            let retry = request(&host, CancellationToken::new());
            restart_ns.push(retried.elapsed().as_nanos() as u64);
            assert_eq!(retry, Outcome::Complete, "the retry completes");
        }
    }

    let document = serde_json::json!({
        "harness": "signature_kernel_cancel_probe",
        "harness_version": 1,
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
        "completed_before_cancel": completed_before_cancel,
        "workloads": {
            "cold_request": { "samples_ns": cold_request_ns },
            "cancel_stop": { "samples_ns": cancel_stop_ns },
            "restart": { "samples_ns": restart_ns },
        },
    });
    println!("{document}");
}
