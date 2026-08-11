# A3 exact candidate record

## Identity

| Field | Value |
|---|---|
| Base | `20acec177` |
| Candidate | `c1aef669d9c1505e69faf0e327a9c1a5069c5798` |
| Candidate tree | `a2fd9db82c6c2ca49f0bfb1cddc15860290b0a66` |
| Accepted | equals the candidate (fast-forward landing, no rebase) |
| Scope | 22 files, +2917 / −531 |

Landing was a fast-forward, so accepted identity equals the reviewed candidate identity and no landing-equivalence artifact is required.

## What the block delivers

A flow-return result the current lowering cannot model is returned as a typed partial
outcome and never admitted warm: it replays cold, publishes no candidate, and recomputes on
demand. Consumers and strongly-connected members propagate the typed outcome rather than a
fabricated complete one.

Scope is the reduced exit criterion: non-G10 wrong-complete retraction only. No syntax-only
completion detector exists, no second graph or classifier was introduced, and structural
completion / G10 discrimination remains debt owned by the flow-graph authority.

## Verification

Full gate (`node scripts/gate.mjs`) on the exact landed tree: **PASS**, all three surfaces.

| Surface | Result |
|---|---|
| 1 — nextest, process isolation | 24103 run, 24103 passed, 0 failed, 582 skipped |
| 2 — in-process libtest | 3 suites clean, 0 non-tolerated failures, 0 tolerated |
| 3 — shipped `cfg(debug_assertions)` off | 8536 run, 8536 passed, 0 failed, 563 skipped |

An earlier run of the same tree failed two `compile_fail` trybuild smoke tests
(`hot_materialize_structural_rails_smoke`,
`hot_materialize_and_script_fact_structural_rails_smoke`) with "Expected test case to fail to
compile, but it succeeded", alongside cargo build-directory lock contention. Both pass in
isolation, both pass under the gate's workspace feature unification, and both pass in the
green full-gate run on the identical tree. The failure is a load-induced race in the trybuild
harness, not a defect in the structural rails. It is recorded here because a gate that fails
under its own parallelism is a real problem independent of this block.

Focused lane at acceptance: 524 passed, 0 failed. Clippy over `verter_session` +
`verter_semantic`, all targets, `-D warnings`: clean. `cargo fmt --all --check`: clean.

## Review

Four review rounds, each independent and adversarial, against pinned immutable diffs. Legs
were split so no single leg carried the whole surface, and the model that authored the work
was outnumbered by legs on a different model.

Every round returned CHANGES. Each fixed its findings and introduced a defect of the opposite
polarity, which is the substantive finding of this block's history:

1. Round 1 fixed a false refusal and left the predicate keyed on the root's authored type,
   admitting wrong-complete results.
2. Round 2 re-keyed to the reaching value and deleted the statement-level impossible-branch
   detector, converting a false refusal into a wrong-complete admission (`g1` published
   `"live"` warm where the checker says `"dead" | "live"`).
3. Round 3 restored a refined detector and replaced the effect gate with a nine-spelling
   allowlist, falsely refusing pure chains such as `a?.[-1]`.
4. Round 4 replaced the allowlist with recursive effect detection and extended the detector to
   ancestor regions, which over-fired into conditional sub-regions where the sibling path still
   reaches the suffix — a false refusal.

The ancestor-region extension was optional scope: it addressed a pre-existing coverage gap the
block never owned. On maintainer ruling it was reverted rather than fixed, removing the defect
class instead of chasing it, and the gap was recorded as debt.

## Final semantics

An optional member chain stays complete when its root is proven `any` at the read and every
step between root and read is a plain member access. A root since written or narrowed, a path
crossing an assertion / `satisfies` / non-null / instantiation / call node, and a chain whose
discarded operand contains an assignment, update, `delete`, `await` or `yield` are unproven and
stay unsupported. An unrecognised operand form is treated as unproven, so new effectful syntax
fails closed rather than leaking through.

Return-type inference is syntactic, so an unreachable `return` still contributes. An impossible
branch degrades the spellings that drop such a contributor, while a spelling reading the
narrowed subject exactly keeps its clean, warm result.

## Preservation

The 154-row preservation cohort is recomputed from the corpus and asserted by set equality
against the lock, then each row is measured for real: cold, no degradation, one candidate, warm
second call, zero cold work, byte-identical projection. It cannot be shrunk silently. Rows
`X05_catch_return_fallthrough`, `X68`, `X80` and `X88` remain clean and warm.

## Debt

`FR-D9`, `DEFER`: a wrong-complete flow-return result whose dropped return contributor lies in
an enclosing region is not retracted, because the detector scans only the current region's
statement suffix. Durable owner: D6 / `U6.LOOP_CLOSURE`. Resolution gate: closes before that
owner enters review. Recorded in `docs/arch/u6-flow-return-gaps-and-target.md`.

## Evidence

Mutation evidence: `A3/mutation-evidence.md` — reversible recipes with proof each plant
applied (present, unique, new) and byte-identical restoration, each requiring its named test to
fail when planted.
