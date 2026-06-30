# Perf-gate baselines — refresh governance

`block6.json` is the committed baseline manifest for the external-TS-engine
self-referential perf gate (design `docs/arch/external-ts-engine-architecture.md`
§2.7). It holds the methodology version, the pinned-baseline git ref, the
corpus content hash, the tsgo engine pin, the thread-count policy, and the
per-workload gated metrics + thresholds.

## What the gate compares (and what it does NOT)

The gate is **self-referential**: a candidate Verter build vs a **pinned-baseline
Verter build**, on the **same** runner / job / corpus / tsgo / thread regime
(axis-A is thread-pinned via `hostCpuThreads`; the axis-B `verter-tsc`/`verter_lsp`/
tsgo subprocesses do NOT accept a harness thread-pin and run at machine-default
parallelism — identical on both interleaved sides, NOT a harness-enforced fixed
count; see Deferred), interleaved, comparing **ratios** (candidate/baseline) —
never committed absolute milliseconds, which are too noisy on shared CI hardware.
Both axes are gated and **vize-free**:

- **AXIS A** — the native-compiler codegen-throughput regression
  (`compile_throughput_ratio`, `codegen_time_ratio`, `source_map_bytes`,
  `generated_carrier_count`, `output_bytes_ratio`). Each side runs in its OWN child
  process loading THAT side's `@verter/native` build, so the self-comparison is
  genuine (two native builds cannot coexist in one process). It is gated only on
  the signals the audited in-process compile actually emits — codegen throughput,
  the codegen(+source-map) emit-time split, output + source-map bytes, and the
  carrier-output count/invariant; axis-A per-PID peak RSS and the full non-checker
  aggregate are deferred (see below).
- **AXIS B** — the tsgo carrier-typecheck/LSP regression, gated on the
  honestly-TS-measurable subset: **exit-code + diagnostic-SET correctness** (cold +
  warm-incremental; the warm-incremental edit is type-CHANGING and must transition
  the dependent diagnostics), the genuinely-warm persistent-LSP latency
  distribution, the interactive latency distributions + the single-file-edit
  locality invariant, and the IDE-query hover/completion **content** equality. The
  `verter-tsc` child peak RSS is **DEFERRED** (verter-tsc spawns tsgo as a SEPARATE
  child, so the wrapper-PID VmHWM misses the engine's memory, and process-tree RSS
  is not cleanly portable in TS — see below). The deeper axis-B attribution that
  needs new Rust is DEFERRED (see below), never faked.

**Corpus scope — Vue `.vue` ONLY (Block-6).** The corpus generator, the axis-A
carrier discovery (`collectVueFiles`), and the LSP workloads (`firstVue`/`countVue`)
all target `.vue`. The gate makes **no claim of current Svelte perf coverage**;
Svelte (`.svelte`) carriers are a later block (B8), so a Svelte perf corpus +
carrier-extension discovery is DEFERRED (see below).

A one-sided metric (`higher-is-better` / `lower-is-better`) fails **only** when
`statisticRatio(candidate, baseline) > threshold` **and** the bootstrap 95% CI
lower bound on the ratio (the 2.5th percentile of the resampled ratio
distribution) also exceeds the threshold — so host variance alone cannot trip it.
An **`invariant`** metric (e.g. the generated-carrier count, output bytes) is
**two-sided**: it ALSO fails on a **drop** — `statisticRatio < 1/threshold`
**and** the 95% CI **upper** bound `< 1/threshold` — so a correctness-bearing
count/byte-size that **shrinks** (skipped work) is a regression, not a perf win.
The statistic is the **median** for time/throughput/RSS
scalars and the **named percentile (p50/p95/p99) over the pooled per-operation
distribution** for tail latency (never collapsed to the median). It is a ratio
of per-side statistics; the interleaving balances drift, it is not a paired
per-sample comparison. On a **full** run, a skipped/unavailable workload, a
degenerate (all-zero) gated metric, an engine-version mismatch, or an armed run
that fell back to self-check are **hard fails** — a misconfiguration or missing
instrument never reads as green. The gate gates the **axis-A Verter-owned codegen overhead**
(codegen(+source-map) emit time, codegen throughput, carrier output count, output
+ source-map bytes, and the carrier+source-map **content** hash) **plus the axis-B
END-TO-END regression signals** (the interactive/warm LSP latency distributions,
the carrier-typecheck exit-code + diagnostic-SET correctness, and the IDE-query
hover/completion content equality). The axis-B signals are whole-pipeline (Verter +
tsgo) end-to-end measurements, **not** a Verter-owned attribution — the
Verter-vs-tsgo phase split that would isolate Verter's share is deferred, so
"Verter-owned overhead/attribution" names ONLY the axis-A codegen/source-map emit
signal. The `verter-tsc` child peak RSS is **not** gated — verter-tsc spawns tsgo
as a separate child, so a wrapper-PID RSS would miss the engine (deferred, see
below). The gate does **not** gate cold total wall-time, which is
tsgo-checker-dominated and noisy (reported-only).

