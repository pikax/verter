# Orchestration doctrine

How Revision 11 work is implemented, reviewed and landed. These rules are **normative for anyone
orchestrating program work** — they are inlined into every manager brief, and a block that ignores
them produces evidence nobody can rely on.

They were not designed up front. Every rule here replaces a specific, observed failure, and the
failure is stated alongside the rule so a future reader can judge whether it still applies.

## The tiers

| Tier | Owns | Writes code |
|---|---|---|
| Program orchestrator | scope, sequencing, landing | no |
| Block manager | one block: dispatch, freeze, collect, re-review to clean | no |
| Implementer | the code and its tests. **Reused** across every fix round | yes, in the candidate worktree |
| Review legs | independent analysis, one lens each. **Never reused** | no |
| Adversarial leg | plants defects and proves the tests catch them | yes, in its own detached worktree |
| Landing manager | the pre-merge checklist; may refuse | no |
| Architect (codex) | rulings on design, scope and convergence | no |

The asymmetry is deliberate: **reuse the implementer, never a reviewer.** The implementer holds why
the code is shaped as it is, so a fresh fix agent discards that. A reviewer that already passed a
tree is invested in that verdict, and one that saw the prior round anchors on its own findings —
independence is per-round, not per-block.

## The documents

| File | What it governs |
|---|---|
| [manager-role.md](manager-role.md) | what a manager does and does not do; seat staffing; dispatch |
| [round-discipline.md](round-discipline.md) | receipt validation, the frozen candidate, writer isolation, severity, the 3-round architect checkpoint |
| [regression-prevention.md](regression-prevention.md) | the rail every closed finding must carry |
| [escalation.md](escalation.md) | diagnosing a leg that did not run; when and how to escalate |
| [adversarial-leg.md](adversarial-leg.md) | how the adversarial leg is briefed and what its evidence must show |
| [landing-checks.md](landing-checks.md) | the pre-merge checklist, refusable |

## The four rules that matter most

**1. A leg that produced no real output did not run.** Not a verdict, never a PASS. Four consecutive
rounds on one block advanced on an adversarial leg emitting 1077, 1118, 683 and 0 bytes while the
other legs produced megabytes each. Nobody checked. This is now a machine check, not a reminder —
see *Receipt validation* below.

**2. Freeze the candidate for the round.** Nothing commits to it between dispatching legs and
collecting them. Two reviewers have burned significant budget separating real defects from artifacts
of commits made after they started reading, and both reported it unprompted.

**3. A closed finding carries a rail.** Before closing, answer: *what edit would reintroduce this,
and does it still compile?* If it compiles, an instance was fixed, not a class. See
[regression-prevention.md](regression-prevention.md) for the tier ladder — and note that a test which
only exercises the permitted route is not a rail. One such test asserted a capability gate that an
adversary then showed was a single visibility edit from disappearing.

**4. Severity gates the round, not the finding.** P0/P1 block. **P2 and below are carried**, and a
round whose findings are all P2-or-lower is a CLEAN round: land, carry, disposition at plan close.

## Receipt validation

Prose said "no verdict is not a pass" long before anything enforced it, and it was skipped for four
rounds. Before a round is acted on, every leg's receipt is machine-checked for:

- a receipt file that exists;
- exactly one terminal verdict, with **conflicting** verdicts rejected (identical repeats are fine —
  agents echo their final message);
- a `REVIEWED_SHA` matching the round's frozen SHA;
- for an adversarial leg, an executed plant ledger with **observed RED and restored GREEN**.

An INVALID leg is BLOCKED — diagnose the dispatch before re-running
([escalation.md](escalation.md)), because a silently retried dispatch bug recurs on every later
block.

The check is **structural, not size-based**: three of the four missed legs above exceeded any
reasonable byte threshold while containing no conclusion at all.

## Known-good practice this doctrine does not yet fix

An architecture consult on this process (2026-08-24) found the review loop does not converge on its
own: fresh full-panel review after every fix is a randomised defect search that can surface a
different finding each round while the code genuinely improves. Its recommended structure — one full
three-lens **discovery** review, findings closed through **delta** reviews by the reviewer that
raised them, then one fresh full **acceptance** review on the final frozen SHA — is not yet reflected
in these documents. Until it is, the 3-round architect checkpoint in
[round-discipline.md](round-discipline.md) is the circuit breaker.

The same consult found that 40–120-commit candidates are failed decomposition, and that review cost
grows faster than linearly with diff size.

## Placeholders

`<repo>` is the repository checkout, `<worktree-root>/verter-<block>` a block worktree,
`<agent-rules>` the orchestration rule directory inlined into briefs, and `<agent-bin>` the
orchestration helper scripts. These live outside the repository because they are operator
environment, not project source; this directory is the durable copy of the doctrine itself.
