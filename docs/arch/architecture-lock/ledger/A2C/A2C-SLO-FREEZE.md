Verdict: **APPROVE only with the replacement text below inserted verbatim.** Until it is present in [A2C.md](<REPO>/docs/arch/refactor/rev11/charters/A2C.md:48), A2C remains blocked. The 3% relative gate stays unchanged; `2 × 0.331745% = 0.663490%`, so noise does not control the bound.

The aggregate instrument justifies retiring per-shape decisions because it measures the universal indexing tax over the repository’s existing flow-shape distribution. It does not justify weakening the 3% bound. Target-heavy shapes remain mandatory diagnostics, but their individual ratios are not acceptance gates.

Exact replacement text:

```markdown
## Required performance evidence

All limits in this section are frozen before implementation. They may not be changed, reweighted, subsetted, or reinterpreted after an A2C candidate exists. The performance baseline is product commit `70ea4c01bea870e9684a66f229230808aeb64235`. Authority commit `05a1332fc3656fadcb64e70cce993111123c810a` has no `crates/` difference from that baseline. All relative comparisons use the exact baseline commit, not a moving `main`.

### Frozen representative corpus

The skeleton-index aggregate corpus is all 199 `Row` entries, in table order and without deduplication, from:

`crates/verter_session/src/u6_flow_shape_corpus_rows_tests.rs`

at commit `05a1332fc3656fadcb64e70cce993111123c810a`. The required Git blob is:

`5fe5b72e6538d8dff9e7e0597d0f1ba0bf8e053c`

A run must read that blob from the named commit, never from the candidate worktree. For every row, it evaluates the Rust string literals for non-empty `aux` followed by `script`. The extractor must reject a non-literal `id`, `aux`, or `script`, a row count other than 199, or a blob mismatch.

Each extracted source is parsed as TypeScript with the baseline-locked OXC version before timing. Parse errors invalidate the run. In lexical order, the harness selects every function declaration, function expression, and arrow whose nearest function-like ancestor is the module. It builds each selected root skeleton once. Nested skeleton construction performed internally for capture analysis remains inside the timed root build and must not be removed, separately timed, or deduplicated. In particular, rows such as `X85_nested_closure_write_updates_captured_binding` and `X87_read_only_let_capture_keeps_reaching_literal` exercise skeletons built solely for nested capture inspection.

One aggregate sample is the elapsed wall time for constructing all selected skeletons, in the frozen order, with every result retained through `black_box` until the end of the sample. Parsing and corpus extraction are outside the timed region. No row or function receives an individual acceptance weight. The five historical per-shape skeleton cells remain reported diagnostics only.

### Aggregate skeleton-index relative gate

Let each paired aggregate sample produce `candidate_ns / baseline_ns - 1`. Using the measurement protocol below, the one-sided bootstrap 95% upper confidence bound of the median paired slowdown must be no greater than:

`max(3.000000%, 2 × 0.331745%) = 3.000000%`

The gate remains 3%. The measured noise contribution is only `0.663490%`; lowering the gate to that value would confuse detectable noise with acceptable product regression. Raising the gate is also rejected. Aggregation removes the synthetic shape-selection distortion of the retired cells and measures the universal index cost across the existing flow workload; it does not authorize a larger regression.

### Frozen public cold flow-request cells

Every public cell invokes exactly:

`VerterHost::get_flow_return_type_with_audit(&measured_identity, ReturnProjectionDemand::whole_return())`

for the function named `measured`. Each measured sample uses a fresh standalone host with one scheduler CPU thread, active flow-return audit, `footprint_capture = false`, and audit timing capture disabled. Host construction, scheduler initialization, upsert, readiness, and content-free function-identity lookup occur before timing. The timed interval begins immediately before the public call and ends after the full `AuditedResult` and audit record have been materialized.

The host must have no prior flow request for the measured function. The audit must report exactly one cold flow computation and no warm replay. The candidate result for every cell must be the clean `number` result with no degradation. A cache hit, retry caused by state churn, typed failure, wrong result, or unknown/degraded result invalidates that sample and therefore the run.

The `mixed(n)` source generator is byte-for-byte:

1. Start with `function measured(x: number) {\n`.
2. For decimal `i` from `0` through `n - 1`, with no leading zeroes, append:
   - `L{i}: for (let j{i} = 0; j{i} < 1; j{i}++) {\n`
   - `switch ((x + j{i}) & 3) {\n`
   - `case 0: x += {i}; continue L{i};\n`
   - `case 1: x -= {i}; break L{i};\n`
   - `case 2: try { if (x) { throw x; } } catch { x += 1; } finally { x ^= {i}; } break;\n`
   - `default: return x;\n}\n}\n`
3. Append `return x;\n}\n`.

All text is UTF-8 without BOM and uses LF, not CRLF.

Required cells:

| Cell | Frozen source | UTF-8 bytes | SHA-256 | Relative gate | Absolute cold-request SLO |
|---|---:|---:|---|---:|---:|
| `A2C_COLD_MIXED_4` | `mixed(4)` | 923 | `0f336767f9ac716158804ab46d1a45acc2cfdaecdfef630b959119a67bdd4f9b` | 3.000000% | p95 ≤ 2.000 ms |
| `A2C_COLD_MIXED_64` | `mixed(64)` | 14,663 | `af6986c1f80de164aa20b059d762a56db49e78196a0954c6eeb975c4a968e348` | 3.000000% | p95 ≤ 8.000 ms |
| `A2C_COLD_MIXED_256` | `mixed(256)` | 60,383 | `f69f21101d009225e8e5040dc1cf5aa7096d009fa30acf9438441376494b54f7` | 3.000000% | p95 ≤ 25.000 ms |
| `A2C_COLD_TARGETS_65` | `targets(65)` | 721 | `6ad40d88012d1ff7dcc6f7b954774e2027c1549d7dff61363c6cf2e9e4a4c6c5` | 3.000000% | p95 ≤ 2.000 ms |

For every cell, the one-sided bootstrap 95% upper confidence bound of the median paired slowdown must be at most 3.000000%. Separately, the one-sided bootstrap 95% upper confidence bound of the candidate p95 must be at or below the absolute SLO. Passing one gate does not compensate for failing the other.

The absolute SLOs are product budgets, not fits to baseline or candidate measurements. A local semantic editor response must remain within 50 ms at p95 for ordinary input. The approximately 1 KiB cold flow primitive receives 2 ms of that budget, leaving parsing, routing, projection, provider work, serialization, and LSP transport outside A2C. The approximately 15 KiB cell receives 8 ms. The approximately 60 KiB adversarial cell receives 25 ms, leaving half of the 50 ms budget to the rest of the request. A 65-live-target function is still a sub-kilobyte semantic input and therefore receives the ordinary 2 ms budget; target depth does not buy a larger latency class.

The `targets(n)` generator is byte-for-byte:

1. Start with `function measured(x: number) {\n`.
2. For `i = 0` through `n - 1`, append `L{ii}: {\n`, where `{ii}` is decimal `i` left-padded to exactly two digits.
3. Append `if (x) break L00;\nreturn x;\n`.
4. Append `}\n` exactly `n` times.
5. Append `return x;\n}\n`.

All text is UTF-8 without BOM and uses LF. The adjacent required diagnostic sources are:

- `targets(64)`: 711 bytes, SHA-256 `ca5c950b74151375334b49ffa969cbd9d4c6347a051bfa3b6ab374bcd0f25dec`
- `targets(65)`: 721 bytes, SHA-256 `6ad40d88012d1ff7dcc6f7b954774e2027c1549d7dff61363c6cf2e9e4a4c6c5`
- `targets(66)`: 731 bytes, SHA-256 `3a413890bcb0fe2c5ba6b3cacb69c1a472303cdeeced4557ecd10c486eadbdc1`

All three must produce an exact clean `number` result. No overflow, unknown coverage, alternate representation, fallback, or typed degradation is permitted at 64, 65, or 66 targets. The upper confidence bound for `T65 / T64` and `T66 / T65` must be no greater than `max(1.05, (Tprevious + 500 ns) / Tprevious)`. Retained bytes may increase by at most 48 bytes and allocator-requested bytes by at most 96 bytes for each additional target. A capacity or representation cliff at 65 fails.

### Frozen linear-work and byte limits

Counts are defined as follows:

- `C`: canonical D6 control records for authored labels, switch statements, switch case groups, iteration statements, try statements, catch clauses, and finally clauses.
- `E`: source-ordered completion events for return, throw, break, and continue.
- `G`: emitted graph edges classified as completion edges, including finally preservation or override edges.
- `W = C + E + G`.

Instrumentation must count every examination of a control record, completion event, target-parent record, and completion edge. An ancestor step or repeated target probe is another examination; it may not be hidden inside an uncounted helper.

For every scale fixture below:

- completion work units must be no greater than `6C + 6E + 4G + 32`;
- the one-sided 95% upper confidence bound of incremental skeleton-index wall time must be no greater than `2,000 ns + 250 ns × (C + E)`;
- the one-sided 95% upper confidence bound of incremental completion-graph wall time must be no greater than `2,000 ns + 200 ns × W`;
- additional allocator-requested skeleton bytes must be no greater than `512 + 96C + 64E`;
- additional allocator-requested graph bytes must be no greater than `512 + 32C + 16E + 48G`;
- additional retained skeleton bytes, including vector capacity and inline topology fields, must be no greater than `128 + 48C + 32E`;
- additional retained graph bytes must be no greater than `128 + 16C + 8E + 24G`;
- combined additional retained bytes must therefore be no greater than `256 + 64C + 40E + 24G`.

“Incremental” means the median for the scale fixture minus the median for the same generator at `n = 0`, floored at zero. It does not mean candidate-minus-baseline and may not omit existing visitor or graph work.

The three generator families are measured at `n = 64, 256, 1024, 4096`:

1. `controls(n)`: start `function measured(x: number) {\n`; append `L{i}: {\n` for decimal `i = 0..n-1`; append `x += 1;\n`; append `}\n` exactly `n` times; append `}\n`.
2. `events(n)`: start `function measured(x: number) {\n`; append `return x;\n` exactly `n` times; append `}\n`.
3. `finally(n)`: start `function measured(x: number) {\n`; append `try { if (x) return x; } finally { x += 1; }\n` exactly `n` times; append `return x;\n}\n`.

Parsing is outside timing. Every size must independently pass every applicable formula. For each family, after subtracting the `n = 0` intercept, normalized time or work per weighted record at the next size may not exceed the previous size by more than 10%. Source review must also establish that no event performs a scan proportional to live-target depth and that no control/event pair is materialized or revisited as a Cartesian product. Timing alone is not proof of asymptotic complexity.

The no-completion allocation cell uses:

`function measured(x: number) { let y = x; y += 0; ... y += 1023; }`

with the 1,024 statements generated in ascending decimal order. It has `C = 0`, `E = 0`, and `G = 0`. Relative to the exact baseline, the candidate must add exactly zero allocator calls and zero allocator-requested bytes during skeleton construction and graph construction. Empty inline handles are allowed; allocating an empty completion vector, map, target table, or graph sidecar is not.

Every retained byte added by A2C must be mapped in the evidence report to a named canonical D6 topology/event/edge field or the root `CompletionCoverage` verdict. Bytes retained solely for A3, a query-specific endpoint fact, a second classifier, target-indexed completion sets, spare fixed-capacity target storage, or benchmark instrumentation fail with a zero-byte allowance.

### Measurement protocol

Latency evidence must use optimized non-instrumented `--release` builds of baseline and candidate from clean worktrees, the same locked Rust toolchain, Cargo features, allocator, linker, codegen settings, and environment. Allocation runs are separate builds/runs and never supply latency samples.

The runner is the same physical machine and runner profile that produced the registered `0.331745%` noise floor. Pin the benchmark to one physical core, use the performance power policy, disconnect variable network work, and close unrelated build, indexer, antivirus-scan, and editor workloads. Record CPU identity, OS build, microcode, memory, toolchain, allocator, power policy, and core affinity.

The stable control is exactly:

```rust
let mut value = 0x9e37_79b9_7f4a_7c15_u64;
for index in 0..256_u64 {
    value = black_box(value.rotate_left(7) ^ index)
        .wrapping_mul(0xbf58_476d_1ce4_e5b9);
}
black_box(value);
```

Before measurement, run 20 discarded warmup samples of each binary and cell in alternating `AB`/`BA` order. Warmup public cold cells still use fresh hosts and cold functions.

The aggregate skeleton cell uses 40 measured baseline/candidate pairs. Each public cold-request cell uses 200 measured baseline/candidate pairs so that p95 is evidence rather than a two-observation tail. Linear-work timing cells use 40 candidate samples per generator and size. No measured sample is reused between cells.

Pair order alternates `AB`, `BA`, beginning with `AB`. Run the stable control immediately before and after every pair. No observation is deleted as an outlier. A failed operation invalidates the run rather than being removed from the sample.

Use 10,000 bootstrap resamples of complete pairs with deterministic seed `0x00A2C00305A1332F`. Relative statistics bootstrap paired ratios. Absolute statistics bootstrap candidate observations. Use a one-sided 95% upper confidence bound. p95 is the nearest-rank empirical p95 within each bootstrap resample.

The noise floor is predeclared as exactly `0.331745%`, measured from 40 interleaved control pairs. It is not re-estimated from candidate results and cannot be replaced by a more favorable calibration after implementation. A new noise record is permissible only through a separately ratified pre-candidate amendment.

A run is invalid and supplies no evidence if any of the following occurs:

- baseline SHA, authority SHA, corpus blob, row count, source byte count, or source SHA-256 differs;
- baseline and candidate differ in toolchain, features, allocator, linker, flags, affinity, or runner profile;
- a parse error, panic, typed request failure, state-churn retry, missing sample, or zero-duration control occurs;
- a public cold cell does not report exactly one cold flow compute;
- the median stable-control ratio differs between binaries by more than 1.000%, or first-half and second-half control medians drift by more than 1.000%;
- the process migrates from the pinned core, the power policy changes, thermal throttling is recorded, or unrelated sustained CPU load exceeds 2%;
- latency is collected with the counting allocator or other A2C instrumentation enabled;
- observations are removed, winsorized, regrouped, or rerun selectively after their direction is known.

An invalid run must be rerun in full. It is neither a pass nor a failure and may not be combined with another run.

### Failure and stop rule

A2C fails and returns to `STOPPED` if any valid run produces any one of the following:

- aggregate skeleton-index upper slowdown bound greater than 3.000000%;
- any public cold-request relative upper slowdown bound greater than 3.000000%;
- any public cold-request absolute p95 upper confidence bound above its cell SLO;
- any work-unit, wall-nanosecond, allocator-byte, or retained-byte bound exceeded at any required size;
- non-linear source structure, target-depth scanning, or more than 10% growth in normalized per-record work between adjacent scale sizes;
- any 64/65/66 target inexactness, degradation, overflow, fallback, timing discontinuity, or byte discontinuity;
- any completion-owned allocation in the `C = E = G = 0` cell;
- any retained byte without sole canonical D6 topology/event/edge/coverage ownership;
- any A3-only retained payload or any second completion classifier, memo, graph, AST rewalk, target-indexed completion set, or fixed target ceiling.

Required cells are conjunctive. An absolute pass cannot offset a relative failure; an aggregate pass cannot offset a cold-cell, linearity, memory, continuity, or zero-allocation failure. The retired five per-shape skeleton ratios remain diagnostics and do not fail A2C by themselves.

On failure, no A2C candidate is accepted and A3 may not begin. The corpus, gates, and SLOs may not be weakened or reselected to admit that candidate. The only permissible next step is redesign/optimization under these same limits or a new pre-candidate architecture amendment after the failed candidate has been abandoned.
```

__DONE__
