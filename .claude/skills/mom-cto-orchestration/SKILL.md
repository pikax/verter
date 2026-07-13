---
name: mom-cto-orchestration
description: >-
  CTO/manager-of-managers methodology for autonomous multi-block plans where the user says
  "you are the MoM/CTO", "orchestrate the whole plan", "drive the migration end-to-end",
  "manager-of-managers", or "dispatch block managers". The CTO dispatches one block
  MANAGER per unit, each manager runs `/multi-agent-orchestration`, and the CTO adds
  independent confirm/integration gates, codex-owned architecture decisions, governance,
  and anti-rogue rule defenses. For a single-block task use `/multi-agent-orchestration`
  directly.
---

# MoM / CTO Orchestration

This is the layer ABOVE `/multi-agent-orchestration`. Load that skill first; it is the block-manager manual. This skill adds the CTO tier: block managers, confirm managers, integration-confirm, codex architecture adjudication, governance, anti-rogue rule defense, and portable account-role discipline.

## Tier Model

```
CTO / MoM (interactive)
  decompose · dispatch managers · codex architecture forks · read reports · checkpoint · advance
    ↓
MANAGERS (one per block/stage/phase/cleanup/investigation/skill-authoring/confirm)
  run /multi-agent-orchestration · own sub-agents · land or confirm
    ↓
sub-agents
  implementer · fix · reviewers · verifier · diagnostic
  + codex (architecture + 2 review legs; never code)
```

The CTO dispatches MANAGERS, never sub-agents. The CTO does not write code, run gates, read full diffs, parse raw codex output, write fix briefs, rebase, land, or dispatch implementer/fix/reviewer agents.

The CTO does not investigate source or originate repository-content facts from memory or from reports. Before a claim changes a brief, baseline, verdict, disposition, or REOPEN, it must pass Decision Admission. For ONE pre-stated binary claim per pending decision, the CTO may run ONE bounded read-only mechanical probe — git metadata, path existence, exact search, or count — with capped output and persisted input/result. A probe requiring chained queries, full-file/diff reading, source interpretation, runtime reproduction, or gate execution is not verification: the claim stays `HYPOTHESIS` and goes to a diagnostic/verify manager. The CTO reads paragraph reports and keeps context low.

The forcing point is the state change, not felt doubt — an optional probe never fires, because an orchestrator confident in a false premise sees no reason to check it.

Every block/stage/phase runs: manager-owned implementation → full 3/3 review → §1a verification → landing → independent confirm manager. A dependent phase starts only after confirmation plus any required integration-confirm.

## Cast / Account Roles

| Role | Owner | Access | Rule |
|---|---|---|---|
| CTO | interactive session | orchestration only | dispatch/decide/checkpoint; never implements |
| Block manager | Agent sub-agent (fresh ctx) | full worktree | owns one unit through land |
| Implementer/fix | Agent sub-agent | full | writes all code/tests/fixes; new commit per finding |
| Review legs x2 | codex | read-only | model + effort from `CODEX_MODEL_POLICY` (never chosen ad hoc), distinct lenses |
| Review leg x1 | Agent sub-agent (fresh ctx) | read-only | separate fresh agent from author; separate account when available/opt-in |
| §1a verifier | Agent sub-agent (fresh ctx) | full/throwaway | proves discrimination + gate + rule integrity |
| Confirm manager | Agent sub-agent (fresh ctx) | full/throwaway | independent post-land best-implementation gate |
| Architect/decider | codex only | read-only | all architecture forks; never code |
| Diagnostic | Agent sub-agent | throwaway/full | empirical check when reports conflict |

