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
| Shared body-obligation consumers | Reuse completed return/effect work under the same full demand | `lazy_decl_body_tests.rs` → `lazy_decl_body_singleflight_lowers_once`; the `ReduceUnion` / `ReduceIntersection` rows of `semantic_query/query_key_spec_table.txt` register both families as `Singleflight`, and the spec table is enumerated against the live key enum. Within one transaction, a proven inline flow-return member is reused rather than re-evaluated: `flow_return_coverage_tests.rs` → `a_generic_call_chain_reuses_each_completed_callee` asserts every added level of a generic call chain costs the same connected work, at 9 / 10 / 11 levels and at 32 / 64 / 128 (it doubled per level before: 20504 units at eleven levels, 306 now), `flow_return_tests.rs` → `a_reused_flow_member_replays_its_reads_into_the_live_scopes` holds that a reuse replays the member's facts, self-roots and canonical evidence into the demanding build, and `a_reused_callee_still_invalidates_its_consumers_on_edit` holds that an edit to the reused callee still reaches the chain. |
| Augmented type lookup | No whole-program scan; no unrelated contributor invalidation | `FileArtifactStore::ensure_augmentation_index_populated` is an inverse index, not a scan; `g_misc3/module_augmentation_stitching.rs` → `session_overlay_augmenter_isolated_from_base_index` holds the no-unrelated-invalidation half across the base/session overlay boundary. |
| Composite construction | No eager overload Cartesian product; no quadratic prefix provenance copying | **Held.** `signature_kernel::discovery::union_signatures` is two phases: phase 1 takes signatures matched in every arm; phase 2 (restricted synthesis) fires only when phase 1 found nothing AND at most one arm has several signatures, so the product can never open. Held by `signature_kernel::discovery_tests::a_union_of_overloaded_arms_without_a_common_signature_synthesizes_nothing`: two overloaded arms with no common signature synthesize nothing (7.0.2: TS2349, not callable), while one overloaded arm still reaches the restricted synthesis; removing the phase-2 precondition fails it. No prefix of the fold copies provenance: `union_synthesis_interns_one_provenance_per_candidate_whatever_the_arm_count` counts one constituent sequence and one provenance per synthesized candidate for three arms and for six, and fails if the fold interns a sequence per prefix. |
| Concurrent repeated demand | Coalesced computation without recursion deadlock or partial publication | `g_block/semantic_determinism_matrix.rs` → `signature_kernel_interned_identities_are_schedule_independent` (duplicate publishers and opposite intern orders at 1/2/4/8 workers converge on one logical identity) and `det_04_worker_counts_1_2_4_8`; `signature_kernel/lifetime_tests.rs` → `concurrent_replace_epoch_publishes_in_order`; `project_semantic_dispatch/signature_epoch_tests.rs` → `a_joiner_never_takes_a_retired_epoch_value_from_the_build_it_joined`. |
| Editor edit/revert soak | Live memory plateaus after retired views are released and the retirement policy runs | **Partially held.** The retirement *mechanism* is guarded: `lifetime_tests.rs` → `live_readers_are_roots_until_drop`, `live_reader_count_includes_pinned_retired_epoch`, `retained_results_outlive_epoch_replacement_until_drained`; `file_artifact_store_tests.rs` → `unreachable_retired_version_is_reclaimed_once_no_root_sees_it`, `captured_root_still_reaches_a_retired_augmenter_set`; and the process-wide byte ceiling is held by `semantic_retention_account_tests.rs` (`many_individually_legal_entries_stop_at_the_aggregate_ceiling`, `a_charge_releases_exactly_once_across_every_ending`, `a_retained_parse_snapshot_charges_its_pin_once_per_snapshot`). The kernel tables' own retention is bounded under sustained unique edits: `project_semantic_dispatch/signature_epoch_tests.rs` → `sustained_unique_edits_keep_the_kernel_store_bounded_and_the_answers_fresh` (the edit path replaces the epoch past `EPOCH_RECORD_CAP`, and every replacement's answers equal a fresh host's), and a warm value of a retired epoch is a miss, never an incomplete answer: `a_warm_signature_set_of_a_retired_epoch_is_a_miss_that_recomputes_the_answer`, `a_replacement_between_the_memo_read_and_the_pin_is_a_miss_not_an_incomplete_answer`, `a_build_that_straddles_a_replacement_cannot_poison_later_reads`. The sustained edit/revert soak under the registered workload is the benchmark harness's `soak` (hundreds of edit/revert rounds on one host, live heap after each), whose plateau verdict the runner reports per arm in the locked session; the editor-process churn (open/edit/close through the language server) is the language-server retention work stacked on this branch. |

