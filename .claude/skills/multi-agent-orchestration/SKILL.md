---
name: multi-agent-orchestration
description: >-
  Methodology for driving a large multi-block plan, refactor, migration, or staged cutover end-to-end
  and autonomously by acting as a pure orchestrator that delegates to sub-agents and gates every block
  on independent review. Covers the orchestrator role (dispatch / decide / verify — never implement
  directly), the sub-agent cast (implementer, reviewer, fix-agent, extraction, diagnostic), the per-block
  workflow (brief → implement → verify git state → dual review → fix cycle → land), dual review with an
  independent reviewer plus a second-opinion model (codex), per-block fix cycles gated on a clean
  re-review, second-opinion consults for any architectural doubt, trust-but-verify, and
  STOP-and-escalate. Use this skill whenever you are handed a multi-phase plan, a handoff document, a
  staged refactor/migration/cutover, or any large autonomous effort spanning multiple blocks or PRs that
  needs sub-agent delegation — even if the user only says "orchestrate", "run this plan", "execute the
  handoff", "drive this to completion", or "continue the cutover". Also use it for a single substantial
  block that needs the implement → dual-review → fix-cycle → land loop. Do not improvise large-plan
  execution ad hoc — load this skill.
---

# Multi-Agent Orchestration

A methodology for driving a large plan — a multi-block refactor, migration, or staged cutover — to completion autonomously, when the work is too large for one agent in one context window to hold coherently.

The core move: split the work into **one orchestrator** and **many sub-agents**. The orchestrator never writes the code. It decomposes the plan into blocks, briefs a sub-agent per block, verifies what comes back, gates every block on independent review, escalates every real doubt to a second-opinion model, and manages the git / branch / worktree / state machinery. Sub-agents do the implementation, the reviewing, the fixing, the investigation — each a bounded job, each disposable.

## Why this shape

A single agent grinding through a 20-step plan in one context window degrades three ways: early decisions fall out of context, raw tool output crowds out reasoning, and there is no second pair of eyes. Splitting fixes all three.

- **The orchestrator's context stays clean.** It reads *reports* — a paragraph each — never raw test output or 2000-line files. So it can coordinate a plan far larger than one context window. The conversation is effectively unbounded; the orchestrator's *working set* is not, so protect it.
- **Sub-agents are cheap and focused.** Each starts fresh, does one block, reports, and is discarded. Their messy intermediate state never pollutes the coordinator.
- **Every block is independently reviewed.** The agent that wrote the code is never the only judge of it.

Use this skill whenever you are handed a large plan, a handoff document, or a staged cutover and asked to execute it. Use the per-block loop (steps 3–8 below) for any single substantial block even outside a multi-block plan.

## The orchestrator role

The orchestrator **only**: decomposes the plan; writes briefs; dispatches sub-agents; reads their reports; verifies git state; runs reviews; makes decisions; consults the second-opinion model on doubt; manages branches, worktrees, and any plan-state file.

The orchestrator **never**: writes or edits production code; runs the test suite itself; investigates a bug by reading source; reviews a diff line-by-line itself. Every one of those is a sub-agent's job. The instant the orchestrator starts doing the work, its context fills with detail and it loses the plot — that is the failure mode this whole structure exists to prevent.

Lightweight mechanics the orchestrator *may* do directly, because they are cheap and keep the loop moving: `git worktree add/remove`, `git cherry-pick`, `git log` / `show` / `status` for state, committing a plan-state file, reading agent and review report files, writing briefs and consult prompts.

## The cast

| Agent | Job | Tool access |
|---|---|---|
| **Orchestrator** | Decompose, brief, dispatch, verify, decide, manage git/state | this session |
| **Implementer** | Implement one block from a self-contained brief | full |
| **Reviewer** | Independent, harsh, production-ready review of one block | read-only |
| **Fix-agent** | Consume review findings, apply fixes as a new commit | full |
| **Extraction agent** | Gather/extract source into one file (e.g. so an external model can see it) | read-only |
| **Diagnostic agent** | Empirically reproduce + instrument a failure to establish ground truth | full, throwaway edits |
| **Second-opinion model** | Architectural consults + an independent code review per block | external CLI (e.g. `codex`) |

The **second-opinion model** is load-bearing, not optional. It is the escalation path for every doubt and the independent half of every review. One model judging its own work — even via its own sub-agents — shares blind spots; an outside judge built on a different model breaks the correlation. Treat it as a peer architect, not a linter.

## Per-block workflow

For each block in the plan:

1. **Front-load the design.** For a non-trivial block, run a *scope consult* with the second-opinion model **before** briefing the implementer. Plans go stale — the consult re-grounds the block's scope against the code as it actually is now. The consult's verdict becomes the brief's scope section.
2. **Isolate.** Create a dedicated worktree + branch for the block. Work never happens on the integration branch directly — that keeps a failed block trivially discardable.
3. **Brief.** Write a self-contained brief. The implementer starts cold with zero shared context; the brief carries everything (see `references/templates.md`).
4. **Dispatch the implementer.**
5. **Verify git state.** The implementer's report states *intent*, not *fact*. Check directly before reviewing: the commit exists, is scoped to the expected files/area, and leaves untouched what should be untouched. A report and a `git show --stat` disagreeing is a real signal.
6. **Dual review.** In parallel, dispatch (a) an independent reviewer sub-agent and (b) a code review by the second-opinion model. They catch different classes of problem — the reviewer reasons about intent and the brief; the second-opinion model reasons about the diff cold.
7. **Fix cycle.** A fix-agent consumes **both** reviews and fixes the findings as a *new* commit (never an amend). Then re-review. For a fix round, a clean re-review by the second-opinion model is the gate. Repeat fix → re-review until clean.
8. **Land.** Re-verify the integration branch has not moved unexpectedly; merge or cherry-pick the block onto it; have a sub-agent run the full verification gate on the integration branch; record the landing (a feedback-log entry); remove the block's worktree.

