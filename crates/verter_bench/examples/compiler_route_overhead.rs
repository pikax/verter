//! Compiler route-overhead measurement harness.
//!
//! Compares `StandaloneCompiler`'s four compile routes — direct, prepared-
//! first, prepared-repeat, and batch — over the same small corpus: output
//! digest (must match across all four routes), a MEASURED carrier-parse
//! call count per route (see below — this is the discriminating
//! performance property, not `cold_build_count`/`reuse_count` alone),
//! wall-clock latency, and process RSS before/after each leg. The batch
//! leg submits the corpus TWICE (item_count = 2n over n distinct source
//! keys) rather than once — a batch of all-distinct items can never
//! observe intra-batch dedup, since every item is trivially its own
//! group whether or not `compile_batch` actually groups by content; the
//! repeated keys make `cold_build_count == n` (independently known from
//! `corpus.len()`, never from the batch's own report) an assertion a
//! no-dedup regression can actually fail.
//!
//! **Fixture FILES shared with
//! `crates/verter_compiler/src/standalone_prepared_tests.rs`'s own
//! result-identity corpus — by FILE PATH, not a shared Rust binding (they
//! live in different crates).** The two corpora are not identical and are
//! not meant to be: this harness takes each source once, because it measures
//! parse work per DISTINCT source and `build_corpus` asserts pairwise
//! distinctness. The identity corpus additionally repeats one source under a
//! different request and adds a diagnostic-producing source, so that the
//! identity digest's map and diagnostic slots carry real content. The
//! output-digest function
//! (`verter_compiler::standalone::direct_compile_output_digest`) is a
//! single production function reused verbatim by both — never a second
//! copy of that logic.
//!
//! **Counters are measured, not asserted into existence.** Deriving
//! `cold_build_count`/`reuse_count` from a leg's own loop-iteration
//! counts (`n`, `0`, `n * repeats`, ...) would be a tautology: those
//! values are true by construction of the loop regardless of whether
//! `compile_prepared` actually reused the parsed AST or silently
//! re-parsed on every call. This harness therefore reads
//! the real `compiler.carrier_parse.calls` counter
//! (`verter_audit::attribution`, the same counter `parse_sfc`/
//! `parse_svelte` increment in production and the same one
//! `performance-gates.toml`'s `A6_META_COMPILE_40_COLD_RUST` cell gates)
//! before and after each leg via `reset()`/`read()`, and asserts the
//! delta — never the loop's own iteration count. The prepared-repeat leg
//! in particular asserts the measured delta is exactly **zero**: that is
//! the load-bearing claim ("a reused `PreparedCarrier` never re-parses")
//! and the one a regression in `compile_prepared` would actually break.
//! The batch leg's self-reported `CompileBatchReport::cold_build_count`
//! is cross-checked against the same measurement rather than trusted on
//! its own. Requires `--features attribution` (see `required-features` on
//! this example in `Cargo.toml`) — without it
//! `verter_audit::attribution::{read, reset}` do not exist in the build
//! at all (that module's own doc: "There is no disabled stub returning
//! zero"), so an unmeasured run is a build error, never a silent pass.
//!
//! **This harness is a measurement, not a locked performance gate.** It
//! fixes no latency or RSS threshold and no `performance-gates.toml`
//! `[[cell]]` stands behind it, so a run of it never proves a
//! performance lock passed. What it DOES enforce immediately, as a
//! conjunctive check that exits 101 on breach, is output identity across
//! the four routes plus the measured parse-work invariants above. A
//! locked threshold would need an independent gate authority, neutral
//! calibration and holdout runs — never limits computed as a margin over
//! numbers this harness produced about the very code it measures.
//!
//! Usage:
//!   cargo run -p verter_bench --release --features attribution --example compiler_route_overhead
//!
//! Environment variables:
//!   VERTER_ROUTE_OVERHEAD_REPEATS   number of extra `compile_prepared` calls
//!                               per already-prepared carrier in the
//!                               prepared-repeat leg (default 5).
//!
//! RSS disclosure: `rss_before_bytes`/`rss_after_bytes` are single
//! `verter_audit::current_process_rss()` point-in-time reads around each
//! leg. `rss_peak_bytes` is a genuine sampled peak — a background thread
//! polls `current_process_rss()` every 200us for the leg's duration and
//! tracks the running max ([`RssSampler`]) — so a transient spike that a
//! single before/after read would miss is still captured. Per-platform
//! caveat, stated once here rather than re-derived per leg:
//! `current_process_rss`'s own doc records that macOS's source
//! (`getrusage().ru_maxrss`) is already the process's LIFETIME peak (it
//! never decreases), so on macOS every sample this thread takes converges
//! to be equal to `rss_after_bytes` — the peak-tracking thread adds no new
//! information there, it is genuinely needed only on Linux/Windows (whose
//! sources return CURRENT, not peak, RSS and can regress within a leg). On
//! a platform `current_process_rss` cannot read at all — `wasm32` (which
//! this native example never targets) or any other target its own
//! per-platform source list does not cover (its doc names its own
//! best-effort fallback) — every one of these fields reads `0`; that never
//! happens on the locked measurement runner (macOS), but no field is
//! silently mislabeled as a real reading either way.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use verter_audit::attribution::{read as read_attribution, reset as reset_attribution, WorkSite};
use verter_audit::current_process_rss;
use verter_compiler::compile::{VueExecutionInputs, VueMacroSemanticInput};
use verter_compiler::compile_request::{
    CompileProduct, CompileRequest, FrameworkCompileRequest, RuntimeProductRequest,
    SvelteCompileRequest, VueCompileRequest,
};
use verter_compiler::standalone::{
    direct_compile_output_digest, BatchCompileItem, DirectExecutionInputs, StandaloneCompiler,
    SvelteExecutionInputs,
};

