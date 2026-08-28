# BF2 second-reopen ruling record

**Ruling reference:** `REV11-BF2-REOPEN2-001` (root cause + approach), followed by
`REV11-BF2-REOPEN2-002` (round-3 review arbitration). Both Codex Sol xhigh, read-only
investigation, dispatched by the program orchestrator after BF2's second invalidation
at candidate `58d42a65d` (ledger tip `79ce71054`).

## REV11-BF2-REOPEN2-001 — root cause + approach (authorizes rounds 2 onward)

**Root cause:** the hardened per-criterion review instruction improved evidence
citation but did not require evidence *adequacy* — it explicitly allowed "exact test
name + pass/fail" as evidence, so reviewers proved tests ran without checking whether
the cited assertion was coextensive with the full contract (quantifier words like
"every"/"all"/"only"/"exact"/"offline"/"atomic" were accepted from narrow/sampled
tests). Compounding failures: test names accepted as category proofs without
per-category accuracy; aggregate charter criteria not atomized consistently across
the three reports (12 vs 11 vs mandate-specific groupings), letting one covered
subpart mask two uncovered ones; review scope anchored to the fix diff instead of
the whole candidate; a README's disclaimer was allowed to override a ratified
charter's owned scope; the three review mandates were correlated rather than
complementary (same checklist/test vocabulary/known-fix focus); and no durable
three-mandate PASS existed for one unchanged candidate identity (reports drifted
across `a7f1eb5d7` vs the accepted `58d42a65d`).

**Approach ruled: (b)** — fix all 10 failing criteria fully in BF2 itself; none may
be deferred to B2/B4/BV1/BS1. BF2 owns the oracle/validator those blocks will be
judged BY — deferring a broken oracle downstream would poison every consumer's
evidence. Existing legitimate downstream manifest-row allocations (BV1/BS1/B2
splits) were unaffected and correct as-is.

**Review-method correction prescribed** (superseded `.agent-run/BF2-REVIEW2-COMMON.md`,
applied to every review from round 2 onward): canonical numbered acceptance matrix
(no combined rows); 10 required fields per row (authority quote, complete domain,
every implementing path, exact test inputs/assertions, contract fields actually
asserted, positive witness, negative witness, reviewer-authored counterexample/fault
injection, exact command+skip-count+digest, verdict); a green test name never
sufficient alone; adversarial reviewer must author fresh black-box probes for
defect-prone families and perform white-box kill tests per mechanism; no
PASS-with-caveat (missing evidence is NOT_PROVEN, violated behavior is
BLOCKING_FINDINGS); full fresh review on the final candidate, no partial-diff-only
review, no approval carried forward across a changed candidate identity.

Full ruling text: `.agent-run/bf2-second-reopen-consult-output.log` (program
orchestrator's session transcript, not committed verbatim — this file is the durable
record of its substance and disposition).

## REV11-BF2-REOPEN2-002 — round-3 review arbitration

Round 3 (candidate `00451700f`) produced a genuine 3-way reviewer disagreement:
conformance 2/16 rows blocking, adversarial 4/16, architecture 10/16 (including a
Row 15/manifest-completion claim that directly contradicted this same ruling
authority's own REOPEN2-001 text). Independently re-verified each disputed claim
with its own kill-mutation probes rather than voting. Verdict: neither reviewer was
fully right — result was 9 real items (not 2/16 or 10/16), with explicit dismissals
justified per item (Row 15 remained PASS; the architecture report's own text
contradicted the round summary's claim of blocking it). Also issued 3 review-method
corrections: freeze row interpretations/downstream-ownership boundaries alongside
the 16 labels; use a common kill-ledger vocabulary distinguishing mechanism logic vs
production callsite vs atomic-commit primitive vs reader schedule; serialize any
performance-cell run under an exclusive machine lease.

Full ruling text: `.agent-run/bf2-round3-arbitration-output.log` (program
orchestrator's session transcript; this file is the durable record).

## Rounds authorized under these rulings

`00451700f` (all 10 criteria) → `f878d9cdd` (arbitrated pass-4, REV11-BF2-REOPEN2-002
disposition) → `a3753c87c` (round-4-convergent) → `19cce22c8` (round-5-convergent) →
`41929246e` (round-6-convergent, landed). Each subsequent round was ordinary
convergent narrowing under the review method these two rulings established — no
further arbitration was required after round 3.
