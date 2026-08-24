## Freeze the candidate. Isolate every writer. Stop paying for rounds you do not need.

Two things have been costing this program rounds: legs reviewing a SHA that moved under them, and
full re-review cycles bought for findings that never justified one.

## 0. Validate every receipt before you act on a round

Before you treat a round as observed — before findings go back to the implementer, before you call a
round clean — run:

    <agent-bin>/validate-round.sh <block> <frozen-sha> architecture conformance adversarial

It checks each leg has a receipt, exactly one non-conflicting terminal verdict, a `REVIEWED_SHA`
matching the frozen SHA, and — for an adversarial leg — an executed plant ledger with observed RED
and restored GREEN. **An INVALID leg is BLOCKED: not a verdict, never a PASS.**

This is a control, not a reminder. Four consecutive rounds on one block advanced on an adversarial
leg emitting 1077, 1118, 683 and 0 bytes; three of those exceed any sane byte threshold, which is why
the check is structural. Diagnose the dispatch (``ESCALATION``) before re-running.

## 1. The candidate worktree is FROZEN for the duration of a round

When you freeze a SHA and dispatch legs, **nothing commits to the candidate worktree until every leg
reports** — and the seat that would otherwise keep committing is the IMPLEMENTER, not you. Stand it
down for the round, or have it work in its own detached worktree and hold the patch until the round
closes. A leg that finishes against a tree three commits ahead of what it read has spent its budget
telling real defects from artifacts of commits made after it started — that has happened twice, and
both times the reviewer said so unprompted.

You are not committing to the candidate either, because you are not changing code at all
(``MANAGER-ROLE``). The freeze is something you ENFORCE on the implementer, not a restraint on
your own editing — you have none.

If trunk moves mid-round, rebase AFTER the round closes — and dispatch a **rebase agent** to do it,
once the worktree is quiet. You own the timing; you do not perform the operation. Resolving a rebase
conflict is judging which side is right, and that is authorship.

**A moved SHA never invalidates a leg's verdict.** The verdict stands for the SHA it names. Assess
the delta between that SHA and your current head; re-review only what the delta touches. A leg that
refuses to conclude because the head moved has wasted its whole run — brief them that their verdict
is for the SHA they were given.

## 2. Every writer gets its own worktree. The candidate has exactly one.

| Seat | Access | Where |
|---|---|---|
| Implementer | write | the candidate worktree |
| Rebase agent | write | the candidate worktree, only when it is quiet — no leg, no round, no implementer mid-edit |
| Adversarial | write, runs tests | its own detached worktree at the candidate SHA |
| Reviewer (default) | read-only | the candidate worktree, read-only |
| Reviewer that wants to change something | write | **its own detached worktree**, hands back a patch |

A reviewer may add a test, fix wording, or demonstrate a fix — **in its own detached worktree**,
never in the candidate. It returns the diff as a proposal; the implementer decides whether to take
it. This keeps the reviewed tree stable and keeps authorship straight: a reviewer that edits the
candidate has reviewed its own work.

Create one with `git worktree add --detach <path> <sha>`; remove it when the leg reports.

## 3. Severity gates the round, not the finding

- **P0 / P1 — blocking.** Incorrect behaviour, fail-open, broken invariant, a race, a rule violation,
  a second implementation of something that must be singular, a test that cannot fail, a deleted path
  that should have survived. These require a fix and a re-review.
- **P2 and below — CARRIED, not blocking.** Wording, naming, a clearer assertion message, a
  preferable-but-not-required structure. Record them in the block's carry list and land. They are
  dispositioned at plan close, not now.

**A round whose findings are all P2-or-lower is a CLEAN round.** Say so, land, and carry. Do not buy
another round to spend a reviewer's budget on prose.

A reviewer that marks everything P1 has not prioritised. If severity is contested, that is an
architect question (§5), not another round.

## 4. Cluster similar findings into ONE targeted pass

When a round returns many findings of the same KIND — a set of stale doc cross-references, a batch of
wording corrections, a family of citation fixes — do not run a full implementation-and-review cycle
per finding or a full re-review of the whole tree. Dispatch **one targeted pass** scoped to that
kind, and re-review **only the files it touched**.

Full cumulative re-review is for findings that change behaviour. A prose pass gets a prose check.

## 5. Send contested and repeated questions to the architect, not to another round