**Dispatch mechanism (default): the Agent/Task tool — gated on harness support.** The CTO spawns each manager as an Agent sub-agent; managers spawn their own implementer/reviewer/fix/verify (§1a) agents as Agent sub-agents (agents may spawn child agents — the manager→children topology). Managers NEVER spawn the confirm manager — only the CTO/MoM dispatches the separate unprimed confirm (and integration-confirm) MANAGER after land, keeping the post-land gate independent of the author. Each starts fresh with a self-contained brief as its prompt; its final message is its report. Agent is the default ONLY WHEN four capability properties are ESTABLISHED AND RECORDED (`reference/PROTOCOL.md` → Dispatch, which is authoritative): (a) no inherited transcript/hidden state beyond the passed prompt, (b) a distinct agent identity, (c) status/stop/continue control, and (d) child-agent spawning where the role needs it. Not "the harness GUARANTEES" — today that record is an ATTESTATION, a risk-accepted precondition and not a proof, and writing "guarantees" here while PROTOCOL says an unproven attestation authorizes dispatch would leave a session both required to fall back and permitted to proceed. Absent, failed, stale, or unknown ⇒ fall back to `claude -p` (a separate process is an explicit fresh boundary). `claude -p` is otherwise OPT-IN only — explicit user request, or a separate account instance for multi-instance parallelism / work that must outlive the parent session (default Agent mode is single-account harness-managed parallelism; `claude -p` restores multi-account instances). Codex is unchanged either way: a Bash-invoked CLI for the two review legs + architecture, never `claude -p`. The mechanism affects oversight-gate PROPERTIES — confirm independence, reviewer model quality, fresh-context isolation, durable auditability — not just transport. Three are enforced by RECORDED PROCEDURE (CTO-only confirm dispatch; the recorded model+effort binding; the persisted brief+report+input-id); fresh-context isolation rests on the attestation and is a risk-accepted precondition, not a proof. "Preserved" would overstate all four — see `reference/PROTOCOL.md` → Dispatch. Each gate rests on a recorded precondition, not an assertion: confirm/integration-confirm independence on CTO-only dispatch; Agent-default on the recorded `CAPABILITY … result=PASS` attestation (an assertion pending an executable probe, not a proof) (absent/stale ⇒ `claude -p`); reviewer/manager quality on the recorded highest-model+max-effort binding (unknown/default ⇒ BLOCK); auditability on the persisted brief+report+input-id+model+effort. A gate whose precondition is missing or stale is unmet.

Role separation is required; account separation is optional. On one account, use separate fresh Agent sub-agents (or fresh `-p` invocations on the opt-in path), neutral prompts, codex second opinions, and serialized heavy gates. Stop/escalate if no implementation agent capacity is available or codex is unavailable. Read live account mapping from the brief/ledger; never hard-code it.

## CTO Rules

