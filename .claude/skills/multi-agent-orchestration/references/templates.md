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

```
You are an independent, harsh, production-ready code reviewer. You did NOT write
this code — review it adversarially. Read-only; modify nothing. Do NOT run the
test suite (a separate integration-verify does that, and the second-opinion
review runs concurrently — avoid memory contention).

Review commit <SHA> on branch <branch> (diff <base>..<branch>).

Context: <the block's purpose; point at the brief>.
The implementer's self-report (verify these claims against the actual diff): <embed it>.

For every finding assign [P0] (blocks landing) / [P1] (must fix) / [P2] (should
fix) / [P3] (nit). Check: correctness; scope completeness vs the brief; test
quality (do new tests discriminate? read every body); architecture & legacy
deletion (no dual paths/shims); rule compliance. End with a one-line verdict:
LAND (clean / only P3) or CHANGES REQUIRED. Be specific — vague findings are
useless to a fix-agent. If nothing is wrong, say so; do not invent issues.
```

## 3. Fix-agent prompt

```
You are the fix-agent for Block <N>. A dual review found issues. Apply the fixes
as a NEW commit on <branch> (never amend). You implement, test, commit, report.

Worktree, gotchas, commit convention: <as in the implementer brief>.

The findings (from BOTH the independent reviewer and the second-opinion review):
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

Always pin model and effort explicitly — an unpinned invocation silently runs at the default and still returns a confident verdict. **Model and effort are POLICY, not discovery.** `CODEX_MODEL_POLICY[role] = { model, reasoning_effort }` comes from ONE ratified authority (`mom-cto-orchestration/reference/codex-model-policy.toml`); preflight only checks whether that policy-selected model is AVAILABLE and what actually BOUND. Preflight never chooses — it can tell you what is available, not what is intended, and a preflight that infers policy from availability is the capability-vs-evidence conflation. **Never hardcode a slug in a durable file**: preflight resolves the entry into two scalars, `CODEX_MODEL` and `CODEX_EFFORT`, and the invocation interpolates those. `${CODEX_MODEL_POLICY[$ROLE].model}` is NOT a shell expression — it is a `bad substitution` that exits 1 — so never write a policy lookup inline in a command. Unavailable, substituted, unknown, or banner-mismatched ⇒ **BLOCK the leg** — never substitute, upgrade, or downgrade.

**The invocation is NOT reproduced here.** It has exactly ONE form in the tree — `mom-cto-orchestration/reference/PROTOCOL.md` → codex Invocation — and this file deliberately does not copy it. A second copy is not a convenience, it is a drift surface: the moment two copies exist they diverge (one grows a `cd`, one grows an exit check, one keeps a stale flag), and an agent obeys whichever it happened to read. Consult and review use the SAME single form; only the prompt file, the output paths, and the working directory differ.

Everything below is the CONTRACT around that call, not a second version of it:

**Lifecycle — one policy, identical to `mom-cto-orchestration/reference/{PROTOCOL,WAIT-PROTOCOL}.md`.** Foreground, explicit bounded timeout (`timeout_seconds` in the policy authority). No trailing `&`, never detached, never background-and-polled. On timeout the leg FAILED: redispatch up to `max_attempts`, then block and escalate. Parallelism comes from separate managed review calls with distinct output paths, never from global shell process manipulation.

**Ownership recording follows the dispatch shape.** A foreground leg has no `&` and therefore no `$!` — `timeout` is its bound and the blocking call is its join, so there is no PID to record. A DETACHED dispatch records its `$!` at spawn, before the wait. **Cleanup is scoped to what you recorded:** terminate only that recorded tree, through the ONE shared helper — `terminate_recorded_tree` (`mom-cto-orchestration/reference/PROTOCOL.md` → Ownership and Termination), which confirms the descendant closure is gone rather than trusting a kill command's exit status. Do not reprint the kill here: the obvious form is wrong in ways that only appear when you run it, and a second copy is a drift surface. NEVER `taskkill /F /IM`, `pkill -f codex`, `killall`, or `Stop-Process -Name` — those kill every codex process on the machine, including the user's own sessions and the sibling review leg, and a leg killed by another's cleanup is indistinguishable from a stalled one.

Verify the model AND effort the CLI actually bound — it echoes both in its startup banner — against the expected `CODEX_MODEL_POLICY` entry; a mismatch BLOCKS the leg. Do not trust the policy; read what actually bound.

**A leg whose verdict artifact you cannot produce did not run.** The leg counts only on a clean exit under its timeout, a matching banner, and a non-empty verdict file (`-o`) whose contents you READ. The full transcript is contaminated by construction — it contains the prompt echoed back — so `LAND`, `CHANGES`, and `__DONE__` all appear in it whether or not the leg ever rendered a verdict; grep the transcript to orient or diagnose, never to conclude. Never run these concurrently with a heavy test suite — memory contention.

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
