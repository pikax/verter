# Orchestration templates & mechanics

Copy-and-fill skeletons for the orchestrator. These are starting points — adapt them to the train/slice. The principle behind every one: a sub-agent or an external model sees *only* what you give it, so give it enough to make judgement calls, not just follow a narrow instruction.

## 1. Self-contained implementer brief

Pass this brief as the Agent tool's `prompt` (the default mechanism) — or write it to a file the agent reads, or feed it to a `claude -p` stdin file on the opt-in CLI path. Either way the implementer sees only the brief, so give it everything. For any gate-bearing dispatch (independent reviewer, §1a/verification, confirm, anti-rogue — any mechanism, Agent or `claude -p`), persist the exact prompt/brief AND the verbatim final report to files with a recorded input id/hash plus the resolved model and effort before acting on the result — inline-only briefs are not allowed for those roles, because anti-rogue and never-prime checks need post-hoc inspection of the exact prompt and output.

```
# Implementer Brief — Train <train-id>, Slice <slice-id>: <title>

## Context
- What the overall plan/migration is, in two sentences.
- Where this train sits; what trains already landed and confirmed; the
  integration branch + HEAD.
- Why this train exists — the problem it solves.

## Scope / deliverables
- Concretely what to build or change. Include the current ratified contract,
  the accepted scope rows, and the executable obligations that bind this
  train. A consult recommendation is binding only after ratification (where
  required) and only while consistent with critical invariants and new
  evidence. Do NOT broaden the train from a consult; scope additions follow
  the five-way admission policy and require explicit user approval if they
  would create a new critical-path train.
- Number the deliverables so the report-back can map to them.

## Legacy deletions (explicit)
- The exact files / functions / code paths / feature flags to DELETE.
- "No dual path, no shim, no compatibility flag." Without an explicit list,
  agents add the new path and leave the old one alive.

## Verification gate
- The exact targeted build/lint/test commands (changed tests + affected
  crates + conservative reverse-dependency closure), each with the
  `cd <worktree> &&` prefix. Targeted runs are iteration evidence only; a
  selector that cannot prove the affected closure MUST fall back to
  full-workspace coverage for that run (still iteration evidence, never
  landing evidence); the canonical full pair runs at final acceptance and
  confirm, not here.
- The expected outcome: baseline test counts + the delta this slice adds.
- Known-flaky failures remain FAILURES pending isolated classification: rerun
  isolated with adequate timeout + no co-resident heavy work, retain both
  artifacts, and hard-fail if the isolated result repeats or classification
  is ambiguous.

## Constraints
- Worktree path + branch. ABSOLUTE paths for every edit. Run every build/test
  command via `cd <worktree> && ...`.
- Commit convention; one clean, separately-testable conventional commit per
  slice; never amend/force/push.
- Anti-cheating: no stubs, no always-true assertions; tests must discriminate
  (mutation-recipe provable).
- What NOT to touch (files owned by other trains; the thing a later train deletes).
- The STOP rule: if a planned change is architecturally wrong or blocked, STOP
  and report — do not shim, do not work around.

## Report back
- Commit SHA(s); verbatim targeted-gate totals; per-deliverable summary;
  legacy-deletion confirmation; main-repo-clean confirmation; a short summary.
```

## 2. Reviewer sub-agent prompt

One prompt per reviewer; three reviewers per round, author-dependent mix (Claude/Fable author → 2 GPT + 1 Claude; GPT/Codex author → 2 Claude + 1 fresh GPT), dispatched in parallel, blind. A codex leg gets this same prompt persisted to a file with the CODEX-ARCHITECT MANDATE prepended, dispatched per §5 — never bare `codex review`.

```
You are one of three independent, blind, harsh, production-bar reviewers. You
did NOT write this code — review it adversarially. Read-only; modify nothing;
do NOT run the test suite (a separately scheduled verifier does that, and the
other reviewers run concurrently — avoid memory contention). You are not given
— and must not seek — the author's correctness claim, the design-adversary
prediction, or the other reviewers' findings.

Review the immutable cumulative train tree at base <SHA>, head <SHA>
(diff <base>..<head>).

Your assigned lens (exactly ONE):
  (A) semantic parity, oracle validity, coverage-dimension completeness; or
  (B) architecture, typed-IR ownership, fail-closed behavior, rule integrity; or
  (C) host integration, caching, source maps, runtime behavior, regression
      blast radius.
The lens sets your SEARCH STRATEGY only, not what is checked: inspect the FULL
cumulative diff and report any defect you find.

Common checklist — ALL THREE reviewers apply every item:
  - correctness;
  - acceptance-scope completeness against the ratified contract;
  - test quality / discriminating evidence — read every new test body; reject
    stubs, always-true assertions, non-discriminating characterization;
  - architecture + typed-IR ownership + required legacy deletion (no dual
    paths, no shims);
  - every CRITICAL invariant + executable obligation;
  - fail-closed behavior, safety, scalability;
  - anti-rogue + rule integrity.

Use the generated evidence packet at <path> (base/head SHA, changed acceptance
rows + invariants, coverage-manifest delta, unsupported→supported transitions,
tests + mutation recipes, cache-key + source-map changes, legacy deletions,
generated artifacts + their source manifest).

For every finding assign [P0] (blocks landing) / [P1] (must fix) / [P2] (should
fix) / [P3] (nit). GATE CRITERION: gate on correctness, ratified-contract
compliance, critical invariants, executable obligations, fail-closed behavior,
discriminating evidence, and anti-rogue integrity. A merely preferable
architecture alone is optional, non-blocking debt and MUST NOT produce CHANGES
REQUIRED — record it as [P3]/optional debt; reopen only for a
correctness/safety/scalability/invariant defect or new evidence invalidating
the ratified contract. End with a one-line INDEPENDENT verdict: LAND (clean /
only P3) or CHANGES REQUIRED. Be specific — vague findings are useless to a
fix-agent. If nothing is wrong, say so; do not invent issues.
```

