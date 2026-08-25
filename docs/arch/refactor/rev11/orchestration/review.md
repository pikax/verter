# Review — three phases, not repeated panels

## 1. Discovery

**One** independent review against a frozen slice. Lanes are selected by risk — correctness;
architecture conformance; adversarial challenge of specification, scope and simplicity; a specialist
lane only where concurrency, security or performance genuinely applies.

Reviewers are blind to each other's findings. A low-risk candidate does not get a full panel.

## 2. Closure

The manager verifies findings before they block. **A finding blocks only when all four hold:**

- supported by concrete evidence or a realistic reproduction;
- introduced or exposed by this candidate;
- within the block's responsibility;
- material to correctness, safety, architecture or acceptance.

Confirmed **P0/P1** findings go to the implementer as **one deduplicated minimal fix packet**;
P2/P3 are carried, never forwarded. The reviewer
that raised a finding may be resumed to verify that finding and its delta — targeted verification,
not a fresh review. Do not rerun the panel after every fix.

## 3. Acceptance

After confirmed findings are closed, **one fresh review of the final frozen candidate** by an agent
that has not seen it. It receives the charter, architecture and final diff — not prior conclusions —
and looks for missed defects and integration problems. A high-risk candidate may get one additional
targeted lane.

## Acceptance is not reopened later — two checks that prevent it

Reopening an accepted block is expensive and always has the same two causes.

**A criterion that names a document requires a verified correspondence to that document.** Not "this
bullet covers it" — an actual comparison, clause by clause, recorded. A block once satisfied
"distinct identity types from the architecture document" with types that collapsed four of that
document's variants into one, losing two identity distinctions. Every review passed, the criterion
looked met, and the divergence surfaced only when a later block became the first consumer and found
the contract's invariant inexpressible. A coverage mapping would not have caught it: the mapping was
correct. What was missing was anyone reading the two side by side. Divergence found this way is
recorded as a deviation for ratification, never resolved silently in either direction.

**An artifact with no consumer has no evidence, and acceptance says so.** The same type had zero
production callers, so no test could fail and none did. Compilation proved only that it was
well-formed. Where a block delivers something nothing yet uses, its acceptance states that plainly
rather than resting on a green gate that never exercised it — and names what would exercise it.

## Sub-blocks — decompose so a review can finish

**A block too large to review in one pass is decomposed into sub-blocks before implementation, by an
architecture consult.** The consult owns the decomposition: each sub-block digestible, independently
reviewable, and bounded so a reviewer can hold all of it at once. A block orchestrator does not
self-decompose — the point is a boundary drawn by someone who is not implementing it.

**Up to three review rounds per sub-block.** Then it is done, or it is escalated — not carried into
the next sub-block.

**When every sub-block is complete, the manager runs one conformance and one adversarial pass over
the whole block.** Sub-block reviews establish each part; that final pair establishes that the parts
compose, and it is where cross-sub-block defects are caught. Neither pass substitutes for the other,
and neither substitutes for the sub-block reviews.

## When rounds are the wrong instrument

**A complete list closes its class and says nothing about any other.** Eighty-three enumerated
restatements were repaired to two, and two internal contradictions then surfaced for the first time in
twenty-three rounds — they had never been enumerated, so no amount of repairing against that list
could reach them. That sharpens the enumeration thesis rather than weakening it: enumerate the class
you intend to close, and do not read its closure as coverage of another.

**Iterative repair on a large interlocking document can introduce defects as fast as it removes
them.** Where a count plateaus and new findings trace to previous rounds' own edits, the method is the
suspect, not the effort. Pre-register the disconfirming result before the next round runs — and treat
reaching it as a question about the method rather than a failure of the block, the same shape as
stopping rounds that cannot enumerate.

**Never judge convergence on count.** One block's finding counts across nine rounds ran 5, 5, 6, 4,
5, 6, 7, 6, 6 — flat, and by that measure nine rounds of nothing. Underneath, its blockers went from
six against legacy text to two against text written in the previous round, and its dominant class from
eighty-three occurrences to two. A convergence judgement on count would have been correct by its own
measure and wrong in fact.

**Read where findings land, not only how many.** Findings against a document's newest additions are
convergence on a moving front; findings against legacy text mean the population is still unenumerated.
A round falling from a dominant class to two of six, with both blockers against work written since the
previous round, says the repair held — which the count alone does not.


**A population no one has enumerated cannot be cleared by sampling it.** One block's surface held 47
non-owner restatements; a review round finds one to six. That is roughly twenty rounds, and twenty-one
had run without converging — not for want of reviewer or repair quality, but because rounds were the
wrong instrument at any quality. Enumerate the population, then repair the class.

