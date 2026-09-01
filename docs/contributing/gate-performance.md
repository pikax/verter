# Gate performance

Working notes and durable rationale for `node scripts/gate.mjs`, the canonical Rust gate. This document
owns the retained one-build/one-run and single-test-universe rules after the historical roadmap ruling
bundle was retired.

## Local and exhaustive execution policies

`node scripts/gate.mjs` is the local fail-fast policy. Surface 1 omits `--no-fail-fast`. After the one
archive/list and all post-list preconditions, Surface 1 is the gate verdict.

TODO: re-enable the shipped-cfg lane (`SHIPPED_CFG_LANE_ENABLED` in `scripts/gate-internals.mjs`)
before the program closes. Until then the gate does not execute tests with `debug_assertions` /
overflow-checks off. That is the only path that catches a state mutation written inside a
`debug_assert!` argument — a silent no-op in every shipped build, while compiling and passing in
debug. `cargo check --workspace --release` compiles the shipped cfg but runs nothing, so it does
not cover this class. The skip is disclosed on every run in the verdict line and the summary; a
PASS means Surface 1 passed.

When the lane is restored: Surface 1 and the small shipped-contract nextest run omit `--no-fail-fast`.
They start concurrently — unless `deriveGateLaneResourceSplit` finds the configured build-jobs/
test-threads ceiling too small to split across both lanes without oversubscribing it (either axis below 2),
in which case the lanes run serially instead (Surface 1 first, then shipped-cfg), still under the same
fail-fast/cancellation rules. When Surface 1 produces a hard receipt, the runner cancels a live shipped
step and prevents the lane's not-yet-admitted contract. Required coverage is then incomplete, so the
invocation can never emit PASS or PASS-WITH-TOLERATED. A shipped-first failure never cancels Surface 1.
A green local run completes every required receipt and has the ordinary canonical PASS contract.

`node scripts/gate.mjs --exhaustive` currently also skips the shipped-cfg lane; it changes Surface 1
failure collection only (`--no-fail-fast`). When the lane is restored it preserves the historical
CI/diagnostic policy: Surface 1 and the small shipped-contract nextest run add `--no-fail-fast`, both
post-list lanes are awaited despite ordinary hard failures, and the shipped lane remains serial
(`check -> contract`, with contract admitted only after a successful check). CI, release,
cached full gates, complete diagnostics, and comparable performance runs must pass the flag explicitly.
This choice is argv-only; no ambient CI environment variable changes it.

## Post-list overlap and isolation

The overlap boundary is deliberately narrow. Build-prerequisite, oracle, harness, freshness, Vue-macro,
the single dev archive, its one list, sidecar restoration, suite inventory, and trybuild coverage all settle
before fan-out. Surface 1 then reads that immutable archive using
`<runnerTarget>/lanes/surface-1/target` and `gate-work/lanes/surface-1/{work,extract,output.log}`. The shipped
check and contract use `<runnerTarget>/lanes/shipped-cfg/target` and
`gate-work/lanes/shipped-cfg/{work,output.log}`. These mutable roots are validated as absolute,
runner-contained, and pairwise disjoint before creation. Command `cwd` remains the repository. The shipped
target is intentionally cold relative to the front archive target; its check warms its following contract.

