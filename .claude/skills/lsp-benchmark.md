# LSP Benchmark — Performance Profile & Improvement Guide

## Purpose

This document provides the current LSP performance profile and explains how to run benchmarks, so you can measure the impact of UX improvements. The benchmark tests Verter's LSP server against PrimeVue (279 .vue files), using DataTable.vue (2,179 lines) as the target file.

## Current Performance (2026-03-13, Windows, Release build)

### Raw Numbers

| Phase                 | Verter (no TP) | Verter (auto/tsserver) | Volar |
| --------------------- | -------------- | ---------------------- | ----- |
| Initialize            | 122ms          | 699ms                  | 880ms |
| Workspace Scan        | 61ms           | 56ms                   | N/A   |
| didOpen → first Hover | 57ms           | 1,365ms                | 121ms |
| Hover (warm, single)  | 0.45ms         | 498ms                  | 4.4ms |
| Hover (median of 5)   | 0.20ms         | 470ms                  | 4.0ms |

### Key Takeaways

1. **Verter without type provider is extremely fast.** Initialize + scan + first hover completes in ~240ms total. Warm hovers are sub-millisecond (0.2ms median). This is the Rust-only path: parsing, template compilation, and Verter's own analysis with zero TypeScript involvement.

2. **The type provider (tsserver) is the dominant bottleneck.** With `auto` (tsserver), every hover takes ~470-500ms because each request round-trips through tsserver's stdio JSON protocol. Initialize is 5.7x slower (699ms vs 122ms) due to tsserver startup. First hover after didOpen is 24x slower (1,365ms vs 57ms) because tsserver must process the synced TSX file.

3. **Volar is faster than Verter+tsserver for hover.** Volar's warm hover is 4ms vs Verter's 470ms. This is because Volar runs TypeScript in-process (same Node.js runtime) while Verter communicates with tsserver over stdio IPC. Volar's initialize is slowest (880ms) since it loads the full TS language service upfront.

4. **Workspace scan is fast and consistent** (~60ms) regardless of type provider mode. The scan itself is Rust-native; type provider sync happens asynchronously afterward.

### Where Time Is Spent (by phase)

**Initialize** (`initialize` → `initialized` response):

- no TP: Rust binary startup + project registry build (~122ms)
- auto: Same + tsserver process spawn + TS project load (~699ms)
- The 577ms delta is purely tsserver startup overhead

**Workspace Scan** (`initialized` → `$/verter/ready`):

- Async background task compiles all .vue files to TSX
- For 279 files, takes ~60ms (Rust compilation is fast)
- Type provider file sync runs concurrently but may finish later

**didOpen → Hover** (first interactive request):

- no TP: Parse + compile single file + Verter-only hover resolution (~57ms)
- auto: Same + sync TSX to tsserver + wait for tsserver hover response (~1,365ms)
- The 1,308ms delta is tsserver processing the file and resolving types

**Warm Hover** (subsequent requests, file already open and synced):

- no TP: Pure Rust span lookup, sub-millisecond (0.2ms)
- auto: Verter resolves position → tsserver request → tsserver response → merge (~470ms)
- Each tsserver round-trip costs ~470ms due to stdio serialization + TS type resolution

### Performance Bottleneck Analysis

The #1 UX bottleneck is **tsserver hover latency** (~470ms per request). This affects:

- Hover tooltips (user sees ~500ms delay on every hover)
- Completions (similar round-trip pattern)
- Go-to-definition through type provider
- Any feature delegated to tsserver

Potential improvement vectors:

1. **Batch/pipeline tsserver requests** — currently sequential, one request at a time
2. **Cache tsserver responses** — hover at same position shouldn't re-query if file unchanged
3. **Speculative prefetch** — predict likely hover targets from cursor movement
4. **TSGO backend** — Go-native TS checker, Verter's preferred provider (one known gap: the TS6133 remove-unused quick fix is not yet ported)
5. **Reduce Verter overhead** — the 0.2ms Verter-only path is already negligible; gains here are marginal

## How to Run the Benchmark