const VUE_SIMPLE: &str = include_str!("../benches/fixtures/simple.vue");
const VUE_MEDIUM: &str = include_str!("../benches/fixtures/medium.vue");
const VUE_LARGE: &str = include_str!("../benches/fixtures/large.vue");
const VUE_VAPOR: &str = include_str!("../benches/fixtures/vapor_simple.vue");
const SVELTE_MARKUP_ONLY: &str = include_str!(
    "../../verter_svelte_conformance/corpus/fixtures/attr-lit-lit-attr-q-el-ext-plain-m.svelte"
);
const SVELTE_PROPS: &str = include_str!(
    "../../verter_svelte_conformance/corpus/fixtures/attr-dyn-lit-attr-q-el-ext-plain-m.svelte"
);
const SVELTE_STATE: &str = include_str!(
    "../../verter_svelte_conformance/corpus/fixtures/attr-dec-lit-attr-q-dynel-ext-plain-m.svelte"
);

enum Framework {
    Vue,
    Svelte,
}

struct Fixture {
    name: &'static str,
    source: &'static str,
    request: CompileRequest,
    framework: Framework,
}

fn vue_request(products: Vec<CompileProduct>) -> CompileRequest {
    CompileRequest::new(
        products,
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        Some("Comp.vue".to_string()),
        None,
        false,
        false,
    )
    .expect("test request constructs")
}

fn svelte_request(products: Vec<CompileProduct>) -> CompileRequest {
    CompileRequest::new(
        products,
        FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
        None,
        Some("Comp.svelte".to_string()),
        None,
        false,
        false,
    )
    .expect("test request constructs")
}

