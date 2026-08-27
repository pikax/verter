# A6 locked cell `A6_META_COMPILE_40_COLD_RUST` — raw receipt on base and candidate

Historical evolving receipt. The final post-authority result and exact production subject are bound
in `final-waiver-application.md`; the immutable review SHA/tree is stamped in the external final
receipt after the evidence-only commit.

## Deviation recorded (landing-path ruling step 2 is maintainer-owned)

`performance-gates.toml:137` pinned the harness as
`git-blob:efa9ea54a14772ecd87511d6bb07017aa33940ba … sha256:1d208e61688efdf6b8e33adce7d0095c1195377e65a06579c26b75b08ea9bb73`.
Derivation on the tree:

| identity | value |
|---|---|
| `git hash-object crates/verter_bench/examples/attribution_baseline.rs` (base and candidate, unchanged) | `efa9ea54a14772ecd87511d6bb07017aa33940ba` |
| `sha256(blob efa9ea54…)` | `5e06d35dda284a8ef049bf0dd3dc39974b904729f740da58c650ec59e806f632` |
| `sha256(blob a74f90c5d1d06f8fc17a71781d28d0c6ea466853)` — the PREVIOUS pin the comment says it was re-pinned from | `1d208e61688efdf6b8e33adce7d0095c1195377e65a06579c26b75b08ea9bb73` |

So the re-pin updated the blob id and carried the old blob's SHA-256 forward. One-line correction applied
in commit `6431b0e2d` (`sha256:` → `5e06d35d…`); `node scripts/validate-performance-gates.mjs --gates
performance-gates.toml` → `PASS … 4 cell(s), 56 metric(s), no placeholders` (the validator checks only
that the field is a non-empty string — it derives neither identity; recorded in `.feedback`). No metric,
limit, statistic or comparison of the cell was touched (S2-F6).

## Protocol executed (exactly `evidence/A6/baseline-measurement.md` §3 and §4)

Disabled arm (timing): `cargo build -p verter_bench --release --example attribution_baseline`, then
4 × `/usr/bin/time -l ./attribution_baseline --files 40 --runs 30`. Enabled arm (counters):
`cargo build -p verter_bench --release --features attribution --example attribution_baseline`, then
`./attribution_baseline --files 40 --runs 3 --format tsv`. Both arms built for BASE
(`d1f3d50a948597f036868543b9bb21acacd730ff`, throwaway worktree, deleted afterwards) and CANDIDATE
(this tree, `Cargo.lock` as committed); binaries copied aside before the enabled-arm build overwrote
the example path. Measurements ran back to back on the same host with no other cargo/test process
alive (the four invocations of each side are consecutive; base first). Raw outputs:
`base-wall.txt`, `cand-wall.txt` (timing + `time -l`), `base-counters.tsv`, `cand-counters.tsv`.

| binary | sha256 |
|---|---|
| base disabled arm | `1b43954cea45cbbec13735f0004bdb6e505104fb8a57f9eb8f2aaac81fadc56c` |
| candidate disabled arm | `24c139104134bc8b7bbd14f51c006de044db8e0d46090aabcf9181f485d46cf5` |
| base enabled arm | `92c59c75a01c282ce154d6e61551a5f6c28f1851cd871f7fca3a7bb6119f6df3` |
| candidate enabled arm | `3b5be99f248afe1363a9b187b4f37ee1e23832f27423b7eb78230d4799fc4a7c` |

Host: the same 8-core machine class the cell locks (`threads = 8`); it is NOT the machine the
70.525 ms baseline was taken on — base measures 91.2 ms here, so absolute limits are read with that
caveat and the relative comparison base→candidate is the meaningful one.

## Wall clock and peak RSS (disabled arm)

_Measurements in this and the following two sections are superseded by the ABBA session below once it is written; numbers left as recorded._

| invocation | base median ms | base min ms | base max RSS | candidate median ms | candidate min ms | candidate max RSS |
|---|---:|---:|---:|---:|---:|---:|
| 1 | 91.48 | 87.65 | 77,299,712 | 100.85 | 98.12 | 76,021,760 |
| 2 | 91.59 | 88.06 | 75,939,840 | 102.35 | 98.30 | 75,661,312 |
| 3 | 90.81 | 87.82 | 75,890,688 | 102.58 | 98.76 | 76,693,504 |
| 4 | 90.93 | 86.87 | 76,234,752 | 102.79 | 98.56 | 76,447,744 |
| **median of medians** | **91.205** | | **76,087,296** | **102.465** | | **76,234,752** |

- `wall_ns` relative: **+12.35 %** (cell no-regression bound 3.000 %) → **FAIL**.
- `wall_ns` absolute_max 100,000,000 ns: candidate 102.465 ms → **FAIL on this host** (base 91.2 ms
  passes; the locked baseline host measured 70.5 ms, so the absolute result carries the host caveat
  above — the relative result does not).
