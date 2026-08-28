# C1 A6 final round-2 performance evidence

> Historical exact-subject evidence only. Later corrective production
> changes descend after the measured subject `e0d6732a…`; therefore none of
> the values or exact-subject dispositions in this report transfer to the
> current candidate. No performance measurement was run during rounds 3 or 4.

Status: **BLOCKED — no acceptance claim**. The exact locked A6 work, output,
digest, completeness, absolute wall, absolute RSS, relative RSS, and measured
correctness gates pass. Relative wall remains a literal **FAIL** under the
user's exact-session waiver. APM-002 allocation count and bytes remain literal
**FAILS**, and `C1-APM002-ALLOC-REL-001` does not cover this subject or these
values; that uncovered regression blocks exact-subject acceptance.

## Identities and conditions

| Item | Base | Production subject |
|---|---|---|
| Commit | `d1f3d50a948597f036868543b9bb21acacd730ff` | `e0d6732a26ce3bb4a3a458ae8c2c484fd42fdc7a` |
| Tree | `2e7cf8637ec5c52b0fa04572d99672b052f1f85f` | `f45176eb1fc7f517a9d2efd6c3a1d2801e8b4172` |
| Harness blob | `efa9ea54a14772ecd87511d6bb07017aa33940ba` | same |
| Harness/corpus SHA-256 | `5e06d35dda284a8ef049bf0dd3dc39974b904729f740da58c650ec59e806f632` | same |
| `rust-toolchain.toml` blob | `27d6fdd1ac927948bafc650047c157b2369a4f9e` | same |
| Root `Cargo.toml` blob | `56a3fc6759c2f0220529ce499b66a5e2d4ef7b74` | same |
| `.cargo/config.toml` blob | `fcee591a87b82fd1b7acd377d8d9c4fbc775fd92` | same |
| Enabled binary SHA-256 | `ebf6c2f0762228ae2a4ac01c3fdca5c436750b312c9d093187db99e0d0d832f3` | `5873cff02066437739175c6cfcc30e9b1870fe3a24a337d776ae770653bbd4f0` |
| Disabled binary SHA-256 | `4936acffe4e563d04c37cd7144fbda74c2ececba14e3fcfa5b0620b0a9d65ffd` | `12a0943011da59e4c36ae4f8e7d3471272166d7a37fe69504a49a4d2e9a0d639` |

`Cargo.lock` differs only for workspace membership. The two
`performance-gates.toml` blobs differ only by the stale-to-correct harness
SHA-256 repair; `node scripts/validate-performance-gates.mjs --gates
performance-gates.toml` passed all four cells and 56 metrics. Clean `git
archive` exports, source trees, Cargo targets, outputs, and executable copies
were isolated under `/tmp/c1-final-round2-performance.ZCJup2`.

The host was macOS 26.6 / Darwin 25.6.0, `Mac15,13`, Apple M3, arm64, eight
logical CPUs, 24 GiB RAM, `rustc 1.97.1 (8bab26f4f)` and Cargo 1.97.1. The
agent was `gpt-5.6-sol` at `xhigh`. Preflight and all 20 disabled-session
pre/post receipts reported AC power, AC low-power mode off, and no thermal,
performance, or CPU-power warning. No cargo, rustc, nextest, benchmark,
BrowserStack, or unrelated build/test competitor was present, so nothing was
terminated. Idle system updater daemons were recorded. RustDesk was preserved
under the explicit waiver; its server sampled 23.8–37.4% CPU. The rust lock was
not used under the exclusive-machine waiver.

## Exact commands

From the repository root, with `BASE`, `SUBJECT`, and `RAW_ROOT` bound to the
identities/root above:

