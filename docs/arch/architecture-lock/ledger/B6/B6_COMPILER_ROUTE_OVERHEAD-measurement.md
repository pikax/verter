# Compiler route-overhead — measured landing evidence

The charter's exit criterion requires this block's four compile routes to be measured.
This document is that measurement. It records what was run, on which tree, and what came
back. It does **not** define, choose, or tune a threshold, and it does not write a
`[[cell]]` entry into `performance-gates.toml`.

**Why no threshold here.** The `B6_COMPILER_ROUTE_OVERHEAD` cell is owned by
`block/route-overhead-cell-lock`, which derived its bounds from the already-locked A6
cell and its own uncontaminated calibration session — deliberately *not* from the
numbers below. A block supplying the threshold its own numbers are then judged against
is exactly the post-measurement gate selection ADR-016 forbids. The numbers here are
contaminated for threshold-selection purposes and must never be used for it: they were
taken on a shared developer machine under concurrent load, not under a locked-machine
protocol.

## How to read this, and when it must be re-taken

These numbers are bound to specific blobs, not to a branch. Any change to
`standalone.rs`, `assembly/publish.rs`, the Svelte runtime, the parser's retained-byte
accounting, or the harness itself invalidates the recorded digest and requires
re-running. That was missed twice during review, so the rule is written down here rather
than left to be rediscovered.

## Measured tree

| | |
|---|---|
| Commit the binary was built from | `26afa1c1873910181273984b422836bbb69dd70c` |
| `crates/verter_bench/examples/compiler_route_overhead.rs` | `fda6bb0a5f868e4ef870c5c36364a324728f9d35` |
| `crates/verter_compiler/src/standalone.rs` | `8024c228c1b4c134e4dc8e7e5a1eb4e31fe0dce7` |
| `crates/verter_compiler/src/assembly/publish.rs` | `8d8b339d17d85af7b7d12a58b59c3a6ff0a14b24` |
| `crates/verter_parser/src/ast/types.rs` | `286b52d4a21096c7208bd05fe243d8d4a93240db` |
| `crates/verter_parser/src/parser/types.rs` | `cd894acb2d184a35e74fcfa8efd1f51fce4a5006` |
| `crates/verter_compiler/src/svelte/parser/retained_weight.rs` | `8cbc75de2ec91b304de3c9c411d15c5b9f73ef0e` |

The check to run: every blob hash above must equal `git rev-parse HEAD:<path>`. If any
differs — including by a comment — this record is out of date and the numbers below are not
evidence for HEAD. Two earlier revisions tried to avoid re-running with a cleverer rule
(a named tip, then a "no path under `crates/`" claim) and both decayed into false
statements while the measurement itself stayed valid, which is the worse failure: a true
number wrapped in a false claim reads as verified.

The digest value below differs from earlier revisions of this record because the hasher
changed: emitted-import facts are no longer a published field and are no longer hashed. The
work counters and cross-route equality are unchanged, which is the point — the oracle got
narrower, the routes still agree.

## Command

```
CARGO_BUILD_JOBS=4 cargo build -p verter_bench --release --features attribution \
  --example compiler_route_overhead
./target/release/examples/compiler_route_overhead      # x10
```

`--features attribution` is required (`required-features` on this example): the harness
reads the real `compiler.carrier_parse.calls` counter
(`verter_audit::attribution::{reset, read}`) around every leg instead of trusting its own
loop-iteration count, and that counter does not exist in the build without the feature.
An unmeasured build is therefore a hard error, not a silently zeroed run.

## Result

```
batch route: item_count=14 distinct_group_count=7 (independently known from corpus.len())
{"route":"direct","cold_build_count":7,"reuse_count":0,"measured_carrier_parse_calls":7,"digest":"e5bb61c9…"}
{"route":"prepared-first","cold_build_count":7,"reuse_count":7,"measured_carrier_parse_calls":7,"digest":"e5bb61c9…"}
{"route":"prepared-repeat","cold_build_count":0,"reuse_count":35,"measured_carrier_parse_calls":0,"digest":"e5bb61c9…"}
{"route":"batch","cold_build_count":7,"reuse_count":14,"measured_carrier_parse_calls":7,"digest":"e5bb61c9…"}
all routes produced byte-identical output (7 fixtures)
measured carrier_parse.calls — direct=7 prepared-first=7 prepared-repeat=0 batch=7 (n=7, repeats=5)
```

