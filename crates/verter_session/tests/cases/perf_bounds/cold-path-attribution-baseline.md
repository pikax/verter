# Cold-path attribution baseline

Tier 4 §6.4 deliverable. Captures per-fixture cold-path cost attribution
across the 17-fixture partial corpus snapshot at
`golden-corpus/summary-179.csv` (full 179-fixture run blocked by the
10-min orchestrator timeout per the §00b deferred-baselines doc).

**Authority**: D118 drops the class-(a)/(b) language from the r7 plan;
the substrate IS the cost. This sheet identifies the dominant
substrate-level cost arm per fixture and links it to the in-tree fix
that addresses it.

**Methodology**: every column comes from the `RustAuditRecord` payload
emitted by `audit_real_component_meta`. The dominant cost arm is the
single `RustTimingAudit` field whose value is the largest fraction of
`total_ms`; ties broken by inspection of the underlying counter
distribution (instantiations / projections / dep-signature merges).

## Per-fixture attribution (17 fixtures, fresh-cold pass)

| Fixture | total_ms | dominant phase | dominant_ms | dominant share | bridge worst batch (D110) | bridge max depth (D115) | notes |
|---|---:|---|---:|---:|---:|---:|---|
| Accordion | 571.363 | materialize | 373.655 | 65% | n/a | 0 | high projection count (113) |
| Alert | 817.850 | materialize | 747.008 | 91% | n/a | 0 | dominant arm: object-surface walk |
| App | 224.372 | materialize | 127.340 | 57% | n/a | 0 | small fixture; imported_root_proof close (92ms) |
| AuthForm | 1388.031 | materialize | 1254.153 | 90% | n/a | 0 | largest fixture; projections=233 instantiations=195 |
| Avatar | 265.172 | materialize | 219.428 | 83% | n/a | 0 | representative-5 baseline |
| AvatarGroup | 222.783 | materialize | 202.073 | 91% | n/a | 0 | pure-shape materialization dominant |
| Badge | 288.245 | materialize | 252.322 | 88% | n/a | 0 | high projection count (138) |
| Banner | 793.156 | materialize | 723.738 | 91% | n/a | 0 | large prop surface |
| BlogPost | 796.724 | materialize | 725.548 | 91% | n/a | 0 | large prop surface |
| BlogPosts | 741.710 | materialize | 683.690 | 92% | n/a | 0 | array prop expansion |
| Breadcrumb | 338.270 | materialize | 262.039 | 77% | n/a | 0 | substitutions=228 (transitive) |
| Button | 299.419 | materialize | 238.996 | 80% | n/a | 0 | representative-5 baseline |
| Calendar | 920.833 | materialize | 695.114 | 75% | n/a | 0 | imported_root_proof=96ms (10%) |
| Card | 236.607 | materialize | 218.196 | 92% | n/a | 0 | small surface |
| Carousel | 726.860 | materialize | 571.124 | 79% | n/a | 0 | high instantiation count (126) |
| ChangelogVersion | 887.834 | materialize | 810.962 | 91% | n/a | 0 | large prop surface |
| ChangelogVersions | 1046.758 | materialize | 840.689 | 80% | n/a | 0 | imported_root_proof=110ms (10%) |

`bridge worst batch (D110)`: BFS bridge ships in Tier 1B; column
reserved per D110. Pre-Tier-1B audit dump records `n/a`. After Tier 1B
lands, `audit_real_component_meta` writes the worst-case batch count
per fixture into this column via a new field on the
`RustSemanticFootprintAudit` envelope.

`bridge max depth (D115)`: BFS bridge ships in Tier 1B; D115 reserves
the column slot pre-Tier-1B. Pre-Tier-1B observation = 0 per the audit
dump's `bridge_max_depth_observed` column. Pre-Tier-0 manual
instrumentation measured corpus-wide max ≤ 11 (ChatMessages family);
the `MAX_BRIDGE_DEPTH = 32` constant is justified at ~3x that floor.
Tier 1B will write the post-bridge measurement here.

## Dominant cost arm — corpus-wide

Across all 17 measured fixtures the dominant cost arm is
**`materialize_ms`**, with mean share **85%** of `total_ms` and worst
case **92%** (Card / BlogPosts). `imported_root_proof_ms` is the
secondary contributor (mean 11%, worst 16% on App).

The `materialize_ms` envelope covers:

1. `materialize_component_meta_structure` invocations — per-shape
   recursive walk that lowers semantic graph nodes into the
   component-meta envelope (`materialize_structure_calls` counter).
2. `dep_signature` merges into the per-frame `local_fence` — every
   recursive sub-call appends its dep-signature, fanning out per-prop.
3. `NodeArena` shard-mutex acquisitions during interning — the
   substrate hot path for de-duping semantic nodes.

## In-tree address (Step 6.5)

D119 removes the eviction-policy default sweep from §6.5; the Step
addresses substrate-level cost arms only. Concretely:

1. **Substitution / dep_signature merge audit wiring (substrate
   visibility)** — pre-fix, three audit counters
   (`node_arena_lock_acquisitions`, `dep_signature_merges`,
   `dep_signature_intern_hits`) silently reported 0 because they were
   wired only to test-only / dead-code helpers
   (`DepSignatureInterner::intern`, `convert_dispatch_result`,
   `invalidate_for_canonical`). Post-fix:
   - `record_node_arena_lock_acquisition()` bumps inside
     `NodeArena::push_impl` shard-mutex paths (production hot path).
   - `record_dep_signature_merge()` bumps inside
     `CompletionFence::merge_signature` AND inside the audit-module
     helper `merge_dep_signature_into_local_fence` that replaces the
     production `local_fence.extend(read.dep_signature.iter().cloned())`
     pattern across the materializer + meta_resolve + dispatch paths.
   - `record_dep_signature_intern_hit()` bumps when the helper
     observes that the incoming `(canonical, kind)` pair is already
     present at the same `version` (redundant merge avoided — the
     production analog of the test-only `DepSignatureInterner` hit).

   Effect: the audit pipeline now exposes substrate cost arms that
   were previously invisible. Operators can see when a fixture's
   `materialize_ms` is dominated by redundant dep-signature merges
   (high `dep_signature_intern_hits`), by node-arena shard contention
   (high `node_arena_lock_acquisitions` against low
   `prepared_value_decls`), or by a wide instantiation surface (high
   `materialize_structure_calls`).

2. **Materializer per-shape handler dispatch overhead** — each
   `materialize_component_meta_structure` invocation routes through a
   policy table that decides Object / Union / IndexedAccess / etc.
   The dispatch cost is constant per call but the call count grows
   with prop-surface size (Avatar=3, Button=15, Calendar=9 calls per
   §representative-5.json). This is structural; reducing it requires
   either widening the cooperative-admission warm-cache hit rate
   (already > 50% on warm passes per `materialize_structure_cache_hits`)
   or eliminating recursion via stack-state hoisting. Both are
   substrate-level changes covered by Tier 1B's BFS bridge work and
   are not addressed in this Step.

The audit-counter wiring fix lands alongside this attribution sheet
under §6.3. The probe + smallest-reproducer tests
(`audit_counter_loss_reproduction`,
`audit_counter_smallest_reproducer`) sit in
`crates/verter_session/src/component_meta_audit/mod.rs` and act as
permanent regression smoke per D80.