### Prerequisites

- Verter LSP binary built in release mode
- PrimeVue cloned locally (or any large Vue project)

### Build the LSP binary

```bash
cargo build --release -p verter_lsp
```

### Run locally (table output)

```bash
# Full benchmark (Verter no-TP + Verter auto + Volar)
pnpm --filter @verter/benchmark bench:lsp

# Skip Volar (faster, only Verter configs)
pnpm --filter @verter/benchmark bench:lsp -- --skip-volar
```

### Run with JSON output

```bash
# JSON to stdout, logs to stderr
pnpm --filter @verter/benchmark bench:lsp:json

# With overrides (simulating CI or custom project)
pnpm --filter @verter/benchmark bench:lsp:json -- \
  --skip-volar \
  --workspace=/path/to/vue-project \
  --verter-bin=target/release/verter-lsp.exe
```

### CLI Flags

| Flag                  | Default                         | Description                                         |
| --------------------- | ------------------------------- | --------------------------------------------------- |
| `--json`              | off                             | Output structured JSON to stdout (table suppressed) |
| `--workspace=<path>`  | Hardcoded PrimeVue path         | Override workspace root                             |
| `--verter-bin=<path>` | `target/release/verter-lsp.exe` | Override LSP binary path                            |
| `--skip-volar`        | off                             | Skip Volar benchmark (no TS SDK needed)             |

### JSON Output Schema

```json
{
  "project": "primevue",
  "vueFileCount": 279,
  "testFile": "src/datatable/DataTable.vue",
  "testFileLines": 2179,
  "configs": {
    "Verter (no TP)": {
      "initialize": 122.31,
      "workspaceScan": 61.09,
      "didOpenToHover": 56.91,
      "hoverWarm": 0.45,
      "hoverMedian": 0.20
    },
    "Verter (auto)": { ... },
    "Volar": { ... }
  },
  "timestamp": "2026-03-13T..."
}
```

All timing values are in milliseconds.

### CI Workflow

The benchmark runs in CI via `.github/workflows/lsp-benchmark.yml`:

- **Trigger**: `/lsp-benchmark` comment on a PR, or manual `workflow_dispatch`
- **What it does**: Builds LSP (release), clones PrimeVue, runs `--json --skip-volar`, posts results as PR comment
- **Artifact**: `lsp-benchmark-results.json` (30-day retention)

### Interpreting Results After Changes

When measuring the impact of a UX change:

1. **Run before and after** on the same machine with the same project
2. **Focus on the phase your change targets**: e.g., if you add hover caching, compare `hoverWarm` and `hoverMedian`
3. **The `no TP` config is the Rust-only ceiling** — if your change is in Rust codegen/analysis, this is the one to watch
4. **The `auto` config shows end-to-end UX** — this is what users actually experience with type checking enabled
5. **Warm hover median is the most stable metric** — it averages out startup jitter

### Benchmark Source

The benchmark source is at `packages/benchmark/src/lsp-bench.ts`. It:

1. Spawns the LSP binary as a child process
2. Sends JSON-RPC over stdio (same protocol as VS Code)
3. Measures wall-clock time for each phase
4. The hover target is `d_rows` on line 19 of DataTable.vue (a template binding `:rows="d_rows"`)

### Key Files for Performance Work

| Area                | Files                                        |
| ------------------- | -------------------------------------------- |
| LSP server entry    | `crates/verter_lsp/src/main.rs`              |
| Hover handler       | `crates/verter_lsp/src/hover.rs`             |
| Type provider trait | `crates/verter_lsp/src/tsgo/traits.rs`       |
| tsserver IPC        | `crates/verter_lsp/src/tsserver/ipc.rs`      |
| TSGO IPC            | `crates/verter_lsp/src/tsgo/ipc.rs`          |
| SyncCoordinator     | `crates/verter_lsp/src/sync_coordinator.rs`  |
| Workspace scanner   | `crates/verter_lsp/src/workspace_scanner.rs` |
| Benchmark script    | `packages/benchmark/src/lsp-bench.ts`        |
