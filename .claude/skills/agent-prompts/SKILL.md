---
name: agent-prompts
description: Generate copy-pasteable prompts for driving separate Claude Code sessions through refactor, review, or migration work. Four variants — implementation (start a plan), continuation (resume after 85% handoff), review (harsh audit of landed work), fix-implementer (apply review findings). Also supports a review-workflow mode that emits the reviewer + fix-implementer PAIR as two distinct prompts for two clean sessions. Triggers on "implementation prompt", "continuation prompt", "review prompt", "reviewer prompt", "fix prompt", "fix-implementer prompt", "review workflow", "two prompts for two sessions", "generate the pair", "prompt for the next agent". Inputs: plan file, repo path, starting/target branches, handoff note (continuation), reviewer persona (review/workflow), focus areas. Output: prompts emitted INLINE in the chat as markdown — not saved as files — with clear `=====` delimiters for unambiguous copy-paste.

---

# Agent Prompts

Generate prompts for driving separate Claude Code sessions. Emit prompts INLINE in the chat (never save as files) so the user can copy-paste directly into fresh agent sessions. Every generated prompt preserves fixed invariants that close specific failure modes observed in prior runs.

## Variants

| Variant | Emits | Trigger phrases |
| --- | --- | --- |
| Implementation | 1 prompt | "implementation prompt", "start prompt", "driver prompt for <plan>" |
| Continuation | 1 prompt | "continuation prompt", "resume prompt", "prior agent hit 85%", "handoff prompt" |
| Review | 1 prompt | "review prompt", "reviewer prompt", "audit prompt" |
| Fix-implementer | 1 prompt | "fix prompt", "fix-implementer prompt", "apply the review" |
| **Review workflow** | **2 prompts (pair)** | "review workflow", "two prompts for two sessions", "generate the pair", "review + fix pair", "prompts for two agent sessions" |

The review-workflow is the common case when the user wants TWO distinct prompts for TWO separate clean sessions (one reviews, the other applies fixes).

## Required inputs

Ask concise questions if not provided; do not invent values.

**All variants:**
- Plan file path (absolute).
- Repo path (default: current working directory).
- Starting / staging branch.
- Target / mainline branch (e.g. `refactor/<track>` or `main`).

**Continuation additionally:**
- Handoff note path OR pasted content.
- **Verify branch state BEFORE generating** — run:
  - `git log --oneline <target>..HEAD` to enumerate actual commits.
  - `git status --short` to check working tree.
  - File-existence spot checks on paths the handoff claims deleted/kept.
  - If handoff claims diverge from reality, note the discrepancies in the generated continuation prompt so the next agent is warned.

