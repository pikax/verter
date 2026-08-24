# Design notes — why these rules exist

**Never injected into an agent.** Kept so a future reader can judge whether a rule still applies, and
delete it when it does not. Only incidents that changed a decision are here.

## Review is three phases because panels do not converge

A fresh full panel after every fix is a randomised defect search: it surfaces a different finding each
round while the code genuinely improves. One block reached round 15 and had not been close since
round 6. An architecture consult also found review cost grows faster than linearly with diff size,
and that 40–120-commit candidates are failed decomposition.

## An absent result is BLOCKED, and the check is structural

Four consecutive rounds advanced on an adversarial leg emitting 1077, 1118, 683 and 0 bytes. Nobody
checked. Three of those four exceed any sane byte threshold, so a size heuristic would have passed
exactly the legs it was meant to catch.

## Results are delimited, and shown literally

An external tool echoes the whole prompt into its own trace, so a naive scan finds a template block
that came from the prompt rather than a conclusion. Two defences: take the last complete pair, and
reject a block whose field values are still specimens.

Those defences are what make it safe to show the format **literally**. A manager once described the
row in prose to dodge the echo — "the word FINDING, then an id, then the severity, each separated by
a vertical bar" — read it back as correct, and got `FINDING | TST-001 | P1 | …` from a real agent: an
extra leading bar, and a refused result.

A real extract also captures the final turn **twice**, so receipts and rows arrive doubled
byte-for-byte. Identical repeats are an artifact; conflicts are the defect. Rejecting duplicates
outright would reject every real result.

## The implementer is resumed; reviewers are not

The implementer holds why the code is shaped as it is. A reviewer that already passed a tree is
invested in that verdict, and one that saw the previous round anchors on its own findings. Reuse is
still not absolute — an agent that repeats mistakes or forgets scope is replaced.

## The candidate is frozen while a reviewer reads

Two reviewers burned significant budget separating real defects from artifacts of commits made after
they started reading, and both reported it unprompted.

## Reviewers are calibrated explicitly

Asking for "blocking findings" biases a reviewer toward defect-scanning, which is a different search
from criterion-by-criterion verification — and only the second catches a required check that was
never run. Two consecutive blocks returned clean three-way PASS verdicts later proven wrong: one
missed its own charter line, the other six skipped tests that were exactly the charter-required
discrimination proofs. Reviewers also inflate severity when nothing tells them not to.

## One writer per worktree

An adversarial reviewer once ran write-enabled inside the candidate, was killed mid-plant, and its
planted defect survived as the only dirty file — one step from being committed.

## A check that matches from its own source proves nothing

Four separate mechanisms here were written that way: a residue check reusing the pattern it verified,
a totality test iterating the map it validated, a specimen test sharing an oracle with a validity
test, and a proposal to compare a candidate against its own nominal claim. Each could not fail for
the case it existed to catch. A recurring authoring failure, not a coincidence — so the rule sits in
`delivery.md` where implementers and reviewers both see it.

The same failure in another guise: normalise absence to a single token before comparing two trees.
A missing blob has no value, so an `|| echo missing-here` fallback invents a different sentinel at
each call site and the sentinels get compared instead of the state — the comparison then fails
identically whether the tree changed or not. Two reviewers hit this on the same six deleted files
within an hour.

## Regression prevention is proportional

The earlier policy told every fix to climb to the strongest available rail — newtypes, type-state,
sealed traits — which added architecture for its own sake. The underlying question still applies: what
edit would reintroduce this, and does it still compile? A fix leaving the defect trivially rewritable
fixed an instance, not a class — but the answer need not be a type-level proof.

## Rebasing is continuous because drift corrupts the question

A branch left behind does not merely make its eventual merge expensive. A drift check compared
against the recorded sha it was checking and read green throughout; a landing rehearsal against a
stale pin reported conflicts that did not exist. Both produced confident false conclusions, which
costs more than a hard merge. The first is the trap above in another guise, which is why that
passage names a pinned baseline.

The exception is anything being compared or measured against the branch, because a rebase moves the
subject of the comparison: a frozen review candidate is one case, a failure triage comparing against
a pre-candidate tree is another, and four completed measurement sessions were lost to a commit
landing mid-run.

## Three causes, one mechanism

A live test mutation committed by `git add -A`; a killed plant left applied in the tree; a peer's
staged work swept into an unrelated commit on a shared branch. Different causes, one mechanism — a
staging command that captures the tree rather than a named set. So the check is mechanical: compare
the staged set against the intended paths before every commit.

## Why the package was cut

At 20 files and ~26,000 words it recreated the token cost and forgetting it exists to prevent:
doctrine inlined into every brief, rationale travelling with every rule, the same rule in a doctrine
file and a template restating it. Now: prompts carry the contract, reference documents carry the
rules, this file carries the reasons.

## ASSUMPTION — teammate depth (unverified)

**Assumed:** a block orchestrator running as a named teammate can spawn one manager subagent, and
that manager retains depth to spawn workers.

**Established:** ordinary subagent nesting works at least three levels down (manager → implementer or
reviewer → a delegated sweep), observed in real work. Levels 3 and 4 rest on that, not the assumption.

**Not established:** whether a teammate can spawn subagents at all, and whether the depth counter
resets per teammate. A probe was inconclusive — the child came back shaped like an ordinary subagent,
so teams likely never activated, and the deepest link was self-reported.

**If falsified:** team mode loses its lower levels for that block — run it on the supplied CLI
pool instead.

## Two runtime modes, neither default

An in-process-only rule was tried. It loses multi-account routing and visible named terminals, and
in-process teammates cannot run background subagents and do not survive `/resume` — while a CLI pool
puts liveness and cleanup on the operator. Neither hosting dominates, so both are explicit modes and
the operator picks.

## Roles removed

- **Rebase agent.** A clean rebase is mechanical tooling; a conflicted one is a code decision and
  goes to the implementer.
- **Landing manager.** Mechanical checks belong in tooling, semantic acceptance to a fresh reviewer.
  The role only relayed a checklist. **Its verification function survives** in `delivery.md`.
- **Recon.** Advisory output the implementer could discard; the block orchestrator already owns that
  context.
- **`acceptance.md`.** Merged into `reviewer.md` as a mode — it shared inputs, calibration, severity
  and receipt, differing only in what it reads and asks.
- **Review tiers, round ceilings, deferral and finding record forms, the doctrine renderer.** Ceremony
  around decisions now owned by a named role.

## Evaluation and rollout

Assess on several completed or representative blocks, small and large: known-defect recall; confirmed
findings versus false positives; scope expansion; patch size and unnecessary abstractions; relevant
versus unnecessary tests; RED/GREEN usefulness; tokens; wall time; review and fix cycles;
full-gate executions; post-acceptance reopenings — the strongest signal that acceptance is too weak.

A full old-versus-new agent evaluation is expensive. Run inexpensive static validation first: replay a
completed block's real review artifacts through `check-results.mjs`, and compare the new prompt set
against the findings that block actually produced — would the lanes have surfaced them, would closure
have confirmed them? Do not spawn a large evaluation team without authorisation, and do not optimise
for finding count or test count.