```sh
git archive --format=tar "$BASE" > "$RAW_ROOT/meta/base-source.tar"
git archive --format=tar "$SUBJECT" > "$RAW_ROOT/meta/subject-source.tar"
tar -xf "$RAW_ROOT/meta/base-source.tar" -C "$RAW_ROOT/source/base"
tar -xf "$RAW_ROOT/meta/subject-source.tar" -C "$RAW_ROOT/source/subject"

(cd "$RAW_ROOT/source/base" && CARGO_TARGET_DIR="$RAW_ROOT/target/base-enabled" \
  cargo build -p verter_bench --release --features attribution --example attribution_baseline)
(cd "$RAW_ROOT/source/subject" && CARGO_TARGET_DIR="$RAW_ROOT/target/subject-enabled" \
  cargo build -p verter_bench --release --features attribution --example attribution_baseline)
"$RAW_ROOT/binaries/base-enabled" --files 40 --runs 3 --format tsv \
  > "$RAW_ROOT/raw/enabled/base.tsv" 2>&1
"$RAW_ROOT/binaries/subject-enabled" --files 40 --runs 3 --format tsv \
  > "$RAW_ROOT/raw/enabled/subject.tsv" 2>&1

(cd "$RAW_ROOT/source/base" && CARGO_TARGET_DIR="$RAW_ROOT/target/base-disabled" \
  cargo build -p verter_bench --release --example attribution_baseline)
(cd "$RAW_ROOT/source/subject" && CARGO_TARGET_DIR="$RAW_ROOT/target/subject-disabled" \
  cargo build -p verter_bench --release --example attribution_baseline)
```

The disabled session captured `pmset -g batt`, `pmset -g therm`, and the
filtered process table before and after every invocation. Every invocation used
the following form, with a 20-second equal idle cadence:

```sh
sleep 20
/usr/bin/time -l "$BINARY" --files 40 --runs 30 > "$RAW_FILE" 2>&1
```

Exactly one complete session ran, without tuning or repetition, in this order:

```text
control-start, A1, B1, B2, A2, A3, B3, B4, A4, control-end
```

The raw manifest was created and verified with:

```sh
(cd "$RAW_ROOT" &&
  find raw meta binaries -type f ! -path 'meta/SHA256SUMS.txt' -print0 |
  LC_ALL=C sort -z | xargs -0 shasum -a 256 > meta/SHA256SUMS.txt &&
  shasum -a 256 -c meta/SHA256SUMS.txt)
```

## Enabled attribution arm

Both results report 41 corpus files (40 components plus one shared module),
three runs, attribution on, warmup 40/40, and deterministic component-meta
run1/run2 agreement. The base/subject raw TSV SHA-256 values are
`d30a5de40abfa95fb103ef39b184e41af770b44c94947488ed2310a54eae21d0`
and `757ff22206d915736a9410aab9534a69365a6e78556eef2a77b7c22f9cbfcef5`.

| Configured metric | Limit | Base | Subject | Literal verdict |
|---|---:|---:|---:|---|
| `compiler.carrier_parse.calls` | max 40 | 40 | 40 | PASS |
| `session.oxc_script_parse.calls` | max 40 | 40 | 40 | PASS |
| `session.oxc_eval_program_parse.calls` | max 42 | 42 | 42 | PASS |
| `session.source_text_copy.amount` | max 124,410 | 124,410 | 124,410 | PASS |
| `workspace.normalize_canonical_id.calls` | max 11,313 | 11,313 | 1,981 | PASS |
| `session.fact_observe.calls` | max 16,917 | 16,917 | 16,917 | PASS |
| `session.indexed_ready_build.calls` | max 8,032 | 8,032 | 8,032 | PASS |
| `session.semantic_cold_build.calls` | max 1,063 | 1,063 | 1,063 | PASS |
| `session.cache_admit_cacheable.calls` | min 1,063 | 1,063 | 1,063 | PASS |
| `session.semantic_dispatch.calls` | max 4,216 | 4,216 | 4,216 | PASS |
| `compiler.source_map_build.calls` | max 40 | 40 | 40 | PASS |
| CSS parse / transform / style-analysis | exact zero | 0 / 0 / 0 | 0 / 0 / 0 | PASS |
| `session.component_meta_digest` | exact `7161214711717846280` | exact | exact | PASS |