## 3. Fix-agent prompt

```
You are the fix-agent for train <train-id>. A three-review round found issues.
Apply the fixes as ONE comprehensive NEW commit on <branch> (never amend). You
implement, test, commit, report.

Worktree, gotchas, commit convention: <as in the implementer brief>.

The findings (consolidated once from all three reviewers):
<embed every finding verbatim, with the prescribed fix direction>.

For each finding: fix it; if a finding is one instance of a class, fix the
whole class. Add a discriminating regression test for every behavioral fix.
After fixing, run every finding's regression test, the affected closure,
applicable architecture guards, clippy, and fmt — do NOT run the canonical
full pair merely because a fix round completed (§1a runs targeted mutation
recipes; the separately scheduled final verifier runs the canonical pair once
the rebased train tree is content-frozen). If a prescribed fix is itself
wrong, STOP and report rather than implement something you believe is
incorrect.
Report: new commit SHA, targeted-gate totals, per-finding fix, regression-test
evidence.
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

Assemble with `cat head.md source.md tail.md > prompt.txt` so the embedded source stays a separate, re-usable extraction file. The open-design consult keeps the best-durable-design bar; review/landing/confirm invocations judge against the ratified contract + invariants instead (see the MoM PROTOCOL mandate).

## 5. Second-opinion CLI invocations (codex)

```bash
# Architectural consult — prompt from a file, output to a file:
codex exec --skip-git-repo-check -c 'model="gpt-5.6-sol"' \
  -c 'model_reasoning_effort="high"' < prompt.txt > consult-out.txt 2>&1

# Reviewer leg (train code review) — NEVER bare `codex review`, never an empty
# stdin. Persist a per-leg reviewer prompt file FIRST, containing:
#   the CODEX-ARCHITECT MANDATE (verbatim, prepended — see the MoM PROTOCOL);
#   the §2 reviewer prompt with exactly ONE assigned lens (A/B/C), including
#     the common checklist + gate criterion;
#   the evidence-packet path/contents;
#   the immutable base..head SHA pair (recorded per leg).
codex exec --sandbox read-only -C <worktree> --skip-git-repo-check \
  --disable image_generation -c 'model="gpt-5.6-sol"' \
  -c 'model_reasoning_effort="high"' \
  < reviewer-leg<A|B|C>.md > review-leg<A|B|C>-out.txt 2>&1
```

Every gate-bearing codex dispatch binds the model's MAXIMUM supported reasoning effort explicitly (`high` is `gpt-5.6-sol`'s maximum tier) — never an uncontrolled default, never a tier below the maximum; an unresolved model or maximum effort fails closed (block the dispatch). A reviewer leg targets the immutable base..head SHA pair persisted in its prompt — never a live branch ref; two legs dispatched without their own lens + packet + mandate are correlated default reviews, which defeats decorrelation. Output files are large. Grep them for findings (`grep -nE '\[P[0-3]\]' out.txt`) and read the tail for the verdict. Ignore trailing process-teardown noise. Never run these concurrently with a heavy test suite — memory contention; isolated workers only.

## 6. Environmental gotchas (carry into every brief)

- **Working-directory leak.** The orchestrator's shell cwd, and a sub-agent's relative-path edits, resolve against the *main* checkout — not a worktree. Every brief: ABSOLUTE worktree paths for edits; explicit `cd <worktree> &&` for every build/test command. Symptom when violated: a plausible but wrong test count from the main tree.
- **Truncated test runs.** A test suite that intermittently aborts (e.g. stack overflow in one binary) yields a short count that looks like a pass. Brief agents to COUNT the per-binary result lines and re-run until the count is complete; never accept a partial sum. A timeout or incomplete run is never a pass.
- **Generated / churn files dirty the tree.** Generated artifacts and line-ending churn show as modified on checkout. Brief: never stage them; stage only intended files by explicit path; never `git add -A`.
- **Trust but verify the git state.** After every implementer/fix-agent: `git log` the commit exists, `git show --stat` it is scoped as expected, confirm the files that must stay untouched are untouched, confirm the main repo is clean (no cwd leak).
- **Re-verify the integration branch before a cherry-pick**, and confirm the integration tree equals the train tree after.

## 7. Recovery situations

- A sub-agent returns a tool-call error mid-task, or two agents touched one worktree: inspect the worktree (`git status` + `git log`). If the work landed, resume via a fresh agent that verifies + commits. If it barely started, reset and re-dispatch.
- A surprising second-opinion finding: cross-check it (e.g. against the verifier job's actual gate summary) before spending a fix round — second-opinion models produce occasional false positives.
- Two agents contradict each other: do not arbitrate from reports. Dispatch a diagnostic agent to reproduce empirically and report ground truth; act on that.

## 8. Evidence packet, design adversary, bakeoff

Do not duplicate these contracts here — the authoritative definitions live in the MoM overlay (`.claude/skills/mom-cto-orchestration/reference/PROTOCOL.md`): the per-round generated EVIDENCE PACKET contents (Review Cadence), the pre-authoring Design-Adversary Contract for contract-heavy trains, and Measured Author Selection (the bakeoff + retention telemetry). Reviewer prompts reference the packet; implementer briefs for contract-heavy trains embed the failure-mode contract.
