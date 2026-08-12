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
The remaining seven cells below (`BV1`/`BS1`/`B6`/`C4`-owned) are unaffected by this
freeze and stay deferred to their owning blocks' own landings, per this document's
original scope — no threshold is invented for them here.

## Required new cells

| cell | owner/candidate boundary | workload and required counters |
|---|---|---|
| `BF2_VUE_ORACLE_MANIFEST_GENERATE` | BF2 harness | **FROZEN** — see above. Official-case enumeration and classification (title-hash extraction, disposition assignment) over the pinned RC.3 source tree — makes ZERO calls to the Vue compiler and produces no golden output, only the `vue-official-cases.tsv` manifest; suite/row/disposition counts, peak RSS, wall time, zero network, byte-exact output digest oracle |
| `BF2_SVELTE_ORACLE_MANIFEST_GENERATE` | BF2 harness | **FROZEN** — see above. Same enumeration/classification scope over the pinned 5.56.8 source tree — ZERO Svelte compiler calls, no golden output, only the `svelte-official-cases.tsv` manifest; same counter classes |
| `BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE` (name provisional) | BF2 harness | **NOT FROZEN, deliberately open.** The wider workload of actually invoking the official Vue/Svelte compilers thousands of times against the enumerated cases to produce immutable golden output — a genuinely heavier, not-yet-measurable BF2 workload, since that harness does not exist yet. Freezing it now would violate the charter's own "no criterion selected after candidate measurement" rule. Deferred to whichever block first builds that harness (most likely BF2 itself, since BF2 is the harness owner); no threshold is invented here. This row exists so the gap stays visible rather than silently absorbed into the two enumeration cells above. |
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
