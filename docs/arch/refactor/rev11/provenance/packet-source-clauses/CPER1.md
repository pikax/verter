# Exact operative source-clause attachment — CPER1

Schema: 1. Node: `CPER1`. Clause count: 19. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L875-574040412B24

- Kind: `context`; source: `compiler-proposal.md:875-875`; target: `node:CPER1`; text SHA-256: `574040412b2438b134a34e657fd2d4e332f5d0e397397caf61bdcf01093214a9`.

~~~~markdown
## `CPER1.md` — Compiler work ledger and lifetime attribution
~~~~

### SRC-COMP-L877-B28AAC0B8060

- Kind: `context`; source: `compiler-proposal.md:877-877`; target: `node:CPER1`; text SHA-256: `b28aac0b8060fbf2f826dbe432bd760ac3cac768907066895a6a7e08c26e2afd`.

~~~~markdown
**Intent:** make compiler work, memory, and reuse mechanically observable with negligible disabled overhead.
~~~~

### SRC-COMP-L879-3B9425DB7788

- Kind: `context`; source: `compiler-proposal.md:879-879`; target: `node:CPER1`; text SHA-256: `3b9425db77881b62444bbf7ceceb13a49b827c16c38996e9b17d0019b9f19fed`.

~~~~markdown
**Problem:** time measurements alone cannot catch extra traversals, reparses, allocations, or unrequested semantic/style work.
~~~~

### SRC-COMP-L881-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:881-881`; target: `node:CPER1`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L883-DD4BC4803A40

- Kind: `context`; source: `compiler-proposal.md:883-883`; target: `node:CPER1`; text SHA-256: `dd4bc4803a4035fd012625477f05720e287f896316031015f4dd604a60652c88`.

~~~~markdown
Implement a versioned `CompileWorkLedger` covering at least:
~~~~

### SRC-COMP-L885-5CA372367AD2

- Kind: `context`; source: `compiler-proposal.md:885-893`; target: `node:CPER1`; text SHA-256: `5ca372367ad2bfa8696286e8d04c67ecd023e0fb535a25c0764457c54598bcc8`.

~~~~markdown
```text
parse.full_source_scans
parse.region_scans[grammar]
parse.bytes[grammar]
parse.expression_attempts
parse.authoritative_expression_parses
parse.downstream_reparses
parse.raw_source_copy_bytes
parse.semantic_normalization_bytes
~~~~

### SRC-COMP-L895-612D858E47A9

- Kind: `requirement`; source: `compiler-proposal.md:895-901`; target: `node:CPER1`; text SHA-256: `612d858e47a9e1350e24db1d8271e32c30186e495d0a4f48072c2570d2e083fe`.

~~~~markdown
semantic.fact_families_demanded
semantic.facts_produced
semantic.fact_reads
semantic.binding_lookups
semantic.dependency_sets
semantic.dependency_edges
semantic.provenance_entries
~~~~

### SRC-COMP-L903-F56CC0F8C6AF

- Kind: `context`; source: `compiler-proposal.md:903-908`; target: `node:CPER1`; text SHA-256: `f56cc0f8c6af7ec2bcaaf071da2d7951ff0cbad23ec5a48a747a0fe46dadc7b8`.

~~~~markdown
structure.nodes_materialized
structure.regions
structure.topology_nodes
structure.source_sized_visits
structure.regional_visits
structure.graph_visits
~~~~

### SRC-COMP-L910-849B4BA66528

- Kind: `context`; source: `compiler-proposal.md:910-918`; target: `node:CPER1`; text SHA-256: `849b4ba66528a2a40d2d5af168fb284c08bce2afbf77dd41f87d9233a6a164a9`.

~~~~markdown
style.blocks
style.selector_plans
style.index_builds
style.candidate_nodes
style.predicate_tests
style.combinator_hops
style.match_yes_maybe_no
style.pruned_rules
style.witnesses_materialized
~~~~

### SRC-COMP-L920-0DFA09081CD6

- Kind: `context`; source: `compiler-proposal.md:920-923`; target: `node:CPER1`; text SHA-256: `0dfa09081cd615d3104138d5fe6871e7551143a1959c9793f1ac4fdc8a51e9e5`.

~~~~markdown
planning.target_entries
planning.effect_nodes
planning.effect_edges
planning.multi_target_shared_prerequisites
~~~~

### SRC-COMP-L925-6B84D73803AE

- Kind: `context`; source: `compiler-proposal.md:925-930`; target: `node:CPER1`; text SHA-256: `6b84d73803ae6cff0b706b7fcc8bea6648e72613870587e7d28aaf17850f4cf2`.

~~~~markdown
emission.segments
emission.source_slice_bytes
emission.generated_bytes
emission.copy_bytes
emission.allocations
emission.map_segments
~~~~

### SRC-COMP-L932-5FAB78C6E566

- Kind: `context`; source: `compiler-proposal.md:932-935`; target: `node:CPER1`; text SHA-256: `5fab78c6e566c4eaef6fc08a4ea0d9265b75cd15c2a98c7b0c7b9bdc57f7eab4`.

~~~~markdown
reuse.candidates
reuse.validated
reuse.rejected_by_basis
reuse.recomputed
~~~~

### SRC-COMP-L937-29D0B743873A

- Kind: `context`; source: `compiler-proposal.md:937-942`; target: `node:CPER1`; text SHA-256: `29d0b743873a1f9812df5b6009c6271e90bc9f3da659b14013d328ab81aa7702`.

~~~~markdown
memory.allocated_by_lifetime
memory.peak_by_lifetime
memory.retained_by_product
concurrency.tasks_spawned
concurrency.cancellation_waste
```
~~~~

