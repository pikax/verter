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
| Shared body-obligation consumers | Reuse completed return/effect work under the same full demand | `lazy_decl_body_tests.rs` → `lazy_decl_body_singleflight_lowers_once`; the `ReduceUnion` / `ReduceIntersection` rows of `semantic_query/query_key_spec_table.txt` register both families as `Singleflight`, and the spec table is enumerated against the live key enum. |
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

## Unmeasured surfaces

Recorded plainly so no reader mistakes absence for a pass. None of the
following has a published distribution in this repository:

* p50/p95/p99 latency for cold project load, warm queries, local edits,
  declaration/augmentation edits, and cancellation/restart.
* Throughput at 1/2/4/8 workers (worker-count **determinism** is driven by
  `det_04_worker_counts_1_2_4_8`; throughput is not).
* Allocation and work profiles beyond the zero-allocation Empty/One canary and
  the fact-emission volume canaries in `tests/allocator_canaries.rs`.
* Sustained edit/revert memory plateau under a registered editor workload.

Publishing these requires a pinned runner class and retained calibration; the
existing locked runner definition is `performance-gates.toml` at the repository
root (`class = "apple-silicon-laptop-8core-24gib"`), whose cells do not cover
the signature-kernel families. Extending it is a lock-record change under that
file's own recalibration rules, not a local adjustment.
