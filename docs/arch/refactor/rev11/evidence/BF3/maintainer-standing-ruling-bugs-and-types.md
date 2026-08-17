# Maintainer standing ruling — bug handling and the type waiver

**Maintainer:** Carlos Rodrigues (`pikax`), the designated maintainer.
**Date:** 2026-08-17.
**Scope:** given in answer to the AT-2 disposition question, but stated as a GENERAL rule and
recorded as one. It binds every remaining block, not this one only.

## The act, verbatim

> I don't want the compile error if the type is, unless there's a legit compilation error,
> verter should compile/build and return, if our tests shows an issue we should fix it as a
> bug, we don't ship something we know that has bugs, but for types I waive that rule, I'll
> personally fix the types after the plan is done, for now all the bugs found should be added
> tests and make them ignored to be fixed in the future

## What it decides

1. **No error path for a type problem.** Verter compiles and RETURNS. Only a genuine compilation
   error produces an error. A wrong or unresolvable TYPE never becomes a compile error, a refusal,
   or any other production failure path. This extends the earlier product ruling recorded in
   [`maintainer-product-ruling-no-error-on-bad-output.md`](maintainer-product-ruling-no-error-on-bad-output.md)
   onto the type surface.
2. **A test-discovered issue is a BUG**, fixed in the owning production code — never wrapped in a
   guard, tracker, refusal, or allowlist that production consumes.
3. **Types are WAIVED from rule 2** for the duration of the program; the maintainer fixes them
   personally afterwards. No block opens type-correctness work.
4. **Interim handling: a bug found during the program is captured as an ADDED TEST, marked
   `#[ignore]`d, with the fix deferred to a named owner.** Find, characterize, dispatch. "We do not
   ship known bugs" binds at RELEASE, not at each intermediate landing.

Two consequences follow that bear directly on this block:

- A finding that is NOT a demonstrated, reproduced defect must NOT carry a required-RED target. A
  target that would fail only because of some other, unrelated ratified gap is a stub, not evidence.
- No block may answer any finding — type-related or otherwise — with a production guard, typed
  refusal, withhold path, retraction, or runtime tracking artifact.

## How it was applied to AT-2, and by whom

The verbatim act does not name AT-2. Its application to that row is the PROGRAM ORCHESTRATOR's
reading of the general rule, taken rather than re-asking the maintainer, and it is recorded as such
here so the provenance is not overstated.

That reading is not a novel construction. The ORCHESTRATOR'S DIRECTION matches, clause for clause,
the act [`at2-deviation-memo.md`](at2-deviation-memo.md) had already asked for on the recommendation
of an independent unprimed disposition consult
([`at2-disposition-ruling.md`](at2-disposition-ruling.md)) — reject the ratified claim, reclassify as
a latent construction hazard with reachability unproven, retain the DEFER to `BA0`, and drop the
requirement that a Svelte-refusal atomicity target be RED. Rule 4 and its anti-stub consequence
decide each of those four points directly. What the MAINTAINER supplied is the general rule; the
clause-for-clause match is between the memo and the direction, not between the memo and the
maintainer's own words.

**A review seat disputes this chain.** Codex, reviewing this delta, holds that because the verbatim
act does not name AT-2 and the earlier independent consult said no track-level actor may amend a
ratified row, the amendment is not authorized and should be reverted pending an explicit maintainer
act naming AT-2. That objection is recorded here and in
[`dispositions.md`](dispositions.md) rather than argued away. If it is upheld, the `AT-2` row and
the `BA0` obligation lines revert to their ratified text and BF3's procedure item 6 returns to
`NOT-EVIDENCED`.

The bytes that act touches, and that this block therefore changed under it, are exactly the two the
memo named: the `AT-2` row in [`dispositions.md`](dispositions.md), and
[`../../charters/BA0.md`](../../charters/BA0.md)'s AT-2 obligation lines. Nothing else in the
ratified findings table was touched.

**If the maintainer intended something narrower for AT-2 specifically, this file and those two
edits are what to correct.**