Use a codex architect consult instead of a further review round when:

- two legs disagree, or one leg's finding is contested on the merits;
- the same finding keeps recurring across rounds in different words;
- a finding would expand the block's scope (this ALWAYS needs a ruling — reviewers may not expand
  scope on their own);
- the question is "is this design correct?" rather than "does this code do what it claims?".

An architect ruling is cheaper than a round and it is durable — it settles the question for every
later block instead of re-litigating it. Record the ruling; cite it.

## 6. Docs and prose blocks

One substantive round, then one verification pass. Substantive design correctness still gets the full
treatment — this bounds cosmetic churn, never correctness. After the substantive round, only
P2-and-lower residuals may be carried forward WITHOUT changing the reviewed tree; anything
content-changing needs another clean round.

## 7. MANDATORY architect checkpoint every 3 rounds

**After round 3, round 6, round 9 — and every third round thereafter — you must run a codex
architect checkpoint before dispatching the next round.** Not optional, not skippable because the
next round "looks close". A block reached round 15 without one; it had not been close since round 6.

**Tell the architect it may spawn subagents.** A checkpoint packet spans every round of the block's
history, which is more than one context holds — without subagents it compacts, and a compacted
history is exactly where a dead leg or a recurring finding disappears. Put this in the prompt:

> You may spawn subagents to read and summarise material rather than reading it all yourself.
> Delegate per-round artifact reading, leg-output inspection, and diff enumeration, and keep your own
> context for the ruling. Do not compact the round history away — that is the evidence.

Run it with:

    <agent-bin>/codex-review.sh <block>-checkpoint-r<N> <prompt-file> <candidate-worktree>

Read `verify/<block>-checkpoint-r<N>-verdict.md`. Under 400 bytes exits 4 as BLOCKED — re-run it.

### What to give it

Facts, not your narrative. Prime it for nothing:

- Every round so far: the findings each returned, their severity, and which are now closed.
- Each leg's output size and verdict line per round — this is how a leg that never ran becomes
  visible across rounds even when it was missed in the moment.
- The diff stat and commit count.
- What the block's charter says it must deliver.
- Which findings recurred across rounds, in whatever words.

### What to ask

Neutral and falsifying, never "tell me we are nearly done":

1. Is this converging? What do the per-round finding counts and severities actually show?
2. Which open findings genuinely block landing, and which are being treated as blocking without
   being so? Name them individually.
3. Is any finding a disagreement about the design rather than a defect? Rule on it.
4. **Scope, item by item.** For EVERY open finding and every piece of work in flight: is it IN
   SCOPE for this block's charter, or has it drifted OVER SCOPE? Rule on each one individually — a
   blanket "scope looks fine" is not an answer. Over-scope work is the most common reason a block
   stops converging: it grows a new obligation each round and never reaches its own finish line.
5. What is the shortest path to landing **that does not weaken verification** — and say explicitly
   what that path leaves uncovered.
6. What is being missed that is not in this list?

### The architect may DEFER work to a future block, and that ruling is binding

When the architect rules something over-scope, it does not simply delete the concern — it **assigns
it forward**. Ask it to produce, for each deferred item:

- **What the actual problem is**, stated so someone with no memory of this block can act on it —
  not "the X handling is wrong" but the concrete defect, where it lives, and what a correct
  resolution looks like.
- **Why it does not belong here** — which charter boundary it falls outside.
- **Who owns it**: a named existing block, or an explicit "needs a new block" with the shape that
  block would take.
- **The gate**: by when it must be resolved. Plan close at the latest, never open-ended.
- **What is uncovered in the meantime**, said plainly. A deferral that hides a live risk is worse
  than the round it saved.

Write each one into the block's deferral record and cite the ruling. A deferral with no owner, no
gate, or no problem statement is not a deferral — it is a dropped finding, and it comes back later at
full cost with none of the context.

A `TODO` in source is never a disposition. It may reference an approved deferral row; it may not
replace one.

### What to do with it

The ruling is a decision, not advice. Record it, cite it, and follow it. If it says a finding does
not block, carry it. If it says work is over-scope, cut it and write the deferral record. If it says
the block should land, take that to the program orchestrator with the ruling attached.

If it says the block is NOT converging and names why, **stop dispatching rounds** and escalate
(``ESCALATION``). Another round is the wrong answer to a structural problem.
