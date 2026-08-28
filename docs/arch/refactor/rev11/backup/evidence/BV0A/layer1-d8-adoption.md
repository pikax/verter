# Layer-1 revision 8 (`DECISION` D-8) — re-adoption record

**Artifact:** `packages/framework-conformance-harness/spec/assembled-map-composition-layer1.md`
**Adopted blob:** `085139c5267136ed0c2fa39d78ad48168c6e0e76`
**Adopted commit:** `a52f3021c` ("add the U8.1 fragment-attribution rule to the composition
specification") on `work/bv0a-layer1-spec`, direct child of the revision-7 freeze commit
`6317cadd5` (blob `0ea47424acfbd4913e11f16156baa597216c84fb`, adopted per
[`layer1-freeze-adoption.md`](layer1-freeze-adoption.md)).
**Prior adoption:** revision 7, closed by `layer1-freeze-adoption.md`. This record closes revision
8 — the single narrow amendment on top of it (`DECISION` D-8, §4.3 step 2.1's `fragment`
attribution for `U8.1`) — as its own adoption, per §12's own requirement that a post-freeze
amendment "requires its own independent review before it is adopted alongside revisions 1–7" and is
"not silently absorbed into" the original freeze.

## Why this amendment exists

`U8.1` (script/template `sourceRoot` disagreement) was reachable under frozen revision 7, but
§4.2's `UncomposableInputMap{family, code, fragment}` shape requires a `fragment` and revision 7
never said which one `U8.1` names. The independent JavaScript reference and the production Rust
implementation — each built against revision 7 with zero visibility into the other, per AMD-008 —
both hit this gap while writing their own `U8.1` handling and each resolved it independently, by
guess rather than by a shared frozen rule. They reached the same answer (`template`), but that is a
fact about two implementers' instincts, not evidence the frozen text was complete — exactly the
failure mode the freeze process exists to prevent. Per CLAUDE.md's explicit-finding-disposition
rule, this was not accepted as an ad hoc agreement between the two implementations: D-8 was drafted
as a dated addendum, independently reviewed, and only then did both implementations get held to the
reviewed wording rather than their own guesses.

## The rule

`U8.1`'s `fragment` is **`template`**, for the present two-fragment §3.3 DTO only. Contributing-map
order is fixed as script, then template (§4.3); `U8.1` is reachable only when both fragments carry
a contributing map and their normalized `sourceRoot`s disagree; the later contributing map in that
fixed order is therefore always the template's. The rule is explicitly **not** claimed to generalise
to a hypothetical third mapped fragment — §11.6 item 4 already reserves that case for its own
re-derivation, and the amendment's revision history records the original future-proofing sentence as
a **rejected** claim, not merely an unstated one.

## Review process

Two full review rounds, four independent blind dispatches (conformance: Codex xhigh; adversarial:
grok-4.6 xhigh — no mandate resumed across rounds).

- **Round 1.** Conformance: **FAIL** — the rule for the live two-fragment DTO was confirmed correct,
  but the draft's "requires no further amendment if a future charter ever admitted a third fragment"
  sentence was found false: with three contributors, "later contributing map" is ambiguous between
  first-mismatch and last-in-list, which coincide only while there are exactly two. Adversarial:
  **PASS_WITH_NOTES** — same finding, plus a wording correction ("stage 1 establishes the baseline"
  overstates; stage 1 only type-checks each map independently, the baseline is a sequential reading
  of stage 2.1). Both converged on the identical root cause.
- **Fix.** The future-proofing sentence was deleted (kept only in revision history as a recorded
  rejected claim, per §12); the amendment now states explicitly that it decides only the present
  two-fragment DTO and is not claimed to generalise; the "stage 1 establishes baseline" phrasing was
  corrected to describe stage 2.1's sequential reading.
- **Round 2.** Conformance: **PASS** — "No blocking findings. Ready to adopt now." confirmed the
  overclaim was gone from normative text (survives only as a deleted, rejected claim in revision
  history), the substantive rule unchanged, and the diff scoped to amendment metadata / D-8 / the
  decision register / revision history only — no unrelated document content touched. Adversarial:
  **PASS** — "Adopt D-8." Independently re-derived the rule from the raw DTO trigger table (zero /
  one / two contributing maps; only the two-contributor case is reachable; fixed script-then-template
  order makes `template` unconditional under this DTO) and confirmed no admissible two-slot input
  produces a different fragment.

Both rounds independently re-verified contamination: `git merge-base --is-ancestor` returns
non-ancestor for all eight superseded-attempt commits (`26f1dae9d`, `db26cde00`, `ddcc255ba`,
`e7ca0e68b`, `1493b158e`, `64cfe9777`, `2b08cddd7`, `32efc149b`) and for both superseded branch tips
(`work/bv0a-implementation`, `work/bv0-relanding`) against `HEAD`.

Independently re-verified by the track orchestrator before this record was written: `6317cadd5` and
`a52f3021c` are both ancestors of this worktree's `HEAD`; the current in-tree spec blob
(`git hash-object`) matches `085139c5267136ed0c2fa39d78ad48168c6e0e76`, the exact blob round 2 both
mandates reviewed; the diff between the revision-7 and revision-8 blobs touches only the front
matter status line, §4.3's D-8 block, the §4.4 decision register row, and §12 — no other section
changed.

## Disposition against the FC-VUE-003 resolution gate

This record, together with `layer1-freeze-adoption.md`, closes gate check 1 (layer-1 completeness)
for revision 8 as amended and check 3 (maintainer adoption — a record distinct from review approval
alone) for D-8 specifically. Check 2 (non-retroactive chronology) for D-8 is closed by the same
contamination re-verification recorded above, plus the specific fact that D-8's addendum was drafted
and reviewed only after BOTH implementations had already been built against revision 7 and had
independently exposed the gap — the amendment did not precede or shape either implementation's
initial construction; it corrected a rule both had to guess at, and both implementations were then
brought into conformance with the reviewed text, not left on their guesses (§12; both implementations
confirmed via direct inspection to route through the reviewed rule at the same call sites this
record's review rounds cite: `map_input.rs`'s `agree_source_root`, `assembled-map-composition-reference.mjs`'s stage 2).

## A note on the blob after this record

Immediately after this record was written, the front matter's own status line was updated in place
(`fdaf069742790836ee1c4ecf5049f12cc21dfcaf`) to narrate that revisions 1–8 are now adopted and to
point at this record — the same purely-narrative pattern the D-8 commit itself used to narrate
revision 7's freeze. That edit touches only the front matter's prose; it does not change §4.2–§4.4,
the `DECISION` register, or §12, and is not itself a reviewable semantic change. The blob this record
adopts, and the one both round-2 review dispatches examined, is `085139c5267136ed0c2fa39d78ad48168c6e0e76` —
diff it against the current tip if verifying that no substantive line moved.

## Next

Both implementations already produce the reviewed answer (confirmed independently by both review
rounds); no further code change is required by this amendment. Layer 1 stands adopted through
revision 8 as of this record. The next post-freeze change to layer 1, if any, requires its own
addendum and its own independent review under the same procedure.