**An inventory column is evidence, not a verdict.** Applied literally it manufactures findings —
most flagged identity bindings were correct by construction, since a plan must define a derived
identity before it exists. Discarding it loses the class. The defect was narrower than the column: a
derived identity used as the *pin* for a measurement or a source location.

**A receipt's count is a claim about what the producer found, never about what it delivered.** An
enumeration reported 477 rows and 47 findings and delivered 120 rows containing 9, with its prompt
already requiring it to declare truncation. Counting the artifact's own rows against its receipt is
what exposed it, so require both numbers — rows found and rows listed — and a truncation flag when
they differ. This is the truncation family again, and the first instance where the truncation was in
the producer rather than the reader.

**Treat a predicate miss as formatting before treating it as absence.** A receipt delivered as a
table rather than marker lines read as zero results. Require markers at column one.

## Ask a reviewer to defeat a gate, not to check it

**A reviewer asked to check a gate reads it; a reviewer asked to defeat it runs it.** Set the task as
construction: make this gate pass while something it exists to catch is broken. That found a totality
check asking whether each scope item had at least one row rather than whether that item's obligations
were present — ten of eleven rows could be deleted and the gate stayed admissible.

**A gate that cannot fail invalidates every result it ever produced.** Not only the current one: each
earlier pass was weaker than it was reported to be, so fixing it means re-running everything it
cleared. Half-fixing it is worse than leaving it alone, because the passes then split into two classes
nobody can tell apart.

**An instruction is evidence about what its sender believed when they wrote it, never about the
current state.** Refuse one that a later verdict has overtaken, and say which verdict overtook it.

## Review churn — stop the reviewer prospecting

**A reviewer may always find a new path.** But if two consecutive rounds surface previously-unexamined
paths, the surface is not being enumerated and further rounds will keep finding more.

**At that point, stop reviewing and dispatch a low-effort consult to enumerate the remaining paths.**
Its output is the surface; review resumes against it. Enumeration is cheaper than discovery repeated
per round, and it converts an unbounded sequence of rounds into a bounded one.

## Documents get two rounds

**A document gets at most two review rounds. Then it ratifies with its residue recorded, or it is
rescoped.** Only a contradiction or a false claim blocks ratification; a proof gap, a wide phrasing,
an underspecified field are recorded as open findings and the plan proceeds.

A plan is a means. A block's deliverable is the code, and a document polished past two rounds is
spending the block's budget on the wrong artifact — one plan took twenty-three ratification rounds and
eight repair rounds while the cutover it gated had not started. Iterative repair on a large
interlocking document introduces defects at roughly the rate it removes them, so the rounds do not
converge; they circulate.

## Convergence

**Default maximum: two substantive fix cycles per slice.** Not converging means stop and rescope,
replace a failing agent, or request an architecture decision — never buy another round.

**Every third round, an architecture consult joins the next cycle.** Not to review the candidate
again — to ask why three rounds have not closed it. Three failed cycles on one slice is evidence
about the approach, not about the fixes: the same defect class keeps reappearing in new instances,
or the mechanism cannot express what its criterion requires, or the work sits with an owner who
cannot finish it. A fix cycle cannot see any of those, because it is looking at the finding.

The consult is given the round history and the findings, and asked what is producing them. Its answer
shapes the next cycle rather than being another verdict on the current one.

**The cap is a signal, never a reason to land or supersede a block that can be made correct.** It
exists because repeating a round that is not working wastes a lane; it does not exist to force a
block out of the program. Where the work is genuinely closable and the only obstacle is a round
count — the block's own or this one — the count is lifted and the block continues until it is
correct. Superseding a block that could have finished, or landing one that is not finished, are both
worse outcomes than another round. Rescope when the approach is wrong; lift the limit when only the
budget is.

## Reviewer calibration

Every reviewer dispatch says this, because reviewers otherwise inflate:

- The objective is finding real defects, **not maximising findings. Reporting none is acceptable.**
- Severity is impact × likelihood — not confidence, and not tone.
- A potential future concern is not a defect without a current requirement or a realistic failure
  path. An architectural preference is not an architecture violation.
- Pre-existing and out-of-scope issues are recorded separately and never silently expand the block.
- Minor issues and suggestions do not block by default.
- No praise, no generic summary, no unrelated recommendations.
- Recommend the **smallest sufficient fix**, never a redesign.

Each potentially blocking finding carries: location; the violated requirement or invariant; a
concrete failure path; evidence or reproduction; whether this change introduced it; whether it is in
scope; calibrated severity; the minimal sufficient fix.