Then move to the next block. Do not pause between blocks to ask for confirmation — the plan was already approved; execute it through. (Re-planning mid-execution is for genuine plan-invalidating discoveries only, and those go through the second-opinion model.)

## Core principles

**Be autonomous; escalate doubt.** Drive the plan without pausing for the user. But for *any* architectural doubt, plan deviation, non-obvious decision, or sub-agent escalation — consult the second-opinion model and follow its recommendation. "When unsure, consult" is the single rule that makes unattended autonomy safe: it bounds the blast radius of a wrong call.

**Trust but verify.** Every report — from a sub-agent *or* the second-opinion model — states what the author *intended*, not necessarily what *happened*. After every implementer and fix-agent, verify the git state yourself. When a test count or a "this is done" claim is surprising, re-verify it. When two agents contradict each other, do not pick the more confident one — dispatch a diagnostic agent to get empirical ground truth, then act on the ground truth. The second-opinion model is not infallible either; a surprising review finding is worth a cheap cross-check (e.g. against the implementer's actual gate output) before you spend a fix round on it.

**A STOP is success, not failure.** Brief every sub-agent: if it hits a gap, blocker, or ambiguity it cannot resolve *within its brief*, it must STOP and report — never paper over with a workaround, a shim, or a stub that happens to pass. When a sub-agent STOPs, that is correct behavior worth rewarding: the orchestrator consults the second-opinion model and spins a new sub-block or fix round. A clean STOP costs one consult; a buried wrong fix costs a debugging expedition three blocks later.

**Comprehensive-audit pattern.** When a review finding is one instance of a class — a per-cache, per-field, per-call-site, per-scan gap — the fix brief must instruct the fix-agent to audit and fix the *whole class*, not just the named site. One reported symptom almost always has siblings; fixing only the named one guarantees a second review round on the same class.

**Dual review on every block.** Never let the block's author be its only judge. A block is not landable until the second-opinion re-review is genuinely clean — not "clean enough", not "only nits remain unaddressed". A real but deferred finding is tracked explicitly (a carry-forward item), never silently dropped.

**Discriminating tests only.** Reject stubs, empty test bodies, always-true assertions (`assert(true)`, `expect(1).toBe(1)`), and characterization tests that pass regardless of the change. A test must *fail* against the pre-change tree and *pass* against the post-change tree — that property is what makes it evidence. Briefs must demand it; reviewers must check it by reading every new test body.

## Self-contained briefs

A sub-agent starts with **zero** shared context — not the conversation, not the plan, not what other agents found. The brief is the entire world it sees. A thin brief produces shallow, generic work. Every brief carries these sections (full skeleton in `references/templates.md`):

- **Context** — why this block exists, where it sits in the plan, what already landed.
- **Scope / deliverables** — concretely what to do; for a substrate block, the second-opinion consult's verdict verbatim.
- **Legacy deletions** — the explicit list of files / functions / code paths / flags to delete. Without an explicit list, agents add the new path and leave the old one alive.
- **Verification** — the exact gate commands and the expected outcome (test counts, clean lint).
- **Constraints** — worktree path, branch, commit conventions, the environmental gotchas (below), anti-cheating rules, what *not* to touch.
- **Report-back** — the exact shape of the report you need, so you can verify it against git state without a second round-trip.

## Second-opinion model mechanics

Two modes (exact invocations in `references/templates.md`):

- **Consult** — an architectural question. The model usually cannot read the repo, so *embed* the relevant source in the prompt (extract it with an extraction agent first). State the question precisely, and always ask it to evaluate for **production-ready** quality — best long-term architecture, with breaking changes and implementation cost explicitly acceptable — so it does not propose a small safe patch when the right answer is a refactor.
- **Review** — an independent code review of a block's diff against a base branch.

Its output is large — grep for the findings, read the tail for the verdict. Do not run it concurrently with a heavy test suite (memory contention). Treat its verdict as authoritative on architecture *and* still verify surprising findings.

## Worktree hygiene & environmental discipline

- **One worktree + branch per block.** Remove the worktree as soon as the block lands — build directories accumulate fast (tens of GB each). The branch survives in git; only the working tree is removed.
- **The orchestrator's shell working directory is not a sub-agent's worktree.** A sub-agent that runs build/test commands without an explicit `cd <worktree> &&` prefix, or that edits files via relative paths, will silently act on the *main* checkout instead. Every brief must require absolute worktree paths and an explicit `cd` prefix for every build/test command. This single mistake produces convincing-but-wrong test counts — call it out in every brief.
- **Verify the integration branch before every cherry-pick / merge**, and confirm the integration tree equals the block tree afterward.
- **Pre-existing dirty files** (generated artifacts, line-ending churn) must never be staged — briefs name them explicitly and stage only the intended files by path.

## Anti-patterns

- The orchestrator reads a 2000-line file "just to check" — context bloat; dispatch an extraction or diagnostic agent.
- Skipping the second-opinion review because the implementer's report looks thorough — the report is intent, not fact.
- Accepting a sub-agent's STOP as a failure and trying to push past it — the STOP is the system working.
- Briefing a fix for only the named finding when it is one instance of a class.
- Pausing between approved blocks to ask "should I continue?" — execute the approved plan through.
- Letting a block land with the re-review still flagging a real issue as "deferred" without an explicit tracked carry-forward.

See `references/templates.md` for the brief / reviewer / fix-agent / consult prompt skeletons and the exact second-opinion CLI invocations.