- Architecture is always codex-owned. Modes differ only by whether codex's verdict is auto-adopted or user-ratified and where product/priority forks go.
- Never prime any reviewer, consult, verifier, confirmer, or adjudicator. Ask neutral questions; do not state the desired conclusion.
- A turn ends ONLY when you are blocked on a synchronous tool result you just invoked, or your work is committed and your report is written. Never yield while completion-critical owned work has no harness-guaranteed resume and no active synchronous join — codex runs blocking-foreground under a bounded timeout (`reference/PROTOCOL.md` → codex Invocation); a stalled leg looks exactly like a slow one.
- Memory is non-authoritative context. Operational commands and volatile identifiers (invocations, kill commands, model slugs, branches, paths) have exactly ONE current authority: the repository protocol/preflight. Memory may link to it, never duplicate or override it — memory is written by one session, executed by every later one, and reviewed by nobody (`reference/PROTOCOL.md` → Memory Is Not Authority).
- Cleanup terminates ONLY a PID/process tree recorded as owned by the current dispatch. Never by image name or pattern — that reaches the user's own sessions and the sibling review leg.
- No block is dispatched without a ratified intent contract: necessity, required/forbidden observable outcomes, authority/fallback order, per-acceptance-ID test or gate, performance bounds (`reference/PROTOCOL.md` → Intent Contract).
- Decision Admission gates every state-changing claim: `VERIFIED` with input-bound evidence, or `HYPOTHESIS` that may only dispatch diagnosis or a conditional fix. A report never verifies its own premise (`reference/PROTOCOL.md` → Decision Admission).
- The second REOPEN on the same acceptance/seam ID lapses that design's approval: no further fix dispatch until an unprimed codex `RETAIN`/`REDESIGN`/`REMOVE` ruling lands.
- Scope-deviating correctness findings get `ADOPT-NOW`/`DEFER`/`REJECT` before related work continues. A TODO or feedback entry is not a disposition.
- A gate passes only on fresh evidence it actually ran its intended surface; exit 0 alone is FAIL (`CLAUDE.md` → Verification Must Prove Execution).
- Every codex architecture/review/approval/adversarial/best-impl prompt prepends the mandate in `reference/PROTOCOL.md`.
- Multiple-choice/high-stakes architecture forks use two neutral codex legs; disagreement uses a third code-verifying codex decider. Claude implements; codex never writes code.
- Default Agent-tool dispatch is harness-managed: a blocking Agent call returns the manager's report; a background (`run_in_background`) Agent call notifies the CTO on completion — no watchdog, no liveness-by-mtime. The watchdog / foreground-poll discipline (`reference/WAIT-PROTOCOL.md`) applies ONLY to the opt-in `claude -p` path, where headless `-p` managers and sub-agents never background-then-yield.
- Every review round is 2 codex + 1 claude, parallel, neutral, distinct lenses, to 3/3 LAND or NIT-only carried forward. A carried-forward NIT is a DEFERRAL and obeys the deferral rule: it is ENUMERATED in the landing record (`PROGRESS.md`) with an owner and a closure point, or it is not carried at all — it is closed as won't-fix, on the record. "NIT-only" must never become an unenumerated bucket that findings quietly fall into; a residual nobody wrote down is a residual nobody owns. Designs/docs/skills get the same bar; skill/design/doc codex review rounds cap at 3. **The cap bounds polish, never correctness:** it exists to stop endless stylistic re-review, so it does NOT bound the rounds needed to clear a substantive or anti-rogue finding. Such a finding blocks, its fix is re-reviewed until clean, and round 4+ is REQUIRED — reading the cap as a ceiling on those rounds would deadlock the gate (the finding blocks, the fix needs review, the review is forbidden) and, worse, would let a substantive defect land by running out of rounds. A cap that can extinguish a blocking finding is a gate with a timer on it.
- After land, the CTO — never the block manager — dispatches a separate confirm MANAGER that must prove: correct/additive/full-gate green, not shallow, no stubs, and best implementation by unprimed codex judgment. `VERDICT:CONFIRMED` alone closes.
- Integration-confirm MANAGER runs at phase/milestone boundaries, before dependent phases, before final close-out, and after every 5 confirmed blocks. Only `VERDICT:INTEGRATION-CONFIRMED` closes a phase.
- Plan-end has zero open deferrals. Mid-plan deferrals require a codex-DEFER ruling and a `docs/arch` debt ledger row.
- Binding designs live in `docs/arch/<name>-design.md` and the master-plan locked-designs index; scratch-only designs are invalid.
- Repo cleanliness is prevent + remove, never per-file gitignore: orchestration state in `/tmp/mom` or `.feedback/`, worktrees outside repo, scoped `git add`, no `git add -A`.
- No plan/phase vocabulary in code/comments/tests or conventional commit messages; scrub at squash.
- Stage/phase cleanup happens only after land + confirmation: remove closed worktrees/temp, preserve durable records, verify clean status.
- Plan-end history purge of scratch/report clutter is a user-authorized destructive block with final user go-ahead at execution time.
- Rule text stays terse; new process rules require governance approval.

## Protocol Files

- `reference/PROTOCOL.md` — Verter overlay and full rule detail: governance, mandate, decision modes, dispatch, review, gates, invariants, cleanliness, anti-rogue, confirm/integration.
- `reference/LANDING-PROTOCOL.md` — pre-land sync, re-review triggers, design mirror, teeth'd squash, true ff, cleanup, CTO confirm handoff.
- `reference/CHECKPOINT-PROTOCOL.md` — append-only progress ledger, artifact validity, idempotence, corruption recovery.
- `reference/WAIT-PROTOCOL.md` — the turn rule (universal: never background-and-yield, on ANY path), per-path waiting, and the headless `-p` foreground chunked poll-loop.
