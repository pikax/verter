# @verter/benchmark

Performance benchmarks comparing Vue and Verter SFC compilation.

## Overview

This package benchmarks Vue and Verter compilers using real-world Vue SFC fixtures:

- **Vue**: 2-phase compilation (parse → compileScript → compileTemplate)
- **Verter**: Single-pass compilation via Rust/NAPI

## Metrics

- **Time**: Compilation time in milliseconds (p50/p95/p99)
- **Throughput**: Megabytes per second (MB/s)
- **Memory**: Peak heap used during compilation (MB)
- **Operations**: Operations per second
- **Speedup**: Verter performance relative to Vue (e.g., 2.0x = twice as fast)

## Fixtures

Eight fixtures from tiny to complex:

1. `tiny-template.vue` - Minimal single element
2. `simple-interactive.vue` - Basic interactivity with ref
3. `list-rendering.vue` - v-for with dynamic lists
4. `conditional-heavy.vue` - Multiple v-if/v-else chains
5. `form-component.vue` - Form with v-model bindings
6. `composition-heavy.vue` - Complex script setup
7. `template-heavy.vue` - Large template structure
8. `kitchen-sink.vue` - All features combined

## Usage

```bash
# Run benchmarks with console output
pnpm bench

# Generate JSON report
pnpm bench:json
```

## LSP Benchmark

The package also includes an LSP benchmark for comparing Verter and Volar:

```bash
# Repo-local smoke benchmark (works on macOS and Windows)
pnpm --filter @verter/benchmark bench:lsp

# JSON output
pnpm --filter @verter/benchmark bench:lsp:json
```

By default, the LSP benchmark uses the checked-in example workspace:

- Workspace: `packages/example`
- File: `Test.vue`
- Hover target: line `2`, char `9` (1-based)

This keeps the benchmark runnable out of the box on both macOS and Windows. For a
real-world comparison, point it at a larger Vue workspace:

```bash
pnpm --filter @verter/benchmark bench:lsp \
  --workspace=/path/to/primevue/packages/primevue \
  --file=src/datatable/DataTable.vue \
  --hover-line=19 \
  --hover-char=20
```

Supported flags:

- `--json` - output structured JSON to stdout
- `--skip-volar` - run only the Verter configurations
- `--workspace=<path>` - workspace root to open
- `--file=<path>` - benchmark target file; relative paths are resolved from the workspace root
- `--hover-line=<n>` - 1-based hover line
- `--hover-char=<n>` - 1-based hover character
- `--verter-bin=<path>` - override the Verter LSP binary path
- `--volar-script=<path>` - override Volar's `vue-language-server.js`
- `--tsdk=<path>` - override the TypeScript SDK directory used by Volar

Binary resolution is platform-aware:

- Verter: checks `target/release/verter-lsp(.exe)` first, then `target/debug/verter-lsp(.exe)`
- Volar: resolves `@vue/language-server` from the benchmark package installation
- TypeScript SDK: prefers `<workspace>/node_modules/typescript/lib`, then `<repo>/node_modules/typescript/lib`, then the benchmark package installation

CI uses a separate `/lsp-benchmark` workflow that runs the PrimeVue target on Linux, macOS, and Windows so the report shows per-OS values instead of a single runner's numbers.

## Meta UI Benchmark

The package also includes a real-project component-meta benchmark for `nuxt/ui`.

Setup the local checkout first:

```bash
pnpm --filter @verter/benchmark bench:meta:ui:setup
```

That setup intentionally uses `pnpm install --frozen-lockfile` so every backend/job benchmarks the same dependency graph. For manual debugging only, you can opt into an unfrozen fallback with `pnpm --filter @verter/benchmark bench:meta:ui:setup -- --allow-unfrozen-install`.

Run a small smoke benchmark:

```bash
pnpm --filter @verter/benchmark bench:meta:ui -- \
  --backends=verter \
  --scenarios=single_cold \
  --limit=2
```

JSON output:

```bash
pnpm --filter @verter/benchmark bench:meta:ui:json -- \
  --backends=verter,vue-component-meta \
  --scenarios=single_cold,repo_warm_second_pass \
  --repeats=5
```

Build or refresh the pinned `vue-component-meta` baseline once and reuse it across later runs:

```bash
pnpm --filter @verter/benchmark bench:meta:ui -- \
  --backends=vue-component-meta \
  --expected=vue-component-meta \
  --expected-dir=packages/benchmark/benchmark-results/meta-ui-expected \
  --build-expected-only
```

Supported flags:

- `--ui-root=<path>` - override the prepared `nuxt-ui` checkout
- `--backends=<csv>` - any of `vue-component-meta,verter,tsserver,tsgo`
- `--scenarios=<csv>` - any of `single_cold,single_warm,repo_first_pass,repo_warm_second_pass`
- `--repeats=<n>` - repeat count per backend/scenario
- `--warmup-passes=<n>` - untimed warmup passes before warm scenarios
- `--components=<csv>` - benchmark only selected component file names or paths
- `--limit=<n>` - limit the discovered component set after sorting
- `--expected=<vue-component-meta|none>` - enable or skip baseline deviation comparison
- `--expected-dir=<path>` - load or write reusable expected artifacts in a dedicated directory
- `--build-expected-only` - build/reuse expected artifacts and exit without running timed scenarios
- `--output-dir=<path>` - write per-run JSON artifacts to a custom directory

The generated JSON artifacts land under `packages/benchmark/benchmark-results/meta-ui/` by default, while reusable expected artifacts default to `packages/benchmark/benchmark-results/meta-ui/.expected-vue-component-meta/`. CI builds the expected artifact set once, uploads it, and reuses it across the backend/scenario matrix in the `/meta-benchmark` workflow.

## CI Integration

Triggered via PR comment `/benchmark` or manually through GitHub Actions.

Results posted as PR comments with:

- ✅ **Pass**: Verter ≥ Vue performance (speedup ≥ 1.0x)
- ⚠️ **Warning**: Verter 50-99% of Vue performance (speedup 0.5-1.0x)
- ❌ **Fail**: Verter < 50% of Vue performance (speedup < 0.5x)

## Stress Test

Includes a stress test with ~20,000 files (created by repeating all fixtures):

- **Default**: 1 iteration, 1 warmup (fast)
- Useful for measuring aggregate throughput on large compilations
- Reports per-file compilation time and overall MB/s throughput