fn build_corpus() -> Vec<Fixture> {
    let corpus = vec![
        Fixture {
            name: "vue_simple",
            source: VUE_SIMPLE,
            request: vue_request(vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )]),
            framework: Framework::Vue,
        },
        Fixture {
            name: "vue_medium",
            source: VUE_MEDIUM,
            request: vue_request(vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )]),
            framework: Framework::Vue,
        },
        Fixture {
            name: "vue_large_dual_runtime",
            source: VUE_LARGE,
            request: vue_request(vec![
                CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
                CompileProduct::RuntimeServer(RuntimeProductRequest::default()),
            ]),
            framework: Framework::Vue,
        },
        Fixture {
            name: "vue_vapor",
            source: VUE_VAPOR,
            request: vue_request(vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )]),
            framework: Framework::Vue,
        },
        Fixture {
            name: "svelte_markup_only",
            source: SVELTE_MARKUP_ONLY,
            request: svelte_request(vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )]),
            framework: Framework::Svelte,
        },
        Fixture {
            name: "svelte_props",
            source: SVELTE_PROPS,
            request: svelte_request(vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )]),
            framework: Framework::Svelte,
        },
        Fixture {
            name: "svelte_state",
            source: SVELTE_STATE,
            request: svelte_request(vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )]),
            framework: Framework::Svelte,
        },
    ];
    // The batch leg expects `cold_build_count == corpus.len()`, which only
    // means anything while every fixture carries distinct source text: two
    // fixtures sharing a source would collapse into one content group and
    // silently lower the true expected count. Check it here instead of
    // asserting it in prose.
    let mut seen: HashSet<&'static str> = HashSet::new();
    for fixture in &corpus {
        assert!(
            seen.insert(fixture.source),
            "corpus fixture `{}` repeats another fixture's source text; the batch leg's distinct-group expectation requires pairwise-distinct sources",
            fixture.name,
        );
    }
    corpus
}

/// Combine one digest per fixture (in fixture order) into a single
/// route-level digest — original to this harness (NOT a duplicate of
/// `direct_compile_output_digest`, which this function's caller applies
/// per-fixture beforehand).
fn combine_digests(digests: &[[u8; 32]]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(digests.len() as u64).to_le_bytes());
    for digest in digests {
        hasher.update(digest);
    }
    *hasher.finalize().as_bytes()
}

fn hex(digest: &[u8; 32]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Background sampler tracking the running max of
/// `verter_audit::current_process_rss()` across one route leg — see this
/// module's own RSS disclosure doc for the per-platform caveat.
struct RssSampler {
    peak: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl RssSampler {
    fn start() -> Self {
        let initial = current_process_rss();
        let peak = Arc::new(AtomicU64::new(initial));
        let stop = Arc::new(AtomicBool::new(false));
        let peak_for_thread = Arc::clone(&peak);
        let stop_for_thread = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            while !stop_for_thread.load(Ordering::Relaxed) {
                let sample = current_process_rss();
                peak_for_thread.fetch_max(sample, Ordering::Relaxed);
                thread::sleep(Duration::from_micros(200));
            }
        });
        Self {
            peak,
            stop,
            handle: Some(handle),
        }
    }

    /// Stops the sampler thread and returns the peak it observed —
    /// including one final sample taken here, so a leg shorter than the
    /// poll interval still gets at least a start-of-leg and end-of-leg
    /// reading folded into the max.
    fn stop(mut self) -> u64 {
        self.peak
            .fetch_max(current_process_rss(), Ordering::Relaxed);
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.peak.load(Ordering::Relaxed)
    }
}

struct RouteReport {
    route: &'static str,
    cold_build_count: usize,
    reuse_count: usize,
    /// The MEASURED delta of `compiler.carrier_parse.calls`
    /// (`verter_audit::attribution::WorkSite::CarrierParse`) across this
    /// leg — read via `reset()`/`read()`, never assigned from the leg's
    /// own loop-iteration count. This is the field every assertion below
    /// actually checks; `cold_build_count` is reported for readability
    /// but is derived FROM this measurement (or, for the batch leg,
    /// cross-validated against it), never the other way around.
    measured_carrier_parse_calls: u64,
    latency_ms: f64,
    rss_before_bytes: u64,
    rss_after_bytes: u64,
    rss_peak_bytes: u64,
    digest: [u8; 32],
}

