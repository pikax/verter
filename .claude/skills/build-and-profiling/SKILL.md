---
name: build-and-profiling
description: "Build dependency chains, rebuild sequences, profiling with MCP, and Analysis MCP server setup for Verter"
---

# Build Dependency Chain & Profiling

## Build Dependency Chain

When changing Rust code, rebuild downstream artifacts in order:

```
verter_compiler + verter_semantic + verter_session + verter_ffi (Rust crates)
    ↓ cargo build
verter_napi (NAPI-RS cdylib)    verter_lsp (LSP binary)    verter_wasm (wasm-bindgen cdylib)
    ↓ pnpm run build:native         ↓ pnpm run build:lsp       ↓ pnpm run build:wasm (dev, no wasm-opt)
@verter/native (.node binary)   verter-lsp (target/<triple>/debug/)  or `pnpm --filter @verter/wasm build`
    ↓                                ↓                       (publication lane: + cached wasm-opt)
@verter/unplugin (bundler)      verter-vscode (F5/VSIX)         ↓
    ↓                                                       @verter/playground (browser, via its own
playground build (Vite)                                     `sync:wasm` — no implicit copy from @verter/wasm)
    ↓
playground E2E tests
```

`pnpm build` (host developer build) is native + lsp + ts only — it never touches WASM. `pnpm dist`
(publication-ready artifacts) adds the full WASM lane (bindgen + cached `wasm-opt` via
`scripts/wasm-opt-cache.mjs`). See `CLAUDE.md` → Build.

## Common Rebuild Sequences

| What changed                          | Rebuild commands (in order)                                                                            |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| Rust crate (`verter_compiler`)            | `pnpm run build:native` → rebuild any downstream consumer                                              |
| Rust LSP (`verter_lsp`)               | `pnpm run build:lsp` (or `build:lsp:release` for optimized) → restart VS Code extension host           |
| Unplugin (`packages/unplugin`)        | `pnpm run build:ts` (or just rebuild unplugin)                                                         |
| Playground after Rust/unplugin change | `pnpm run build:native` → `cd packages/playground && rm -rf dist node_modules/.vite && pnpm run sync:wasm && npx vite build` (playground never implicitly copies the WASM artifact — see Key Details; its own `build` script runs `sync:wasm` too and is the safer default) |
| WASM, developer iteration             | `pnpm run build:wasm` (bindgen only, no `wasm-opt`, no playground copy)                                |
| WASM, publication-ready               | `pnpm --filter @verter/wasm build` (bindgen + cached `wasm-opt`)                                       |
| Host developer build                  | `pnpm build` (runs native → lsp → ts, no WASM)                                                         |
| Publication-ready artifacts           | `pnpm dist` (runs native → lsp → wasm → ts in correct order)                                           |

## Key Details

- `@verter/unplugin` depends on `@verter/native` — compiles `.vue` files at build time via the Rust native binary
- `@verter/playground` uses `@verter/unplugin` (devDep) for Vue SFC compilation, and `@verter/wasm` (dep) for the in-browser editor
- Native binary lives in `packages/native/dist/` after `build:native`
- LSP binary lives in `target/<host-triple>/debug/verter-lsp` (or `.../release/verter-lsp` with `build:lsp:release`) — `pnpm run build:lsp`/`build:lsp:release` and `build-host.mjs` all pass an explicit `--target <host-triple>` (see the "one explicit host target" build-lane rule), so the output is triple-qualified, not the bare `target/debug/`/`target/release/` a plain `cargo build` (no `--target`) would produce
- Clear Vite cache (`node_modules/.vite`) when rebuilding playground after native changes

### Canonical gate build telemetry

