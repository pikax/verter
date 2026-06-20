# Orchestration templates & mechanics

Copy-and-fill skeletons for the orchestrator. These are starting points — adapt them to the block. The principle behind every one: a sub-agent or an external model sees *only* what you give it, so give it enough to make judgement calls, not just follow a narrow instruction.

## 1. Self-contained implementer brief

Pass this brief as the Agent tool's `prompt` (the default mechanism) — or write it to a file the agent reads, or feed it to a `claude -p` stdin file on the opt-in CLI path. Either way the implementer sees only the brief, so give it everything. For any gate-bearing dispatch (independent reviewer, §1a/verification, confirm, anti-rogue — any mechanism, Agent or `claude -p`), persist the exact prompt/brief AND the verbatim final report to files with a recorded input id/hash plus the resolved model and effort before acting on the result — inline-only briefs are not allowed for those roles, because anti-rogue and never-prime checks need post-hoc inspection of the exact prompt and output.

```
# Implementer Brief — Block <N>: <title>

## Context
- What the overall plan/cutover is, in two sentences.
- Where this block sits; what blocks already landed; the integration branch + HEAD.
- Why this block exists — the problem it solves.

## Scope / deliverables
- Concretely what to build or change. For a block whose design came from a
  second-opinion consult, paste the consult's verdict here verbatim and say
  "implement all of it; do not narrow it".
- Number the deliverables so the report-back can map to them.

## Legacy deletions (explicit)
- The exact files / functions / code paths / feature flags to DELETE.
- "No dual path, no shim, no compatibility flag." Without an explicit list,
  agents add the new path and leave the old one alive.

## Verification gate
- The exact build/lint/test commands, each with the `cd <worktree> &&` prefix.
- The expected outcome: baseline test counts + the delta this block adds.
- Known-flaky tests to re-run in isolation rather than treat as failures.

## Constraints
- Worktree path + branch. ABSOLUTE paths for every edit. Run every build/test
  command via `cd <worktree> && ...`.
- Commit convention; one or a few well-scoped commits; never amend/force/push.
- Anti-cheating: no stubs, no always-true assertions; tests must discriminate.
- What NOT to touch (files owned by other blocks; the thing a later block deletes).
- The STOP rule: if a planned change is architecturally wrong or blocked, STOP
  and report — do not shim, do not work around.

## Report back
- Commit SHA(s); verbatim verification-gate totals; per-deliverable summary;
  legacy-deletion confirmation; main-repo-clean confirmation; a short summary.
```

## 2. Reviewer sub-agent prompt

This skeleton is ADVERSARIAL by default — it must dispatch a refute-first reviewer, not a confirmatory one. Its semantics match the CTO-tier CLAUDE-REVIEWER MANDATE. It is the claude leg of the base two-leg dual review (claude + second-opinion/codex); when MoM/CTO orchestration is in force its three-leg composition (1 adversarial claude + 1 claims-aware codex + 1 unprimed codex) SUPERSEDES the base loop — but the adversarial claude STANCE below is identical in both tiers. Standalone `/multi-agent-orchestration` may dispatch this self-contained skeleton as-is (no external mandate file required); under MoM/CTO, prepend `CLAUDE-REVIEWER-MANDATE.md` verbatim first, then append this gate-specific prompt.

```
You are an ADVERSARIAL, independent, production-ready code reviewer. You did NOT
write this code. Your job is to BREAK this change, not to bless it — default to
REJECT. Read-only; modify nothing. Do NOT run the test suite (a separate
integration-verify does that, and the second-opinion review runs concurrently —
avoid memory contention).

Review commit <SHA> on branch <branch> (diff <base>..<branch>).

Context: <the block's purpose; point at the brief>.
The implementer's self-report — treat every claim as an assertion to TEST, not a
fact, and verify it against the actual diff: <embed it>.

Review to REFUTE: actively hunt the bug, the over-claim, the missed case, the
silent weakening, the non-discriminating test. Assume a defect is present until
you have genuinely tried and failed to find one. Check: correctness; scope
completeness vs the brief; test quality — read EVERY new test body and prove it
discriminates (would FAIL pre-change, PASS post-change); architecture & legacy
deletion (no dual paths/shims); rule compliance.

Your report MUST include:
- A "what I tried to break + the result of each attempt" section (the cases,
  inputs, paths, and claims you attacked, and what happened).
- The STRONGEST counter-argument you found and why it does or does not sink the
  change.
- A list of every risk, uncertainty, scope gap, and weakly-supported claim.
For every finding assign [P0] (blocks landing) / [P1] (must fix) / [P2] (should
fix) / [P3] (nit), and make each finding enumerated and actionable with its
file / section / exact change. Be specific — vague findings are useless to a fix-agent.

End with a one-line verdict: LAND ONLY IF you genuinely tried to find a
bug / over-claim / weakening / non-discriminating test and could not (a stub,
always-true assert, or non-discriminating characterization is a finding, not a
pass); otherwise CHANGES REQUIRED. Never invent issues to look thorough, and
never soften a real one to be agreeable — if the change is wrong, say so plainly.
```

