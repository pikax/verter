# 12 — Runtime profile-audit mode for `bench:meta:ui`

Opt-in per-component profiling — allocator-level memory attribution,
native phase timings, and sampled allocation call sites — from ONE
single binary. Off by default; the runtime gate keeps the disabled cost
at one cached branch per allocator call, so regular timing behavior is
unchanged and users can ship-enable it in the field to see where time
and memory go.

## Design

- **Always-compiled runtime-gated allocator**
  (`crates/verter_napi/src/memory_audit.rs`) — a counting
  `#[global_allocator]` wrapper over `System` ships in every build.
  Disabled (default): delegate to `System` plus exactly ONE cached
  relaxed atomic load + branch — no counters, no thread-locals, no env
  reads on the allocator path. Enabled: atomics for `alloc_count`,
  `dealloc_count`, `allocated_bytes_total`, `live_bytes` (SIGNED — the
  enable epoch starts at zero, so pre-epoch blocks freed later drive it
  negative rather than corrupting counters), `peak_live_bytes`
  (high-water via `fetch_max`; `realloc` counts as one alloc of the new
  size plus one dealloc of the old size). Enabling starts a FRESH
  counter epoch.
- **Enabling** — runtime only, two equivalent routes:
  - `memoryAuditEnable({ sampleEvery? })` from JS, called before the
    workload (the bench worker does this at setup);
  - env `VERTER_MEMORY_AUDIT=1` (counters) and/or
    `VERTER_MEMORY_AUDIT_SAMPLE=N` (counters + site sampling), read
    ONCE per process on the first memory-audit NAPI call — never on the
    allocator path.
- **NAPI surface (always exported)** — `memoryAuditEnable(options?)`,
  `memoryAuditSnapshot(): {...} | null`,
  `memoryAuditResetHighWater(): boolean`, and
  `memoryAuditSites(topK): string | null`; snapshot/sites are `null`
  (reset `false`) while the gate is disabled. Re-exported through
  `packages/native/index.js` / typed in `packages/native/index.ts`.
- **Harness flag `--profile-audit`**
  (`packages/benchmark/src/meta-ui-bench.ts`; `--memory-audit` is kept
  as an alias) — threaded to the query worker's init payload. The
  worker enables the runtime audit at setup and brackets each query
  OUTSIDE the timed window: `memoryAuditResetHighWater()` + snapshot
  before, snapshot delta + `process.memoryUsage()` after, attaching
  per-component `{allocCount, allocatedBytes, peakLiveBytes, rssBytes,
  jsHeapUsedBytes}`.
- **Phase timings (verter backend)** — the worker creates the compat
  checker with `logging: { audit: true }` and the MEASURED query is the
  audited native variant (`MetaSession.getComponentMetaWithAudit`,
  reached through the checker's runtime session; defensively guarded —
  timings degrade to absent, never fail the run). The audit record's
  `timings` block folds into the row as `{totalMs, materializeMs,
  solverMs, storeReadMs, storeMergeMs}`; the compat-shaped artifact for
  deviation checks comes from a warm follow-up `getComponentMeta` read
  outside the measured window (identical result by cache correctness).

## Sampled allocation sites (`sampleEvery` / `VERTER_MEMORY_AUDIT_SAMPLE`)

Counters answer "how much"; sites answer "WHERE". macOS
`malloc_history` under-captures our deep resolver stacks, so the
attribution is in-process: with sampling armed (N > 0; **97
recommended** — a prime interval avoids locking onto allocation-pattern
periodicity), every Nth allocating call captures an UNRESOLVED
backtrace (32 raw ips) into a bounded site table (4096 sites, keyed by
frame-hash; new sites beyond the cap are dropped, hot sites keep
accumulating). Symbols resolve lazily at read time only.

- **`memoryAuditSites(topK): string | null`** — JSON
  `[{count, bytes, estimatedTotalBytes, frames}, ...]`, top-K by
  sampled bytes, `frames` ≤ 8 resolved symbols (innermost first,
  leading allocator/backtrace plumbing skipped). `null` ⇔ audit
  disabled OR sampling not armed.
- **estimatedTotalBytes math:** `bytes * N` — with uniform every-Nth
  sampling, each sampled byte stands for N real ones, so the row is an
  unbiased estimate of the site's total allocated bytes
  (`sum(estimatedTotalBytes) ≈ allocatedBytesTotal` for the window,
  within sampling noise; rare-site estimates are noisier than hot-site
  ones).
- **Harness:** when `--profile-audit` is on AND
  `VERTER_MEMORY_AUDIT_SAMPLE` is set (the env is inherited by the
  query worker), the runner asks the worker for `memoryAuditSites(50)`
  once at end of pass (outside the measured window) and attaches the
  rows to the `.profile.json` artifact under `sites`. Sites are
  ADDITIVE: a missing export or unarmed sampling just omits the key.
  Repo scenarios only (`repo_first_pass` / `repo_warm_second_pass`):
  single_* scenarios spawn one worker per component, so no single
  end-of-pass report can represent the pass. With `--repeats=N` the
  freshest pass wins.

