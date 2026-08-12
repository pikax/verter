# Framework conformance performance impact and pre-candidate lock

This proposal records required performance cells but deliberately does not invent
numeric thresholds without a BF1 baseline. BF1 must freeze exact fixtures, runner,
machine class, samples, statistics, absolute and relative thresholds, memory ceilings,
work counters, correctness oracle, and lease policy before BF2 implementation begins.
Thresholds cannot be changed after observing a successor candidate.

**FROZEN (2026-08-12):** `BF2_VUE_ORACLE_MANIFEST_GENERATE` and
`BF2_SVELTE_ORACLE_MANIFEST_GENERATE` are locked in `performance-gates.toml` (repository
root), closing the corresponding row of BF1's reopened exit criterion #6 for these two
cells specifically. They measure the real, already-authored, already-run BF1
evidence-preparation tool `generate-official-case-manifests.mjs` — NOT BF2's future
test-execution harness, which does not exist yet and cannot be measured without
violating "a performance criterion selected after candidate measurement" (BF1 charter,
Abort/rescope). Full derivation, raw 10-run session, sandbox network-denial profile, and
threshold arithmetic:
[`command-proofs/bf2-oracle-manifest-generate/`](command-proofs/bf2-oracle-manifest-generate/).
**ATTEMPTED-THEN-INVALIDATED (2026-08-12, BF2 harness landing) — STAYS OPEN:**
`BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE` was briefly locked in
`performance-gates.toml` from a 10-run measurement of BF2's own,
just-built golden generator
(`packages/framework-conformance-harness/bin/generate-goldens.mjs`). That
freeze was INVALID — the candidate chose its own pass criteria from its own
implementation, exactly what this document's abort language and
`docs/arch/refactor/rev11/governance.md`'s Gate authority rule forbid. A
Codex Sol xhigh architecture consult confirmed the violation and proposed a
maintainer-ratified bootstrap-gate protocol (independent gate authority,
pre-measure registration, disjoint calibration + holdout sessions); the
maintainer ruled FALLBACK — withhold the freeze rather than pursue that
protocol now. The row is restored to explicitly OPEN, exactly as this
document originally left it ("Deferred to whichever block first builds that
harness"); no cell is defined for this id in `performance-gates.toml`. The
invalidated 10-run session is retained only as audit evidence, never as
inputs to a future freeze:
[`command-proofs/bf2-official-compiler-invocation-golden-generate/`](command-proofs/bf2-official-compiler-invocation-golden-generate/).
Tracked as debt — durable owner, resolution gate, and acceptance ID at
[`../BF2/debt-BF2-perf-gate-deferred.md`](../BF2/debt-BF2-perf-gate-deferred.md).

The remaining six cells below (`BV1`/`BS1`/`B6`/`C4`-owned) are unaffected by
either freeze and stay deferred to their owning blocks' own landings, per
this document's original scope — no threshold is invented for them here.

## Required new cells

| cell | owner/candidate boundary | workload and required counters |
|---|---|---|
| `BF2_VUE_ORACLE_MANIFEST_GENERATE` | BF2 harness | **FROZEN** — see above. Official-case enumeration and classification (title-hash extraction, disposition assignment) over the pinned RC.3 source tree — makes ZERO calls to the Vue compiler and produces no golden output, only the `vue-official-cases.tsv` manifest; suite/row/disposition counts, peak RSS, wall time, zero network, byte-exact output digest oracle |
| `BF2_SVELTE_ORACLE_MANIFEST_GENERATE` | BF2 harness | **FROZEN** — see above. Same enumeration/classification scope over the pinned 5.56.8 source tree — ZERO Svelte compiler calls, no golden output, only the `svelte-official-cases.tsv` manifest; same counter classes |
| `BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE` | downstream owner (see debt record) | **OPEN — tracked debt.** The wider workload of actually invoking the official Vue/Svelte compilers against the harness's fixture/coverage-axis corpus to produce immutable golden output; golden-count/per-framework work counters, peak RSS, wall time, zero network, byte-exact combined-digest oracle. Not frozen by BF2 — see above and [`../BF2/debt-BF2-perf-gate-deferred.md`](../BF2/debt-BF2-perf-gate-deferred.md) |
| `BV1_VUE_VDOM_DIRECT_CORE` | BV1 algorithm | independent local core corpus, cold and warm; parse/semantic/plan/emit/map counts, bytes, allocations/RSS, latency |
| `BV1_VUE_VAPOR_DIRECT_CORE` | BV1 algorithm | same, distinct Vapor corpus and helper topology oracle |
| `BV1_VUE_SSR_DIRECT_CORE` | BV1 algorithm | server corpus with SSR correctness oracle and map on/off |
| `BS1_SVELTE_CLIENT_RUNES_DIRECT_CORE` | BS1 algorithm | runes corpus, dev/prod, topology/runtime oracle and work counts |
| `BS1_SVELTE_CLIENT_LEGACY_DIRECT_CORE` | BS1 algorithm | legacy only where supported; same counter classes |
| `BS1_SVELTE_SERVER_DIRECT_CORE` | BS1 algorithm | server corpus, render oracle, maps and memory |
| `B6_COMPILER_ROUTE_OVERHEAD` | B6 routes | identical corpus across direct, prepared first/repeat, and batch; output digest, reuse/cold-build counts, latency/RSS |
| `C4_PROJECT_ROUTE_EQUIVALENCE` | C4 staging | local imported-macro corpus; direct/staged output and type-query counts, latency/RSS |

Each cell is conjunctive: correctness, non-vacuity, zero-unrequested-work, absolute
limit, relative limit, memory, and required work counters all pass. A faster wrong or
zero-work run fails. Existing `A6_META_COMPILE_40_COLD_RUST` and every already-required
cell remain required and cannot be replaced or reweighted.

Fixtures are independently authored Verter-local inputs. Official-core cases may
measure harness scalability but third-party benchmarks and excluded repositories
cannot be correctness or performance acceptance corpora.

## Machine and concurrency policy

Runs use the A6 locked machine protocol and bounded process/thread settings. Command
records include `CARGO_BUILD_JOBS=4` or a lower explicit cap for any Cargo command and
bounded Node worker counts. No bare workspace Cargo build/test and no broad repository
gate is part of these cells.

BV1 and BS1 may overlap only when separate writable code, fixture roots, manifests,
golden output roots, package stores, target directories, ports, and explicit
heavy-machine leases are proven disjoint. Shared `Cargo.lock`, root package locks,
core compiler files, or one performance lease force serialization.