- `peak_rss_bytes` relative: +0.19 % (bound 4.952 %) → pass; absolute 76.2 MB ≪ 256 MiB → pass.

## Work counters (enabled arm, `--runs 3`, last measured run)

| metric | limit (`absolute_max`) | base | candidate | verdict |
|---|---:|---:|---:|---|
| `workspace.normalize_canonical_id.calls` | 11,313 | 11,313 (175,101 bytes) | **20,969** (374,745 bytes) | **FAIL (+85 %)** |
| `session.oxc_script_parse.calls` | (cell) | 40 | 40 | = |
| `session.oxc_eval_program_parse.calls` | 42 | 42 | 42 | = |
| `session.source_text_copy.amount` | 124,410 | 124,410 | 124,410 | = |
| `session.fact_observe.calls` | 16,917 | 16,917 | 16,917 | = |
| `session.indexed_ready_build.calls` | 8,032 | 8,032 | 8,032 | = |
| `session.semantic_cold_build.calls` | (cell) | 1,063 | 1,063 | = |
| `session.cache_admit_cacheable.calls` | (cell) | 1,063 | 1,063 | = |
| `session.semantic_dispatch.calls` | (cell) | 4,216 (alloc_count 477,396; alloc_bytes 103,677,354) | 4,216 (alloc_count **635,891**; alloc_bytes **130,438,210**) | calls =; allocations +33 % / +26 % |
| `compiler.carrier_parse.calls` | (cell) | 40 | 40 | = |
| `compiler.source_map_build.calls` | (cell) | 40 | 40 | = |
| `compiler.css_parse`, `compiler.css_transform`, `compiler.style_analysis` | zero-counter assertions | absent (0) | absent (0) | pass |
| `session.component_meta_digest` (output oracle `== 7161214711717846280`) | equality | 7161214711717846280 | 7161214711717846280 | pass — output digest equal |

## Reading

The component-meta output digest oracle is equal on base and candidate (`7161214711717846280`); the
compiled-output digest is UNOBSERVED by this cell (`evidence/A6/baseline-measurement.md:177`; the
`compiled_output` determinism line reads `N/A (no observations)` in both TSVs), so no equivalence
claim is made for compiled output. Every parse/build/admission `calls` counter is equal; the rows
that differ are listed exhaustively in "Counters that differ" below. The candidate is **slower**: +12.35 % wall on a cold 40-component batch, driven by an **85 % increase in
canonical-id normalizations** (9,656 extra calls, longer inputs — 20.7 vs 15.5 bytes per call) and
+33 % allocations inside `session.semantic_dispatch` (see "Counters that differ" for the full set of
rows that moved). The counter that exposes
this is the one the crate move had silently dropped (`governance-join.md`, class 2 defect): had it
not been restored, `workspace.normalize_canonical_id.calls` would read 0 and the `absolute_max` gate
would PASS.

