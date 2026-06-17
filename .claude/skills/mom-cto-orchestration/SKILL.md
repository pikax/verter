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

The CTO dispatches MANAGERS, never sub-agents. The CTO does not write code, run gates, read full diffs, parse raw codex output, write fix briefs, rebase, land, investigate source, or dispatch implementer/fix/reviewer agents. Cheap git/status/report checks are allowed; source grep/read goes to a manager. The CTO reads paragraph reports and keeps context low.

Every block/stage/phase runs: manager-owned implementation → full 3/3 review → §1a verification → landing → independent confirm manager. A dependent phase starts only after confirmation plus any required integration-confirm.

## Cast / Account Roles

| Role | Owner | Access | Rule |
|---|---|---|---|
| CTO | interactive session | orchestration only | dispatch/decide/checkpoint; never implements |
| Block manager | Agent sub-agent (fresh ctx) | full worktree | owns one unit through land |
| Implementer/fix | Agent sub-agent | full | writes all code/tests/fixes; new commit per finding |
| Review legs x2 | codex | read-only | latest/high reasoning, distinct lenses |
| Review leg x1 | Agent sub-agent (fresh ctx) | read-only | separate fresh agent from author; separate account when available/opt-in |
| §1a verifier | Agent sub-agent (fresh ctx) | full/throwaway | proves discrimination + gate + rule integrity |
| Confirm manager | Agent sub-agent (fresh ctx) | full/throwaway | independent post-land best-implementation gate |
| Architect/decider | codex only | read-only | all architecture forks; never code |
| Diagnostic | Agent sub-agent | throwaway/full | empirical check when reports conflict |

**Dispatch mechanism (default): the Agent/Task tool — gated on harness support.** The CTO spawns each manager as an Agent sub-agent; managers spawn their own implementer/reviewer/fix/verify (§1a) agents as Agent sub-agents (agents may spawn child agents — the manager→children topology). Managers NEVER spawn the confirm manager — only the CTO/MoM dispatches the separate unprimed confirm (and integration-confirm) MANAGER after land, keeping the post-land gate independent of the author. Each starts fresh with a self-contained brief as its prompt; its final message is its report. Agent is the default ONLY WHEN the harness guarantees (a) no inherited transcript/hidden state beyond the passed prompt, (b) a distinct agent identity, (c) status/stop/continue control, and (d) child-agent spawning where the role needs it; if any property is absent or unknown, fall back to `claude -p` (a separate process is an explicit fresh boundary). `claude -p` is otherwise OPT-IN only — explicit user request, or a separate account instance for multi-instance parallelism / work that must outlive the parent session (default Agent mode is single-account harness-managed parallelism; `claude -p` restores multi-account instances). Codex is unchanged either way: a Bash-invoked CLI for the two review legs + architecture, never `claude -p`. The mechanism affects oversight-gate PROPERTIES — confirm independence, reviewer model quality, fresh-context isolation, durable auditability — not just transport; those are preserved by the safeguards here, not by the swap alone. Each gate rests on a recorded precondition, not an assertion: confirm/integration-confirm independence on CTO-only dispatch; Agent-default on the recorded `CAPABILITY … result=PASS` proof (absent/stale ⇒ `claude -p`); reviewer/manager quality on the recorded highest-model+max-effort binding (unknown/default ⇒ BLOCK); auditability on the persisted brief+report+input-id+model+effort. A gate whose precondition is missing or stale is unmet.

Role separation is required; account separation is optional. On one account, use separate fresh Agent sub-agents (or fresh `-p` invocations on the opt-in path), neutral prompts, codex second opinions, and serialized heavy gates. Stop/escalate if no implementation agent capacity is available or codex is unavailable. Read live account mapping from the brief/ledger; never hard-code it.

## CTO Rules

- Architecture is always codex-owned. Modes differ only by whether codex's verdict is auto-adopted or user-ratified and where product/priority forks go.
- Never prime any reviewer, consult, verifier, confirmer, or adjudicator. Ask neutral questions; do not state the desired conclusion.
- Every codex architecture/review/approval/adversarial/best-impl prompt prepends the mandate in `reference/PROTOCOL.md`.
- Multiple-choice/high-stakes architecture forks use two neutral codex legs; disagreement uses a third code-verifying codex decider. Claude implements; codex never writes code.
- Default Agent-tool dispatch is harness-managed: a blocking Agent call returns the manager's report; a background (`run_in_background`) Agent call notifies the CTO on completion — no watchdog, no liveness-by-mtime. The watchdog / foreground-poll discipline (`reference/WAIT-PROTOCOL.md`) applies ONLY to the opt-in `claude -p` path, where headless `-p` managers and sub-agents never background-then-yield.
- Every review round is 2 codex + 1 claude, parallel, neutral, distinct lenses, to 3/3 LAND or NIT-only carried forward. Designs/docs/skills get the same bar; skill/design/doc codex review rounds cap at 3 except substantive or anti-rogue findings.
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
- `reference/WAIT-PROTOCOL.md` — OPT-IN `claude -p` path only: headless `-p` waiting via foreground chunked poll-loop; no background-then-yield. Default Agent-tool dispatch is harness-managed and needs none of it.
