# The claim sweep — its universe, and why that is the universe

This block's recurring defect has one shape: **an asserted scope wider than the scope actually
examined.** It appeared in prose claims, in a ratified act's wording, and finally inside the gate that
certifies the rest. This record exists because the same shape appeared one level further out, in the
*sweep for that shape*, and nothing in a sweep's own report reveals it.

**A complete sweep of the wrong universe reads exactly like a complete sweep.** Every count is honest,
every row is checked, the receipt reconciles — and the answer is still wrong, because the question was
asked of a subset. The universe is the one part a sweep's report never checks. So it is stated here,
before the numbers, and defended.

## The selection criterion

**The block is answerable for every artifact in the folder it owns, plus its own summary — regardless of
whether the candidate under review modifies that artifact.**

The criterion is deliberately NOT "the files this change touches". That was the earlier criterion and it
was wrong for a reason worth keeping: acceptance asks whether this block's evidence is true, not whether
its recent edits are true. A false claim in a file nobody edited this week is exactly as false, and it is
exactly as much this block's to answer for. Scoping a sweep by `git diff` measures authorship recency,
which is not the property under test.

Two consequences follow, and both were previously missed:

1. **Files the candidate does not modify are in.** Nineteen of the forty-nine artifacts below were never
   touched by this candidate. Two of them carried real defects that three review rounds and two sweeps
   never saw, because no sweep had ever looked at them.
2. **The executable probes are in, for what they claim about themselves.** A probe's comments, assertion
   messages and printed output make claims — about what it exercises, what it establishes, how wide its
   domain is — and those claims are evidence in exactly the way a paragraph is. A probe whose comment
   says it covers fourteen cases while it executes nine is the same defect as a document that says so.
   The sweep reads a code file's claim surface (comments, assertion messages, prose string literals),
   never its control flow.

## The universe

Every file under `evidence/TCM0/`, plus `../TCM0-summary.md`, which sits one directory above and has been
missed by a folder-scoped check before — the same "scoped to where most of them are" error this record is
about. Nothing under `charters/` (another owner's, corrected by specification and handed up) and nothing
under `docs/arch/architecture-lock/` (program-owned; this branch carries no delta to it).

Review records under `reviews/` are IN the universe. They are retained because a row inside one may
either NARRATE what a reviewer once said — which is not this block asserting it — or state something as
fact, which is. That distinction is made per row, not by excluding the file: excluding a file is a
universe decision and this record exists to stop those being made casually.

## What a row is, and what makes one defective

A row is a span carrying a universal or exhaustive quantifier, a count, a residue or open-item
statement, or an assertion that something is settled. A row is DEFECTIVE if and only if it is one of:

1. **a universal asserted from a sample** — the claim ranges over a class, the evidence covers some
   members;
2. **a stale residue** — it names as open or underived something this same evidence set now establishes;
3. **a settlement asserted with nothing cited** — it states that something is ratified, lifted,
   superseded or cancelled without naming the instrument.

A merely emphatic row, and a universal the evidence genuinely establishes, are clean. **A clean row is a
result.** Most rows are clean, and a sweep that finds otherwise is miscalibrated rather than thorough.
