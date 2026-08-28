# CSS arena lifecycle — provenance (not a landing baseline)

This record is provenance of the `:slotted()` argument-splice allocation
claim. It is not a landing baseline and its historical counts are not
proof of the 1.2x allocation ceiling. A later recapture owns landing
numbers.

## Owners

Allocation magnitudes and ceilings:
`crates/verter_compiler/tests/allocator_canaries.rs`
(`slotted_occurrence_arena_bytes`). Build counts, output shapes, map anchors
and the arm sweep:
`crates/verter_compiler/src/direct_result_tests/style_planner.rs`. Production
structure: `crates/verter_compiler/src/style_planner.rs`.

## The decision

`:slotted()` argument scoping contributes its edits directly to the outer edit
vector that `emit` applies through its one `CodeTransform`, instead of
composing the argument separately and handing back rendered text.

Two earlier shapes were rejected. At `749873cb6` — this line's base —
`render_special_argument` minted a fresh `Allocator` and a nested
`CodeTransform` per `:slotted()` occurrence, so the cost recurred once per
occurrence rather than once per stage. Its first replacement removed that
recurrence with a hand-written string splice and was rejected on review: it
composed CSS output bytes outside `CodeTransform`, which the charter and the
clean-cutover directive both forbid. Changing the implementation rather than
relaxing the invariant is what kept the resulting amendment narrow.

## Actions performed

The measurement command below was **run**, not merely offered: in this
worktree, `crates/verter_compiler`, dev profile, macOS aarch64, at
`af4bebeea`, and re-run at each later sha named in this section.

```
cargo nextest run -p verter_compiler --no-fail-fast --no-capture \
  -E 'test(slotted_occurrence) or test(multi_edit) or test(three_edit) or test(slotted_argument) or test(parser_only_residual) or test(attribution_slotted_rules_and_mixed_vue_totals) or test(share_one_emit_build_string) or test(build_string_call_count_matches_edit_composition_depth)'
```

Five plants, each against the canonical test entry point, each under the same
protocol: marker proven absent from the tree before planting, present exactly
once after, reverted by targeted replacement leaving `git status --porcelain`
empty. A green planted run was treated as a failed plant until shown
otherwise.

| plant | marker | gate | run at |
|---|---|---|---|
| A | `CSSPLANT_MULTIEDIT_NESTED_ARENA_A7Q` | more than one argument edit | `af4bebeea` |
| B | `CSSPLANT_UNCONDITIONAL_NESTED_ARENA_Z9K` | unconditional | `af4bebeea` |
| C | `PLANT_MARKER_SPLICE` | the rejected splice, restored verbatim | `368f8c072` |
| D | `CSSPLANT_THREEEDIT_NESTED_ARENA_R4T` | more than two argument edits | `a04f6fc34` |
| E | `CSSPLANT_GT3_ARENA_W8N` | more than three argument edits | `24fabfff0` |

At the sha named for each, the gate was reachable from ordinary CSS and the
corpus as it then stood did not catch it except where a fixture had been added
for that rung. Each plant's red and green columns are held by the tests named
under Owners; re-run them against a plant to reproduce a column rather than
reading one here.

One plant was initially mis-calibrated: its gate counted the prefix
`Overwrite` alongside the argument's inserts, so it fired one rung below the
threshold intended. The tell was which case failed, not that one did. It was
recalibrated before its result was used.

## Dispositions

- The donor's `emit`-arena sizing change was **cut**, not carried: no criterion
  measured it, so it could neither be bound nor honestly claimed.
- The byte counter is **retained** alongside the call count. At `24fabfff0` a
  count-only ceiling at the ratio the charter already fixes would have fired on
  the slotted row and not on the mixed row, so the count is a proxy whose
  margin depends on how many occurrences a generator emits. The byte counter
  was kept because it measures the imposed cost directly rather than through
  that proxy.
- The `:slotted()` map anchoring changes, reachably and deliberately. It is
  **accepted** as a correction rather than a regression: authored bytes now map
  where they were authored, which the stage's declared fidelity asks for. It is
  pinned at `368f8c072` by `slotted_argument_bytes_map_to_their_own_authored_offsets`. No test
  asserted the previous anchoring, which is why nothing failed when it moved.
- Closing edit-count rungs one at a time was **abandoned** after three rounds.
  The argument family is unbounded, so a fixture at N arms always leaves a
  regression gated above N. `slotted_argument_edit_count_sweep_never_mints_a_nested_build`
  replaced that approach with a sweep whose limit is named in the test as
  `ARM_SWEEP_MAX`.

## Accepted residuals

- At `24fabfff0`, no gate in this corpus observes `emit`'s own arena or
  `CodeTransform`'s per-emit minimum. **Accepted:** both are paid once per emit
  stage rather than per occurrence, and changing the latter would retune every
  compiler consumer of `CodeTransform`.
- At `24fabfff0`, `parser_only_residual_between_deep_and_slotted_generators`
  reports a difference between the two arms and bounds nothing. **Accepted:**
  the isolation figures are therefore upper bounds on rewrite cost rather than
  exact attributions, which is sufficient for the ceilings the canaries assert.
- At `24fabfff0`, no gate observes a regression gated above `ARM_SWEEP_MAX`
  arms. **Accepted:** inherent to example-based testing over an unbounded
  family.
- At `368f8c072`, `slotted_argument_bytes_map_to_their_own_authored_offsets`
  observes the `:slotted()` arm on one single-line fixture, and observes map
  anchors rather than chunk provenance. **Accepted:** `:deep()` renders its
  argument and overwrites the component, and `:global()` renders its argument
  and overwrites the selector's content span; the rendered text is inserted
  content either way, so authored-offset preservation does not apply to them;
  and an implementation that re-rendered the
  argument and deliberately re-anchored it would satisfy the guard. The
  single-mechanism property is therefore held by the production structure
  rather than by this guard.

## Process fact

At `368f8c072`, `crates/verter_compiler/src/style_planner.rs` reached the form
described under The decision, and is unaltered through `ac60c33c6`. Every
review finding between those two shas concerned this record or test
coverage, never behaviour. That is what makes the recurrence a records problem rather than a
code problem.