**Review / workflow additionally:**
- Reviewer persona (default: `harsh reviewer, in a bad mood, DRY / KISS / long-term-maintainability focused`).
- Author framing for the work under review (default: `Codex` — keeps critique independent of reviewer's identity).
- Optional focus areas (security, performance, API design, accessibility).

**Fix-implementer additionally:**
- Tell the user the review output must be pasted into the fix session BEFORE the fix prompt.

## Generation workflow

1. Identify variant from the user's trigger phrase.
2. Ask for any missing required inputs in a single concise message.
3. For continuation: run verification commands and capture output before templating.
4. Parameterise the matching template(s) below with inputs + verified state.
5. Emit the prompt(s) INLINE in the chat as markdown code blocks with `=====` delimiters. Do NOT save to files. Do NOT reference paths.
6. Brief reporting line after the prompt block(s) — see Output format.

## Invariants (NEVER strip from any generated prompt)

Each closes a specific failure mode observed in prior runs. If the user asks to omit any of these, refuse and state which failure mode the invariant prevents.

1. **Stub Prevention citation.** Name `C:/Users/david/.claude/CLAUDE.md` `# Stub Prevention` (global) and project-level `CLAUDE.md` `### Stub Prevention (CRITICAL)` if present. Restate the five anti-patterns inline:
   - Empty `#[test]` bodies (or equivalent) un-ignored.
   - Unconditional-default returns masquerading as implementation (`Unknown`, `None`, `Ok(())`, `Miss`, `return null`, `return true`).
   - Always-true assertions (`assert!(true)`, `|| true` predicates, `expect(true).toBe(true)`).
   - "Real body deferred to follow-up commit" commit messages claiming gate-pass via stub.
   - Characterization tests that don't discriminate.
2. **Three only-stop cases:** (1) truly stuck with no plan-section resolution, (2) gate unfixable after multiple fix-commit cycles, (3) 85% context handoff.
3. **85% context handoff protocol:** commit pending, write handoff note with current step / exact next action / unresolved ambiguities / evidence paths, do NOT run the full gate at handoff.
4. **Fixed completion output format:** short structured report, no victory-lap prose, no menus.
5. **Forbid scope negotiation:** no "I'll do these but not those", no splitting proposals, no "honest scope assessment" dialogues. Work within the plan.

## Output format

### Single-prompt variants

Emit in the chat:

```
Paste the block below into a fresh Claude Code 1M Context session.

===== PROMPT — <VARIANT NAME> =====

<prompt content verbatim, as markdown>

===== END PROMPT =====

<One-sentence note on what this prompt drives.>
```

### Review-workflow pair

Emit in the chat:

```
Open TWO fresh Claude Code 1M Context sessions.

1. Paste PROMPT 1 into Session A. Wait for its review output.
2. Paste that review output INTO Session B, then paste PROMPT 2 below it.

===== PROMPT 1 — REVIEWER (Session A) =====

<reviewer prompt content verbatim>

===== END PROMPT 1 =====

===== PROMPT 2 — FIX-IMPLEMENTER (Session B) =====

<fix-implementer prompt content verbatim>

===== END PROMPT 2 =====

Workflow: Session A reviews and emits findings → paste findings into Session B → Session B applies them, runs the gate, squashes.
```

The `=====` delimiters make the copy-paste boundaries unambiguous. Never concatenate the pair.

---

## Templates

Placeholders (substitute from elicited inputs):

- `{{PLAN_PATH}}` — absolute plan file path.
- `{{REPO_PATH}}` — repo root (e.g. `D:\dev\personal\verter`).
- `{{STAGING_BRANCH}}` — e.g. `staging/d-cutover`.
- `{{TARGET_BRANCH}}` — e.g. `refactor/semantic-db-overhaul`.
- `{{GLOBAL_RULES}}` — `C:/Users/david/.claude/CLAUDE.md`.
- `{{PROJECT_RULES}}` — `<repo>/CLAUDE.md` if it exists; else omit the line that cites it.
- `{{GATE_SECTION}}` — plan's landing-gate section ref (e.g. `§7.5`).
- `{{LAST_STEP_SECTION}}` — plan's squash step (e.g. `§5.13`).
- `{{AUTHOR_FRAME}}` — who "implemented" it (for review persona). Default: `Codex`.
- `{{REVIEWER_PERSONA}}` — default: `harsh reviewer, in a bad mood, DRY/KISS religious, long-term-maintainability focused`.
- `{{FOCUS_AREAS}}` — optional extras for the reviewer.
- `{{HANDOFF_SUMMARY}}` — for continuation: verified landed commits + gotchas.

### Implementation template

```
You are executing the refactor plan at `{{PLAN_PATH}}`. Read it in full before starting. Every section is load-bearing.

Repo: `{{REPO_PATH}}`
Starting branch: `{{STAGING_BRANCH}}`
Target branch: `{{TARGET_BRANCH}}`

Execute the plan's sequencing steps in order. Pass the landing gate at `{{GATE_SECTION}}`. Squash-merge to `{{TARGET_BRANCH}}` at `{{LAST_STEP_SECTION}}`. Completion = gate passes zero-exit on every check AND the squash commit is on `{{TARGET_BRANCH}}`.

## Execution discipline

- Do not stop until the gate passes and the squash is complete.
- Do not ask for permission at WIP boundaries. Sequencing is linear.
- Do not complain the plan is too big. Do not propose splitting across sessions. You have 1M context.
- Do not invent scope. The plan's architectural decisions and change list are the contract; the out-of-scope section is the hard boundary.

## Rules — read these before starting

Global rules: `{{GLOBAL_RULES}}` — includes `# Stub Prevention`.
Project rules: `{{PROJECT_RULES}}` — includes `### Stub Prevention (CRITICAL)` under Agent Implementation Rules.

**Stub Prevention applies to every landed commit and to the squash.** The five forbidden patterns:

1. Empty `#[test]` bodies un-ignored — pass trivially while falsely advertising coverage. Keep `#[ignore]` until you can write a discriminating body.
2. Unconditional-default function bodies advertised as implementation (`RelationResult::Unknown`, `None`, `Ok(())`, `Opaque(Miss)`, `return null`, `return true`). Use `todo!()` / `unimplemented!()` / `throw new Error("not implemented")` so callers panic loudly.
3. Always-true assertions (`assert!(true)`, `|| true`, `expect(true).toBe(true)`).
4. Commit messages claiming gate-pass via stub ("real body deferred to follow-up").
5. Characterization tests that don't discriminate (pass regardless of code under test).

WIP exemption: staging-branch commits may contain `todo!()` / stubs / empty tests. The rule bites at squash / mainline / landed state.

## Only-stop cases

Stop ONLY for:

1. Truly stuck — no plan-section resolution works after multi-attempt evidence. Record in `.claude/feedback/feedback-<YYYY-MM-DD>-<track>.md` and report.
2. Gate unfixable by additional WIP commits after multiple fix-commit cycles. Record evidence.
3. Context crosses 85%. Hand off per protocol below. Not before 85%. Not after 85%.

Token cost of long cargo / test runs is NOT a stop case. "Plan is big" is NOT a stop case.

## 85% context handoff protocol

1. Commit pending work with `wip(session): <track> handoff at <step> — <summary>`.
2. Write `.claude/feedback/feedback-<YYYY-MM-DD>-<track>-handoff.md` with current step, exact next action (file:line), unresolved ambiguities, evidence paths.
3. Do NOT run `{{GATE_SECTION}}` at handoff — it burns remaining context.
4. Return with one short status line.

## Completion criteria

Done when ALL hold:

- `{{GATE_SECTION}}` landing gate passes zero-exit on every check.
- `{{STAGING_BRANCH}}` has been squash-merged onto `{{TARGET_BRANCH}}` per `{{LAST_STEP_SECTION}}`.
- Squash commit message enumerates every deleted file / API / renamed identifier / new variant / retired surrogate per plan's checklist.

Output EXACTLY:

<track> complete.
- Squash commit: <sha> on {{TARGET_BRANCH}}.
- Gate passed.
- Feedback: <path>.

Nothing more.

Proceed. Start at the plan's first sequencing step. Do not ask for confirmation.
```

### Continuation template

```
You are resuming `{{PLAN_PATH}}` after a prior agent hit 85% context.

Repo: `{{REPO_PATH}}`
Current branch: `{{STAGING_BRANCH}}`
Target: `{{TARGET_BRANCH}}`

## BEFORE ANYTHING — rule refresh

Read both files now:

- Global rules: `{{GLOBAL_RULES}}` (includes `# Stub Prevention`).
- Project rules: `{{PROJECT_RULES}}` (includes `### Stub Prevention (CRITICAL)` if present).

The prior agent may have tripped on Stub Prevention. The five forbidden patterns on landed commits:

1. Empty `#[test]` bodies un-ignored.
2. Unconditional-default function bodies advertised as implementation.
3. Always-true assertions.
4. "Deferred to follow-up commit" messages claiming gate-pass via stub.
5. Characterization tests that don't discriminate.

WIP exemption applies to `{{STAGING_BRANCH}}`. The rule bites at squash on `{{TARGET_BRANCH}}`.

## Verified repo state

{{HANDOFF_SUMMARY}}

(Includes: HEAD sha, git status, landed commits, file-existence checks, and any handoff-vs-actual discrepancies flagged inline.)

## Your task

Continue the plan's sequencing from the next unlanded step. Pass `{{GATE_SECTION}}`. Squash at `{{LAST_STEP_SECTION}}`.

[Plan-section-specific remaining work — elicit from handoff note.]

## Execution discipline

- Do not stop until the gate passes and the squash is complete.
- Do not ask permission at WIP boundaries.
- Do not complain about scope. Do not propose splits.
- Do not invent scope.

## Only-stop cases

1. Truly stuck with no plan-section resolution.
2. Gate unfixable after multiple fix-commit cycles.
3. Context crosses 85% — handoff per below.

## 85% handoff protocol

1. Commit pending with `wip(session): <track> handoff at <step> — <summary>`.
2. Write `.claude/feedback/feedback-<YYYY-MM-DD>-<track>-handoff.md`.
3. Do NOT run `{{GATE_SECTION}}` at handoff.
4. Return with one short status line.

## Completion criteria

Done when ALL hold:

- `{{GATE_SECTION}}` passes zero-exit.
- `{{STAGING_BRANCH}}` squash-merged onto `{{TARGET_BRANCH}}`.
- Squash message enumerates retired items per plan checklist.

Output EXACTLY:

<track> complete.
- Squash commit: <sha> on {{TARGET_BRANCH}}.
- Gate passed.
- Feedback: <path>.

Proceed. Start with the first remaining step. Do not ask for confirmation.
```

### Reviewer template

```
You are reviewing the implementation that `{{AUTHOR_FRAME}}` produced on `{{STAGING_BRANCH}}`. You are a {{REVIEWER_PERSONA}}. You trust no commit message. You verify every claim against the tree.

## Your scope

- Plan (contract `{{AUTHOR_FRAME}}` was meant to honor): `{{PLAN_PATH}}`.
- Repo: `{{REPO_PATH}}`, branch `{{STAGING_BRANCH}}`.
- Rules: `{{GLOBAL_RULES}}` + `{{PROJECT_RULES}}`.

## What to review

Every WIP commit on `{{STAGING_BRANCH}}` that didn't exist before plan execution began. `git log --oneline {{TARGET_BRANCH}}..HEAD`. Also uncommitted changes.

For each commit and the current tree, check:

1. **Stub Prevention compliance.** Hunt all five anti-patterns:
   - Empty `#[test]` bodies. Scan `fn <name>() {}`.
   - Unconditional-default returns advertised as implementation. Compare commit-message claims against actual body.
   - Always-true assertions.
   - "Deferred to follow-up" in commit messages.
   - Characterization tests that pass regardless of the code under test. For each un-ignored test: would it FAIL pre-change AND PASS post-change?

2. **DRY.** Hunt copy-paste, duplicated helpers, repeated constants, fixtures with drift. Propose single canonical form.

3. **KISS.** Type aliases without semantic payload, traits with one implementor, abstractions for hypothetical futures, cosmetic module splits, delegating methods.

4. **Long-term maintainability.** Names that lie. Comments that will rot (rev-N, session numbers, commit hashes). Magic numbers without justification. Swallowed errors. Dead code + `#[allow(dead_code)]` optimism bets.

5. **Plan compliance.** Every commit should map to a numbered plan step. Any generator or lint-script modifications must produce identical output to the plan's version.

6. **Commit-message honesty.** Flag messages claiming gate-pass via stub, deferring load-bearing work, or citing plan sections they don't satisfy.

7. **Characterization-test quality.** For each un-ignored test: file:line, body quote, judgement (discriminating / cosmetic / incorrect).

{{FOCUS_AREAS}}

## How to investigate

Do not trust commit messages alone. For every claim, read the diff.

- `git log --oneline {{TARGET_BRANCH}}..HEAD` and `git show <sha>` on each.
- `git status` / `git diff` for uncommitted.
- Open each touched file.
- `cargo check --package <crate> --tests` to confirm compilation claims.
- `rg -n 'todo!\(\)|unimplemented!\(\)|assert!\(true\)|\|\| true'` to find stubs mechanically.

## Output format

Produce a single markdown response:

# <Track> Implementation Review (<staging-branch> @ <HEAD sha>)

Verdict: REVISE / APPROVE WITH EDITS / APPROVE

## Summary

<3-5 sentences.>

## Findings

### [F1 — CRITICAL / MAJOR / MODERATE / MINOR] <Short title>

**Location:** `path/file.rs:LINE` (or commit <sha>)

**What's wrong:** <quote offending code>

**Why it's wrong:** <cite rule — stub prevention, DRY, KISS, plan §X>

**Fix:** <specific remediation>

[... severity-ordered ...]

## Stub audit table

| Test/function | File:line | Body has assertions? | Discriminating? | Verdict |

## DRY audit, KISS audit, Long-term-maintenance hazards

<lists>

## Plan-compliance delta

| Plan step | Claimed complete by commit | Actually complete? | Gap |

## Concrete plan of action for the fix-implementer

<numbered list, fix order, each citing a finding>

## Verdict

REVISE. <one-sentence reason.>

## Rules for your review

- Cite file:line for every finding.
- Do not propose rewrites deviating from the plan's architectural decisions.
- "It's WIP, will be polished" is NOT a defence — deferred-polish on landed commits is Stub Prevention violation.
- Quote offending code.
- One verdict top AND bottom, same verdict.

Start. When the review document is complete, output it and stop.
```

### Fix-implementer template

```
You are applying the review findings from the prior agent. The review output is pasted into this conversation BEFORE this prompt. Treat it as authoritative.

## Scope

- Plan: `{{PLAN_PATH}}`.
- Repo: `{{REPO_PATH}}`, branch `{{STAGING_BRANCH}}`.
- Rules: `{{GLOBAL_RULES}}` + `{{PROJECT_RULES}}`.

## Discipline

- Apply every finding in the review's "Concrete plan of action", in the order given.
- Do NOT negotiate, summarise, or produce menus.
- Do NOT ask permission between fixes.
- Do NOT introduce new stubs while fixing existing ones. Re-read Stub Prevention in `{{GLOBAL_RULES}}` and `{{PROJECT_RULES}}`.
- Push back only with concrete evidence — write a rebuttal with file:line in `.claude/feedback/feedback-<YYYY-MM-DD>-<track>.md` and skip that finding. Do not skip findings you merely dislike.
- Do NOT rewrite scaffolding commits via `--amend` or `rebase -i`. Additive fix commits only.

## Only-stop cases

1. Truly stuck on a finding the plan cannot resolve.
2. Cascading failures that cannot be addressed without re-planning.
3. Context crosses 85% — handoff per below.

## Working order

1. Re-read the review in full before touching code.
2. Group findings by file.
3. Stub Prevention findings first.
4. DRY / KISS findings next.
5. Plan-compliance findings next.
6. Long-term-maintenance findings last.
7. After each fix commit: `cargo check --package <crate> --tests` minimum. Do NOT run full workspace after every commit.
8. Once all closed: run plan's mutating-tools step, then gate `{{GATE_SECTION}}`. Squash per `{{LAST_STEP_SECTION}}` on pass.

## Commit message shape

Each fix commit:

wip(session): <track> review-fix — close <F-IDs> (<short summary>)

Review finding(s) closed:
- Fx — <title>
- Fy — <title>

Behaviour change: <what the tree does now that it didn't before>

Verification: <what you ran, what passed>

No "scaffolding", "will be fleshed out", "deferred to follow-up".

## 85% handoff

Per Implementation template.

## Completion criteria

Done when ALL hold:

- Every finding closed (fixed or explicitly rebutted in feedback with evidence).
- Plan's mutating-tools step runs clean.
- `{{GATE_SECTION}}` passes zero-exit.
- `{{STAGING_BRANCH}}` squash-merged per `{{LAST_STEP_SECTION}}`.

Output EXACTLY:

<track> fix pass complete.
- Findings closed: N of N.
- Fix commits: <sha list>.
- Squash commit: <sha> on {{TARGET_BRANCH}}.
- Gate passed.

Start. Apply the first finding now. Do not ask for confirmation.
```

---

## Operational notes

- **Never save generated prompts to files.** Emit them inline in the chat as markdown code blocks with `=====` delimiters. Users copy from the chat directly into fresh sessions.
- When generating the review-workflow pair, emit both prompts in the same response with clearly-separated `===== PROMPT 1 =====` / `===== PROMPT 2 =====` blocks. Never concatenate.
- Substitute every `{{PLACEHOLDER}}` from user input + verification. Don't leave placeholders in output.
- If the user's project lacks a `CLAUDE.md` Stub Prevention section, inline the rule body in the generated prompt anyway — the prompt is the only rule reference the downstream agent will load.
- If user requests a custom persona or focus area, add it to the reviewer template without removing DRY / KISS / long-term-maintenance / Stub Prevention lines. Those stay.
- If user requests omission of any invariant in §Invariants, refuse with one sentence naming the failure mode the invariant prevents.
