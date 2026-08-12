# Framework conformance performance impact and pre-candidate lock

This proposal records required performance cells but deliberately does not invent
numeric thresholds without a BF1 baseline. BF1 must freeze exact fixtures, runner,
machine class, samples, statistics, absolute and relative thresholds, memory ceilings,
work counters, correctness oracle, and lease policy before BF2 implementation begins.
Thresholds cannot be changed after observing a successor candidate.

## Required new cells

| cell | owner/candidate boundary | workload and required counters |
|---|---|---|
| `BF2_VUE_ORACLE_MANIFEST_GENERATE` | BF2 harness | complete RC.3 manifest/golden shard; enumerated/imported/blocked counts, compiler calls, peak RSS, wall time, zero network |
| `BF2_SVELTE_ORACLE_MANIFEST_GENERATE` | BF2 harness | complete 5.56.8 manifest/golden shard; same counters |
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
