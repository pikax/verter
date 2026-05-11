# Cycle-safety failure-mode investigation

Stage-0 sub-task 2(a) — pre-Stage-3 commitment of the pre-change failure mode.

The fact-based cache plan asks Stage 3 to land a stack-safe semantic
fingerprint computation under `R27` ("All semantic fingerprint
computation is stack-safe: implemented as an explicit worklist with a
`VisitedSet`."). Before Stage 3 designs its replacement test, Stage 0
fixes which failure mode the current tree exhibits so Stage 3's test
discriminates the right behaviour.

The plan posed two candidate failure modes for today:

- **(a)** existing recursion-limit panic (stack overflow inside hashing
  or materialisation of mutually-recursive type aliases), or
- **(b)** content-hash memoisation brittleness (cache miss explosion
  under mutually-recursive types).

## Conclusion

**Today's failure mode is neither (a) nor (b) in isolation.** A third
mode, layered cooperative cycle guards, terminates recursive type
aliases at the policy walker before they reach a hashing or
memoisation layer. The cooperative termination produces a structurally
typed "semantic-miss" sentinel rather than a stack overflow or a
cache miss explosion. This conclusion is supported by direct
inspection of the current tree and by two characterisation tests that
ALREADY pass on the audited base SHA `ccc05223`.

The relevant code paths the investigation walked:

1. `crates/verter_session/src/component_meta_materialize.rs`
   - Line 213: `pub const MAX_DEPTH: usize = 4096;`
   - Lines 220-261: `MaterializeInFlightGuard` — per-thread RAII guard
     stacking the current `MaterializeStructureCacheKey` in
     `MATERIALIZE_IN_FLIGHT` and incrementing `MATERIALIZE_DEPTH`. The
     guard is push-on-entry / pop-on-drop and is panic-safe.
   - Lines 371-381: same-key thread-local re-entry detection. When
     `MaterializeInFlightGuard::contains_key(&key)` is `true`, the
     materialiser returns
     `MaterializeOutcome::Recursive(opaque_miss)` with an empty
     `dep_signature` and `cache_suppress: false`. **No cache write
     occurs**; the result is propagated up the call chain as
     non-cacheable.
   - Lines 384-391: pre-admission depth fuse. When
     `MaterializeInFlightGuard::current_depth() >= MAX_DEPTH`, the
     materialiser returns `MaterializeOutcome::Tainted(key.base)` with
     an empty `dep_signature`. Same suppression semantics.

2. `crates/verter_session/src/component_meta_resolution_policy_cycle_tests.rs`
   - `recursive_pick_local_alias_terminates_via_semantic_miss`
     (lines 173-240): drives `type Recursive = Pick<Recursive, 'x'>`
     through `apply_component_meta_resolution_policy`. The test
     accepts a small set of structurally typed sentinels as a valid
     terminator: `TypeExpr::Unknown { raw: "semanticMiss…" }`,
     `TypeExpr::RecursiveRef { name: "Recursive" }`, the preserved
     `Ref { name: "Pick", type_arguments: [Ref { name: "Recursive" }, …] }`,
     or the bare zero-arg `Ref { name: "Recursive" }`. The test runs
     the policy on a 256 KiB stack worker via
     `assert_no_stack_overflow` (see file `capture_token.rs:839`) and
     expects `Ok(meta)` — i.e. no `StackOverflow`. The test currently
     passes.
   - `recursive_omit_self_referential_alias_terminates`
     (lines 246-290): mirror test for
     `type SelfOmit = Omit<SelfOmit, 'gone'>`. Same termination
     sentinels, same expectation. The test currently passes.

3. `crates/verter_session/src/capture_token.rs:839`
   - `assert_no_stack_overflow` runs the closure on a 256 KiB stack
     thread. On Linux/macOS the OS converts the SIGSEGV at the guard
     page into a thread-only panic that `JoinHandle::join` reports as
     `Err`. On Windows the process aborts with
     `STATUS_STACK_OVERFLOW = 0xC0000FD` and the test runner reports a
     process abort.
   - The harness exists to stress-test the cycle-guard machinery: when
     the guard is wrongly keyed and the policy walker chases an
     infinite chain, the 256 KiB stack catches it quickly.

4. `crates/verter_session/src/resolver_core/fuses.rs:8`
   - `FuseBudgets` carries the architecture rules for related
     traversal layers: `member_surface_recursion_depth: 10`,
     `projection_op_count: 2000`, `union_member_explosion: 100`. These
     are tier-2 backstops; the policy-walker's `active_refs` guard is
     tier-1 and fires first on the recursive type aliases tested
     above.

## Layered cycle-guard topology (current tree)

The cycle protection is layered across three guards that fire in
priority order. A recursive type alias is terminated at the first
guard that recognises it:

| Layer | Mechanism | Effect on recursive type alias | Cacheable result |
|---|---|---|---|
| **Policy walker (tier-1)** | `active_refs` set on `(DeclIdentity, NormalizedTypeArgs)` inside `apply_component_meta_resolution_policy` | Bails with `semanticMiss` / preserved `Ref` sentinel | No — the surface is published, the registry entry stays open |
| **Materialise (tier-2)** | `MaterializeInFlightGuard` per-thread stack on `MaterializeStructureCacheKey` | Returns `MaterializeOutcome::Recursive(opaque_miss)`; `cache_suppress: false`; cooperative-admission skips the publish | No — non-cacheable outcome bypasses `MaterializeStructureDb` write |
| **Defensive depth fuse (tier-3)** | `MATERIALIZE_DEPTH` thread-local counter; `MAX_DEPTH = 4096` | Returns `MaterializeOutcome::Tainted(key.base)`; same non-cacheable semantics | No |

The first guard that fires for `type Self = { next: Pick<Self, "next"> }`
is the policy walker's `active_refs` (tier-1), because the recursive
alias re-enters the body chase before the materialiser ever sees the
same `MaterializeStructureCacheKey` twice. Tier-2 and tier-3 are
defensive backstops; they do exist on the current tree and are
exercised by tests for shapes the policy walker cannot recognise.

## What this means for Stage 3

R27's "stack-safe explicit worklist with `VisitedSet` and `CycleRef`
placeholder" replaces all three tiers with a single canonical-visit-
order traversal that emits `CycleRef(visit_index)` placeholders rather
than the current grab-bag of structurally typed sentinels
(`Unknown { raw: "semanticMiss…" }`, `RecursiveRef`, preserved `Ref`).
A Stage 3 cycle-safety test must therefore characterise BOTH:

1. **Pre-change discriminator** — recursive aliases terminate today
   under one of the four sentinel shapes documented in the Stage 0
   characterisation tests (the test FAILS pre-change because the
   sentinel is one of the LEGACY shapes, not `CycleRef`); AND
2. **Post-change discriminator** — recursive aliases terminate under
   Stage 3's canonical `CycleRef(visit_index)` placeholder with stable
   identity across source-text reordering.

Concretely, a Stage 3 test against
`type Self = { next: Pick<Self, "next"> }` should assert that the
visit order is lexicographic by `(name, symbol_space)` (R27) and that
the produced fingerprint is byte-identical to the fingerprint
produced after source-text reordering of the same file. This is
weaker than "today's behaviour" — today's tests accept any of four
sentinel shapes — and stronger than "no stack overflow" — today's
behaviour already satisfies that on the policy-walker path.

## Why neither (a) nor (b) describe the current failure mode

**Not (a):** the policy walker terminates before the recursive shape
reaches a hashing or materialisation stack. The
`assert_no_stack_overflow(256 KiB)` harness confirms this — recursive
aliases tested above complete inside the small stack. A stack
overflow today requires bypassing the policy walker (e.g.
constructing a malformed registry that the walker has no `DeclIdentity`
record for), which is not the shape Stage 3 needs to characterise.

**Not (b):** the policy walker's sentinel is `cache_suppress: false`
AND the materialiser's `Recursive` / `Tainted` outcomes carry empty
`dep_signature`s and skip the `MaterializeStructureDb` publish. So
recursive aliases are NOT memoised under a content hash — they
short-circuit at the policy walker BEFORE any memoisation key is
formed. There is no "cache miss explosion under content-hash
brittleness" today because there is no admission to the content-
hashed cache for these shapes.

The actual mode — cooperative termination through layered cycle
guards — is what Stage 3 is replacing, and Stage 3's test design
should discriminate against the LEGACY sentinels (this file is the
formal record of those sentinels).

## File references (audited base SHA `ccc05223`)

- `crates/verter_session/src/component_meta_materialize.rs:213`
  (`MAX_DEPTH = 4096`)
- `crates/verter_session/src/component_meta_materialize.rs:220-261`
  (`MaterializeInFlightGuard`)
- `crates/verter_session/src/component_meta_materialize.rs:371-381`
  (same-key re-entry detection)
- `crates/verter_session/src/component_meta_materialize.rs:384-391`
  (depth fuse)
- `crates/verter_session/src/component_meta_resolution_policy_cycle_tests.rs:173-240`
  (`recursive_pick_local_alias_terminates_via_semantic_miss`)
- `crates/verter_session/src/component_meta_resolution_policy_cycle_tests.rs:246-290`
  (`recursive_omit_self_referential_alias_terminates`)
- `crates/verter_session/src/component_meta_resolution_policy_cycle_tests.rs:145-167`
  (`run_policy_with_overflow_check` harness)
- `crates/verter_session/src/capture_token.rs:839-903`
  (`assert_no_stack_overflow`)
- `crates/verter_session/src/resolver_core/fuses.rs:8-38`
  (`FuseBudgets` defaults)
- `crates/verter_session/src/semantic_query_memo/family.rs`
  (semantic query memo — cycles never reach it from these shapes)
