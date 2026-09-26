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
| A finite linear call chain consumes connected work, not native stack or connected-query depth per level | The flow-return callee schedule (`project_semantic_dispatch/flow_return_schedule.rs`) evaluates the callee returns a frame's body will demand bottom-up from an explicit stack before the body runs, and records each as a reusable completed member exactly as the inline path does, so the body reuses it instead of recursing. `flow_return_coverage_tests.rs` → `schedule::a_128_level_generic_chain_needs_no_more_query_depth_than_a_short_one` (the 128-level chain answers like the three-level one under the smallest depth cap the three-level one needs), `schedule::a_128_level_generic_chain_runs_on_the_short_chains_native_stack` (the 128-level chain on a 512 KiB thread), `schedule::chains_through_other_call_shapes_consume_no_depth_per_level` (non-generic, `const`-arrow and local-arrow chains), `schedule::a_chain_across_modules_needs_no_more_query_depth_than_a_short_one` (generic and non-generic chains of imported callees: 32 modules under the 4-module chain's depth cap and on its stack), `schedule::a_new_instantiation_of_a_warm_chain_runs_on_the_short_chains_native_stack` (a new call site instantiating a warm 128-level chain, on a 512 KiB thread), `schedule::a_200_level_nested_argument_chain_answers_on_the_default_stack` and `schedule::a_nested_argument_chain_costs_the_same_work_per_level` (every edge a call inside a generic call's argument: 200 levels on the default test stack under the three-level chain's depth cap, the same work per level at 16, 64 and 200 levels), `schedule::a_200_level_nested_argument_chain_in_local_arrows_answers_on_the_default_stack` and `schedule::a_nested_argument_chain_in_local_arrows_costs_the_same_work_per_level` (the same edge inside a local arrow function, `aN<T>(x: T) { const f = (y: T) => id(a(N-1)(y)); return f(x); }`: each level's probe records its callee's uninstantiated and instantiated returns, and the instantiated one is read off the uninstantiated one once that is evaluated — a read the schedule counts as reusable as a member, since every later demand reads it the same way; 200 levels on the default test stack under the three-level chain's depth cap, 37 units of work per level at 16, 64 and 200 levels, and at most two flow evaluations open at any length. Counting that read unreusable left every level still being walked to its demand, each evaluated beneath the one above it: refused on the depth rail from 12 levels, and overflowing the default test stack before 200 in an unoptimized build), `schedule::a_200_level_type_position_chain_answers_on_the_default_stack`, `schedule::a_type_position_chain_costs_the_same_work_per_level` and `schedule::a_1000_level_type_position_chain_answers` (every edge a `ReturnType<typeof f>` type position: 200 levels on the default test stack under the three-level chain's depth cap, the same work per level at 16, 64 and 200 levels, and 1,000 levels answer), `schedule::a_new_instantiation_of_a_warm_200_level_local_arrow_chain_answers_on_the_default_stack` and `schedule::a_new_instantiation_of_a_warm_local_arrow_chain_costs_the_same_work_per_level` (a new call site instantiating a warm 200-level chain whose levels call through local arrow functions, on the default test stack, the same work per level), `schedule::a_warm_chain_passing_a_parameter_member_read_instantiates_stacklessly`, `…_a_local_member_read_…`, `…_a_literal_…` and `…_a_call_on_its_parameter_…` (a new call site instantiating a warm 200-level chain whose levels pass the next a member read, a literal or a call: on the default test stack, the same work per level at 16, 32 and 64 levels), `schedule::a_deep_chain_over_a_reduced_budget_ends_on_the_work_rail` (a reduced work budget ends the 128-level chain on `PROJECTION_WORK_LIMIT`, never the depth rail), and `schedule::recursive_components_evaluate_exactly_as_the_recursive_path` (self-recursive and mutually recursive components answer from the same connected work as with the schedule off: a cycle is never evaluated out of order). |
| Native recursion the schedule does not predict ends typed, never in a stack overflow | An inline flow evaluation that would open more nested frames than the connected demand's depth cap (24) is refused with `CONNECTED_QUERY_DEPTH_LIMIT` — the same typed incompleteness the connected-query depth guard ends query nesting with. `flow_return_coverage_tests.rs` → `schedule::an_unpredicted_deep_chain_ends_in_the_typed_depth_refusal` (a same-file chain evaluated with the schedule off, so every level nests its callee natively: 16 levels answer; at 256 levels, which overflows the 8 MiB worker stack without the bound in an unoptimized build, it is refused partial and never admitted). |
| An instantiated callee return is read off its uninstantiated return, so a generic chain across modules costs the same connected work per module | `ProjectSemanticDispatch::instantiated_from_uninstantiated` (`project_semantic_dispatch/flow_return.rs`) answers an instantiated `FlowReturn` key whose uninstantiated return is already answered (a validated warm candidate or a reusable completed member) with that return under the instantiation — the checker's rule, the return type of an instantiated signature is its target's return type under the mapper — instead of re-evaluating the body; nothing new is stored (the substitution goes through the store-owned substitution memo). `flow_return_coverage_tests.rs` → `schedule::a_generic_chain_across_modules_costs_the_same_work_per_module` (9 / 10 / 11 and 32 / 64 / 128 modules: every added module costs the same work, asserted structurally), `schedule::a_generic_chain_across_201_modules_answers_without_a_refusal` (the checker's `{ v: string \| number; tag: "c"; }`, clean and admitted, under the production budget), `schedule::an_edit_in_the_middle_of_a_module_chain_reaches_the_top` (an edit to module 100 of 201 reaches the witness), `schedule::a_new_instantiation_of_a_warm_local_arrow_chain_is_read_off_its_uninstantiated_return` (256 levels, on a 512 KiB stack), and `schedule::a_256_level_chain_returning_a_same_name_generic_answers` (a head returning `id: <T,>(z: T) => z` or `K: class<T> { own!: T; }`: the nested clause interns its own binders, 256 levels answer and a new instantiation answers on 512 KiB), `an_instantiated_return_keeps_a_nested_clause_of_the_same_name` (every nested clause keeps its own parameter; a function type written in the body with a same-name clause is evaluated under the instantiation) and `an_instantiation_swapping_same_file_binders_binds_them_at_once` (the frame's clause is bound simultaneously). The schedule's warm check validates a candidate as the warm read does: measured on the unoptimized test profile, it adds 7–20 µs per check only where a candidate exists (one validation of that candidate's recorded facts — 204 to 1,004 facts in single-file chains of 200 and 1,000 functions, up to 605 across 201 modules), 3.5 ms of a 2.6 s re-evaluation of the 201-module chain after an edit, 29 ms of 22 s for the 1,000-function file; with no candidate it costs the lookup alone. |

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