### SRC-COMP-L944-2DDF91B0A790

- Kind: `context`; source: `compiler-proposal.md:944-944`; target: `node:CPER1`; text SHA-256: `2ddf91b0a790d8023643bda2318b3cb6d8fb6f990fdc2c75aabf07d847282f34`.

~~~~markdown
**Suggested predecessors:** `CPER0`, `CMP0`.
~~~~

### SRC-COMP-L946-D500CCA55222

- Kind: `context`; source: `compiler-proposal.md:946-946`; target: `node:CPER1`; text SHA-256: `d500cca55222b4a4ba5cb015e34a867bb128989370550e5e32119b1465a19d3c`.

~~~~markdown
**Suggested subblocks:** instrumentation schema, leaf counters, memory/lifetime hooks, deterministic export, disabled-overhead benchmark, architecture gate integration.
~~~~

### SRC-COMP-L948-AA5DA2A2F4C7

- Kind: `acceptance`; source: `compiler-proposal.md:948-948`; target: `node:CPER1`; text SHA-256: `aa5da2a2f4c7bc8d993425fb2129552a6051404da1884a9dba1eec380c411696`.

~~~~markdown
**Acceptance:** counters are deterministic for equivalent single-thread work, attributable to named capabilities, stable-schema versioned, and cheap when disabled; strict valid compilation reports zero lossless-sidecar and downstream-reparse work.
~~~~

### SRC-COMP-L950-6FCE8FDECDD9

- Kind: `forbidden`; source: `compiler-proposal.md:950-950`; target: `node:CPER1`; text SHA-256: `6fce8fdecdd971ea290345561cf9c7ee0047f7d136b58c15e2f8ee830c1a4495`.

~~~~markdown
**Forbidden:** counters becoming semantic authority, string-heavy per-node tracing in production, timing-based correctness, or a metric without an owner and definition.
~~~~

### SRC-COMP-L952-E333D370B088

- Kind: `deletion`; source: `compiler-proposal.md:952-952`; target: `node:CPER1`; text SHA-256: `e333d370b08834abb36c519b500596df2aca9ca1b7073e427177e4ecb99822a0`.

~~~~markdown
**Deletion/abort:** remove superseded ad hoc compiler telemetry only after parity; abort counters whose disabled cost exceeds the prelocked budget.
~~~~

### SRC-COMP-L954-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:954-954`; target: `node:CPER1`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
