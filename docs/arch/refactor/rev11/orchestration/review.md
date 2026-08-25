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

## Convergence

**Default maximum: two substantive fix cycles per slice.** Not converging means stop and rescope,
replace a failing agent, or request an architecture decision — never buy another round.

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

## Model routing

Capabilities, not model names. Keep assignments here rather than repeated across prompts.

| Lane | Default | Note |
|---|---|---|
| correctness, bug detection, architecture conformance | Codex | prime with the exact architecture documents; explicitly forbid severity inflation and scope expansion |
| adversarial challenge of assumptions, spec, simplicity, scope | Opus | not the sole bug detector |
| broad exploration, inexpensive second opinion | Grok, optional | not a sole correctness gate |

Use max-effort settings only where judgment or risk justifies them. Routine coordination, status
handling and mechanical work use cheaper settings.