## 3. Fix-agent prompt

```
You are the fix-agent for Block <N>. The block's review found issues. Apply the
fixes as a NEW commit on <branch> (never amend). You implement, test, commit,
report.

Worktree, gotchas, commit convention: <as in the implementer brief>.

The findings (from EVERY review leg that ran — under the base tier the adversarial
claude reviewer + the second-opinion/codex review; under MoM/CTO orchestration the
adversarial claude leg + the claims-aware codex leg + the unprimed codex leg):
<embed every finding verbatim, with the prescribed fix direction>.

For each finding: fix it; if a finding is one instance of a class, fix the whole
class. Add a discriminating regression test for every behavioral fix. Re-run the
FULL verification gate after fixing. If a prescribed fix is itself wrong, STOP
and report rather than implement something you believe is incorrect.
Report: new commit SHA, gate totals, per-finding fix, regression-test evidence.
```

## 4. Second-opinion architectural consult

The external model usually cannot read the repo. Structure the prompt file:

```
# Consult — <one-line question>

You are reviewing an architectural question for <project>. Evaluate for
PRODUCTION-READY quality: recommend the best long-term architecture. Breaking
changes, broad refactors, and implementation cost are all acceptable — do not
trade architectural correctness for a smaller fix.

## Context
<the plan; what landed; why this question arises>

## The verified situation
<the empirical findings — what was reproduced/instrumented, not speculation>

## Full source extraction (the model cannot read repo files)
<embed the relevant functions/types, with file:line — extract via an agent first>

## Questions
Q1 ... Q2 ... (numbered, explicit). End: "Give a clear verdict on each question
and a clear final scoping decision."
```

Assemble with `cat head.md source.md tail.md > prompt.txt` so the embedded source stays a separate, re-usable extraction file.

## 5. Second-opinion CLI invocations (codex)

```bash
# Architectural consult — prompt from a file, output to a file:
codex exec --skip-git-repo-check < prompt.txt > consult-out.txt 2>&1

# Code review of a block — no prompt arg; run from inside the worktree:
cd <worktree> && codex review --base <base-branch> < /dev/null > review-out.txt 2>&1
```

Output files are large. Grep them for findings (`grep -nE '\[P[0-3]\]' out.txt`) and read the tail for the verdict. Ignore trailing process-teardown noise. Never run these concurrently with a heavy test suite — memory contention.

## 6. Environmental gotchas (carry into every brief)

- **Working-directory leak.** The orchestrator's shell cwd, and a sub-agent's relative-path edits, resolve against the *main* checkout — not a worktree. Every brief: ABSOLUTE worktree paths for edits; explicit `cd <worktree> &&` for every build/test command. Symptom when violated: a plausible but wrong test count from the main tree.
- **Truncated test runs.** A test suite that intermittently aborts (e.g. stack overflow in one binary) yields a short count that looks like a pass. Brief agents to COUNT the per-binary result lines and re-run until the count is complete; never accept a partial sum.
- **Generated / churn files dirty the tree.** Generated artifacts and line-ending churn show as modified on checkout. Brief: never stage them; stage only intended files by explicit path; never `git add -A`.
- **Trust but verify the git state.** After every implementer/fix-agent: `git log` the commit exists, `git show --stat` it is scoped as expected, confirm the files that must stay untouched are untouched, confirm the main repo is clean (no cwd leak).
- **Re-verify the integration branch before a cherry-pick**, and confirm the integration tree equals the block tree after.

## 7. Recovery situations

- A sub-agent returns a tool-call error mid-task, or two agents touched one worktree: inspect the worktree (`git status` + `git log`). If the work landed, resume via a fresh agent that verifies + commits. If it barely started, reset and re-dispatch.
- A surprising second-opinion finding: cross-check it (e.g. against the implementer's actual gate output) before spending a fix round — second-opinion models produce occasional false positives.
- Two agents contradict each other: do not arbitrate from reports. Dispatch a diagnostic agent to reproduce empirically and report ground truth; act on that.