Work, admission, output oracle, and the measured correctness/digest contract are
equal. `compiled_output` is `N/A (no observations)` in both arms, as defined by
this harness; no compiled-output equivalence claim is made.

| APM-002 `session.semantic_dispatch` measure | Base | Subject | Delta | Literal verdict |
|---|---:|---:|---:|---|
| Allocation count | 477,383 | 595,309 | +117,926 (+24.702597286%) | **FAIL** |
| Allocation bytes | 104,078,206 | 137,007,048 | +32,928,842 (+31.638556491%) | **FAIL** |

The ratified `C1-APM002-ALLOC-REL-001` disposition is expressly limited to
subject `22532f39faa649c5d818baa67dee0a78ab18bb3a` and the exact comparisons
477,374 to 599,479 allocations and 104,034,454 to 137,459,420 bytes. It says it
waives no future-subject result. Therefore it cannot cover this measured
subject or either measured value, and the current wall/control waivers do not
dispose of this failure.

## Default-feature disabled arm

All ten invocations exited zero and report 40 files, 30 samples, attribution
off, and warmup 40/40. The four base and four subject invocations are the sole
inputs to the arm aggregates.

| Aggregate | Base/control start | Subject/control end | Delta | Literal verdict |
|---|---:|---:|---:|---|
| Control median wall | 89.12 ms | 88.22 ms | -1.009874327% signed; 1.009874327% absolute | PASS (`<=3%`; waiver unused) |
| Median of four wall medians | 88.440 ms | 98.805 ms | +10.365 ms; +11.719810041% | **FAIL** (`>3%`; user-waived, not relabelled) |
| Absolute wall | — | 98.805 ms | — | PASS (`<=100 ms`) |
| Maximum RSS | 77,840,384 B | 77,512,704 B | -0.420964008% same-host | PASS absolute |
| RSS vs frozen 74,850,304 B | — | 77,512,704 B | +3.556966181% | PASS (`<=4.952%`) |
| Median instructions | 53,217,039,534.0 | 58,509,081,512.5 | +9.944262260% | diagnostic |
| Median cycles | 17,896,029,896.5 | 19,191,696,179.5 | +7.239964900% | diagnostic |

Absolute RSS is 77,512,704 B, below 268,435,456 B. Session completeness is
10/10 invocations, 30/30 samples per invocation, equal cadence, and 40/40
warmup each. No control or sample was discarded.

## Raw evidence and protected-byte proof

The curated external bundle contains 66 SHA-256 entries covering every regular
file under `raw/`, `meta/`, and `binaries/` except the self-referential manifest.
It includes both source-export tar archives, all build/run logs, condition
receipts, summaries, identities, protected-byte receipts, and all four accepted
binary copies. Disposable extracted `source/` trees and isolated `target/`
trees are represented by the source archives, build logs, identity receipts,
and copied binaries. `meta/SHA256SUMS.txt` verifies cleanly and has SHA-256
`91cb61887d97c6baba21cbb1cf9ba2e98a7b0ee640313e6354799ca54890b91b`.

Before measurement, the worktree was clean and SHA-256 was recorded for all
13,965 tracked files. After both arms, the same command regenerated an
entry-for-entry identical list (`cmp` PASS), and the worktree was still clean.
The only repository mutation after that proof is this report; no production,
test, harness, corpus, toolchain, configuration, gate, authority, ledger, or
registry byte was changed. Code-quality and canonical-gate claims are not
restamped by this measurement-only report; their existing subject evidence is
preserved unchanged.

**Final verdict: BLOCKED by the uncovered exact-subject APM-002 allocation
regression.** All other measured retained gates above pass, while the waived
relative-wall failure remains recorded literally.