**A blocking severity requires direct reproduction, incontrovertible evidence, or independent
verification before an implementer is interrupted.**

## What a review does not cover

**A review answers the question it was asked, not the document it was pointed at.** An artifact can
accumulate passing rounds while a false sentence inside it is never in any lane's frame. A claim in
an evidence document survived three rounds — a categorical assertion that a count-only canary could
not catch a regression, contradicted by that block's own measured delta — because every lane was
scoped to the tests and none to the prose.

So prose claims are unreviewed however many rounds the artifact survived, unless a lane was scoped to
the prose explicitly. Where a document's assertions are load-bearing, scope a lane to them and say so.

**A PASS with zero blockers is not readiness while findings are open.** Dispositioning carried
findings is new work, not a further fix cycle, and a candidate is not ready until each is adopted,
deferred with its owner and gate named, or rejected with evidence.

## Result contract

Every reviewer ends its output with exactly this block, and writes the two marker lines nowhere else:

    ===VERTER-RECEIPT-BEGIN===
    LANE: <this reviewer's lane>
    RESULT: <PASS|FAIL>
    REVIEWED: <the sha reviewed>
    FINDINGS: <n, or the word none>
    FINDING <id> | <P0|P1|P2|P3> | <file>:<line> | <one-line summary>
    ===VERTER-RECEIPT-END===

Show that block literally in a prompt; never describe it in prose.

`PASS` means no P0/P1. `FAIL` names what blocks. P2 and below are recorded and carried.

**Severity is defined, not felt.** P0: data loss, a security or fail-open hole, or demonstrated
incorrect behaviour on an input a user would actually hit — demonstrated, not reachable in
principle. P1: a broken named invariant, a demonstrated race, a second implementation of something
required to be singular, a test that cannot fail. A rule violation is P1 only with the rule named,
the `file:line` and a concrete consequence; a violated convention or preference is P2. P2: a real
but non-blocking defect — naming, an unclear assertion, a preferable structure. P3: a nit. **P0 and
P1 block; P2 and P3 are carried and dispositioned later.**

**Pre-existing and out-of-scope issues go in the reviewer's body, never in a `FINDING` row.** A row is
a claim on this block's fix budget, so putting an inherited problem there is how a review silently
expands scope.

**A receipt relocated so that it validates must be proven byte-identical to the original.** Re-filing
a misnamed or misplaced result is the block owner's act and is legitimate; editing it until it passes
is not, and the two have the same visible outcome — the mechanical check sees a valid receipt either
way. Compare digests before and after the move. For the same reason the landing agent never relocates
a receipt itself: curating the evidence into the place that makes it pass is precisely what that check
exists to catch.

**`ALL SOUND` is a verdict about form, not outcome.** A `FAIL` is a sound result. A readiness claim
that reads structural soundness as passing has not read its own evidence.

Verify with `check-results.mjs`, which binds each result to its lane and to the reviewed sha, and
rejects competing or stale files. The results directory is named for the frozen sha, so a leftover
file from an earlier freeze cannot answer for a lane that produced nothing. An absent, truncated or
inconclusive result is BLOCKED, never a pass.

## Shaping a consult

**Name the class, not the finding.** A prompt pointing at the row where a defect was found tests
whether that fix landed. A prompt stating the defect as a general check — an invariant narrower or
wider than the gate its document names — tests whether the class is gone, and only the second finds
instances nobody has seen. Give the seat the surface and the questions; keep the block's conclusions
out of it.

**Before carrying a citation forward, open it.** A cite that no reader can follow reads as a
transcription error rather than as the deletion it records, and a finding whose only carrier died
with a branch needs its live carrier named instead of its dead one reproduced.


**A seat that can only choose among the offered options will choose among them.** Give every consult
an explicit escape — if the information does not determine the answer, say so and say what would —
or a forced choice comes back looking like a finding.

**Supply the facts, not the inference.** Give the assignments and let the pattern be reachable;
naming it as a precedent tells the seat what to conclude. Never state a distinction as an argument
toward an answer when it can be stated as a fact about the subject, and never frame a choice as
binary when a third shape is available from the same facts.

## Model routing

Capabilities, not model names. Keep assignments here rather than repeated across prompts.

| Lane | Default | Note |
|---|---|---|
| correctness, bug detection, architecture conformance | Codex | prime with the exact architecture documents; explicitly forbid severity inflation and scope expansion |
| adversarial challenge of assumptions, spec, simplicity, scope | Opus | not the sole bug detector |
| broad exploration, inexpensive second opinion | Grok, optional | not a sole correctness gate |

Use max-effort settings only where judgment or risk justifies them. Routine coordination, status
handling and mechanical work use cheaper settings.