Commands, from the repository root:

```text
# full session (lock evidence on the locked runner class)
node scripts/benchmark/signature-kernel-perf.mjs
# quick session (pipeline smoke test: small corpus, 2 invocations, no control)
node scripts/benchmark/signature-kernel-perf.mjs --quick

# the harness alone, full and quick
cargo run --release -p verter_session --example signature_kernel_bench
cargo run --release -p verter_session --example signature_kernel_bench -- \
    --modules 4 --depth 4 --samples 6 --cold-samples 3 --soak 20
# the cancellation probe alone, full and quick
cargo run --release -p verter_session --example signature_kernel_cancel_probe
cargo run --release -p verter_session --example signature_kernel_cancel_probe -- \
    --modules 4 --depth 4 --cold-samples 3 --rounds 3
```

The harness prints one JSON document (`harness_version` 3). What each number
means:

* **`workloads`** — per workload (`cold_load`, `warm_query`, `local_edit`,
  `declaration_edit`, `augmentation_edit`, `restart`), latency samples in
  nanoseconds and the allocation count and bytes of one untimed accounting
  pass. `warm_query` samples are per query, each timed in a batch of full
  sweeps. The runner reports p50 / p95 / p99 per arm and, over matched
  work, the candidate / baseline ratio with its 95% hierarchical bootstrap
  interval and verdict.