`node scripts/gate.mjs` capability-probes Cargo's stable HTML timings and, when supported, adds
`--timings` only to the dev `cargo nextest archive`, shipped-cfg `cargo check`, and package-scoped shipped
contract `cargo nextest run`. It never adds the flag to archive-backed Surface 1 and never launches a
second build/run for telemetry. The dev archive reads the front target timing source; shipped check and
contract read their isolated shipped-lane target source sequentially. Each overwrite-prone report is cleared at its exact file,
validated after each producing command settles, and snapshotted immediately under
`gate-work/cargo-timings/`. The source must be proven absent before launch, or—if exact-file deletion
fails—have a changed pre/post SHA-256 content identity; unchanged or ambiguous sources are refused.
Version/help probe budgets share a separate hard aggregate startup-reporting deadline and hard-terminate
their direct child. The canonical build/test deadline begins only after startup collection settles;
reporting failures warn and mark telemetry partial without changing the gate verdict.

The terminal `gate-work/gate-telemetry-v1.{log,json}` pair records a bounded tool/host fingerprint,
stable build/test phase durations, cold/empty/warm target state, lane layout/policy, and the maximum
same-snapshot aggregate live-forest RSS with its total process count and per-lane contributions. Use these artifacts when comparing build-job or
test-thread configurations; do not infer build cost from per-test summed seconds.
After the one archive/list and all post-list preconditions, Surface 1 overlaps the serial shipped
`check -> contract` lane in separate Cargo targets. The shipped target is intentionally cold relative to
the front archive target; its check warms the contract. One supervisor retains the absolute whole-gate
deadline, aggregate stall vector, and one aggregate memory ceiling, and raw output is replayed once in
Surface/check/contract order. The bare local gate cancels shipped after a hard Surface receipt and may omit
the remaining shipped phases; use
`node scripts/gate.mjs --exhaustive` for every comparable full-run benchmark. A truncated local report is
correctly `partial` and must not be compared against an exhaustive baseline as a performance improvement.

The current independently measured policy caps at 12 Cargo build jobs and 12
nextest threads. Both are CPU-clamped; omitted build jobs are also memory-tiered
from the effective child-tree ceiling (12 jobs at >=16 GiB, 8 at >=12 GiB,
otherwise 4). On the
32-logical-CPU / 127.17-GiB Windows reference host, cold target-absent dev
archives measured 422.775s / 283.920s / 234.792s at 4/8/12 jobs, with peak RSS
7.72 / 9.90 / 11.60 GiB. Identical-inventory Surface 1 runs measured 695.769s /
426.028s / 357.825s at 4/8/12 threads, with peak RSS 1.85 / 3.08 / 3.84 GiB.
The global memory rule remains unchanged. The tier matters on the documented
24-GiB host: its 12-GiB default ceiling selects 8 jobs, because the measured
12-job peak was already 11.60 GiB; 8 jobs peaked at 9.90 GiB. Explicit positive
resource overrides are never clamped, so future comparisons can retest either
axis independently; retain the nextest serialized heavy-test groups during
every comparison.

`--prepare` may reuse an already-built archive target as a first-launch check.
On Windows, proc-macro test harnesses need the host runtime DLL search path;
the prepare launcher derives it from each suite's nextest list metadata and
prepends it to the sanitized child PATH. Do not copy DLLs, filter proc-macro
suites, or treat loader exits as warmed: missing metadata and every non-zero
launch remain strict setup failures.

## Quick Rebuild (Native)

```bash
# Quick rebuild native + copy
cargo build --release --package verter_napi && rm -f packages/native/dist/verter-native.win32-x64-msvc.node && cp target/release/verter_napi.dll packages/native/dist/verter-native.win32-x64-msvc.node
```

## Profiling with Hotpath

The `hotpath` feature flag enables `#[hotpath::measure]` annotations on key functions for timing/allocation profiling. Propagates across 7 crates:

```
verter_bench --features hotpath
  ├── verter_compiler/hotpath         (compile_inner, generate_ide_script, generate_ide_template)
  ├── verter_session/hotpath         (upsert_via_scheduler, ensure_compiled, compile_entry, execute_source)
    │   ├── verter_semantic/hotpath (build_script_analysis_with_scope)
  │   ├── verter_scheduler/hotpath (execute_source_stage)
  │   └── verter_workspace/hotpath      (read_file, resolve_import)
  └── verter_diagnostics/hotpath  (lint_inner)
```