/// Current value of the `compiler.carrier_parse.calls` counter — the same
/// counter `parse_sfc`/`parse_svelte` increment in production
/// (`crates/verter_compiler/src/compile/mod.rs`,
/// `crates/verter_compiler/src/svelte/parser/tokenizer.rs`) and the same
/// one `performance-gates.toml`'s `A6_META_COMPILE_40_COLD_RUST` cell
/// gates. Call `reset_attribution()` at the start of a leg, run the leg,
/// then call this to get the leg's real parse-call delta.
fn carrier_parse_calls() -> u64 {
    read_attribution(WorkSite::CarrierParse).calls
}

/// Plain function (not a closure) so the returned lifetime is an ordinary
/// generic parameter inferred per call site, not an ambiguous closure
/// higher-ranked bound.
fn direct_inputs_for<'a>(
    framework: &Framework,
    vue_execution: &'a VueExecutionInputs,
    vue_macros: &'a VueMacroSemanticInput,
    svelte_execution: &'a SvelteExecutionInputs,
) -> DirectExecutionInputs<'a> {
    match framework {
        Framework::Vue => DirectExecutionInputs::Vue {
            execution: vue_execution,
            macros: vue_macros,
        },
        Framework::Svelte => DirectExecutionInputs::Svelte {
            execution: svelte_execution,
        },
    }
}

fn print_report(r: &RouteReport) {
    println!(
        "{{\"route\":\"{}\",\"cold_build_count\":{},\"reuse_count\":{},\"measured_carrier_parse_calls\":{},\"latency_ms\":{:.3},\"rss_before_bytes\":{},\"rss_after_bytes\":{},\"rss_peak_bytes\":{},\"digest\":\"{}\"}}",
        r.route,
        r.cold_build_count,
        r.reuse_count,
        r.measured_carrier_parse_calls,
        r.latency_ms,
        r.rss_before_bytes,
        r.rss_after_bytes,
        r.rss_peak_bytes,
        hex(&r.digest),
    );
}