Ten consecutive runs, forty leg-runs in total. Every run exited 0. Across all forty, the
digest was the single value
`e5bb61c90a3eea9154593277e63f90633678382b1b251f9feff230218fa6204b`, and each route's
`(cold_build_count, reuse_count, measured_carrier_parse_calls)` triple was constant:
`direct (7, 0, 7)`, `prepared-first (7, 7, 7)`, `prepared-repeat (0, 35, 0)`,
`batch (7, 14, 7)`. `latency_ms` and `rss_*` are the ONLY fields that varied, and they
are in the raw log rather than here — see below for why.

Complete unedited stdout for all ten runs: [`route-overhead-run.txt`](route-overhead-run.txt).
It is committed beside this file. The `.txt` extension is deliberate: `*.log` is
gitignored, so an earlier revision of this record linked to a receipt the tree did not
contain.

## What in this actually discriminates

Two things, and they are the reason the run is worth taking:

- **The output oracle.** All four routes produce the same 32-byte result digest over
  every field the published artifacts, styles and diagnostics expose. The harness
  asserts equality itself and exits 101 on breach. The digest VALUE is a per-tree
  constant with no meaning of its own; the load-bearing claim is that the four routes
  agree on it. Coverage of the digest's inputs is NOT uniform, and the difference
  matters: every per-artifact, per-style and per-diagnostic FIELD has its own
  discriminating test in the `identity_digest_*` suite in
  `assembly/publish_tests.rs`, with the executed plants recorded in
  `docs/arch/refactor/rev11/evidence/B6/mutation-replay-recipes.md`. Two of the
  suite's COUNT prefixes — `artifacts.len()` and `diagnostics.len()` — have no
  individual isolator at all: each can be deleted from the hasher with every test in
  that module still green, and they are covered only in aggregate. Two other
  discriminators exist but need a pair that a single-record comparison cannot express
  — a diagnostic span's presence tag and the `styles.len()` prefix — because the
  encoding is prefix-free within one record but not across concatenated ones.
- **The measured work counters,** asserted as exact equality in both directions, so a
  missing parse and an extra parse both fail. `prepared-repeat` asserting exactly ZERO
  carrier parses is the load-bearing claim that a reused carrier never re-parses; it is
  the assertion a `compile_prepared` regression would actually break. The batch leg
  submits the corpus TWICE (`item_count` = 2n over n independently known distinct
  source keys), so `cold_build_count == n` is a real dedup observation rather than a
  restatement of the loop bound.

## What in this does NOT discriminate

`latency_ms` and `rss_*` are recorded below and in the raw log for completeness. They are
**informational only** and must not be read as evidence that the routes are performing
correctly:

- The whole measured operation is sub-millisecond, so process-start and scheduler jitter
  dominate any real signal. On this run the observed `latency_ms` range across all forty
  leg-runs was 1.127–7.282 ms — a 6x spread on an operation whose real work is a fraction of a millisecond and `rss_peak_bytes` 7.03–7.80 MiB.
- Independent verification of the locked cell measured its wall-clock arms as having
  near-zero teeth at this operation's size: the absolute wall bound sits far above the
  holdout median and first trips only at a regression of several thousand percent. A
  green wall result is close to unfalsifiable here.
- This machine was under concurrent load from other blocks' builds throughout. That is
  fine for the counters and the digest, which are deterministic, and disqualifying for
  the timings, which are not.

Judge the routes by the counters and the oracle. Treat the timings as context.

## Corpus

The corpus asserts its own pairwise source distinctness inside `build_corpus`, so the
`distinct_group_count == corpus.len()` expectation is checked rather than assumed. The
identity corpus additionally repeats one source under a different request and includes a
diagnostic-producing source and a map-requesting product, so the digest's map and
diagnostic sections carry real content instead of being empty on every fixture.

## Superseded

Earlier revisions of this document recorded runs that have since been replaced, each for
a stated reason: a harness whose `cold_build_count`/`reuse_count` were assigned from its
own loop-iteration counts (true by construction, and unable to fail when
`compile_prepared` regressed to re-parsing); a batch leg that submitted one item per
distinct source, so `cold_build_count == n` held whether or not anything deduped; and
two receipts whose recorded commit no longer matched the harness or production blobs the
numbers were taken from. Only the measurement above is current. The stable conjuncts
across every revision have been the work counters, cross-route digest equality, and
exit 0.
