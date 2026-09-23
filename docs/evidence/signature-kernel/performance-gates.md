# Performance gates — Verter Semantic Signature Kernel

The §12 work-accounting and performance gates of
[`docs/arch/signature-kernel.md`](../../arch/signature-kernel.md), mapped to
the executable evidence that holds each one.

**This document records structure, not numbers.** Latency distributions,
throughput curves, allocation totals and retained-byte figures are
machine-bound: they belong to a run report against a pinned runner class, never
to the tracked tree (the repository-wide rule in `CLAUDE.md` →
Cross-Platform Portability). A gate below is either held by a guard that fails
on regression, or it is recorded as unheld. No gate is described as passing on
the strength of a number nobody can recompute from this checkout.

## Structural gates

| §12 path | Required property | Evidence |
|---|---|---|
| Complete direct Empty/One read | No result heap allocation or candidate-table access; common evidence protocol retained | `tests/allocator_canaries.rs` → `signature_kernel_warm_positional::warm_positional_read_does_not_allocate_or_lock`. A counting `#[global_allocator]` asserts **zero** allocations over repeated warm `One` and `Many` reads, and `SemanticReadView::shard_lock_acquires()` asserts the read took **no** intern-shard lock. Runs in its own test binary (allowlisted in `scripts/integration-test-layout-allowlist.json`) because the allocator is process-global. |
| Repeated complete signature/intersection query | No producer rebuild or new semantic records | `signature_kernel/substitution_tests.rs` → `repeated_warm_read_walks_no_descriptor_chain` (`descriptor_chain_walks() == 0` on the repeated read); `cache_runtime/node_tests.rs` → `lookup_dedups_cold_compute_under_same_view`; `project_global_cache_tests.rs` → `semantic_subqueries_dedup_across_request_boundaries`. |
| Shared body-obligation consumers | Reuse completed return/effect work under the same full demand | `lazy_decl_body_tests.rs` → `lazy_decl_body_singleflight_lowers_once`; the `ReduceUnion` / `ReduceIntersection` rows of `semantic_query/query_key_spec_table.txt` register both families as `Singleflight`, and the spec table is enumerated against the live key enum. Within one transaction, a proven inline flow-return member is reused rather than re-evaluated: `flow_return_coverage_tests.rs` → `a_generic_call_chain_reuses_each_completed_callee` asserts every added level of a generic call chain costs the same connected work (it doubled per level before: 20504 units at eleven levels, 306 now), `flow_return_tests.rs` → `a_reused_flow_member_replays_its_reads_into_the_live_scopes` holds that a reuse replays the member's facts, self-roots and canonical evidence into the demanding build, and `a_reused_callee_still_invalidates_its_consumers_on_edit` holds that an edit to the reused callee still reaches the chain. |
| Augmented type lookup | No whole-program scan; no unrelated contributor invalidation | `FileArtifactStore::ensure_augmentation_index_populated` is an inverse index, not a scan; `g_misc3/module_augmentation_stitching.rs` → `session_overlay_augmenter_isolated_from_base_index` holds the no-unrelated-invalidation half across the base/session overlay boundary. |
| Composite construction | No eager overload Cartesian product; no quadratic prefix provenance copying | **Structural only.** `signature_kernel::discovery::union_signatures` is two phases: phase 1 takes signatures matched in every arm; phase 2 (restricted synthesis) fires only when phase 1 found nothing AND at most one arm has several signatures, so the product can never open. There is no dedicated regression test that would fail if a future edit removed the phase-2 precondition — **recorded as a coverage gap**, not as a satisfied gate. |
| Concurrent repeated demand | Coalesced computation without recursion deadlock or partial publication | `g_block/semantic_determinism_matrix.rs` → `signature_kernel_interned_identities_are_schedule_independent` (duplicate publishers and opposite intern orders at 1/2/4/8 workers converge on one logical identity) and `det_04_worker_counts_1_2_4_8`; `signature_kernel/lifetime_tests.rs` → `concurrent_replace_epoch_publishes_in_order`. |
| Editor edit/revert soak | Live memory plateaus after retired views are released and the retirement policy runs | **Partially held.** The retirement *mechanism* is guarded: `lifetime_tests.rs` → `live_readers_are_roots_until_drop`, `live_reader_count_includes_pinned_retired_epoch`, `retained_results_outlive_epoch_replacement_until_drained`; `file_artifact_store_tests.rs` → `unreachable_retired_version_is_reclaimed_once_no_root_sees_it`, `captured_root_still_reaches_a_retired_augmenter_set`; and the process-wide byte ceiling is held by `semantic_retention_account_tests.rs` (`many_individually_legal_entries_stop_at_the_aggregate_ceiling`, `a_charge_releases_exactly_once_across_every_ending`, `a_retained_parse_snapshot_charges_its_pin_once_per_snapshot`). A sustained editor edit/revert **soak** under a registered workload is **not** run in-tree — recorded as a coverage gap. |

## Additional §12 gates

