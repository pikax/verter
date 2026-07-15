# 12 — Deep memory-audit mode for `bench:meta:ui`

Opt-in allocator-level memory attribution per component query. Off by
default; timing behavior without the flag is unchanged.

## Design

- **Cargo feature `memory_audit` (`crates/verter_napi`)** — compiles a
  counting `#[global_allocator]` wrapper over `System`
  (`crates/verter_napi/src/memory_audit.rs`). Atomics: `alloc_count`,
  `dealloc_count`, `allocated_bytes_total`, `live_bytes`,
  `peak_live_bytes` (high-water via `fetch_max`; `realloc` counts as one
  alloc of the new size plus one dealloc of the old size). Feature OFF ⇒
  the wrapper is not compiled at all — zero overhead.
- **NAPI surface (always exported, feature on or off)** —
  `memoryAuditSnapshot(): {allocCount, deallocCount, allocatedBytesTotal,
  liveBytes, peakLiveBytes} | null` (`null` ⇔ non-instrumented binary) and
  `memoryAuditResetHighWater(): boolean` (`false` ⇔ non-instrumented).
  Re-exported through `packages/native/index.js` / typed in
  `packages/native/index.ts`.
- **Harness flag `--memory-audit`** (`packages/benchmark/src/meta-ui-bench.ts`)
  — threaded to the query worker's init payload. The worker
  (`meta-ui-query-worker.ts`) brackets each query OUTSIDE the timed
  window: `memoryAuditResetHighWater()` + snapshot before, snapshot delta +
  `process.memoryUsage()` after, attaching per-component
  `{allocCount, allocatedBytes, peakLiveBytes, rssBytes, jsHeapUsedBytes}`.

## Artifact

The runner writes a SEPARATE
`meta-ui-<backend>-<scenario>.memory.json` next to the timing artifact
(which is never altered):

```json
{
  "kind": "meta-ui-memory-audit",
  "generatedAt": "…",
  "backend": "verter",
  "scenario": "repo_first_pass",
  "components": [
    {
      "relativePath": "src/runtime/components/Alert.vue",
      "repeatIndex": 1,
      "allocCount": 0,
      "allocatedBytes": 0,
      "peakLiveBytes": 0,
      "rssBytes": 0,
      "jsHeapUsedBytes": 0
    }
  ],
  "totals": {
    "components": 1,
    "allocCount": 0,
    "allocatedBytes": 0,
    "maxPeakLiveBytes": 0,
    "maxRssBytes": 0,
    "maxJsHeapUsedBytes": 0
  }
}
```

Rows are collected for steady-state measured queries only (not warmup
passes); with `--repeats=N` each repeat contributes rows tagged
`repeatIndex`.

## Loud-failure contract

If `--memory-audit` is set and the loaded `@verter/native` binding is not
instrumented (missing exports on an older binary, or
`memoryAuditSnapshot()` returning `null` because the feature was off),
worker setup THROWS naming the required build
(`pnpm --filter @verter/native run build:memory-audit`, i.e. `napi build
--release --features memory_audit`). There is no silent fallback — a
non-instrumented run would report all-zero counters.
Gate helper: `ensureMemoryAuditCapable` in
`packages/benchmark/src/meta-ui-core.ts`.

## How to run

```bash
pnpm --filter @verter/native run build:memory-audit   # instrumented binding
pnpm run build:ts && pnpm --filter @verter/component-meta build
cd packages/benchmark
pnpm bench:meta:ui -- --memory-audit --backends=verter \
  --scenarios=repo_first_pass --expected=none --limit=25
# -> benchmark-results/meta-ui/meta-ui-verter-repo_first_pass.memory.json
```

**Never use the instrumented binary for timing runs** — the counting
allocator adds per-allocation overhead. Rebuild with the regular
`pnpm --filter @verter/native run build` before benchmarking latency;
timing runs without `--memory-audit` are behavior-identical to before
this mode existed.

Tests: `cargo test -p verter_napi --features memory_audit` (counter and
high-water-reset semantics; the default-features run pins the
`null`/`false` uninstrumented contract) and
`packages/benchmark/src/meta-ui-memory-audit.spec.ts` (flag parsing,
loud-failure gate, delta math, artifact writer).