### Core-only profiling

Two pipeline modes for compiler-level profiling:

```bash
# AST-only pipeline (tokenize → parse → OXC expressions):
pnpm run profile:hotpath          # Timing hotspots
pnpm run profile:hotpath:alloc    # Timing + allocation hotspots

# Full compile pipeline (tokenize → parse → style → script → template codegen):
pnpm run profile:hotpath:full          # Timing hotspots
pnpm run profile:hotpath:full:alloc    # Timing + allocation hotspots
```

### Host-level profiling (`profile_host` example)

Exercises the full host pipeline (upsert → bundler compile → IDE compile → lint) across real project directories from the `verter-test-repos` checkout:

```bash
# Without hotpath (wall-clock timing only):
cargo run --package verter_bench --example profile_host

# With hotpath instrumentation (per-function timing):
cargo run --package verter_bench --example profile_host --features hotpath
```

Requires `VERTER_TEST_REPOS` env var or a sibling `verter-test-repos` directory. Processes all `.vue` files in each project subdirectory.

## Analysis MCP Server (`verter_mcp`)

`verter-mcp` exposes Verter's full analysis, diagnostics, compilation, and scoring pipeline via MCP. Provides 33 tools for AI agents to understand Vue codebases without reading files directly.

```bash
# Build
pnpm run build:mcp            # Debug build
pnpm run build:mcp:release    # Release build

# Run (stdio — agent spawns as child process)
verter-mcp --project-root /path/to/vue-project

# Run (HTTP — remote/shared access)
verter-mcp --transport http --project-root /path/to/vue-project
# Serves at http://localhost:6772/mcp
```

MCP config files:

- `mcp/verter.mcp.json` (stdio)
- `mcp/verter-http.mcp.json` (HTTP)

For the full tool catalog and agent workflow guide, see [mcp/README.md](../../../mcp/README.md).

## Meta UI Benchmark

Repository-owned real-project component-meta benchmark in `packages/benchmark`:

```bash
pnpm --filter @verter/benchmark bench:meta:ui:setup
pnpm --filter @verter/benchmark bench:meta:ui -- --backends=verter --scenarios=single_cold --limit=2
```

CI uses `.github/workflows/meta-benchmark.yml` to pin the latest `nuxt/ui` `v4` SHA once, run the backend/scenario matrix, and aggregate JSON artifacts into one markdown report.

### CPU Saturation Diagnostic (`bench:meta:ui:saturation`)

When the question is "does the scheduler actually use the CPU?", use the saturation bench rather than `bench:meta:ui`. The standard runner drives the **interactive single-request path** one component at a time (child-process per query), so it never spikes the CPU by design — parallelism only comes from the **batch path** (`getComponentMetaBatch` → `Scheduler::dispatch_meta_jobs` → `cpu_pool.install(|| par_iter)`).

```bash
pnpm --filter @verter/benchmark bench:meta:ui:saturation -- --limit=24
```

`src/meta-ui-saturation.ts` drives the same corpus two ways against cold sessions and reports **cores used = process CPU time / wall time** for each (`process.cpuUsage()` is RUSAGE_SELF, so it counts the native Rayon workers). A sequential pass near `1.0x` confirms the single-core behaviour; a batch pass approaching `availableParallelism()` confirms the pool fanned out. Requires the prepared corpus (`bench:meta:ui:setup`) and a built native binding; it is a dev diagnostic and is excluded from `pnpm test`.