* **Scalability.** Three sections, each point its own distribution. Caller
  threads are created once per point, outside timing; each sample is
  released by a start barrier and timed from the first caller's start to
  the last caller's finish, each read by the caller itself, so neither
  thread spawn and join nor a wake-up is inside a sample. One untimed
  warm-up sample precedes each point. The worker count of a point is the
  host's scheduler CPU workers (`SchedulerConfig::cpu_threads`); the host
  CPU pool and the declaration-lowering workers keep their default sizes.
  * **`concurrent_queries`** (concurrent query scalability) — one warm
    host with a fixed worker count (`--host-workers`, default 4), queried
    by 1, 2, 4 and 8 callers at once. Every caller runs the same work per
    sample, ten full sweeps over every witness from its own offset, so
    perfect scaling keeps the sample's wall time flat. Per point:
    `samples_ns` (wall), `qps` (all callers' queries over the wall) and
    `query_latency_ns` (each query's own latency, p50 / p95 / p99 / max
    over every caller and sample).
  * **`scheduler_scaling`** (internal scheduler scalability) — one caller;
    hosts with 1, 2, 4 and 8 workers. Each sample gets a fresh host loaded
    with the check corpus untimed; the sample is the cold first query of
    every witness, independent roots across every module. Per point:
    `samples_ns`, `median_ns`, `speedup_vs_1` (the one-worker median over
    this point's), and the CPU fields below. A speedup is work the host
    spreads over its scheduler by itself.
  * **`full_check`** (full-check throughput) — hosts with 1, 2, 4 and 8
    workers and as many callers, on a fresh host per sample: the two shared
    inputs are loaded untimed, then the check corpus's files are dealt out
    round-robin and each caller upserts its files and answers their
    witnesses, as a project checker drives one host with a checking thread
    per worker. Per point: `samples_ns`, `files_per_second` and the CPU
    fields.
  * CPU fields (`scheduler_scaling`, `full_check`): `cpu_ns`, the process
    CPU time (every thread, user plus system) across each sample;
    `cpu_utilisation`, `cpu / (wall × workers)` per sample; and
    `cpu_utilisation_total`, `Σ cpu / (Σ wall × workers)` over the point,
    which the runner reports. The callers' own CPU counts, so utilisation
    can exceed 1. `cpu_time_source` names the clock:
    `clock_gettime(CLOCK_PROCESS_CPUTIME_ID)` on macOS and Linux,
    `GetProcessTimes` on Windows (which advances in scheduler ticks of
    about 15.6 ms, so read the point's total there, not one sample), and
    null elsewhere.
  * The **check corpus** is the corpus's module shape at `--check-modules`
    modules (default four times `--modules`); its outcomes are the
    `check` state, and the scheduler and full-check points are matched
    only when both arms agree on every witness in it.

  The runner compares each point's wall time like a workload (ratio,
  interval, gate, verdict) and reports queries per second and query
  latency, speedup, files per second and CPU utilisation as the medians
  across invocations. The per-query latency percentiles are the median of
  each invocation's percentile.
* **`soak_live_bytes`** — live heap after each edit/revert round.
* **`census`**, **`census_by_witness`** and **`outcomes`** — what every
  witness answers, untimed (see *Matched work* below).

The cancellation probe prints its own document (`harness_version` 2):
`cold_request`, `cancel_stop` and `restart` samples in aggregate, and
`by_fraction`, one entry per injection point (10, 30, 50, 70 and 90% of
the invocation's median cold request, `cold_request_median_ns`), each with
its `delay_ns`, `landed_ns` (from the request's start to `cancel()`),
`cancel_stop` and `restart` samples and `completed_before_cancel`; a
request that completed before its cancellation landed gives neither a stop
nor a restart sample (its retry is a warm read). The
canceller thread is parked before the request starts and counts from the
request's own start instant, so where a cancellation lands does not depend
on thread creation. The runner reports p50 / p95 / p99 per point beside the
aggregate.

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
  distributions, in aggregate and per injection point. No locked session
  has run it yet.
* Completion and diagnostic agreement rates against the pinned official
  compiler on a full project check.
* A LOCKED cell: `performance-gates.toml` has no signature-kernel cell.
  Adding one is an extension under that file's own rules — a new lock-record
  digest and the independent performance review class — not a local edit.

## Known limits

* **Connected-query depth and native nesting are backstops.** A call chain
  the callee schedule reaches consumes connected work and no native stack
  or connected-query depth per level (see *Candidate gates*). Discovery
  predicts a same-file direct call, one made through a local arrow
  function, a callee imported from another module, a call inside the
  argument of a call the executor resolves (`return id(c(x))` with a
  generic `id`), a function read through a `typeof` in a type position
  (`let r!: ReturnType<typeof f>`), and the instantiations each of them
  demands — read from the uninstantiated frame's recorded call resolution
  when it evaluated in the same demand, and from the two signatures the
  call executor reads when it was already warm and the call — by the
  frame or by a local function value it composes — forwards the frame's
  own binders. Every other callee return an evaluation demands under an
  open schedule is found when it is demanded: beneath a scheduled
  evaluation the probe records it and refuses it (typed, inside the
  probe's private rails), and the entry is evaluated again once the
  recorded return is; outside one it is scheduled where it is made. So an
  instantiation read from an argument of any form (a member read, a
  literal, a call on a parameter), and a call nested in an argument inside
  a local arrow function, are evaluated from the explicit stack too. The
  128-level generic chain needs the depth (3) and stack of a three-level
  one, a 32-module chain the depth (5) of a 4-module one, the 200-level
  nested-argument (direct and through a local arrow function), type-position, warm local-arrow and warm argument-form
  chains answer on the default test stack, and the 1,000-level
  type-position chain answers. A scheduled evaluation is reusable only
  when its reads are recorded, which takes a live fact tracer, and every
  flow frame has one: an obligation frame is pushed only inside a flow,
  relation or call-resolution evaluation (`flow_frame_open`,
  `relate` frame open, `resolve_call_frame_open`), the root of each is a
  cold build through the dispatch choke point
  (`execute_via_cold_build_helper_with_publication_capture`), and that
  build installs the tracer around everything it computes. The schedule
  asserts it in debug builds, over every lib and integration test. Two typed bounds end whatever still
  recurses, each with the depth rail's `CONNECTED_QUERY_DEPTH_LIMIT`
  incompleteness rather than a stack overflow: the connected-query depth
  guard (24 nested query boundaries —
  `projection_stack_safety_tests.rs` →
  `connected_query_depth_limit_allows_boundary_and_trips_at_plus_one` and
  `connected_query_depth_limit_is_distinct_diagnostic_and_is_not_cached`),
  and the native nesting bound (24 nested inline flow evaluations —
  `flow_return_coverage_tests.rs` →
  `schedule::an_unpredicted_deep_chain_ends_in_the_typed_depth_refusal`,
  which drives it with the schedule off). What still recurses:
  * a callee whose evaluation is not reusable (a degraded or unproven
    return) is left to its demand; the first unreusable callee of a branch
    costs one extra evaluation before the schedule leaves that branch, and
    a chain of them nests natively, one evaluation and two nested queries
    per level, and each level evaluates the one below twice, so its work
    doubles per level. An object or array literal argument holding a
    frame binding (`b(N-1)({ a: o.a })`) is a frame value evaluated where
    it is written, so that chain answers, bottom-up, at the same work per
    level on the default test stack
    (`schedule::a_chain_of_degraded_callees_answers_on_the_default_stack`,
    `schedule::an_object_literal_argument_chain_costs_the_same_work_per_level`).
    The two typed bounds end a chain the schedule leaves to its demand
    from 12 levels, partial and never admitted, on the 8 MiB production
    worker stack
    (`schedule::a_chain_of_degraded_callees_ends_in_the_typed_refusal_on_the_worker_stack`,
    which drives the object-literal chain with the schedule off). An
    unoptimized build overflows the 2 MiB default test stack before they
    trip: the bounds are sized for the worker stack, not for a test
    thread. Skipped: a `const` type parameter's frame literal argument,
    read in its const context
    (`schedule::a_const_type_parameter_reads_a_frame_literal_argument_in_its_const_context`);
  * a new instantiation of a warm chain nests nothing, whatever shape its
    calls take: it is read off the warm uninstantiated returns, so a
    256-level chain through local arrow functions answers
    `{ v: boolean; tag: "c"; }` on a 512 KiB stack. A nested function
    value's or class expression's clause interns its own binders
    (qualified by the declaration's offset), so a return holding one that
    re-declares a chain binder's name (`id: <T,>(z: T) => z`,
    `K: class<T> { own!: T; }`) is read off the same way: both 256-level
    chains answer. Only a return holding a function TYPE written in the
    body with a same-name clause (`const id: <T>(z: T) => T`), whose
    binder is the outer one's node, is evaluated under the instantiation
    instead: over a 256-level chain through local arrow functions its
    `witness` answers and so does a new instantiation `second(v:
    boolean)` (tsc: `{ v: boolean; tag: "c"; id: <T>(z: T) => T; }`;
    held by `schedule::a_256_level_chain_returning_a_same_name_function_type_answers`);
  * a cycle among callee returns is evaluated from its first-discovered
    member through the ordinary path, where the re-entry intercept holds
    each back-edge; its members nest natively beneath that root;
  * a `this.m()` method chain nests nothing, because the flow lane answers
    its first hop with the typed `UnresolvedValue` degradation (tsc:
    `{ v: string | number; tag: "c"; }`) — a separate gap.

  A generic chain across modules is flat in depth and LINEAR in connected
  work. Each module's binder is its own, so no two levels share an
  instantiation (`c(j)` is demanded over the binder of every module above
  it); re-evaluating the body under each instantiation re-instantiated the
  chain beneath every level, and the work grew with the square of the
  chain (25 / 50 / 100 / 200 modules cost 4,322 / 16,772 / 66,047 /
  262,097 units, and from 201 modules the work rail refused the chain,
  typed). An instantiated return is read off the uninstantiated one
  instead, as the checker instantiates a signature's return type, so each
  function's body is evaluated once: 25 / 50 / 100 / 200 / 201 modules
  cost 397 / 797 / 1,597 / 3,197 / 3,213 units, one evaluation and 16
  units per module. After an edit, the schedule counts only a warm answer
  that still validates as answered, so the functions above the edit are
  re-evaluated bottom-up rather than one nested demand per invalidated
  level (counting a stale candidate as answered, the edited chain answered
  at 16 modules and missed at 64, 128 and 201).

  A body's signature-utility application over a function or object type
  (`ReturnType<typeof f>`, `Parameters<…>`, …) resolves where the body
  builds it, as the checker resolves a conditional type whose check type
  is not generic, so a chain through type positions is linear in work
  too: 12 units per level at 16, 64 and 200 levels, 11,990 in all at
  1,000 (`schedule::a_type_position_chain_costs_the_same_work_per_level`,
  `schedule::a_1000_level_type_position_chain_answers`). An application
  over a type parameter stays the deferred carrier, as the checker defers
  it (`schedule::a_return_type_in_a_body_resolves_unless_its_check_type_is_generic`).

* **Type syntax lowers from an explicit stack; the relation still recurses
  per structural level.** `lower_type_expr_with_infer_factory`
  (`project_semantic_dispatch/lower.rs`) lowers the positions whose child
  shares the node's scope, binder environment and infer factory — a named
  reference's arguments (planned through `plan_bare_ref_head`, finished
  through its owned continuation once every argument is lowered), union
  and intersection arms, array and tuple elements, template holes, a
  parenthesised type, a `keyof` operand, an indexed access's object and
  index, and an object type's property values and index signatures — from
  an explicit stack of frames, in the order and under the contexts the
  recursive descent used, so interning, queries and substitutions happen
  in the same sequence. Each such position recursed natively before, about
  24 KiB of stack per level in an unoptimized build: an 80-deep
  `Box<Box<…<1>>>` overflowed the 2 MiB default test stack, and a 400-long
  `R['v']…` chain the 8 MiB worker stack. Both answer now
  (`type_syntax_depth_tests.rs` →
  `an_80_deep_nested_generic_application_reads_on_the_default_stack`, the
  checker's `1` for 80 member reads and for the application as either
  side of a relation, and
  `a_1000_long_indexed_access_chain_lowers_on_the_default_stack`, the
  checker's `R`). A conditional, mapped, function, `typeof` or import type
  still costs one native level per nesting there. The carrier-only
  locator-shape lowering (`lower_locator_shape_node`,
  `project_semantic_dispatch/locator_shape.rs`), which interns every
  declaration body's authored shape, lowers the same positions — its
  reference arguments planned through `plan_locator_ref_head` — a
  conditional's check, `extends` clause and branches (the clause and the
  true branch under the binder frames the conditional declares), and an
  object type's property values and index-signature types from an
  explicit stack too. A nested object type (`{ v: { v: … } }`) recursed
  there once per level, about 16 KiB unoptimized; a 400-deep one reads
  through 400 member accesses on the default test stack now
  (`a_400_deep_nested_object_type_reads_on_the_default_stack`, the
  checker's `1`). A conditional chained through its false branches
  recursed there once per link, about 18 KiB per link unoptimized: a
  160-link chain overflowed the default test stack. A 320-link chain
  resolves now (`type_syntax_depth_tests.rs` →
  `a_320_deep_conditional_chain_resolves_on_the_default_stack`, the
  checker's `"c319"`, `"none"` and, over 160 links, `"c159"`). The
  oxc-AST-to-`TypeExpr` conversion (`verter_type_expr_oxc::lower_ts_type`,
  on the declaration-lowering workers) recursed once per level of every
  form, about 4.1 KiB per level unoptimized: on the 8 MiB workers a
  `Box<…>` nesting overflowed it from about 1,950 levels, before the
  parser's own limit (about 2,200 on the I/O worker), and a generic
  function type's rewrite of its type-parameter references
  (`normalize_type_parameter_refs`) recursed the same way, about 3.5 KiB
  per level. Both lower from an explicit stack now: each node's builder
  runs once with a placeholder per non-leaf child to enumerate its children
  and once over their results, and a nested function's type parameters
  resolve through a chain of scopes rather than a copy of the enclosing
  ones. `lower_depth_tests.rs` →
  `a_type_nested_5000_levels_lowers_on_a_small_stack` (a reference's
  argument, a parenthesised type, an array element, a `keyof` operand, a
  function's return, an object property, a tuple element, a conditional's
  false branch, an indexed access's object and a mapped type's value, each
  5,000 deep, on a 256 KiB thread) and
  `a_generic_function_type_nested_5000_levels_lowers_on_a_small_stack`
  (a generic function's 5,000-deep return, and 1,000 nested generic
  function types) each overflow the small thread with the recursion they
  cover restored. A 2,100-deep
  `Box` nesting now answers in the lane, and the deepest stack on the
  declaration-lowering worker is the parser's re-parse (5.8 MiB); the I/O
  worker's parser and syntax-tree clone overflow first, from about 2,300.
  The conversion costs about 30 ns more per annotation (20.4 ms against
  16.5 ms for twenty conversions of the 6,816 property and alias
  annotations of `lib.dom.d.ts`, optimized). Nested generic function
  types still rewrite the functions inside them again, a cost that grows
  with the square of their nesting (0.9 s for 1,000 unoptimized). The
  relation engine
  recurses once per structural level, and is bounded as the checker bounds
  it (`CHECKER_RELATION_DEPTH_LIMIT`, `project_semantic_dispatch/relation.rs`).
  The relation frames stacked directly on one another are one checker
  `checkTypeRelatedTo` call; a frame whose operands, unwrapped, are a
  structured pair is one `recursiveTypeRelatedTo` entry. The 101st entry
  overflows: the relation is false, every structured relation after it in
  the chain is false (the checker's `overflow` flag), and nothing computed
  under it is admitted. Measured on TypeScript 7.0.2 with each probe in its
  own file: `[B] extends [<B around number>] ? 1 : 2` is `1` over 97–99
  nested `Box` applications or `{ v: … }` literals and `2` with TS2321
  over 100, 101, 102 and 500 (the tuple wrapper is the first entry); the
  lane answers the same (`relation_depth_tests.rs` →
  `nested_object_types_overflow_at_the_checkers_depth`,
  `nested_generic_applications_overflow_at_the_checkers_depth`). The
  checker's `isDeeplyNestedType` stops a recursion earlier: three entries
  of one recursion identity on both stacks, each read from a newer
  instantiation, answer `Maybe`, which holds. The lane gives a type
  alias's applications that identity, so a finite recursive alias over a
  type literal relates `1` to `string` from its third level, and an
  infinitely recursive one stops there
  (`a_recursive_alias_is_deeply_nested_from_its_third_instantiation`,
  `an_infinitely_recursive_alias_stops_as_deeply_nested`). Two known
  differences stay open, each a skipped test asserting the checker's
  answer: an interface or class reference has no recursion identity,
  because the checker relates two references to one generic interface by
  its type arguments' variance and this engine does not
  (`an_infinitely_recursive_interface_relates_by_its_variance`: the lane
  overflows to `2` where the checker's variance measurement answers
  `1`); and the checker's cache keeps the failures its overflow produced,
  so in one file a 99-deep relation after a 100-deep one is `2`, where the
  lane answers `1`, as the relation does alone — an answer computed under
  an overflow depends on where its relation began, and the relation memo is
  shared across requests
  (`a_relation_after_an_overflowed_one_reads_its_failures`). The native
  stack one relation uses, measured from the relating thread's start: 1.9
  MiB for 100 `Box` levels and 1.1 MiB for 100 object levels unoptimized
  (under the 2 MiB default test stack), 709 KiB and 586 KiB optimized
  (`opt-level = 3`), flat from 100 to 400 levels — under the 1 MiB stack
  of `verter_wasm` (wasm32's linker default; no override is configured),
  the 2 MiB default of the tokio blocking pool, and the 8 MiB of the LSP
  serve thread, the host CPU pool, the scheduler's I/O and CPU workers and
  the declaration-lowering workers.

* **A long logical chain applies a quadratic count of guards and
  evaluates without a native level per operand.** Each operand of `a && b
  && c …` is evaluated under every earlier operand's guard and each
  short-circuit edge under their negations, as the checker's flow walk
  reads every earlier condition for each operand — a count that grows with
  the square of the chain. The short-circuit edge's union
  (`apply_guard_union`) reads the earlier parts' negations as one growing
  prefix whose standing facts are kept one per subject; re-reading the
  whole prefix for every alternative made the count grow with the cube
  (the second difference of the count per five operands was 650 at 20 and
  1,150 at 40 operands; it is now the same at both). The evaluator walks a
  chain's nested left operands from the innermost one outward
  (`eval_logical`), so a 300-operand chain answers on a 1 MiB thread
  (`flow_return_null_policy_tests.rs` →
  `an_and_chain_applies_a_quadratic_count_of_guards` and
  `a_300_operand_and_chain_evaluates_on_a_small_stack`, the checker's
  `"" | 0 | 1 | false | null | undefined`). The slice lowering of the chain
  (`Lowerer::lower_logical_value`, on the declaration-lowering workers)
  recursed once per operand, about 21 KiB per operand unoptimized (320
  operands used 6.9 MB of the 8 MiB worker stack), and each node
  reclassified its whole left operand as a guard, a count that grows with
  the square of the chain and a composition with its cube: 2,000 operands
  took 70 s of lowering. It now walks the chain's left spine from an
  explicit stack, and each node hands its guard disposition to the node
  enclosing it, so each operand is classified a bounded number of times
  (`flow_slice_content_tests.rs` →
  `an_and_chain_classifies_each_operand_a_bounded_number_of_times`: 200,
  400 and 600 classifications at 100, 200 and 300 operands; 5,247,
  20,497 and 45,747 when every node reclassifies). The evaluator's
  fresh-literal collection and effect walk over a chain walk its spine too,
  and the evaluation stops at the first operand after the connected
  demand's work budget trips, instead of evaluating the rest of the chain
  under a result already refused. A 2,000-operand chain lowers and
  evaluates on a 1 MiB thread in 8 s unoptimized
  (`flow_return_null_policy_tests.rs` →
  `a_2000_operand_and_chain_lowers_and_evaluates_on_a_small_stack`: the
  typed `Budget(WorkBudgetExceeded)` refusal, with the guard applications of
  a 1,000-operand chain; without the early stop, 6.0 million against 1.5
  million and 90 s). Recursive slice lowering overflows the worker, and a
  recursive fresh-literal collection or effect walk the 1 MiB thread. The
  checker answers the chain in about a second
  (`"" | 0 | 1 | false | null | undefined`); the lane's connected work
  outgrows the budget from about 420 operands
  (`a_2000_operand_and_chain_answers_the_checkers_type`, skipped).

* **The scheduler's workers parse on 8 MiB stacks.** The oxc parser, the
  syntax-tree clone the semantic builder reads (`clone_in`) and the
  semantic builder recurse once per nesting level and carry no depth guard
  of their own, and they run on the scheduler worker that loads a file.
  Those workers (`verter-io-*`, `verter-cpu-*`) ran on the platform default
  of 2 MiB, where a 700-deep `Box<…<1>…>` (2.0 MiB of `clone_in` on the
  I/O worker, unoptimized) or a 2,000-operand `&&` chain overflowed a
  worker and aborted the process. They now reserve 8 MiB
  (`WORKER_STACK_BYTES`, `verter_scheduler/src/pool.rs`), the stack of the
  LSP serve thread, the host CPU pool and the declaration-lowering workers;
  a reservation costs address space, not memory. `type_syntax_depth_tests.rs`
  → `a_700_deep_nested_generic_application_parses_on_a_scheduler_worker`
  answers the checker's `"v"` for `keyof D` and `1` for `D extends
  Box<unknown> ? 1 : 2` (at 2 MiB it overflows `verter-io-0`). The parser
  itself still refuses nothing: on the 8 MiB I/O worker it overflows at
  about 2,200 nested type arguments and 5,000 nested parentheses
  (unoptimized), where TypeScript 7.0.2 answers at 10,000 of either
  (`"v"` and `1` for the `Box` nesting, `1` for `(((…1…)))`, each in under
  2 s). The slice lowering of a returned value (`Lowerer::lower_expr`,
  on the declaration-lowering workers) re-entered itself once per
  parenthesis, about 15 KiB per level unoptimized, and overflowed the 8 MiB
  worker from about 550; each level is now re-entered from its loop, and a
  return in 2,000 parentheses answers the checker's `number`
  (`flow_return_null_policy_tests.rs` →
  `a_return_in_2000_parentheses_lowers_on_the_worker_stack`; it overflows
  with the recursion restored). The declaration-lowering worker's deepest
  stack there is its re-parse of the file (4.0 MiB at 2,000).

* **Every production thread that analyzes reserves the workers' stack.**
  Several host APIs compute on the caller's own thread (`compile_entry`,
  public-API extraction, the scheduler-missed analysis lane), so the
  caller's stack matters as much as the workers'. Before, the LSP's tokio
  workers and blocking pool (sync coordinator, background drain,
  diagnostics, scanner compile, document analysis), every MCP tool call,
  `verter-tsc`'s main thread and rayon pool, and the WebAssembly module ran
  on 2 MiB, 1 MiB (Windows main threads) or 1 MiB (wasm32's linker
  default). Measured on the calling thread with a 256-deep nest, the flow
  evaluation of nested functions, blocks and object literals takes up to
  2.6 MB optimized and 11.4 MB unoptimized (arrow functions; `if` 1.2 /
  6.1 MB, object literals 0.5 / 2.6 MB, class expressions 1.6 / 6.8 MB), and
  the declaration-lowering worker up to 1.3 / 7.2 MB. Every analysis
  thread now reserves `verter_scheduler::WORKER_STACK_BYTES` (8 MiB
  optimized, 32 MiB unoptimized): the scheduler's and host's workers, the
  declaration-lowering workers, the LSP serve thread and its runtime's
  workers and blocking threads (`thread_stack_size`), MCP's server thread
  and runtime (`verter_mcp::run::run_blocking`), `verter-tsc`'s checker
  thread and global rayon pool, and the WebAssembly module
  (`crates/verter_wasm/build.rs`: `-zstack-size=8388608`; the built
  module's stack pointer starts at 8 MiB). Node's own main thread, which
  `verter_napi` computes on synchronously, is 8 MiB on Windows
  (`SizeOfStackReserve` of node.exe) and the `ulimit` on Linux. On the
  8 MiB wasm module every measured form at 250 levels answers through
  `evaluateTypeExpressionWithAudit`.

* **oxc's parse runs on a stack its source cannot exhaust.** oxc_parser
  0.126's recursive descent has no depth limit and aborts the process when
  a thread's stack runs out, before returning an AST: on an 8 MiB stack at
  4,512 nested type arguments or object literals and 5,664 parentheses
  (optimized), 2,272 and 3,424 unoptimized
  (`docs/evidence/signature-kernel/oxc-deep-parse.md`, reproduced by
  `crates/verter_parser/examples/oxc_deep_parse.rs` with nothing of
  Verter's). Every production parse goes through
  `verter_parser::oxc_parse::Parser`, a drop-in that parses exactly what
  oxc parses: a linear scan bounds the syntax tree's depth from above and
  the parse gets 8 KiB per level of it (twice oxc's costliest measured
  level), in place when the thread has it and on a `stacker` segment
  otherwise. No source is refused and no depth is imposed.
  `oxc_parse/tests.rs` parses ten forms 10,000 deep and an unclosed
  200,000-deep nest on a 1 MiB thread (with the parse in place, or with a
  per-level stack below what oxc spends, it overflows), and
  `no_crate_parses_around_the_guard` fails on a direct
  `oxc_parser::Parser`. The scan costs about a third of the parse (8.1 ms
  against 6.1 ms for `lib.dom.d.ts`, optimized); a source short enough
  that every byte could be a level skips it. `stacker` cannot grow the
  stack on wasm32, where the module's 8 MiB bounds the parse. What runs
  after the parse (the syntax-tree clone, the semantic builder and
  Verter's own passes) still recurses per level in places.

* **A class expression raises one instance per level.** The lane's graph
  of `class { m() { return class { … } } }` nested `n` deep is linear
  (evaluation 3–10 ms from 4 to 24 levels, optimized), but its raised type
  printed the `prototype` property the checker declares on every class
  constructor beside the construct signature, each spelling the anonymous
  instance structurally, so the raised shape doubled per level: 8,451,
  143,091, 2,297,331 and 36,765,171 JSON bytes at 4, 8, 12 and 16 levels,
  62 s to serialize 16 levels natively, 72 s for
  `evaluateTypeExpressionWithAudit` on the WebAssembly host at 16 and no
  answer at 20. No query or memo recomputed work (the audit's hops and
  expansions are 3 and 2 at every depth); the output did. The raise now
  omits that synthetic prototype property, as the checker's declaration
  emit does (`{ new (): { m(): … } }`, 9,806 bytes at 24 levels), while
  the graph keeps it for member reads and `keyof`: 1,088 to 6,348 bytes
  from 4 to 24 levels, 3.5 ms to serialize 24, and 77–86 ms per call on
  the WebAssembly host at every depth.
  `flow_return_class_tests.rs` →
  `a_nested_class_expression_raises_one_instance_per_level` asserts the
  folded nodes (a test counter in the raise's `fold_node`) and the raised
  size grow by the same amount per level over 4, 8 and 12; with the
  prototype printed they grow 136, 2,296, 36,856.

* **A call's arguments lower once per enclosing call.** A call nested
  as an argument (`g(g(…g(1)…))`) lowers again from the enclosing call's
  frame-lowered arguments (`Lowerer::lower_call_arguments`), and each
  such lowering recorded its own arguments again
  (`Lowerer::record_call_arguments`), so the work doubled per level:
  383, 6,143 and 98,303 expression lowerings at 8, 12 and 16 levels, and
  256 levels never finished. A call whose arguments are already recorded
  keeps them; the lowerings grow with the square of the depth
  (`flow_slice_content_tests.rs` →
  `nested_calls_lower_their_arguments_once_per_enclosing_call`, the second
  difference over 8/12/16 equal to that over 12/16/20; it fails with the
  re-recording restored), and 256 nested calls lower in 0.5 s optimized.

* **A pair of literal types relates without a query.** Two literal types
  relate, under every relation kind, exactly when they are one value, so
  the relation authority decides such a pair before its reentry intercept,
  memo and cold build (`literal_pair_relation`, depositing the operands'
  file roots as the cold read would), unless an inference session
  collects. A chain of `if (x === "k<i>") throw` guards over an
  `N`-member literal union filters the remaining arms at every guard, as
  the checker's `filterType` does — about `2N²` relation checks, the
  checker's own count — and paid a cold relation query for each: 200
  guards took 4.9 s unoptimized, 800 minutes. Now none of them reaches the
  structural reducer, 200 guards take 0.33 s, and 800 answer the checker's
  `"k799"` (`wide_union_relation_tests.rs` →
  `an_equality_guard_chain_over_a_literal_union_reduces_no_relation`).
* **The demand slice plans at most 256 return sites.** A function with
  more `return` statements on its demanded paths is refused before
  evaluating, with `Budget(WorkBudgetExceeded)`
  (`FlowSliceBudget::max_return_sites`): 255 `if (x === i) return …`
  guards and the final return answer, 256 are refused. The work each such
  guard costs is constant (relation checks `5N + 2`, guard applications
  `2N` at 50, 100 and 200 guards), so the refusal is the planner's
  return-site cap, not super-linear work; the skipped
  `differential_depth_tests.rs` →
  `an_800_return_if_chain_answers_within_the_work_budget` holds it open.