## Deferred — requires new Rust instrumentation (named tracked follow-ups)

A smaller HONEST gate is strictly better than a larger gate full of
non-discriminating metrics. The following attribution cannot be measured
honestly in TypeScript today and is **removed from the gated set** (recorded in
the manifest `deferred` section), NOT faked as a passing metric:

- **Axis-B `verter-tsc` peak RSS (the tsgo engine's resident set)** — `verter-tsc`
  spawns tsgo as a **separate child process** (`checker.rs` `invoke_checker`), so the
  harness's wrapper-PID `VmHWM` sample (Linux `/proc`) measures only `verter-tsc`'s
  own codegen resident set and **misses the tsgo type-check engine's (dominant)
  memory**. Gating that wrapper-PID number would be a memory metric that misses the
  engine. Full subprocess-**tree** peak RSS is not cleanly portable in TypeScript
  today (`/proc` is Linux-only; summing per-PID `VmHWM` over a discovered tree is not
  a faithful tree peak; Windows/macOS expose no equivalent), so axis-B peak RSS was
  **removed from the gated set** for both cold and warm-incremental. `rssBytes` is
  still recorded raw (wrapper-PID only); no metric gates it. Needs per-PID /
  per-engine peak (and steady-state) RSS emitted from the `verter-tsc` + tsgo audit
  substrate (a `verter-tsc --audit/json` mode). This subsumes the prior warm
  steady-state-RSS deferral.
- **Axis-A per-PID peak RSS** — the axis-A child IS the in-process native
  compile, so there is no spawned child to RSS-sample, and the audit footprint
  RSS sampler is not armed on the compile path (`process_rss_peak_bytes` is null
  on a real run); the Node-process maxRSS is not the compiler's resident set and
  is not substituted. Removed from the axis-A gated set; needs the audit footprint
  per-PID peak-RSS sampler armed on the native compile path
  (`verter_audit`/`verter_napi`). Axis-B cold/warm peak RSS is DEFERRED for a
  different reason (the wrapper-PID misses the tsgo child — see the axis-B peak RSS
  bullet above), not gated.
- **Axis-A full non-checker aggregate** — the compile audit record emits the
  codegen + source-map phase timings but NOT the parse/transform/transport phase
  timing, so the full `nonCheckerMs` aggregate is null on a real run; axis A gates
  the measurable codegen(+source-map) emit-time sub-signal (`codegen_time_ratio`)
  instead. Needs the parse/transform/transport phase timing emitted in the compile
  `RequestAuditRecord` (`verter_compiler`/`verter_napi`).
- **Axis-B cold/warm non-checker time split** — `verter-tsc` emits only
  `error TSxxxx` lines; needs a `verter-tsc --audit/json` mode emitting per-request
  `RequestAuditRecord`s. (The codegen(+source-map) emit-time sub-signal is gated on
  axis A only; the full non-checker aggregate is itself deferred — see above.)
- **Axis-B cold/warm carriers-generated count** — the static `.vue` file count
  never discriminates; needs a real per-request carriers-generated counter in
  `verter_audit`, surfaced via `verter-tsc --audit`.
- **Warm changed-carrier count** — needs a per-edit carriers-changed counter.
- **Warm true Program-reuse rate** — `verter-tsc` is always cold; needs a
  `verter-tsc --session`/daemon mode + a reuse-rate counter. (Warm-Program
  behavior is driven through the persistent LSP instead.)
- **Interactive Verter-vs-tsgo phase split** — needs a per-phase LSP latency
  split (Verter-owned vs tsgo) in the `verter_lsp` audit.
- **Source-map segment count** — `code_transform_ops` is a coarse proxy;
  `source_map_bytes` is the gated byte-size signal.
- **Dedicated hashing/cache/sync time bucket** — no such bucket exists; the
  `nonCheckerMs` attribution honestly sums producer parse/transform +
  store/transport ms (and is itself deferred from the axis-A gated set — see above).
- **Subprocess thread enforcement** — only axis-A's in-process host is
  thread-pinned; `verter-tsc`/`verter_lsp` need a thread-pin flag.
- **Svelte (`.svelte`) perf corpus + carrier-extension discovery** — a
  corpus-SCOPE deferral (not a Rust-instrumentation gap). Block-6's corpus +
  axis-A discovery (`collectVueFiles`) + LSP workloads (`firstVue`/`countVue`) are
  Vue `.vue`-only; Svelte LSP/IDE is a later block (B8). The gate makes no Svelte
  perf claim rather than fabricate one against a non-existent corpus. **Follow-up
  (gated on B8):** a pinned hermetic `.svelte` corpus + discovery generalized to
  the registered carrier extensions (`.vue` + `.svelte`) + the Svelte
  axis-A/axis-B workloads — landing only once Svelte support exists.

The offline **vize** comparison is a separate, manager-run, `VIZE_PATH`-gated
script (`pnpm --filter @verter/benchmark bench:perf:vize`). It is **never** part
of this gate and **never** a CI test — CI is 100% self-referential.

## Refreshing the baseline — a reviewed change, like an API change

A baseline refresh is **not** an incidental edit. It is a **dedicated change**:

1. **Title it** `perf: refresh baseline` (a conventional-commit `perf` change),
   on its own commit/PR — never folded into an unrelated change.
2. **Attach a before/after report.** Run the gate against the OLD baseline and
   the NEW candidate and record both sides' numbers (ratios, CIs, memory) in the
   PR description. A refresh that moves a threshold must justify the move with
   data, the same way an API change justifies a contract change.
3. **Preserve history.** Do **not** overwrite `block6.json` in place when the
   methodology or thresholds change materially: snapshot the prior manifest as
   `block6.<methodologyVersion>.json` (or a dated sibling) before bumping
   `methodologyVersion`, so a chain of "just under threshold" regressions stays
   visible across refreshes — a slow drift can never hide behind a moving
   baseline.
4. **Bump `baselineRef`** to the new pinned commit. The pinned baseline is the
   exact build the candidate is measured against; it should be a known-good
   commit on `main`.
5. **Keep the corpus hash in sync.** If the refresh accompanies a corpus change,
   `corpusHash` must equal the corpus `manifest.json` `contentHash` (the
   `gate.spec.ts` freshness rail and the gate's corpus-hash refusal both enforce
   this). A corpus change is itself a deliberate, reviewed act (see
   `test-corpora/perf/synthetic-15k/README.md`).

## Why thresholds, not absolute milliseconds

Absolute wall-times on GitHub-hosted runners vary run-to-run (shared tenancy,
thermal, noisy neighbors). Committing absolute ms as the pass/fail truth would
make the gate a flaky developer tax. The ratio + CI predicate is robust to that
variance while still catching a real regression: a candidate that is genuinely
slower shows a ratio whose **whole CI** sits above the threshold, regardless of
the absolute speed of the day's runner.

## The pinned-baseline lifecycle

`baselineRef` ships as `PENDING` — an **unarmed placeholder** that is physically
incapable of a green *armed* gate. While it is PENDING:

- The **scheduled nightly blocking run fails RED-until-armed** (exit nonzero). An
  unpinned baseline can never be a green blocking gate; the nightly red is the
  **intended honest state** until the baseline is pinned — not a green self-check.
- A manual `workflow_dispatch` run (and a local / on-demand `--smoke` run)
  executes the **same-commit self-check** (candidate === baseline, ~1.0 ratios,
  a loud NOT-ARMED warning) — this proves the predicate does not false-fail
  before the gate is armed against a real historical baseline.
- The gate itself forces self-check mode for a PENDING baseline and HARD-FAILS a
  non-self-check invocation; an *armed* comparison additionally REQUIRES
  `--baseline-root` (the baseline tsgo engine is resolved from that worktree,
  never the candidate root).

Arming the gate is itself a `perf: refresh baseline` change (see "Refreshing the
baseline" above) that sets `baselineRef` to the real pinned commit. **The baseline
is armed by a subsequent change** (not the same change) that pins the commit SHA,
since a squash-merge SHA cannot self-reference at author time.