fn main() {
    let repeats: usize = std::env::var("VERTER_ROUTE_OVERHEAD_REPEATS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

    let vue_execution = VueExecutionInputs::default();
    let vue_macros = VueMacroSemanticInput::Unavailable;
    let svelte_execution = SvelteExecutionInputs::default();

    let corpus = build_corpus();
    let compiler = StandaloneCompiler;

    let n = corpus.len();
    let mut reports = Vec::new();

    // ── Direct: N cold `compile()` calls ────────────────────────────
    {
        reset_attribution();
        let rss_before = current_process_rss();
        let sampler = RssSampler::start();
        let start = Instant::now();
        let mut digests = Vec::with_capacity(n);
        for fixture in &corpus {
            let output = compiler
                .compile(
                    fixture.source,
                    &fixture.request,
                    direct_inputs_for(
                        &fixture.framework,
                        &vue_execution,
                        &vue_macros,
                        &svelte_execution,
                    ),
                )
                .unwrap_or_else(|e| panic!("{}: direct compile failed: {e:?}", fixture.name));
            digests.push(direct_compile_output_digest(&output));
        }
        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
        let rss_peak = sampler.stop();
        let rss_after = current_process_rss();
        // MEASURED, not asserted into existence: `compile()` has no reuse
        // seam at all, so every one of the N calls must parse — a real
        // `compiler.carrier_parse.calls` delta, checked below, never the
        // loop's own iteration count.
        let measured = carrier_parse_calls();
        assert_eq!(
            measured, n as u64,
            "direct route: expected exactly one carrier parse per fixture (N cold compile() calls, no reuse seam) — measured {measured} carrier-parse calls, expected {n}. route-overhead measurement VIOLATED."
        );
        reports.push((
            RouteReport {
                route: "direct",
                cold_build_count: measured as usize,
                reuse_count: 0,
                measured_carrier_parse_calls: measured,
                latency_ms,
                rss_before_bytes: rss_before,
                rss_after_bytes: rss_after,
                rss_peak_bytes: rss_peak,
                digest: combine_digests(&digests),
            },
            digests,
        ));
    }

    // ── Prepared-first: N `prepare()` + N `compile_prepared()` ──────
    let mut prepared_carriers = Vec::with_capacity(n);
    {
        reset_attribution();
        let rss_before = current_process_rss();
        let sampler = RssSampler::start();
        let start = Instant::now();
        let mut digests = Vec::with_capacity(n);
        for fixture in &corpus {
            let prepared = compiler.prepare(fixture.source, &fixture.request);
            let output = compiler
                .compile_prepared(
                    fixture.source,
                    &prepared,
                    &fixture.request,
                    direct_inputs_for(
                        &fixture.framework,
                        &vue_execution,
                        &vue_macros,
                        &svelte_execution,
                    ),
                )
                .unwrap_or_else(|e| {
                    panic!("{}: prepared-first compile failed: {e:?}", fixture.name)
                });
            digests.push(direct_compile_output_digest(&output));
            prepared_carriers.push(prepared);
        }
        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
        let rss_peak = sampler.stop();
        let rss_after = current_process_rss();
        // MEASURED: `prepare()` parses once per fixture and
        // `compile_prepared()` must not parse again — a real
        // `compiler.carrier_parse.calls` delta of exactly N proves both
        // halves happened exactly once each. A `compile_prepared` that
        // silently re-parsed would push this delta ABOVE N (N prepare()
        // parses plus one extra per re-parsing fixture), which this
        // assertion catches: routing `compile_prepared`'s Vue arm back
        // through a full re-parse moves the measured count from 7 to 11
        // here. The prepared-repeat leg below isolates the
        // SAME defect class with zero `prepare()` calls at all, so its
        // own assertion is not dependent on this one having run first.
        let measured = carrier_parse_calls();
        assert_eq!(
            measured, n as u64,
            "prepared-first route: expected exactly one carrier parse per fixture (N prepare() calls) — measured {measured} carrier-parse calls, expected {n}. route-overhead measurement VIOLATED."
        );
        reports.push((
            RouteReport {
                route: "prepared-first",
                cold_build_count: measured as usize,
                reuse_count: n,
                measured_carrier_parse_calls: measured,
                latency_ms,
                rss_before_bytes: rss_before,
                rss_after_bytes: rss_after,
                rss_peak_bytes: rss_peak,
                digest: combine_digests(&digests),
            },
            digests,
        ));
    }

    // ── Prepared-repeat: reuse each carrier for `repeats` more calls ─
    {
        reset_attribution();
        let rss_before = current_process_rss();
        let sampler = RssSampler::start();
        let start = Instant::now();
        let mut digests = Vec::with_capacity(n);
        for (fixture, prepared) in corpus.iter().zip(prepared_carriers.iter()) {
            let mut last = None;
            for _ in 0..repeats {
                let output = compiler
                    .compile_prepared(
                        fixture.source,
                        prepared,
                        &fixture.request,
                        direct_inputs_for(
                            &fixture.framework,
                            &vue_execution,
                            &vue_macros,
                            &svelte_execution,
                        ),
                    )
                    .unwrap_or_else(|e| {
                        panic!("{}: prepared-repeat compile failed: {e:?}", fixture.name)
                    });
                last = Some(direct_compile_output_digest(&output));
            }
            digests.push(last.expect("repeats > 0"));
        }
        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
        let rss_peak = sampler.stop();
        let rss_after = current_process_rss();
        // THE load-bearing assertion: this leg makes ZERO `prepare()`
        // calls (every carrier was already built in the prior leg), so a
        // correct `compile_prepared` must produce a `compiler.
        // carrier_parse.calls` delta of exactly zero across `n * repeats`
        // compile calls. A `compile_prepared` that silently fell back to
        // re-parsing would move this counter to `n * repeats` and this
        // assertion — unlike a hardcoded `cold_build_count: 0` — actually
        // catches that.
        let measured = carrier_parse_calls();
        assert_eq!(
            measured, 0,
            "prepared-repeat route: expected ZERO carrier parses across {} compile_prepared() calls on already-prepared carriers — measured {measured} carrier-parse calls. This is the reuse-without-reparse guarantee; a nonzero count means compile_prepared silently re-parsed.",
            n * repeats,
        );
        reports.push((
            RouteReport {
                route: "prepared-repeat",
                cold_build_count: measured as usize,
                reuse_count: n * repeats,
                measured_carrier_parse_calls: measured,
                latency_ms,
                rss_before_bytes: rss_before,
                rss_after_bytes: rss_after,
                rss_peak_bytes: rss_peak,
                digest: combine_digests(&digests),
            },
            digests,
        ));
    }

    // ── Batch: one `compile_batch()` call over REPEATED group keys ───
    // The corpus has `n` fixtures with all-distinct source text (asserted
    // in `build_corpus`), so a 1-item-per-fixture batch (the original shape
    // of this leg) can never observe intra-batch dedup at all: every item is
    // already its own group, so `cold_build_count == n` holds trivially
    // regardless of whether `compile_batch` groups by content or just
    // assigns one group per item. This leg instead submits the corpus TWICE
    // (item_count = 2n), so the SAME `n` distinct-source keys repeat within
    // one batch call. `n` (the expected distinct-group count) stays
    // independently known from `corpus.len()` — never derived from what
    // `compile_batch` itself reports — so a broken implementation that
    // fails to dedup the repeat (one group per item, cold_build_count ==
    // item_count == 2n) is caught below, not just a broken implementation
    // that over-merges.
    let item_count = n * 2;
    {
        let items: Vec<BatchCompileItem<'_>> = corpus
            .iter()
            .chain(corpus.iter())
            .map(|f| BatchCompileItem {
                source: f.source,
                request: &f.request,
                inputs: direct_inputs_for(
                    &f.framework,
                    &vue_execution,
                    &vue_macros,
                    &svelte_execution,
                ),
            })
            .collect();
        assert_eq!(
            items.len(),
            item_count,
            "batch route: expected {item_count} items (corpus submitted twice)"
        );
        reset_attribution();
        let rss_before = current_process_rss();
        let sampler = RssSampler::start();
        let start = Instant::now();
        let batch = compiler.compile_batch(&items);
        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
        let rss_peak = sampler.stop();
        let rss_after = current_process_rss();
        assert_eq!(
            batch.results.len(),
            item_count,
            "batch route: expected {item_count} results, got {}",
            batch.results.len(),
        );
        // First-pass digests (items 0..n, one per distinct fixture) — this
        // is what the cross-route identity check below compares against
        // the other three legs.
        let digests: Vec<[u8; 32]> = corpus
            .iter()
            .zip(batch.results[..n].iter())
            .map(|(fixture, result)| {
                let output = result
                    .as_ref()
                    .unwrap_or_else(|e| panic!("{}: batch compile failed: {e:?}", fixture.name));
                direct_compile_output_digest(output)
            })
            .collect();
        // Second-pass digests (items n..2n, the REPEATED group keys) must
        // be byte-identical to the first pass's — this is the correctness
        // half of the dedup claim: a reused group must serve the SAME
        // content, not merely fewer parses. A `compile_batch` that dedup'd
        // by the wrong key (e.g. index instead of source) could pass the
        // count assertions below while silently serving item `i`'s output
        // for item `n + j`.
        for (fixture, (first, second)) in corpus
            .iter()
            .zip(digests.iter().zip(batch.results[n..].iter()))
        {
            let repeat_output = second.as_ref().unwrap_or_else(|e| {
                panic!(
                    "{}: batch compile failed on repeated item: {e:?}",
                    fixture.name
                )
            });
            let repeat_digest = direct_compile_output_digest(repeat_output);
            assert_eq!(
                *first, repeat_digest,
                "batch route: repeated item '{}' (second occurrence, sharing a group with the first) produced a DIFFERENT digest than its first occurrence — route-overhead measurement VIOLATED.",
                fixture.name,
            );
        }
        // `CompileBatchReport::cold_build_count` is `compile_batch`'s own
        // self-reported bookkeeping (a dedup-by-source-digest count) — NOT
        // trusted on its own. Cross-validated against the real measured
        // `compiler.carrier_parse.calls` delta: a `compile_batch` that
        // claimed to skip a parse while actually re-parsing (or vice
        // versa) would desynchronize these two numbers and this assertion
        // catches it.
        let measured = carrier_parse_calls();
        assert_eq!(
            measured, batch.report.cold_build_count as u64,
            "batch route: self-reported cold_build_count ({}) disagreed with the measured carrier-parse-call count ({measured}). route-overhead measurement VIOLATED — compile_batch's own accounting does not match what it actually did.",
            batch.report.cold_build_count,
        );
        // This leg submits the corpus's `n` distinct-source fixtures
        // TWICE (item_count = 2n), so `compile_batch`'s dedup-by-
        // `(framework, source, Vue parse-options)` group key must collapse
        // the repeat and find exactly `n` distinct groups — independently
        // known from `corpus.len()`, never derived from what
        // `compile_batch` itself reports. A regression that failed to
        // dedup the repeat at all (one group per item) would report
        // `item_count` (2n) groups here, not `n`; a regression that
        // over-merged distinct sources would report fewer than `n`. Both
        // directions are gated.
        assert_eq!(
            batch.report.cold_build_count, n,
            "batch route: corpus submitted twice ({item_count} items) over {n} distinct source keys, so compile_batch should form {n} distinct groups — got {}.",
            batch.report.cold_build_count,
        );
        // A BOOKKEEPING CONTROL, not a discriminator, and labelled as one:
        // `reuse_count` is `compile_batch`'s own counter, incremented by the
        // same loop that pushes results, so it moves in lockstep with any
        // regression in that loop and cannot independently witness that each
        // item really went through `compile_prepared`. What it does catch is
        // an internally inconsistent report — a run whose two counters
        // disagree with the item count it was handed. The discriminating
        // evidence for the prepared path is above: the measured
        // `carrier_parse.calls` delta equalling `cold_build_count`, and the
        // repeat-digest equality proving a reused group served the same
        // content.
        assert_eq!(
            batch.report.reuse_count, item_count,
            "batch route: expected reuse_count == item_count ({item_count}, every item served through compile_prepared) — got {}.",
            batch.report.reuse_count,
        );
        println!(
            "batch route: item_count={item_count} distinct_group_count={n} (independently known from corpus.len())"
        );
        reports.push((
            RouteReport {
                route: "batch",
                cold_build_count: batch.report.cold_build_count,
                reuse_count: batch.report.reuse_count,
                measured_carrier_parse_calls: measured,
                latency_ms,
                rss_before_bytes: rss_before,
                rss_after_bytes: rss_after,
                rss_peak_bytes: rss_peak,
                digest: combine_digests(&digests),
            },
            digests,
        ));
    }

    for (report, _) in &reports {
        print_report(report);
    }

    let baseline = reports[0].0.digest;
    for (report, _) in &reports[1..] {
        assert_eq!(
            baseline, report.digest,
            "route '{}' produced different output than route '{}' — cross-route result identity VIOLATED",
            report.route, reports[0].0.route,
        );
    }
    println!("all routes produced byte-identical output ({} fixtures)", n);

    // Cross-leg summary of the property this whole cell exists to prove:
    // parse work collapses to zero once a carrier is reused, and every
    // number here is the measured `compiler.carrier_parse.calls` delta,
    // not a value assigned from the leg's own loop-iteration count.
    let by_route = |name: &str| {
        reports
            .iter()
            .find(|(r, _)| r.route == name)
            .map(|(r, _)| r.measured_carrier_parse_calls)
            .unwrap_or_else(|| panic!("route '{name}' missing from reports"))
    };
    println!(
        "measured carrier_parse.calls — direct={} prepared-first={} prepared-repeat={} batch={} (n={n}, repeats={repeats})",
        by_route("direct"),
        by_route("prepared-first"),
        by_route("prepared-repeat"),
        by_route("batch"),
    );
}