Diagnosis (mechanism, for the adversarial-performance mandate to confirm): the relocated resolver's
call sites are one-to-one with the deleted `ProjectResolver` (compare the `normalize_canonical_id`
call lists in `identity-map.md`'s source and destination files), so the extra calls are not new
sites — they are **re-executions**. The `ModuleResolverCore::resolve_attempt` kernel is a pure
function of the `ResolverAttemptView`; the workspace driver (`verter_workspace/src/resolver.rs::
drive_attempt`) re-runs the whole attempt after every `NeedInputs` wave, so the pure resolution
prefix (`parent_dir`/`join_paths` → `collapse_path` → `normalize_canonical_id`, owner selection,
`normalized_starts_with`) executes once per wave instead of once per resolution
(`resolution_conversion_tests::full_driver_resolves_via_a_workspace_alias` asserts ≥ 3 waves for
one alias resolution). The old engine resolved in a single pass with live reads. This is a property
of the ratified `AttemptOutcome`/`NeedInputs` design as implemented (no prefetching frontier across
waves, no per-attempt memo), not of any one edit — which is why it is reported here rather than
patched: reducing waves or memoizing derived path values across waves is a production design change
the landing-path ruling does not order ("no production rewrite is ordered"), and reweighting or
reinterpreting the cell is forbidden (S2-F6).

**Verdict on the locked cell as it stands: the candidate FAILS `A6_META_COMPILE_40_COLD_RUST`**
(two required metrics: `wall_ns` relative +12.35 % > 3 %, `workspace.normalize_canonical_id.calls`
20,969 > 11,313). This was an open in-scope finding at that historical recovery point; later sections
record the correction and the final supersession.

## Counters that differ

Column-wise diff of `base-counters.tsv` against `cand-counters.tsv` (every `site` row whose line is
not byte-identical; the `digest` column is equal on every row and omitted). No causal claim is made
here beyond the numbers themselves; `ns` and allocation columns are single-run (`--runs 3`, last run)
observations. Rows not listed are identical in both files. The enabled-arm `wall_median_ms` is
116.76 (base) vs 119.91 (candidate); `wall_min_ms` 113.43 vs 118.03.

| site | calls base | calls cand | amount base | amount cand | ns base | ns cand | alloc_count base | alloc_count cand | alloc_bytes base | alloc_bytes cand | dealloc_bytes base | dealloc_bytes cand |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `workspace.normalize_canonical_id` | 11313 | 20969 | 175101 | 374745 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `workspace.env_hash` | 4 | 4 | 213 | 221 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `session.indexed_ready_build` | 8032 | 8032 | 0 | 0 | 15770176 | 15409361 | 107929 | 107923 | 17035295 | 16715151 | 11478891 | 11320511 |
| `session.shallow_file_state_build` | 41 | 41 | 0 | 0 | 75915 | 77751 | 1285 | 1285 | 239747 | 239747 | 3009 | 3009 |
| `session.eval_env_build` | 41 | 41 | 0 | 0 | 253831 | 262624 | 1994 | 1994 | 358753 | 358753 | 195603 | 195603 |
| `session.prepared_decl_build` | 41 | 41 | 0 | 0 | 97129 | 81961 | 1590 | 1590 | 105309 | 105309 | 18124 | 18124 |
| `session.publish_field_types` | 40 | 40 | 0 | 0 | 985377 | 938997 | 10661 | 10663 | 869441 | 876909 | 763179 | 767091 |
| `session.semantic_dispatch` | 4216 | 4216 | 0 | 0 | 338433318 | 337507018 | 477396 | 635891 | 103677354 | 130438210 | 71071025 | 97687469 |
| `session.resolve_decl` | 4 | 4 | 0 | 0 | 30083 | 26834 | 186 | 187 | 16304 | 16636 | 13592 | 13592 |
| `session.import_route_resolve` | 160 | 160 | 0 | 0 | 4539374 | 6323912 | 29760 | 29760 | 1917600 | 1917600 | 1716440 | 1716440 |
| `session.instantiate` | 38 | 38 | 0 | 0 | 2270458 | 1700836 | 6899 | 6897 | 598170 | 587654 | 506575 | 501383 |
| `compiler.template_codegen_runtime` | 40 | 40 | 0 | 0 | 511044 | 590039 | 3680 | 3680 | 369070 | 369070 | 372430 | 372430 |
| `runtime.unattributed_allocation` | 0 | 0 | 0 | 0 | 0 | 0 | 839015 | 939642 | 128974689 | 145684402 | 168051974 | 184718381 |
| `scheduler.task_execute` | 130 | 149 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `scheduler.task_wait` | 130 | 149 | 153597792 | 158421791 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `scheduler.queue_depth` | 130 | 149 | 41 | 41 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Recovery candidate sessions

No completed ABBA timing claim is made for the recovery candidate because the conjunctive work-counter
gate already fails. The exact enabled-arm rerun was produced on the code tree later committed as
`5b3b58dd9fb6587e5a3a6bd64db55515a0294c91` (tree
`7fa962b50a779525d2321cef39ed4811b1e1856a`):

- candidate binary SHA-256: `3603084a134ee9669e96ad8c2179517b972a4e6f1f7ab0d3e28a2d57b31b1df0`;
- `workspace.normalize_canonical_id.calls`: **17,349** (down from 20,969, but still above the
  locked `absolute_max = 11,313`) — **FAIL**;
- `session.semantic_dispatch.calls`: 4,216, `session.semantic_cold_build.calls`: 1,063,
  `session.cache_admit_cacheable.calls`: 1,063 — equal to base;
- `session.component_meta_digest`: `7161214711717846280` — equal to the locked oracle;
- raw output: `cand-counters.tsv`.

The first recovery correction retains a request-local `ResolveFrame` across `NeedInputs` waves and
lazily constructs unused resolution geometry. Its discriminating test proves input-only retries do
not renormalize prior geometry while preserving result, ordered load sets and consumed observations;
the 24 converted cases and seven driver cases remain green.

The follow-up architecture consult (`unblock-architecture-consult.md`) identified the remaining
ratified request-local correction: preserve canonical provenance inside that frame's existing memo
and trust canonical values only on the private workspace replay path. The correction landed in
`7e5bc72e2f0e8533b80de99aa593631f64ae5463` and
`799140d8bbf5021efd9354f94254ad2f0d424a30`. Focused provenance tests were RED 4/5 before the first
change and GREEN 5/5 after it; the canonical workspace replay test was compile-RED before its private
methods existed and GREEN after them; the fixed manifest append control was compile-RED then GREEN.
Independent frames and a cleared basis each retain their required first-use normalization.

The exact enabled-arm rerun on `799140d8bbf5021efd9354f94254ad2f0d424a30` (tree
`b516b209d148a31aa79a9c69ec6ec1a28bf04671`) produced:

- candidate binary SHA-256: `78a761fad10fc5d438bbdcbbf6b3ae6a445331e53f6db4ccdefb636858c7d235`;
- `workspace.normalize_canonical_id.calls`: **11,557**, still above the locked
  `absolute_max = 11,313` by 244 — **FAIL**;
- the smaller-corpus control is deterministic and linear: files 0/1/2/3 produce
  13/364/651/938 calls;
- all other gated enabled metrics pass: carrier parse 40, script parse 40, eval parse 42,
  source text copy 124,410, fact observations 16,917, indexed-ready builds 8,032, semantic
  dispatches 4,216, cold builds 1,063, cacheable admissions 1,063, source-map builds 40, and
  the three CSS counters remain zero;
- `session.component_meta_digest`: `7161214711717846280`, equal to the locked oracle;
- raw output: `cand-counters.tsv`.

The disabled timing/RSS arm was not run on this exact candidate. The cell is conjunctive and already
fails its deterministic work-counter arm, so timing cannot cure or reinterpret it. The unblock consult
requires a stop when the enumerated canonical-provenance duplicates have all been removed and the
exact cell still exceeds 11,313. That stop condition now holds.

The earlier memo consult (`memo-architecture-consult.md`) still forbids cross-request
`ResolutionSharedMemo` retention as a new unbounded cache authority. No cache, owner, public API,
metric, load-set, witness, or output change was introduced. That intermediate Step-6 candidate could
not freeze.

## Residual-244 fast-path session

The source-coverage diagnostic `residual-244-diagnostic.md` attributed 9,576 of the remaining 11,557
calls to `ResolutionOverlaySnapshot::get` canonicalizing lookups against an empty immutable
request-local overlay. Its permitted private emptiness return landed in
`1a4e41d5c604f7cf2e36933ca09bbd8c5ff6ea8e` (tree
`3cfc2f81b4b451519c3074ddfd165c6367048a5c`). The focused lookup controls were RED 1/4 before the
branch (only the empty case failed with one call) and GREEN 4/4 after it; upsert, tombstone, and
unknown-key lookups on a nonempty overlay continue to normalize exactly once.

Exact enabled arm:

- binary SHA-256: `b9e36e285e12672f0280b60b729a3137ebee77e0a9be16dd5707d10d106dfe83`;
- `workspace.normalize_canonical_id.calls`: **1,981** <= 11,313 — pass;
- every other gated counter is at its lock or below, the three CSS assertions remain zero, and
  `session.component_meta_digest` is exactly `7161214711717846280`;
- raw output: `cand-counters-fast-path.tsv`.

Exact disabled arm (binary SHA-256
`2b84640168ae5db0e6bdf0d449d8261c12f224ea8bb1cc64f126b921c93f7ecf`) ran four consecutive
30-sample invocations. Medians were 99.27, 100.08, 100.43, and 101.20 ms; the median of medians is
**100.255 ms**:

- `wall_ns absolute_max = 100.000 ms`: **FAIL** by 0.255 ms;
- versus the locked 70.525 ms baseline: **+42.1553%**, above the 3.000% no-regression limit;
- same-host historical base from this receipt is 91.205 ms, so even that diagnostic comparison is
  **+9.9227%**;
- maximum-RSS readings were 77,119,488, 75,415,552, 74,809,344, and 75,661,312 bytes; their median
  is **75,538,432 bytes**, +0.9193% versus 74,850,304 and below both RSS limits — pass;
- exact gated fields: `cand-wall-fast-path.txt`.

The locked cell remains **FAIL** because its metrics are conjunctive. Enabled work and output now
pass, but disabled wall time does not. No later pre-freeze obligation or freeze could proceed on that
intermediate candidate.

## Final post-authority supersession of the recovery blocker

The registered performance ruling does not rewrite any historical result above. A fresh exact-source
session on production/evidence subject `6fd3356e3d1ec7d21e4f03850a224283ef43371e` is recorded in
`final-waiver-application.md`: every enabled counter/digest, absolute wall, and both RSS gates PASS;
relative wall remains literal **FAIL** at `+10.940378%` and is covered only by
`C1-A6-WALL-REL-001`. The prior absolute-wall blocker is therefore closed without changing a lock or
restamping old evidence.