## Additional §12 gates

| Gate | Evidence |
|---|---|
| A repeated ready result read performs no historical descriptor-chain walk | `repeated_warm_read_walks_no_descriptor_chain` (the direct counter assertion). |
| Simple binary intersection input takes no recipe allocation | Held by construction: `SemanticQueryKey::reduce_intersection_operands` builds an `IntersectionInputRef` over the operand slice; `build_reduce_intersection` reads `input.as_steps()` without minting a recipe. No counter assertion. |
| A resident valid previously built semantic union view is not sorted again | Held by the memo: a warm `ReduceUnion` hit returns the interned node; `intern_ordered_union` is not re-entered. No counter assertion. |
| Rendering preference changes rebuild no semantic query | Held by the key composition: rendering/presentation is not a `SemanticQueryKey` dimension (R21 scoping — see `/type-cache-architecture`). |
| Diagnostic materialization at a second call site uses that site's location | Not separately guarded here. |

## Candidate gates

Correctness and stack-safety gates proposed for registration, each held by a
guard that fails on regression.

| Gate | Evidence |
|---|---|
| A finite linear call chain consumes connected work, not native stack or connected-query depth per level | The flow-return callee schedule (`project_semantic_dispatch/flow_return_schedule.rs`) evaluates the callee returns a frame's body will demand bottom-up from an explicit stack before the body runs, and records each as a reusable completed member exactly as the inline path does, so the body reuses it instead of recursing. `flow_return_coverage_tests.rs` → `schedule::a_128_level_generic_chain_needs_no_more_query_depth_than_a_short_one` (the 128-level chain answers like the three-level one under the smallest depth cap the three-level one needs), `schedule::a_128_level_generic_chain_runs_on_the_short_chains_native_stack` (the 128-level chain on a 512 KiB thread), `schedule::chains_through_other_call_shapes_consume_no_depth_per_level` (non-generic, `const`-arrow and local-arrow chains), `schedule::a_chain_across_modules_needs_no_more_query_depth_than_a_short_one` (generic and non-generic chains of imported callees: 32 modules under the 4-module chain's depth cap and on its stack), `schedule::a_new_instantiation_of_a_warm_chain_runs_on_the_short_chains_native_stack` (a new call site instantiating a warm 128-level chain, on a 512 KiB thread), `schedule::a_200_level_nested_argument_chain_answers_on_the_default_stack` and `schedule::a_nested_argument_chain_costs_the_same_work_per_level` (every edge a call inside a generic call's argument: 200 levels on the default test stack under the three-level chain's depth cap, the same work per level at 16, 64 and 200 levels), `schedule::a_200_level_type_position_chain_answers_on_the_default_stack` and `schedule::a_type_position_chain_dispatches_the_same_queries_per_level` (every edge a `ReturnType<typeof f>` type position: 200 levels on the default test stack under the three-level chain's depth cap, one `TypeOf` and one `LowerLocator` per level), `schedule::a_new_instantiation_of_a_warm_200_level_local_arrow_chain_answers_on_the_default_stack` and `schedule::a_new_instantiation_of_a_warm_local_arrow_chain_costs_the_same_work_per_level` (a new call site instantiating a warm 200-level chain whose levels call through local arrow functions, on the default test stack, the same work per level), `schedule::a_deep_chain_over_a_reduced_budget_ends_on_the_work_rail` (a reduced work budget ends the 128-level chain on `PROJECTION_WORK_LIMIT`, never the depth rail), and `schedule::recursive_components_evaluate_exactly_as_the_recursive_path` (self-recursive and mutually recursive components answer from the same connected work as with the schedule off: a cycle is never evaluated out of order). |
| Native recursion the schedule does not predict ends typed, never in a stack overflow | An inline flow evaluation that would open more nested frames than the connected demand's depth cap (24) is refused with `CONNECTED_QUERY_DEPTH_LIMIT` — the same typed incompleteness the connected-query depth guard ends query nesting with. `flow_return_coverage_tests.rs` → `schedule::an_unpredicted_deep_chain_ends_in_the_typed_depth_refusal` (a new instantiation of a warm local-arrow chain evaluated with the schedule off, one native evaluation per level: 16 levels answer; at 256 levels, which overflows the 8 MiB worker stack without the bound in an unoptimized build, it is refused partial and never admitted). |

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
evidence**.

Lock evidence requires, beyond an idle machine and a control drift inside the
gate:

* **The locked runner class.** The machine, both trees' Rust toolchains, the
  Node runtime and the power state must be the `[runner]` of
  `performance-gates.toml`, read from that file at run time (never restated in
  the runner): OS, CPU, logical CPUs, memory, toolchain, runtime, AC power and
  low-power mode off. Any mismatch is a refusal, whatever the control says.
* **Matched work.** The harness records, untimed, every witness's outcome —
  completion, the typed degradation or refusal, and the answered type rendered
  structurally (union members order-insensitive) — in the original corpus and
  in each edited state an edit workload reaches. A workload gets a ratio,
  interval and verdict only when both arms have the same outcome on every
  witness it queries, and each arm's invocations agree with each other;
  otherwise it is reported as not comparable. Aggregate completion counts are
  no longer the matching criterion.

**The receipt is pending.** No locked session has run the current harness and
runner on the current head. The session of 2026-09-23 (Apple M3) ran an
earlier harness with a pooled bootstrap, unbatched warm queries and the
earlier verdict wording; its ratios are historical evidence only and are not
this node's performance receipt. The receipt is the next locked session on the
current head.

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

* **Connected-query depth and native nesting are backstops.** A call chain
  the callee schedule discovers consumes connected work and no native stack
  or connected-query depth per level (see *Candidate gates*): a same-file
  direct call, one made through a local arrow function, a callee imported
  from another module, a call inside the argument of a call the executor
  resolves (`return id(c(x))` with a generic `id`), a function read through
  a `typeof` in a type position (`let r!: ReturnType<typeof f>`), and the
  instantiations each of them demands — read from the uninstantiated
  frame's recorded call resolution when it evaluated in the same demand,
  and from the two signatures the call executor reads when it was already
  warm and the call — by the frame or by a local function value it
  composes — forwards the frame's own binders. The 128-level generic chain
  needs the depth (3) and stack of a three-level one, a 32-module chain the
  depth (5) of a 4-module one, and the 200-level nested-argument,
  type-position and warm local-arrow chains answer on the default test
  stack. Two typed bounds end whatever the schedule leaves to the
  recursive path, each with the depth rail's `CONNECTED_QUERY_DEPTH_LIMIT`
  incompleteness rather than a stack overflow: the connected-query depth
  guard (24 nested query boundaries —
  `projection_stack_safety_tests.rs` →
  `connected_query_depth_limit_allows_boundary_and_trips_at_plus_one` and
  `connected_query_depth_limit_is_distinct_diagnostic_and_is_not_cached`),
  and the native nesting bound (24 nested inline flow evaluations —
  `flow_return_coverage_tests.rs` →
  `schedule::an_unpredicted_deep_chain_ends_in_the_typed_depth_refusal`).
  What the schedule still leaves recursive:
  * an argument of a call the executor does not resolve (a non-generic,
    non-overloaded or annotated callee), and a call in a callee position:
    the body evaluates it where it sits (a non-generic `id` evaluates no
    argument and nests nothing);
  * a new instantiation of a warm chain whose call passes anything but a
    bare read of a parameter declared with the frame's binder: one nested
    inline evaluation per level, which the native nesting bound refuses
    from 24 levels;
  * a callee whose evaluation is not reusable (a degraded or unproven
    return) is left to its demand; the first unreusable callee of a branch
    costs one extra evaluation before the schedule leaves that branch;
  * a `this.m()` method chain nests nothing, because the flow lane answers
    its first hop with the typed `UnresolvedValue` degradation (tsc:
    `{ v: string | number; tag: "c"; }`) — a separate gap.

  A generic chain across modules is flat in depth but QUADRATIC in
  connected work: each module's binder is its own, so every level
  instantiates the chain beneath it afresh (4 / 32 / 64 / 128 modules cost
  143 / 6989 / 27293 / 107837 units). The work budget, not depth, ends it:
  200 modules cost 262,097 of the 262,144 units and answer, and from 201
  modules the work rail refuses the chain, typed. A same-file chain
  shares one binder per name and stays linear.

  A chain through type positions is flat in depth and in queries (one
  `TypeOf` and one `LowerLocator` per level) but QUADRATIC in connected
  work. Each level's return is published as the `ReturnType<…>` carrier
  over the level below's function type, whose return is that level's
  carrier in turn, and each level's `LowerLocator` view projection walks
  the whole nested carrier chain beneath it (16 / 64 / 200 levels: 52 /
  196 / 604 units for the next level, 60,498 in all at 200). The work
  budget ends it: 417 levels cost 261,874 units and answer, and from 418
  levels the chain ends partial and is never admitted. The checker
  resolves `ReturnType` of a closed function type where it is written, so
  its chain stays flat; the growth is the carrier representation's view
  projection, not the schedule.
