# C1 A6 final round-6 performance evidence

Status: **EVIDENCE COMPLETE; LITERAL FAILURES USER-WAIVED; EXACT-SUBJECT REGISTRATION REQUIRED.**
This report makes no performance PASS claim for a failed metric. The measured production subject is
`2820cf2eb790caffdb69f59bc20402d7d0a6647b`, tree
`ef8efbec06c8e87d1d6d72d9ea8e69fa624f515b`. The registered comparison base is
`d1f3d50a948597f036868543b9bb21acacd730ff`, tree
`2e7cf8637ec5c52b0fa04572d99672b052f1f85f`.

The final complete session measures absolute wall **111.895 ms > 100 ms: FAIL**, relative wall
**+16.910458677% > 3%: FAIL**, and control drift **+3.096636412% > 3%: FAIL**. The user explicitly
waived the relative performance blocker, then control drift below 10%, and finally this exact C1
absolute-wall blocker; each result remains a literal FAIL. A narrow exact-subject authority act and
registry/program-state registration are still required before C1 may inherit those dispositions.
This evidence agent created no ruling or registry edit.

## Identity, isolation, and protocol

Base and subject used the same harness Git blob
`efa9ea54a14772ecd87511d6bb07017aa33940ba`, SHA-256
`5e06d35dda284a8ef049bf0dd3dc39974b904729f740da58c650ec59e806f632`; the same
`rust-toolchain.toml`, root `Cargo.toml`, and `.cargo/config.toml` blobs; Rust 1.97.1; macOS 26.6 /
Darwin 25.6.0; Apple M3; eight logical CPUs; and 24 GiB RAM. The subject's locked performance-gate
file validated as four cells and 56 metrics with no placeholders.

Clean `git archive` exports, source trees, Cargo targets, outputs, and executable copies were isolated
under external raw-root basename `c1-final-round6-performance.I9FZyo` in the host temporary directory.
The worktree was clean before measurement. SHA-256 over all 13,965 tracked files was identical after
measurement; the report is the only later repository mutation.

The user waived the Rust lock and required RustDesk to remain running. Grok remained present but had no
heavy child, J1 compilation, test, gate, or benchmark during any admitted build or timed invocation.
Every build and invocation had a fresh process check. Every timed pre/post receipt recorded AC power,
AC low-power mode off, no thermal warning, no performance warning, and no CPU-power warning. All 40
quiet receipts are empty. No process was terminated and no sample was excluded.

The binaries were built once per arm from isolated archive exports:

```sh
git archive --format=tar "$BASE" > "$RAW_ROOT/meta/base-source.tar"
git archive --format=tar "$SUBJECT" > "$RAW_ROOT/meta/subject-source.tar"
tar -xf "$RAW_ROOT/meta/base-source.tar" -C "$RAW_ROOT/source/base"
tar -xf "$RAW_ROOT/meta/subject-source.tar" -C "$RAW_ROOT/source/subject"

(cd "$RAW_ROOT/source/base" && CARGO_TARGET_DIR="$RAW_ROOT/target/base-enabled" \
  cargo build -p verter_bench --release --features attribution --example attribution_baseline)
(cd "$RAW_ROOT/source/subject" && CARGO_TARGET_DIR="$RAW_ROOT/target/subject-enabled" \
  cargo build -p verter_bench --release --features attribution --example attribution_baseline)
"$RAW_ROOT/binaries/base-enabled" --files 40 --runs 3 --format tsv
"$RAW_ROOT/binaries/subject-enabled" --files 40 --runs 3 --format tsv

(cd "$RAW_ROOT/source/base" && CARGO_TARGET_DIR="$RAW_ROOT/target/base-disabled" \
  cargo build -p verter_bench --release --example attribution_baseline)
(cd "$RAW_ROOT/source/subject" && CARGO_TARGET_DIR="$RAW_ROOT/target/subject-disabled" \
  cargo build -p verter_bench --release --example attribution_baseline)
```

Each disabled invocation used a 20-second cool cadence followed by a fresh quiet receipt and:

```sh
/usr/bin/time -l "$BINARY" --files 40 --runs 30
```

Both complete sessions used exactly
`control-start, A1, B1, B2, A2, A3, B3, B4, A4, control-end` with all 30 samples,
40/40 warmup, and no discretionary exclusion. No selective rerun occurred. After session 1 initially
failed the locked control fence, the manager predeclared a second-and-final pass, a five-minute monitored
no-build/no-benchmark cooldown, and no third pass. The user's below-10% control waiver arrived after that
authorization; session 2 therefore proceeded as already declared. The cooldown stayed on AC power with
no thermal/performance/CPU-power warning and no heavy process.

## Enabled attribution arm

Both results report 41 corpus files, three runs, attribution on, 40/40 warmup, and deterministic
component-meta run1/run2 agreement.

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

The measured work, admission, output oracle, and digest/correctness contract are equal. Compiled output
is `N/A (no observations)` in both arms, so this report makes no compiled-output equivalence claim.