## Overhead expectation (measured, repo_first_pass --limit=12, macOS arm64)

| variant | steady-state wall (12 components) |
| --- | --- |
| pre-wrapper binary, no flags | 724 ms |
| single binary, audit DISABLED | 638 ms |
| `--profile-audit` (counters + audited-query timings + footprints) | 1124 ms |
| `--profile-audit` + sampling N=97 | 4835 ms |

- **Disabled (default):** one cached branch per allocator call —
  indistinguishable from the pre-wrapper binary (the difference above
  is run-to-run noise, the new binary measured FASTER). This is the
  always-on production state and the only timing-comparable one.
- **`--profile-audit`:** relaxed-atomic counter updates per allocator
  call, plus the audited native query variant per component (audit
  record + footprint capture + a ~1.6 MB JSON bundle per query) —
  ≈1.8× wall on this workload. An audit mode, never a timing mode.
- **`--profile-audit` + sampling at N=97:** additionally one relaxed
  `fetch_add` + divisibility check per allocation and an unresolved
  32-frame stack walk on every 97th allocation. On this
  allocation-heavy resolver workload (~2.4M allocations per cold
  component) sampling is a REAL multiplier — ≈7.6× wall vs disabled.
  Sampling is for attribution, never for timing; raise N to trade
  attribution resolution for speed.

Compare timing numbers only against runs with identical audit flags.

## Artifact

The runner writes a SEPARATE
`meta-ui-<backend>-<scenario>.profile.json` next to the timing artifact
(which is never altered):

```json
{
  "kind": "meta-ui-profile-audit",
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
      "jsHeapUsedBytes": 0,
      "timings": {
        "totalMs": 0,
        "materializeMs": 0,
        "solverMs": 0,
        "storeReadMs": 0,
        "storeMergeMs": 0
      }
    }
  ],
  "totals": {
    "components": 1,
    "allocCount": 0,
    "allocatedBytes": 0,
    "maxPeakLiveBytes": 0,
    "maxRssBytes": 0,
    "maxJsHeapUsedBytes": 0
  },
  "sites": [
    {
      "count": 0,
      "bytes": 0,
      "estimatedTotalBytes": 0,
      "frames": ["verter_session::…"]
    }
  ]
}
```

Rows are collected for steady-state measured queries only (not warmup
passes); with `--repeats=N` each repeat contributes rows tagged
`repeatIndex`. `timings` is verter-backend-only; `sites` appears only
when sampling was armed.

## Loud-failure contract

If `--profile-audit` is set and the loaded `@verter/native` binding
predates the runtime memory-audit surface (missing
`memoryAuditEnable`/`memoryAuditSnapshot`/`memoryAuditResetHighWater`
exports on an older binary), worker setup THROWS naming the fix
(rebuild `@verter/native`). There is no silent fallback — such a run
would report all-zero counters. Timings and sites are ADDITIVE on top:
they degrade to absent, never loud.
Gate helper: `ensureMemoryAuditCapable` in
`packages/benchmark/src/meta-ui-core.ts`.

## How to run

```bash
pnpm --filter @verter/native run build     # the one regular binary
pnpm run build:ts && pnpm --filter @verter/component-meta build
cd packages/benchmark
VERTER_MEMORY_AUDIT_SAMPLE=97 pnpm bench:meta:ui -- --profile-audit \
  --backends=verter --scenarios=repo_first_pass --expected=none --limit=25
# -> benchmark-results/meta-ui/meta-ui-verter-repo_first_pass.profile.json
#    (rows with timings + memory deltas, plus top-50 allocation sites)
```

Omit `VERTER_MEMORY_AUDIT_SAMPLE` for counters + timings without site
sampling. Outside the harness, any consumer can ship-enable the audit
on the regular binary: set `VERTER_MEMORY_AUDIT=1` (+
`VERTER_MEMORY_AUDIT_SAMPLE=N`) or call
`memoryAuditEnable({ sampleEvery: N })`, then read
`memoryAuditSnapshot()` / `memoryAuditSites(topK)`.

Tests: `cargo test -p verter_napi --lib` (disabled contract, runtime
enable + fresh epoch, counter and high-water-reset semantics, sampled
named-site capture through lazy resolution, interval-scaled estimates,
recursion-guard/deadlock stress, unarmed ⇒ `null`) and
`packages/benchmark/src/meta-ui-memory-audit.spec.ts` (flag parsing +
alias, loud-failure gate + enable handshake, delta math, timing
extraction, sites parsing, artifact writer).