| Gate | Evidence |
|---|---|
| A repeated ready result read performs no historical descriptor-chain walk | `repeated_warm_read_walks_no_descriptor_chain` (the direct counter assertion). |
| Simple binary intersection input takes no recipe allocation | Held by construction: `SemanticQueryKey::reduce_intersection_operands` builds an `IntersectionInputRef` over the operand slice; `build_reduce_intersection` reads `input.as_steps()` without minting a recipe. No counter assertion. |
| A resident valid previously built semantic union view is not sorted again | Held by the memo: a warm `ReduceUnion` hit returns the interned node; `intern_ordered_union` is not re-entered. No counter assertion. |
| Rendering preference changes rebuild no semantic query | Held by the key composition: rendering/presentation is not a `SemanticQueryKey` dimension (R21 scoping — see `/type-cache-architecture`). |
| Diagnostic materialization at a second call site uses that site's location | Not separately guarded here. |

## Regression policy

Adopted from §12, unchanged:

* A **5% regression investigation gate** applies to matched pre-existing
  end-to-end workloads, when the difference exceeds the benchmark's measured
  noise. It is an investigation trigger, not a promised speedup.
* A regression is **not** waived because a microbenchmark improved. Record the
  cause and obtain an explicit performance decision.
* Newly supported work is reported **separately**, with its absolute cost and
  amortization — never folded into a matched-workload comparison.
* A typed gap is not a fast success. Do not compare a partial Verter query to a
  complete TypeScript project check and call the ratio a compiler speedup.
* Intentional `VerterStableV1` semantic differences are reported separately
  from exact-agreement workloads, through
  [`semantic-difference-ledger.md`](semantic-difference-ledger.md).
* Determinism is never bought by serialising node allocation (§12, explicit).

## Measured distribution

`crates/verter_session/examples/signature_kernel_bench.rs` drives the §12
workloads through the public host API, and
`scripts/benchmark/signature-kernel-perf.mjs` runs it against the pre-kernel
baseline — the first parent of the landing "resolve effective tsconfig
semantic options into the type environment" — under the lock's statistics and
idle-machine policy. Raw logs and run reports stay out of the tree; a
distribution is published here only from a session that reports **lock
evidence** on the locked runner class.

**Session of 2026-09-23** on `apple-silicon-laptop-8core-24gib` (Apple M3,
8 CPUs, 24 GiB, AC power, low-power mode off): 4 invocations per arm, ABBA; a
180 s settle after the builds and a 120 s cool-down before each control and
invocation; control drift 0.5% (gate 3%); load average 1.22 at start, no
foreign build process at any point. Corpus: 24 modules, chain depth 8. The
`Call` witness was excluded because only the candidate completes it (newly
supported work, reported separately below), so the census matched and every
ratio compares the same completed work.

| Workload | Baseline p50 | Candidate p50 | Ratio (95% CI) | Gate | Verdict |
|---|---|---|---|---|---|
| cold load | 205.8 ms | 112.4 ms | 0.546 (0.544–0.547) | ±5.0% | improvement |
| restart | 208.1 ms | 114.9 ms | 0.552 (0.551–0.553) | ±5.0% | improvement |
| local edit | 58.2 ms | 3.93 ms | 0.068 (0.067–0.068) | ±5.0% | improvement |
| declaration edit | 5.49 ms | 5.62 ms | 1.024 (1.019–1.029) | ±5.0% | within noise |
| augmentation edit | 3.18 ms | 3.34 ms | 1.053 (1.042–1.061) | ±7.8% | within noise |
| warm query | 0.011 ms | 0.011 ms | 1.015 (1.004–1.029) | ±60.7% | within noise |

The gate is max(5%, 2 × the baseline's between-invocation noise). Throughput
at 1/2/4/8 workers is within 3% of the baseline at every count. Allocation per
pass falls 42% on cold load and 94% on local edit. The edit/revert soak
plateaus in both arms (−0.8% / −0.7% from the second to the last quarter).

Two matched workloads are slower without crossing the gate:
**augmentation edit** by 5.3% (its CI excludes zero, and it sits above the 5%
floor but inside the measured-noise threshold) and **declaration edit** by
2.4%. Neither is a gate failure under the policy above; both are recorded here
rather than left inside the verdict column.

**Newly supported work.** The `Call` witness (a call over an intersection
whose signatures differ only in their result) completes on the candidate and
is degraded on the baseline. Its cost is not folded into any ratio above.

## Unmeasured surfaces

Recorded plainly so no reader mistakes absence for a pass:

* Cancellation latency: the audited entry takes the caller’s cancellation
  token, and determinism row DET-05 drives its correctness (a cancelled
  request answers `Cancelled` or a complete answer, and its retry equals a
  cold host). The baseline has no cancellable entry, so no matched workload
  exists; the candidate-only probe
  (`crates/verter_session/examples/signature_kernel_cancel_probe.rs`), which
  the runner runs inside the same session, records the stop and restart
  distributions. No locked session has run it yet.
* Completion and diagnostic agreement rates against the pinned official
  compiler on a full project check.
* A LOCKED cell: `performance-gates.toml` has no signature-kernel cell.
  Adding one is an extension under that file's own rules — a new lock-record
  digest and the independent performance review class — not a local edit.

## Known limits

* **Connected-query depth.** Each level of a generic call chain nests two
  connected queries, and the dispatch's depth guard (24) is a stack-safety
  bound, so a chain longer than eleven levels ends in a typed budget refusal
  rather than a stack overflow. The work per level is constant (see *Shared
  body-obligation consumers*); raising the bound is a stack-budget decision,
  not a performance one.
