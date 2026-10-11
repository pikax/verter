# Relation rows of the semantic benchmark: answers and work growth

This page records the semantic benchmark's relation rows
(`relation-aligned-N`, `relation-aligned-false-N`, `relation-reversed-N`):
each row's answer class against tsc 7.0.2, the warm-repeat checks, and the
deterministic work and allocation counts at several sizes with their growth
ratios. It is a portable summary; the raw harness runs stay out of the tree.

## How it was measured

- **Worker.** One shared Windows 11 x86_64 developer worker, not the
  benchmark machine. Under the
  [measurement rule](../contributing/semantic-benchmark.md#measurement-rule)
  every time and memory cell of this run is **not measured**; the cells below
  are answer classes and work counts, which are the same on any worker.
- **Tree.** "fix(core): end a union relation at its first unrelated arm and
  name a refused check's subjects" (2026-10-09), plus the tsc references of
  the `relation-aligned-false-N` series added to
  `scripts/benchmark/semantic-perf/expected.json`.
- **References.** `node scripts/benchmark/semantic-perf/measure-expected.mjs
  --only relation-aligned-false` under `verter-supervise` (hard containment):
  tsc 7.0.2 answers `2`, with no diagnostic, at 200, 600, 1,800 and 3,200
  arms in all four `strictNullChecks` × `noImplicitAny` settings.
- **Answers and allocations.** `node scripts/benchmark/semantic-perf.mjs
  --tier stress --only relation-`. The quick tier admits only the three
  200-arm rows (`relation-aligned-200`, `relation-aligned-false-200`,
  `relation-reversed-200`); every other size in the tables below is a
  standard or stress scenario, and the stress tier (which includes the
  lighter tiers) is the one that runs them all. Arms: `verter`, `tsc-api`,
  `verter-obs`, `verter-observe`, `verter-counted`, one warmup, three
  measured fresh processes and three in-process warm repeats per arm,
  setting `strict`. The harness validates that every tsc answer reproduces
  its reference and that repetitions of each arm agree on the answer and its
  class. The quick-tier rows are the 200-arm subset of this run.
- **Work.** `cargo run --release -p verter_bench --features semantic-observe
  --example resolver_dispatch_profile -- <scenario-dir>...` over the run's
  `scenarios/relation-*/strict` directories: resolver port calls and
  semantic nodes per phase on a fresh host (cold, warm, then an unrelated
  declaration appended and the probe cold and warm again).
- **Allocations.** The `verter-counted` arm's cold-request allocation count
  (median of three processes; the three readings of every row agree to within
  0.02%).

## Answer classes

| scenario | tsc 7.0.2 (measured) | Verter | warm repeats answer the cold answer | time and memory |
| --- | --- | --- | --- | --- |
| relation-aligned-200 | `1` | matched | yes | not measured |
| relation-aligned-600 | `1` | matched | yes | not measured |
| relation-aligned-1800 | `1` | matched | yes | not measured |
| relation-aligned-3200 | `1` | matched | yes | not measured |
| relation-aligned-false-200 | `2` | matched | yes | not measured |
| relation-aligned-false-600 | `2` | matched | yes | not measured |
| relation-aligned-false-1800 | `2` | matched | yes | not measured |
| relation-aligned-false-3200 | `2` | matched | yes | not measured |
| relation-reversed-200 | `1` | matched | yes | not measured |
| relation-reversed-600 | `1` | matched | yes | not measured |
| relation-reversed-1800 | `1` | partial (unevaluated conditional) | yes | not measured |
| relation-reversed-2100 | `2` + TS2859 | partial (unevaluated conditional) | yes | not measured |
| relation-reversed-3200 | `2` + TS2859 | partial (unevaluated conditional) | yes | not measured |

No row is refused, beyond-tsc, killed or unverified, and no row's
repetitions differ. Every false aligned row is matched and complete: the
conditional is decided to tsc's `2`, never left unevaluated. Every true
aligned row is matched, as before the false series was added.

## Work and allocation growth

Cold request on a fresh host. "Port calls" are the resolver-context calls the
cold request makes; "memo entries" are the semantic memo entries retained
after it.

| scenario | arms | semantic nodes | memo entries | port calls | cold allocations |
| --- | ---: | ---: | ---: | ---: | ---: |
| relation-aligned-200 | 200 | 617 | 424 | 178 | 35,938 |
| relation-aligned-600 | 600 | 1,817 | 1,224 | 178 | 103,154 |
| relation-aligned-1800 | 1,800 | 5,417 | 3,624 | 178 | 304,372 |
| relation-aligned-3200 | 3,200 | 9,617 | 23 | 177 | 449,303 |
| relation-aligned-false-200 | 200 | 629 | 231 | 2,814 | 37,116 |
| relation-aligned-false-600 | 600 | 1,829 | 631 | 8,014 | 103,937 |
| relation-aligned-false-1800 | 1,800 | 5,429 | 1,831 | 23,614 | 303,982 |
| relation-aligned-false-3200 | 3,200 | 9,629 | 3,231 | 41,814 | 537,167 |
| relation-reversed-200 | 200 | 629 | 29 | 260,226 | 1,292,910 |
| relation-reversed-600 | 600 | 1,829 | 29 | 2,340,226 | 11,431,345 |
| relation-reversed-1800 | 1,800 | 5,431 | 23 | 3,406,096 | 16,659,342 |
| relation-reversed-2100 | 2,100 | 6,331 | 23 | 3,406,408 | 16,682,514 |
| relation-reversed-3200 | 3,200 | 9,631 | 23 | 3,407,006 | 16,767,546 |

Growth between consecutive sizes (work ratio against size ratio; a linear
path keeps the two about equal):

| series | step | size ×  | semantic nodes × | port calls × | allocations × |
| --- | --- | ---: | ---: | ---: | ---: |
| aligned (true) | 200 → 600 | 3.00 | 2.94 | 1.00 | 2.87 |
| aligned (true) | 600 → 1800 | 3.00 | 2.98 | 1.00 | 2.95 |
| aligned (true) | 1800 → 3200 | 1.78 | 1.78 | 0.99 | 1.48 |
| aligned (false) | 200 → 600 | 3.00 | 2.91 | 2.85 | 2.80 |
| aligned (false) | 600 → 1800 | 3.00 | 2.97 | 2.95 | 2.92 |
| aligned (false) | 1800 → 3200 | 1.78 | 1.77 | 1.77 | 1.77 |

Both aligned series grow linearly in the union size: no work count grows
faster than the input, and the false series costs about 13 port calls per
arm (it ends at the first arm that relates to no target arm) with no
fallback to deferral at any size. The reversed series grows quadratically
up to 600 arms (×9.0 port calls for ×3 arms, the scan of the target per
arm), then flattens at about 3.4 million port calls from 1,800 arms on:
the relation stops on Verter's own work allowance there, which is the
partial answer above.

`relation-aligned-3200` retains 23 memo entries where the smaller aligned
rows retain about two per arm; its answer, node count and warm repeats are
unaffected (recorded as observed, not a class change).

The growth ratio of the aligned true and false series' relation work is
asserted by `false_aligned_object_unions_decide_at_the_first_arm`
(`crates/verter_session/src/project_semantic_dispatch_tests/relation_work_tests.rs`):
doubling the arms at most about doubles each series' connected work, and the
false relation costs no more than the true one. The allocation growth is
asserted by `aligned_relation_allocation_grows_linearly_with_the_arms`
(`crates/verter_session/tests/allocation_cases/construction_bytes.rs`): the bytes
allocated by each series at 200, 400 and 800 arms at most about double per
doubling.

## Warm repeats

Port calls of the repeated request on the same host, and after an unrelated
edit to the scenario (cold, then warm):

| scenario | cold | warm | edit-cold | edit-warm | refusal repeated without recomputation |
| --- | ---: | ---: | ---: | ---: | --- |
| relation-aligned-N (every size) | 177–178 | 13 | 204–205 | 15 | no refusal |
| relation-aligned-false-200 | 2,814 | 13 | 2,792 | 15 | no refusal |
| relation-aligned-false-600 | 8,014 | 13 | 7,992 | 15 | no refusal |
| relation-aligned-false-1800 | 23,614 | 13 | 23,592 | 15 | no refusal |
| relation-aligned-false-3200 | 41,814 | 13 | 41,792 | 15 | no refusal |
| relation-reversed-200 | 260,226 | 13 | 260,204 | 15 | no refusal |
| relation-reversed-600 | 2,340,226 | 13 | 2,340,204 | 15 | no refusal |
| relation-reversed-1800 | 3,406,096 | 3,405,945 | 3,406,072 | 3,405,953 | **no**: every warm repeat recomputes |
| relation-reversed-2100 | 3,406,408 | 3,406,257 | 3,406,384 | 3,406,265 | **no**: every warm repeat recomputes |
| relation-reversed-3200 | 3,407,006 | 3,406,855 | 3,406,982 | 3,406,863 | **no**: every warm repeat recomputes |

Every decided row's warm repeat is a cache hit (13 port calls) that answers
the cold answer. The three partial reversed rows repeat their answer too, but
each warm repeat makes the cold request's work again: the stopped relation is
not retained, so the refusal is recomputed rather than repeated. This is an
open finding against the relation engine, not against the benchmark: until a
same-root refusal is retained at its operation identity, these rows' warm
repeats cost what their cold request costs.