One supervisor owns both lanes. Its deadline remains the original whole-gate absolute deadline, its stall
clock observes one aggregate live-progress vector, and one same-snapshot sum of every disjoint registered
process forest is compared with the unchanged memory ceiling. Deadline, stall, memory-monitor failure, or
setup abort closes admission and reaps every exact registered process forest. Before mutex release,
teardown also runs a provenance backstop over the minimized mutex-owned `runnerTarget` umbrella (nested
historical registration roots are deduplicated); that sweep is distinct from exact identity-bound forest
reaping. Raw per-lane output (concurrent or serial, per `deriveGateLaneResourceSplit`'s `concurrent` flag)
is buffered lane-locally and replayed exactly once under the existing Surface/check/contract headers, so
parseable nextest rows remain deterministic.

## WASM JavaScript-boundary lane

The workspace's `#[wasm_bindgen_test]` cases are `#[cfg(target_arch = "wasm32")]`. No host-target run can
compile them, so archive-backed Surface 1 can never execute them: they existed, compiled, and proved
nothing. The canonical gate runs them itself, in a lane that is required on every real invocation — bare
and exhaustive — and is never path-filtered inside the gate. `--prepare` runs no test and is exempt.

Two halves:

- **Prerequisite preflight**, in the Cargo phase, immediately before the lane that consumes it. It is
  deliberately NOT one of the pre-archive preflights: those are node-only and must stay that way, while
  this check needs a working `cargo` and Rust toolchain. It derives
  the lane's scope from `cargo metadata --no-deps` (every package that dev-depends on `wasm-bindgen-test`,
  with the directories of that package's `test = true` targets as its scan roots), derives the required
  runner version from that scope's `wasm-bindgen` dependency, proves the `wasm32-unknown-unknown` standard
  library is installed (`rustc --print target-libdir` names a directory holding a compiled `core` rlib —
  the path alone is printed for any recognised triple and proves nothing), resolves
  `wasm-bindgen-test-runner` on the same constructed PATH the lane's Cargo child receives, and requires its
  version to EQUAL the derived one. The runner and the library are one ABI: a skew compiles cleanly and
  then fails inside generated JavaScript, which reads as a product regression rather than a toolchain
  problem. A missing target, a missing runner, a skew, an empty scope, or an empty case inventory each fail
  setup with exit 127 and `WASM-LANE PREREQUISITE MISSING`, naming the exact prerequisite and the command
  that produces it. The gate never installs a prerequisite (its verdict must not depend on a mutation it
  performed) and never degrades to a skip.
- **Execution**, immediately after that check — after the one archive/list front half and before the host
  lanes — serialized: no other
  Cargo is live, so the lane takes the whole build ceiling rather than a share of it, under the same
  supervisor, deadline, stall clock and process-forest RSS ceiling as every other phase, on its own
  `<runnerTarget>/lanes/wasm-js-boundary/target` root. Each scoped package runs
  `cargo test --target wasm32-unknown-unknown -p <pkg> --tests` with
  `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER` set to the resolved absolute runner (so `.cargo/config.toml`
  never acquires a repo-wide runner every unrelated wasm invocation would inherit).

The lane's receipt is required by the same fixed-order reducer that owns the rest of the verdict, and it
carries two independent obligations. A **terminal result**: every announced `running N tests` must be
closed by a `test result:` line — a transcript that announced work and never closed it is a run that did
not finish. And **executed-vs-declared parity**: both the announced count and the *executed* count
(`passed + failed`) must equal an independent source scan of the scope's `#[wasm_bindgen_test]` attributes.
Those two counts are not interchangeable — the harness prints `running N tests` *before* it applies the
ignore filter, and the scan counts the attribute either way because `#[ignore]` is a separate line, so a
comparison against the announced count alone would report full coverage for a lane that executed nothing.
An unexecuted boundary case has no tolerated disposition here: it is fixed or deleted, never ignored. Zero
declared cases is a failure, and a superset is equally untrusted (it means the scan missed a source the
lane compiled). Both comparisons are applied per package as well as on the totals, because a sum
reconciles under compensating drift between two scoped packages. There is no enable flag and no skip
disposition: a run with no wasm receipt reduces to the same FAIL as any other incomplete coverage.

One shape the scan cannot see: a `#[wasm_bindgen_test]` written in a lib *outside* `#[cfg(test)]` compiles
into both the lib test binary and any integration test binary that links it, so it executes twice against a
single scanned attribute and fails parity as a superset. Keep boundary cases inside `#[cfg(test)]`.

Both discovery inputs are derived rather than listed — the package set from `cargo metadata`, the case
inventory from a tree scan — so a case added to a file the lane has never seen is executed with no edit to
the gate. `scripts/gate-wasm-lane-selftest.mjs` (run by `pnpm run test:scripts`) drives each of those
decisions directly and proves it fails in its own direction.

CI provisions the target and the runner for the two jobs that already invoke the canonical gate
exhaustively (`ci.yml`'s `rust-test` and `release.yml`'s `test`). It does not own or duplicate the lane;
`wasm-build` keeps its separate artifact responsibilities. The workflow pin is a literal because a
GitHub action input cannot be computed, so `gate-wasm-lane-selftest.mjs` also asserts every workflow pin
equals the version the tree declares — a dependency bump that forgot a workflow fails locally instead of
reddening CI at the skew check.

## Measured resource defaults and prepare warm environment

Build jobs and test threads are independent policies even though their current
measured winners match on the reference host. Omitted test threads default to
`min(12, availableParallelism())`. Omitted build jobs are CPU-clamped and use
the effective child-tree memory ceiling: 12 jobs at `>=16GiB`, 8 jobs at
`>=12GiB`, and 4 below that. An explicit positive `--build-jobs` or
`--test-threads` value remains exact and is not clamped. The child-tree memory
ceiling itself stays `max(512MiB, 50% of physical RAM)` unless explicitly set.

The Windows reference host had 32 available logical CPUs and 127.17 GiB RAM.
All cold archive runs began with an absent, distinct target and produced the
same 804 MiB archive / 86 listed binaries:

| build jobs | dev archive | peak RSS |
|---:|---:|---:|
| 4 | 422.775s | 7.72 GiB |
| 8 | 283.920s | 9.90 GiB |
| 12 | **234.792s** | 11.60 GiB |

The Surface-1 comparison held revision, selection, nextest groups, inventory
(24,843 run / 597 configured skips / none unrun), and terminal outcome constant
(24,837 passed / the same 6 Windows-only failures outside this slice and outside
the canonical verdict / no timeout or exec failure):

| test threads | Surface 1 | nextest wall | peak RSS |
|---:|---:|---:|---:|
| 4 | 695.769s | 683.990s | 1.85 GiB |
| 8 | 426.028s | 416.015s | 3.08 GiB |
| 12 | **357.825s** | **347.571s** | 3.84 GiB |

These Windows measurement runs were red and are scheduling evidence, not gate-pass receipts. The original
baseline was red, while the later macOS canonical run is proven green. The six Windows product/test
failures remain non-canonical and outside this harness-performance work; they are not tolerated or fixed
here. The Windows measurements do establish
twelve as the fastest tested point on both controlled axes with ample memory
headroom on the 127.17-GiB host. They do **not** establish twelve build jobs as
portable on the documented 24-GiB host: its default ceiling is 12 GiB, only
0.40 GiB above the measured 11.60-GiB peak. That host therefore defaults to
eight jobs (9.90-GiB measured peak, 2.10-GiB headroom); twelve remains available
as an explicit override, or by default once the effective ceiling reaches 16
GiB. The test-thread axis remains twelve because its measured peak is only 3.84
GiB. `.config/nextest.toml` continues to serialize
`shared-provider-live` and `lsp-server-unit` at `max-threads = 1` under their
exact default/CI selectors; global concurrency never widens those lanes.

On Windows, the three listed `kind: "proc-macro"`, `build-platform: "host"`
suites contain 41 real tests and remain in the full warm loop. Their standalone
test harnesses dynamically load Rust host libraries. Before the fix, nextest
ran them successfully but `--prepare`'s direct `spawnSync(..., ["--list"] )`
inherited no host-libdir PATH and each exited `0xC0000135`
(`STATUS_DLL_NOT_FOUND`). The prepare launcher now reads
`rust-build-meta.platforms[suite.build-platform].libdir`, requires available
CWD-independent absolute metadata, and prepends that directory to the already-
sanitized per-child PATH. It still accepts only status 0. Missing metadata,
missing binaries, signals, timeouts, and every non-zero exit fail setup; no
proc-macro name is allowlisted, filtered, skipped, or tolerated.

## Lane resource partition (post-list concurrency, not per-lane sizing)

`deriveGateResourceLimits` sizes ONE build-jobs/test-threads ceiling from the host's CPU count and memory
tier (above). That ceiling used to be handed to Surface 1 and the shipped-cfg lane INDEPENDENTLY — each
lane's `cargo`/`nextest` invocation was sized to the full ceiling, even on a ceiling wide enough that the
two lanes run CONCURRENTLY after archive/list ("Post-list overlap and isolation" above; today the split
falls back to serial only when the ceiling is too small on either axis — see below). On an 8-core host this
logged
`resource ceiling: cargo build jobs=8, test threads=8` and then ran both lanes at that width AT THE SAME
TIME — an 8-core host requesting 16 cores' worth of concurrent work for the whole overlap window. Surface
1's nextest run (test-threads=8) overlapped the shipped-cfg lane's own cold `cargo check --workspace
--all-targets` compile (build-jobs=8) and then its `nextest run` build+execute (build-jobs=8, then
test-threads=8). This means self-competition for the machine's cores was POSSIBLE for the entire duration
of the longer lane — **it is a hypothesis, not a proven finding, that this self-competition caused the
specific timeouts and Surface-1 wall-clock inflation observed earlier**: no controlled Cargo-backed
before/after comparison has been run to isolate self-competition from other variance (host load, disk
cache state, thermal throttling). Treat "tests that pass in 2-85s standalone lost budget to a gate
competing with itself for the machine" as the leading explanatory theory, not a measured conclusion, until
such a comparison exists.

`deriveGateLaneResourceSplit` (`scripts/gate-internals.mjs`) fixes the oversubscription itself (which IS
directly measured, independent of the causal hypothesis above) by partitioning the ONE ceiling across the
two lanes instead of handing each lane the whole thing: Surface 1 (the full workspace test universe) gets
the majority share, the shipped-cfg lane (a small package-scoped contract — ten-ish tests, whose own
wall-clock matters far less than Surface 1's) gets a minority share (`SHIPPED_CFG_LANE_SHARE = 0.25`,
floored at 1 core). Both the build-jobs axis and the test-threads axis are split independently, so the
COMBINED demand on either axis never exceeds the ceiling `deriveGateResourceLimits` derived from the host —
**including at a ceiling of 1**, where a numeric per-lane split can't give both lanes >= 1 unit while still
summing to 1 (a lane cannot run `cargo`/`nextest` with 0 build jobs or 0 test threads). At that ceiling the
split function reports `concurrent: false` and `orchestrateGateLanes` runs the two lanes SERIALLY instead
of admitting them together — shipped-cfg's `runShippedLane` is not even invoked until Surface 1 has
settled, so only one lane's `cargo`/`nextest` invocation is ever live and the combined demand at any
instant stays at the ceiling, never double it. The split — including the `concurrent` flag and which
scheduling mode ran — is logged explicitly (`lane resource partition (combined demand bounded to the
ceiling above — …): surface-1 build-jobs=… test-threads=…; shipped-cfg build-jobs=… test-threads=…`) and
recorded on `telemetry.lanes.resourceSplit` (plus per-lane `buildJobs`/`testThreads` fields on
`telemetry.lanes.surface1` / `.shippedCfg`), so a telemetry artifact proves the combined demand fit the
ceiling rather than asserting it. The front archive/list phase (before either lane starts) is sequential
and keeps the full, unsplit ceiling — there is no concurrent consumer to share it with at that point.

Stall detection remains a SEPARATE, still-open gap: the supervisor observes one aggregate live-progress
vector across both lanes ("Post-list overlap and isolation" above), so a hung shipped-cfg lane can hide
behind Surface 1's throughput advancing the aggregate vector. Splitting resource sizing does not change
that; per-lane stall detection is unaddressed follow-up work, not folded into this fix.

## Comparable gate telemetry

After acquiring the single-flight mutex, the gate starts a report-only `GateTelemetry` accumulator and
gives all startup reporting probes one separate hard aggregate deadline. The canonical build/test deadline
is established only after startup collection settles, so telemetry consumes none of that budget. Its
whole elapsed time ends only after the oversize-source advisory and teardown. Stable gate phase IDs are
`build-prerequisite`, `oracle-cache`, `harness-smoke-vapor`, `harness-smoke-typescript`,
`freshness-tooling`, `vue-macro-oracle-check`, `vue-macro-oracle-tests`, `dev-archive`, `dev-list`,
`surface-1`, `shipped-check`, `shipped-contract`, `advisory`, and `teardown`. A failed command remains in
the table; an unreached or watchdog-aborted phase makes measurement completeness `partial`. A fully
executed red test run can still have complete measurement, because completeness describes observations,
not correctness.

A local fail-fast cancellation marks a live shipped phase `aborted` and any unadmitted remainder `not-run`,
so measurement is `partial`. That is corroborating telemetry only: the pure receipt reducer independently
requires a complete parseable Surface result, successful shipped check, complete contract analysis, and
expected-count parity before either green verdict.

Each phase retains its lane-local `peakRssBytes` and process count. The whole monitored RSS value also
consumes the supervisor's highest same-snapshot aggregate across every live forest, including its total
process count and per-lane `{ rssBytes, processCount }` contributions; it is never `max(surface, shipped)`
and never a sum of peaks from different times. Synchronous prerequisite probes are timed but are not
sampled by the contained-child watchdog, so their RSS row is zero rather than an invented estimate.

The bounded environment fingerprint contains UTC instant; OS type/platform/architecture/release/version;
CPU model plus logical and available counts; total RAM; Node/V8; rustc and Cargo versions/hosts;
cargo-nextest; initial target state (`absent`, `empty`, or `nonempty`); configured jobs, threads, memory and
profiles; `NEXTEST_PROFILE`; incremental mode; wrapper basename; and sccache presence/hit rate when safely
available. It excludes hostname, username, full wrapper paths, and broad environment dumps. Every command
probe has a short timeout clamped to the remaining aggregate startup-reporting time, refuses to spawn when
that allowance is spent, and uses direct unignorable termination rather than catchable `SIGTERM`. A probe
failure reports
`unavailable`, warns, marks measurement partial, and cannot affect the gate verdict/failure accumulator.

Terminal text telemetry is mirrored to `gate-work/gate-telemetry-v1.log`, with additive schema-v1 JSON at
`gate-work/gate-telemetry-v1.json`. Cargo capability probes add stable HTML `--timings` to exactly the dev
nextest archive, shipped-cfg check, and shipped-cfg contract. Before each producer the runner removes only
its exact producing target's `cargo-timings/cargo-timing.html` source and that phase's old destination, then
validates and snapshots the settled report immediately to the paths below. Proven pre-launch absence is the
normal freshness root. If an exact-file clear fails, the snapshot is accepted only when a pre/post SHA-256
content identity proves that the producer replaced the old report; unchanged or ambiguous identity warns,
marks measurement partial, and is refused even when the mtime is within filesystem tolerance.
The dev archive reads the front target source; shipped check and contract read the isolated shipped target
source sequentially. The three final artifact identities remain unchanged.

- `gate-work/cargo-timings/dev-nextest-archive.html`
- `gate-work/cargo-timings/shipped-cfg-check.html`
- `gate-work/cargo-timings/shipped-cfg-contract.html`

Archive-backed Surface 1 never receives `--timings` because it performs no Cargo build. Unsupported help
output retains the old argv. Missing, stale, unreadable, or uncopyable reports emit explicit warnings and
never cause a retry, second archive/run, or verdict change.

Nextest attribution counts every final terminal identity as `processCount` and every parseable duration as
`timedCount`; `totalSec` sums only timed identities. The total, package/crate, binary and family rows all
carry those fields. Legacy `count` remains exactly equal to `timedCount`, and `perPackage`, `perBinary`, and
`topFamilies` remain present. Retry/progress events still use the existing final-status supersession rule.
The shipped contract uses the same summarizer and the already parsed dev archive package map—no new list or
test subprocess.

## Canonical conformance-harness preflight

The gate now detects two broad JavaScript harness incompatibility classes
before the expensive Rust archive build. After the build-prerequisite load
and pinned oracle-cache realization succeed, and before freshness tooling or
Cargo, it runs `packages/framework-conformance-harness/bin/gate-smoke.mjs`
in two explicit modes:

- `vapor` calls the harness's exported `ensureVaporRuntimePreloaded()` path,
  including its real jsdom bootstrap and pinned with-vapor runtime import.
- `typescript` calls the exported `observeTypeScript()` over a small
  multi-file, in-memory workspace-domain graph, then asserts the intended
  export and zero relevant diagnostics. This exercises the canonical virtual
  host rather than a mirrored TypeScript setup.

Each mode runs separately through `runContainedStep`, so the existing whole-
gate deadline, stall detection, process-tree RSS ceiling, and teardown apply.
Each also emits its own duration/peak-RSS telemetry line. Success requires the
exact-key, mode-bound JSON object receipt
`{"schema":"verter-harness-smoke/v1","mode":"<mode>","ok":true}`,
which the executable writes only after the real work and assertions complete.
A non-zero exit, timeout, stall, memory ceiling/monitor abort, signal, spawn
failure, or missing/invalid/mismatched/extra-key receipt fails setup with exit 127 and
`HARNESS-SMOKE FAILED [<mode>]`; neither mode can warn, skip, or degrade.

`gate-selftest.mjs` drives this ordering through the real production CLI but
cannot install into the developer checkout: its child PATH makes `pnpm`
unresolvable and provides temporary executable `buf`/`oxfmt` shims. The leg
refuses to launch unless those facts are proven, and accepts only the
production preflight's non-installing `already-present` or `path-fallback`
outcome before the Cargo stand-in is reached. Its omission plant continues
past strict command-constructor refusal and proves the mutated real CLI omits
TypeScript while improperly reaching Cargo.

Oracle-cache realization alone therefore proves only that the pinned install
closure is usable. It is not evidence that Vapor's DOM-sensitive bootstrap or
the workspace TypeScript virtual host is compatible with the current Node and
harness environment; the two real smokes own those claims.

## Memory ceiling — measured, not assumed (step 1)

`parsePosixProcessTableRss` used to sum RSS by POSIX process-GROUP membership,
which undercounted catastrophically (nextest reassigns each executing test to
its own fresh process group, so a pgid-based sum saw only the
`cargo`/`cargo-nextest` wrapper — "43 MiB across 1 process(es)" on a real
full-workspace run). Fixed to a parent-pid TREE walk. That fix is not
report-only: `parsePosixProcessTableRss` feeds the memory-ceiling watchdog
(`gate-internals.mjs` `runContainedStep`), so it arms the ceiling for the
first time — a ceiling that had never realistically been reachable before.

A dedicated `node scripts/gate.mjs --memory-limit 18GiB` run on a 24 GiB host
(default ceiling `max(512MiB, 50% RAM)` = 12 GiB) measured peak RSS per phase
with the corrected sampler:

| phase                        | peak RSS  | processes\* |
|-------------------------------|-----------|-------------|
| archive build (dev profile)   | 4.87 GiB  | —           |
| SURFACE 1 (nextest run)       | 1.88 GiB  | —           |
| SURFACE 2 (in-process)        | 1.54 GiB  | —           |
| archive build (shipped-cfg)   | 5.50 GiB  | —           |
| SURFACE 3 (nextest run)       | 1.02 GiB  | —           |

\* The per-sample process count reported alongside peak RSS was, at
measurement time, a second bug in the same telemetry: `runContainedStep`
overwrote its process-count field on every watchdog tick rather than
capturing the count AT the sample that set the new peak, so a build's true
mid-run peak (several concurrent rustc) could be reported paired with a much
later, lower-parallelism sample's count. Fixed by tracking a dedicated
`peakRssProcessCount`, updated only when a sample ties or extends the running
max — see the `peakRssProcessCount` doc comment in `gate-internals.mjs` and
the discriminating scripted-sampler regression test in
`gate-memory-selftest.mjs`.

**Ceiling decision:** the measured maximum across every phase is **5.50 GiB**
(shipped-cfg archive build), 45.8% of the 12 GiB default — **6.5 GiB of
headroom**. The default ceiling is correct as-is and was NOT raised; arming
the watchdog does not break a default (no `--memory-limit`) run on this class
of host. This machine's default is itself `50% of physical RAM`, so a much
smaller host (roughly <11 GiB RAM) would compute a default ceiling below the
measured absolute peak — noted for awareness, not acted on here, since no
such host is in scope for this measurement.

The gate run that produced this table also surfaced one pre-existing,
unrelated failure: `verter_lsp sync_coordinator::tests::hanging_provider_diagnostics_do_not_starve_verter_owned_batch`,
owned by a different block.

## Surface-replay reduction (step 2)

The directive named two files (`architecture_guards.rs`,
`output_projector_residual_guards.rs`, ~250 tests / ~40,000 lines) as the
source-scanning inventory compiled into `verter_session`'s consolidated test
binary and replayed under all three surfaces for no behavioral reason (a
source scan has no shared-process failure mode and no `debug_assertions`
sensitivity). The real surface, measured on the tree, was twelve files.
Verified per file (not by filename) rather than assumed:

| file | verdict | why |
|---|---|---|
| `architecture_guards.rs` | **kept in `verter_session`** | one `#[test]` (`foundations_guards::external_corpus_paths_not_present_outside_gated_tests`) is bound by name into `typeinfo_ignored_test_manifest.rs`'s `live_guard!` registry, which requires the bound test to run in the SAME binary the canonical gates execute; the file's `foundations_guards`/`w5f_test_archaeology`/`general_test_archaeology`/`packages_ts_archaeology` submodules also share internal helper functions across that boundary. Relocating it needs a scoped design decision (repoint the one binding, or prove the "same binary" invariant can relax), not a same-session mechanical move. |
| `output_projector_residual_guards.rs` | moved | pure scan plus a check against `verter_session`'s public API (sealed `TypeExpr`/`NoTypeExpr` capability fence) — never runs it |
| `whole_env_consumer_graph_native_inventory.rs` | moved | pure `syn`/`walkdir` scan of `verter_session`'s `src/` |
| `residual_type_expr_body_reader_inventory.rs` | moved | pure `syn`/`walkdir` scan |
| `handle_capable_consumer_guards.rs` | moved | pure `syn`/`walkdir` scan (reads sibling `verter_session` test files that stayed behind, e.g. `architecture_guards.rs`) |
| `tracked_paths_are_portable.rs` | moved | scan plus a check against `verter_session`'s public API (`framework::descriptor` registry) — never runs it |
| `scanners_replacement.rs` | moved | pure JSON/schema scan |
| `tracked_paths_no_machine_roots.rs` | moved | pure `git ls-files` scan |
| `framework_known_bug_manifest.rs` | moved | pure `syn` scan |
| `svelte_typecheck_gate.rs` | kept | drives a real `tsc.js` subprocess — not a scan |
| `vue_macro_tsc_typecheck_gate.rs` | kept | drives a real `tsc.js` subprocess — not a scan |
| `defect_b_corpus_prevention_gate.rs` | kept, N/A | `#![cfg(feature = "external-corpus")]` — already excluded from the default gate entirely; not compiled into any of the three surfaces to begin with |

The eight movable files relocated to a new crate, `verter_source_policy_gate`
(`crates/verter_source_policy_gate/tests/cases/`), with its own single
`tests/main.rs` (Anti-Binary-Growth-compliant — no allowlist entry needed).
`cargo nextest run --workspace` (Surface 1) still runs its 180 tests once, as
before; Surface 2 (`verter_session`-only shared-process) and Surface 3 (the
five-package filterset above) select by PACKAGE, so this crate's tests are
structurally invisible to both regardless of what it depends on — no filter
change was needed on either surface to achieve the exclusion. Two of the
eight (`tracked_paths_are_portable.rs`, `output_projector_residual_guards.rs`)
turned out to depend on `verter_session`'s public API partway through the
file (not visible from the file's own header `use` block) rather than being
pure disk scans; that dependency does not affect the exclusion, since it is
a library dependency, not a package-selection criterion.

Four of the eight scan `verter_session`'s own `src/` tree, so their
`crate_root()` helper — previously `CARGO_MANIFEST_DIR`, correct only because
`CARGO_MANIFEST_DIR` used to BE `verter_session`'s own directory — was
re-anchored to `workspace_root().join("crates/verter_session")` explicitly.
`output_projector_residual_guards.rs` additionally has one genuine
self-reference (reading its own source for a doc/list-parity check), which
resolves against the new crate's own `CARGO_MANIFEST_DIR` via a second,
distinct `own_crate_root()` helper — the two must not be conflated.

Verified: `cargo test -p verter_source_policy_gate` (180/180 passing,
non-vacuous — several tests assert the production scan found real production
matches, not an empty/mocked tree), plus
`cargo test -p verter_session --test main -- cases::typeinfo_ignored_test_manifest cases::g_misc0::critical_rules_have_guards cases::architecture_guards cases::integration_test_layout_guard`
(318/319, 1 pre-existing `#[ignore]`, 0 failed) — covering the R6 registry
scanner (finds the moved guard names at their new path), the `live_guard!`
binary-identity binding (still resolves — `architecture_guards.rs` stayed
put), and the anti-binary-growth layout guard (the new crate needs no
allowlist entry).

The non-blocking oversize-source-line advisory (scan of `crates/*/src` for
files over 1,500 lines) used to run synchronously before the mutex
acquisition, ahead of the archive build. It is now kicked off on a deferred
microtask and joined in the outer `finally` — after `runGate`/`runPrepare`
returns — so its cost — a fast synchronous scan, but still nonzero —
overlaps the multi-hundred-second archive build instead of serializing ahead
of it. On a gate path that reaches a terminal verdict the advisory line
therefore prints just after `VERDICT:`. It does not universally follow a
verdict: `--prepare` prints `PREPARED_NOT_GATE` instead, and several early
`runGate` returns emit no `VERDICT:` line at all.

Not done in this step (later blocks, per the ratified plan): removing
Surface 2 as a blanket rerun, removing the second whole-workspace archive,
or moving `architecture_guards.rs`. The deletion bar is unchanged — a surface
is removed only after a seeded-defect mutation proves its replacement
catches what the surface existed for.

## Removing Surface 2 and Surface 3 (step 3)

Implements the SINGLE-TEST-UNIVERSE directive. Surface 2 (the direct
in-process `verter_session` libtest replay) is deleted entirely — its
shared-process rationale now lives inside Surface 1's ONE archive/run as
`verter_session/tests/cases/shared_process_contract.rs`, ordinary `#[test]`
functions that perform many operations sequentially in the one process
nextest already gives each test. Surface 3 (a second whole-workspace archive
under `no-debug-assertions`, plus a 5-package/15,454-test filtered nextest
run) is deleted and replaced with a small shipped-cfg guard:
`cargo check --workspace --all-targets --profile no-debug-assertions`
(compile-only) followed by `cargo nextest run -p verter_shipped_cfg_contract
--cargo-profile no-debug-assertions` (package-scoped, not `--workspace` —
never a second whole-workspace archive).

**Seeded-defect proof, Surface 2 replacement.** Temporarily wrapped
`VerterHost::get_component_meta` with a process-global `HashMap` keyed only
on canonical id, ignoring host identity — exactly the class Surface 2 existed
to catch. `cargo test -p verter_session --test main shared_process_contract::`
went from 8/8 passing to 4/8 failing:
`create_use_drop_recreate_hosts_stay_independent`,
`multiple_hosts_coexist_in_one_process_without_cross_contamination`,
`cache_and_registry_invalidation_survives_many_repeated_edits`,
`failure_then_recovery_in_one_host_and_process` — each failing with the
leaked host's stale prop names instead of the querying host's own content.
Seed reverted, all 8 pass again.

**Seeded-defect proof, Surface 3 replacement.** Temporarily split
`get_component_meta` into a `#[cfg(debug_assertions)]` correct arm and a
`#[cfg(not(debug_assertions))]` arm that drops the last resolved prop — the
same observable hazard a `debug_assert!(mutating_call())` produces (correct
under `dev`, silently different under the shipped configuration).

At the time of this proof `verter_shipped_cfg_contract` carried 8 tests
total: 6 behavioral tests under `mod shipped_cfg_behaviour` (profile-
agnostic — expected to pass under EITHER profile) plus 2 profile-sanity
canaries under `mod profile_contract`
(`debug_assertions_are_off_under_this_profile`,
`overflow_checks_are_off_under_this_profile` — DESIGNED to fail under `dev`
and pass only under `no-debug-assertions`, by construction, seed or no
seed). An unfiltered `cargo test -p verter_shipped_cfg_contract` therefore
cannot report "6/6" under `dev` — the two canaries run too and fail
regardless. The actual commands, filtered to isolate the 6 behavioral tests
so the seed's effect is unambiguous:

- `cargo test -p verter_shipped_cfg_contract shipped_cfg_behaviour::` under
  `dev`: stayed 6/6 passing (the seed is inert under `dev` — the
  `#[cfg(debug_assertions)]` arm is what compiles in, so the correct value is
  still returned — proving Surface 1 alone cannot see this class).
- `cargo test -p verter_shipped_cfg_contract --profile no-debug-assertions
  shipped_cfg_behaviour::` (bare `cargo test`'s custom-profile flag is
  `--profile`, not nextest's `--cargo-profile`): went to 6 FAILED / 0 passed
  (the `#[cfg(not(debug_assertions))]` arm compiles in instead and drops the
  prop).
- Unfiltered `cargo nextest run -p verter_shipped_cfg_contract
  --cargo-profile no-debug-assertions` (all 8 tests, matching what the live
  guard actually runs): 2 passed (the two canaries, which pass under this
  profile by construction) / 6 failed (the seeded behavioral tests).

Seed reverted: unfiltered `cargo nextest run -p verter_shipped_cfg_contract
--cargo-profile no-debug-assertions` back to 8/8 passing.

**Structural-elimination audit** (the three SINGLE-TEST-UNIVERSE
prerequisites for retiring the guard entirely): every
`debug_assert!`/`debug_assert_eq!`/`debug_assert_ne!` call in `crates/*/src`
carrying a method/function call was enumerated (134 call-bearing sites out
of 274 total) and inspected — none perform semantic work; every hit is a
pure observation (`is_none`, `is_empty`, `len`, `matches!`, `is_char_boundary`,
etc.). Every production `#[cfg(debug_assertions)]` block
(`resolver_core/runtime_values.rs`, `parse.rs`, `host_manage/eval_env.rs`) is
a non-breaking oracle cross-check whose comparison result never feeds the
returned value. Explicit overflow-safe arithmetic (`checked_*`/`wrapping_*`/
`saturating_*`) was **not** audited workspace-wide — out of scope for this
block. Two of three prerequisites verified holding; the third is unverified
and clearly not holding project-wide, so the guard stays per the directive.

**Measured, this branch, both runs with `node scripts/gate.mjs
--memory-limit 16GiB` on a heavily loaded shared host** (several unrelated
concurrent `cargo`/`rust-lock.sh` jobs from other agents were running
throughout both measurements — Surface 1's absolute wall-clock is noisy
against the pre-step-3 497s baseline for that reason):

| phase | before (this branch) | after |
|---|---|---|
| dev archive build | ~443s cold / 65s warm | 10s (warm, unchanged mechanism) |
| SURFACE 1 | 497s, 24777 tests | 712-925s, 24785-24793 tests (contended host; test count differs only by the 8 new `shared_process_contract` tests) |
| SURFACE 2 | 3 suites in-process (sequential, on top of the above) | **removed** |
| SURFACE 3 | 221s test run + a second whole-workspace archive build | **removed** |
| shipped-cfg guard | n/a | 100s check + 68s run = 168s (cold, first commit) → 6s + 5s = 11s (warm, second run) |

The unambiguous, contention-independent win is the shipped-cfg guard number:
what used to require a full second `cargo nextest archive --workspace`
compile (~443s cold / 65s warm) plus a 221s/15,454-test run now costs 11s
warm — no second whole-workspace compile at all, ever. Final verdict both
runs: `VERDICT: PASS` (first run, pre-existing-in-this-branch failures fixed
in the same commit; second run clean).