| APM-002 `session.semantic_dispatch` | Base | Subject | Delta | Literal verdict |
|---|---:|---:|---:|---|
| Allocation count | 477,365 | 604,706 | +127,341 (+26.675814105%) | **FAIL** |
| Allocation bytes | 104,030,662 | 137,711,352 | +33,680,690 (+32.375733608%) | **FAIL** |

These comparative allocation limitations are not fixed or relabelled in C1. They join the existing
end-of-C-train performance-consolidation obligation under equivalent work and output.

## Disabled default-feature arm

### Session 1

Session 1 was initially classified invalid because its locked control drift was 5.184446660%. The later
user waiver for control drift below 10% covers session validity for C1 only; the locked result stays FAIL.
The second session had already been authorized before that waiver and was still run.

| Aggregate | Base/control start | Subject/control end | Delta | Literal verdict |
|---|---:|---:|---:|---|
| Control median wall | 100.30 ms | 95.10 ms | -5.184446660% signed; 5.184446660% absolute | **FAIL**, user-waived for validity |
| Median of four wall medians | 98.330 ms | 117.075 ms | +18.745 ms; +19.063358080% | **FAIL**, relative user waiver |
| Absolute wall | — | 117.075 ms | — | **FAIL** (`>100 ms`), later exact-subject user waiver |
| Maximum RSS | 77,692,928 B | 78,020,608 B | +0.421762969% same-host | PASS absolute |
| RSS vs frozen 74,850,304 B | — | 78,020,608 B | +4.235525884% | PASS (`<=4.952%`) |
| Median instructions | 53,580,977,176.5 | 61,591,567,078.5 | +14.950436375% | diagnostic |
| Median cycles | 19,250,470,098.5 | 21,639,489,218.5 | +12.410185870% | diagnostic |

### Session 2 — second and final

| Aggregate | Base/control start | Subject/control end | Delta | Literal verdict |
|---|---:|---:|---:|---|
| Control median wall | 93.65 ms | 96.55 ms | +3.096636412% | **FAIL**, user-waived for validity (`<10%`) |
| Median of four wall medians | 95.710 ms | 111.895 ms | +16.185 ms; +16.910458677% | **FAIL**, relative user waiver |
| Absolute wall | — | 111.895 ms | — | **FAIL** (`>100 ms`), exact-subject user waiver |
| Maximum RSS | 77,512,704 B | 76,709,888 B | -1.035721835% same-host | PASS absolute |
| RSS vs frozen 74,850,304 B | — | 76,709,888 B | +2.484404071% | PASS (`<=4.952%`) |
| Median instructions | 53,562,478,015.0 | 61,504,764,806.5 | +14.828079443% | diagnostic |
| Median cycles | 19,078,038,655.5 | 21,050,038,357.0 | +10.336490753% | diagnostic |

Session 2 is complete and usable only under the user's below-10% control-drift waiver. Its absolute-wall
failure independently corroborates session 1 and is not a control artifact. Both sessions are retained
whole; there is no third session.

## Raw evidence and landing implication

The external bundle contains 149 SHA-256 entries covering every regular file under `raw/`, `meta/`, and
`binaries/` except the self-referential manifest. It includes both source-export archives, four build
logs, all enabled/disabled raw results, 80 condition/quiet receipts, the five-minute cooldown receipts,
user/manager authority chronology, identities, summaries, protected-byte proof, and all four executable
copies. `meta/SHA256SUMS.txt` verifies cleanly and has SHA-256
`11c016f9a80ea8c8ffc16d564e69a4eb52b8cc426deb4e60b9b18cd9eff86892`.

Relevant raw identities:

| Artifact | Base SHA-256 | Subject SHA-256 |
|---|---|---|
| Enabled binary | `ebf6c2f0762228ae2a4ac01c3fdca5c436750b312c9d093187db99e0d0d832f3` | `16c08912af5255a641001d1b132034198ede0019144977be76a51a75425a7321` |
| Disabled binary | `4936acffe4e563d04c37cd7144fbda74c2ececba14e3fcfa5b0620b0a9d65ffd` | `8a53c2cb7fc2220df2830d626995e4bcb009fba493fd72d9413b08e64d1b4b1c` |
| Enabled raw result | `76afc8c3b747c92cb2af0fb47f1dad14d40344cfa4392bac481a9b1319fd04ff` | `d193ac0d04f516ae0517a668518ccb3a2504d6d2d958644c2016c71c9d5ad341` |

The literal failures are control drift, relative wall, absolute wall, and APM-002 allocation count/bytes.
The user has waived each performance blocker for this exact C1 landing and requires the limitations to be
addressed at end of the C train. Thresholds remain unchanged. Correctness/digest, configured work and zero
counters, absolute and relative RSS, code quality, scope, review identity, canonical gate, and landing
equivalence remain non-waivable.

Therefore C1 performance no longer blocks **after** a narrow exact-subject authority/disposition is
recorded and registered against this subject, report, raw manifest, literal results, waiver chronology,
and the existing non-rollable end-of-C-train owner. Until that registration exists, this report is evidence,
not an acceptance act.