The matching scheduler-side invariant is guarded in Rust by `SchedulerCounters::cpu_inflight_peak` (a `fetch_max` high-water-mark of concurrently-executing meta jobs, set via `enter_cpu_task()` inside `dispatch_meta_jobs`): the unit test `dispatch_meta_jobs_fans_out_across_cpu_pool` and the integration test `batch_component_meta_fans_out_across_cpu_pool` both assert the peak climbs above 1 (a serialized dispatch leaves it at 1 and fails).

### Component-Meta Trace / No-Trace Workflow

For component-meta optimization work, use the trace runner directly instead of guessing from ad-hoc requests.

```bash
# Ground-truth request timing for one real component
node scripts/benchmark/trace-component-corpus.mjs \
  --output-dir=tmp/cm-notrace \
  --filter=Accordion.vue \
  --no-trace

# Traced run for route correctness + stage attribution
node scripts/benchmark/trace-component-corpus.mjs \
  --output-dir=tmp/cm-trace \
  --filter=Accordion.vue

# Full corpus timing sweep
node scripts/benchmark/trace-component-corpus.mjs \
  --output-dir=tmp/cm-full \
  --no-trace
```

Interpretation:

- `query_ms_from_stdout` is the best lightweight request-latency number.
- `wall_ms` includes Node/bootstrap/teardown overhead.
- `trace_resolve_ms` is only the primary `resolve_component_meta` root span.
- `trace_query_ms` is the sum of all traced root spans in the request; better when secondary extraction/fallthrough/imported-local work matters.

Trace checker validates both performance rules and expected metadata artifacts:

```bash
npx tsx packages/benchmark/src/trace-check.ts \
  tmp/cm-trace \
  --batch "Accordion,Alert,App" \
  --strict \
  --check-expected
```

### Real Component-Meta Profiler

For real-project native hotspot attribution:

```bash
cargo run -p verter_bench --example profile_real_component_meta --release --features=hotpath -- Accordion
```

Useful environment variables:

- `VERTER_PROFILE_PROJECT_ROOT` - override the project root
- `VERTER_PROFILE_REPEATS` - repeat the request multiple times
- `HOTPATH_METRICS_PORT` - select a different hotpath port when another profiling run is active
- `HOTPATH_METRICS_SERVER_OFF=1` - disable the hotpath HTTP metrics server when only local output is needed

Practical guidance:

- First use `trace-component-corpus.mjs --no-trace` to confirm a real regression.
- Then use traced runs to identify the owning stage.
- Only then use `profile_real_component_meta` or an external sampler for native call-tree attribution.

### External Sampling Profilers

- `samply` is useful for sampling native + Node-backed component-meta work on supported platforms.
- On Windows, `samply` requires the Windows Performance Toolkit (`xperf`). Without `xperf`, sampling capture will fail even if `samply` itself is installed.
- After the first `cargo run ... profile_real_component_meta ...` build, prefer running the built example binary directly from `target/release/examples/` during iteration so Cargo rebuild cost does not pollute profiling sessions.

### Canonical Corpus for Component-Meta Baselines

Canonical corpus for component-meta perf baselines is `nuxt-ui-codex-bench`, NOT `nuxt-ui`. The `.integration-tests/repos/nuxt-ui` symlink points to a checkout that lacks `src/runtime/components/`; treat it as a stale clone destination and ignore it. Always pass `--ui-root=.integration-tests/repos/nuxt-ui-codex-bench` (or `VERTER_AUDIT_PROJECT_ROOT=...nuxt-ui-codex-bench`) to baseline runners. Corpus commit is locked at integration-branch creation time in `tmp/perf-baselines/pre/baseline-commit.txt` (gitignored), recording `baseline-commit`, `corpus-path`, and `corpus-commit` entries; downstream verification re-reads `corpus-commit` and asserts the live corpus tree still matches before dispatching dependent waves. Bound JSONs under `crates/verter_session/tests/perf_bounds/{component-id}.json` use portable component IDs (lower-kebab) plus relative corpus paths plus the corpus-commit SHA; they MUST NOT contain absolute host paths because they ship to `main` and would break every contributor's checkout.
