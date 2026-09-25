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
  checker's `"c319"`, `"none"` and, over 160 links, `"c159"`); the
  deepest remaining recursion over such a chain is the oxc-AST-to-`TypeExpr`
  conversion (`verter_type_expr_oxc::lower_ts_type`, on the declaration
  lowering workers), about 4.6 KiB per link. What still recurses per
  structural level is the relation engine: relating two nested
  applications (`[D] extends [Box<…<number>>]`) opens one inline relation
  frame per level (`execute_relate_inline` → `reduce_relation` → …
  `relate_property_pair` → `relate_member`, about 16 KiB per level
  unoptimized). It answers 85 levels on the 2 MiB default test stack and
  overflows it from 90; on an 8 MiB thread it answers 400 levels and
  overflows at 500, where at 600 the oxc parser's own recursion over the
  source's type arguments overflows the IO thread first. The checker
  bounds the same recursion: from 100 nested levels it reports TS2321
  (excessive stack depth comparing types) and the relation is false.

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
  (`Lowerer::lower_logical_value`, on the 8 MiB declaration-lowering
  workers) still recurses once per operand, about 21 KiB per operand
  unoptimized: 320 operands use 6.9 MB of it.

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
