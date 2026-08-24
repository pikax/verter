## Every fix lands with a rail that stops it coming back

A fix that only corrects today's instance is half a fix. Before a finding is closed, answer one
question in writing:

> **What edit would reintroduce this defect, and does that edit still compile?**

If it still compiles, you have fixed an instance, not a class.

## The ladder — climb as high as you can, and say where you stopped

**Tier 1 — unrepresentable.** The defect cannot be written. Make the field private and the only
mutator do the whole job; give the value a newtype whose constructor is the one legal path; use
type-state so the wrong order is not a type; seal the trait so no outside impl exists; route every
caller through a closed inherent gateway rather than a trait method anyone can add. This is the only
tier that survives a determined future contributor who has not read the plan.

**Tier 2 — non-compiling.** The defect is writable but rejected: `E0603` privacy, `E0616` private
field, `E0624` private method, a missing required argument, an exhaustive `match` over a sealed enum
that stops compiling when a variant appears.

**Tier 3 — a discriminating test.** Only when 1 and 2 are genuinely unavailable. It must be
**plant-proven**: apply the reintroducing edit, watch the test go RED, revert, watch it go GREEN. A
test whose body only exercises the permitted route is NOT tier 3 — it is decoration, and it is what
`live_generation_bump_requires_the_publication_capability` was caught doing.

**Tier 4 — documentation.** Prevents nothing. Legitimate only when 1–3 are impossible, and then it
is recorded as **explicitly accepted, review-governed residue** — never described as prevention.

**Never a name-keyed scanner.** `CLAUDE.md`'s forward-only rule forbids landing a guard that greps
the source tree for a spelled identifier, path, or token — `syn`/AST scanning included. Existing
scanners are grandfathered by temporal status; new ones are not. Climb the ladder or accept the
residue honestly.

## Two worked examples from this program

**A lost wake.** `promote_after_isolated_edit` mutates a receipt to the exact key a waiter is
watching but never notifies it. Tier 3 would be a test that promotes and asserts the waiter woke.
**Tier 1 is available and better:** make the receipt field private and expose one method that
mutates *and* notifies, so a future caller that forgets to notify cannot be written. Ask which the
fix chose, and why, if it chose lower.

**A capability gate.** `bump_generation` requires a `&SourcePublication` that cannot be constructed
outside the module. The gate is genuinely tier 1 — but the TEST asserting it stays green if someone
adds a second bare-bump route beside it. Exhaustiveness is not provable by a test that only walks the
allowed path; it needs a **closed inherent gateway** the new route would have to go through, or an
`E0603` that the new route trips. Otherwise the honest record is: the gate holds today, and nothing
stops a sibling being added tomorrow.

## What a fix report must state

Per finding: the tier reached, the mechanism, and — for tier 3 — the plant, with RED and GREEN both
observed. For anything landing at tier 4, say plainly that reintroduction is uncovered and who
accepted that.

"Fixed" without a tier is not a closed finding.
