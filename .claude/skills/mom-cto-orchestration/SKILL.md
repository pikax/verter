---
name: mom-cto-orchestration
description: >-
  CTO/manager-of-managers methodology for autonomous multi-train plans where the user says
  "you are the MoM/CTO", "orchestrate the whole plan", "drive the migration end-to-end",
  "manager-of-managers", or "dispatch managers". The CTO dispatches one implementation
  MANAGER per landing train, each manager runs `/multi-agent-orchestration`, and the CTO
  schedules the review/verify/landing/confirm jobs and adds independent confirm/integration
  gates, codex-owned architecture decisions, governance, and anti-rogue rule defenses. For a
  single-train task use `/multi-agent-orchestration` directly.
---

# MoM / CTO Orchestration

This is the layer ABOVE `/multi-agent-orchestration`. Load that skill first; it is the implementation-manager manual. This skill adds the CTO tier: implementation managers, CTO-scheduled gate/review jobs, confirm managers, integration-confirm, codex architecture adjudication, governance, anti-rogue rule defense, and portable account-role discipline.

## Tier Model

```
CTO / MoM (interactive)
  decompose · dispatch managers · schedule SHA-bound review/§1a/verifier/landing/confirm jobs
  · codex architecture forks · read terse durable summaries · checkpoint · advance
    ↓
MANAGERS (one per train / cleanup / investigation / skill-authoring,
  plus the CTO-dispatched confirm / integration-confirm managers)
  run /multi-agent-orchestration · own implementation + comprehensive fix rounds · report
    ↓
sub-agents
  implementer · fix · diagnostic (manager-owned)
  reviewers ×3 · §1a verifier (CTO-scheduled gate jobs, author-independent)
```

The confirm and integration-confirm managers are CTO-dispatched MANAGER-tier units, not leaf sub-agents; the leaf-tier author-independent gate jobs are the reviewers ×3 + §1a verifier only.

The CTO dispatches MANAGERS, never implementation sub-agents. The implementation manager owns implementation and comprehensive fix rounds; the CTO owns SCHEDULING of the SHA-bound review, §1a, verifier, landing, and confirmer jobs. Each job persists full evidence (raw logs, full reports) and returns a terse durable summary; the CTO consumes summaries — it does not write code, execute heavy commands, ingest raw logs, read full diffs, write fix briefs, rebase, land, or investigate source. Cheap git/status/report checks are allowed; source grep/read goes to a manager. No implementation manager owns a long-running gate/review waiter.

Lifecycle: a **slice** is a bounded TDD change — targeted tests, focused author feedback, one clean separately-testable conventional commit. A **landing train** is a cohesive sequence of slices receiving ONE cumulative three-review barrier, one §1a mutation-recipe set, one canonical final gate, one landing, and one independent confirmation. A **milestone** is a dependency/integration boundary grouping one or more landing trains, where integration-confirm gates before dependent work proceeds. Never confirm each slice as its own train. During a train's confirmation the CTO may reserve capacity and run extraction or provisional design for the next train; no implementation relies on the preceding train as confirmed until `VERDICT:CONFIRMED`, and integrated work never advances more than one unconfirmed train deep. A dependent train starts only after confirmation plus any required integration-confirm at its milestone boundary.

## Cast / Account Roles

| Role | Owner | Access | Rule |
|---|---|---|---|
| CTO | interactive session | orchestration only | dispatch/schedule/decide/checkpoint; never implements |
| Implementation manager | Agent sub-agent (fresh ctx) | full worktree | owns one train's implementation + fix rounds |
| Implementer/fix | Claude/Fable Agent sub-agent OR GPT/Codex `codex exec` write | full worktree | measured bakeoff winner; a GPT author runs in an isolated write-enabled worktree; no author reviews or confirms its own work |
| Reviewers ×3 | author-dependent mix: Claude/Fable author → 2 GPT + 1 Claude; GPT/Codex author → 2 Claude + 1 fresh GPT | read-only | independent, blind, parallel, one assigned lens each; author/design-adversary/confirmer never count as reviewers |
| §1a verifier | Agent sub-agent (fresh ctx) | full/throwaway | executes the mutation recipes + rule integrity |
| Confirm manager | Agent sub-agent (fresh ctx) | full/throwaway | independent post-land ratified-contract gate |
| Architect/decider | codex (`gpt-5.6-sol`) | read-only | all architecture forks; the architect/decider, reviewer, and confirmer seats never write code |
| Diagnostic | Agent sub-agent | throwaway/full | empirical check when reports conflict |

**Dispatch mechanism (default): the Agent/Task tool — gated on harness support.** The CTO spawns each manager as an Agent sub-agent; managers spawn their implementer/fix/diagnostic agents as Agent sub-agents (agents may spawn child agents — the manager→children topology). Reviewer, §1a, verifier, and landing jobs are CTO-scheduled; managers NEVER spawn them, and NEVER spawn the confirm manager — only the CTO/MoM dispatches the separate unprimed confirm (and integration-confirm) MANAGER after land, keeping every gate independent of the author. Each starts fresh with a self-contained brief as its prompt; its final message is its report. Agent is the default ONLY WHEN the harness guarantees (a) no inherited transcript/hidden state beyond the passed prompt, (b) a distinct agent identity, (c) status/stop/continue control, and (d) child-agent spawning where the role needs it; if any property is absent or unknown, fall back to `claude -p` (a separate process is an explicit fresh boundary). `claude -p` is otherwise OPT-IN only — explicit user request, or a separate account instance for multi-instance parallelism / work that must outlive the parent session (default Agent mode is single-account harness-managed parallelism; `claude -p` restores multi-account instances). Codex seats split by role: architecture/review/confirm invocations are read-only Bash-invoked CLI subprocesses (never `claude -p`); a GPT/Codex IMPLEMENTER seat is a separate write-enabled `codex exec` worktree invocation and can never be the same invocation as any architecture/review/confirm seat. The mechanism affects oversight-gate PROPERTIES — confirm independence, reviewer model quality, fresh-context isolation, durable auditability — not just transport; those are preserved by the safeguards here, not by the swap alone. Each gate rests on a recorded precondition, not an assertion: confirm/integration-confirm independence on CTO-only dispatch; Agent-default on the recorded `CAPABILITY … result=PASS` proof (absent/stale ⇒ `claude -p`); review/verify/confirm/architect quality on the recorded highest-model+max-effort binding (unknown/default ⇒ BLOCK; implementer/fix binds the measured author policy instead); auditability on the persisted brief+report+input-id+model+effort. A gate whose precondition is missing or stale is unmet.

Role separation is required; account separation is optional. On one account, use separate fresh Agent sub-agents (or fresh `-p` invocations on the opt-in path), neutral prompts, cross-model checks, and serialized heavy gates. Stop/escalate if no implementation agent capacity is available or codex is unavailable. Read live account mapping from the brief/ledger; never hard-code it.

## CTO Rules

- Architecture is always codex-owned. Modes differ only by whether codex's verdict is auto-adopted or user-ratified and where product/priority forks go.
- Never prime any reviewer, consult, verifier, confirmer, or adjudicator. Ask neutral questions; do not state the desired conclusion.
- Every codex architecture/review/approval/adversarial prompt prepends the mandate in `reference/PROTOCOL.md` — the open-decision bar for design forks, the ratified-contract bar for review/landing/confirm.
- Multiple-choice/high-stakes architecture forks use two neutral codex legs; disagreement uses a third code-verifying codex decider. Claude/Fable or GPT/Codex may implement; architecture, review, verification, and confirmation stay independent of the author — those seats are read-only and never write code, and a GPT implementer seat does not make them write-enabled.
- Default Agent-tool dispatch is harness-managed: a blocking Agent call returns the manager's report; a background (`run_in_background`) Agent call notifies the CTO on completion — no watchdog, no liveness-by-mtime. The watchdog / foreground-poll discipline (`reference/WAIT-PROTOCOL.md`) applies ONLY to the opt-in `claude -p` path, where headless `-p` managers and sub-agents never background-then-yield.
- Every train review round is three independent blind reviewers — author-dependent mix (Claude/Fable author → 2 GPT + 1 Claude; GPT/Codex author → 2 Claude + 1 fresh GPT) — parallel, neutral, with MANDATORY distinct lenses: (A) semantic parity / oracle validity / coverage-dimension completeness; (B) architecture / typed-IR ownership / fail-closed / rule integrity; (C) host integration / caching / source maps / runtime behavior / regression blast radius. Reviewed to a final clean 3/3 LAND (or NIT-only carried forward) over the complete cumulative diff. Designs/docs/skills get the same bar; skill/design/doc review rounds cap at 3 — after the round bound, only P3/NIT residuals may be carried forward WITHOUT changing the reviewed tree; any substantive, anti-rogue, or content-changing finding still requires another full clean 3/3 cumulative round. The cap bounds cosmetic-residual churn, never the final-clean-3/3-on-content-change rule.
- After a train lands, the CTO — never the implementation manager — dispatches a separate confirm MANAGER gating correctness, ratified-contract compliance, all critical invariants, executable obligations, fail-closed behavior, discriminating tests (independently re-executing the mutation recipes), and anti-rogue integrity. A merely preferable architecture is optional debt and never reopens; new evidence of a correctness, safety, scalability, or invariant failure does. Confirmation also runs a separate unprimed, read-only, highest-model/max-effort codex adversarial leg (correctness / CRITICAL-rule / fail-open / mutation / anti-rogue; preferable-architecture findings non-blocking). `VERDICT:CONFIRMED` alone closes.
- Integration-confirm MANAGER — a cross-train coherence check distinct from per-train confirm — runs at milestone/dependency boundaries, before any dependent train relies on integrated work, before final close-out, and as a periodic floor after every five confirmed landing trains. Only `VERDICT:INTEGRATION-CONFIRMED` closes a milestone.
- Release scope is a FROZEN finite train manifest with a FIXED denominator. Dispatch NO feature train until the supported-release manifest AND the exact remaining train DAG/dependencies are frozen and explicitly USER-ratified. Classify EVERY discovery via the five-way scope-admission policy: (1) blocking defect (incorrect/fail-open inside the supported surface) → fix in the owning train; (2) invariant defect → fix before landing; (3) required acceptance row already implied by the frozen contract → fold into the owning train, no new landing lifecycle; (4) unsupported completeness (safely, exactly refused) → record post-release, fail-closed; (5) optional architecture improvement → non-blocking unless current code is incorrect, unsafe, unscalable, or violates a ratified invariant. NO new critical-path train without explicit USER approval; never report against a denominator that can grow silently.
- Release close requires zero correctness/invariant debt in the supported surface, zero fail-open, and exact fail-closed coverage outside it; explicitly classified post-release completeness debt and optional architecture improvements may remain, and no supported-surface correctness, safety, scalability, invariant, or executable-obligation defect may be relabeled as completeness debt. Mid-release deferrals require a codex-DEFER ruling and a `docs/arch` debt ledger row.
- Every CTO progress checkpoint records: frozen-manifest content identity; total/confirmed/active/remaining trains; blocking acceptance rows open/closed; scope additions since the last checkpoint; active implementation time vs queue/wait/review/gate time; review rounds + initial P0/P1 counts; confirm-reopen count; the exact next finish condition.
- Binding designs live in `docs/arch/<name>-design.md` and the master-plan locked-designs index; scratch-only designs are invalid.
- Repo cleanliness is prevent + remove, never per-file gitignore: orchestration state in `/tmp/mom` or `.feedback/`, worktrees outside repo, scoped `git add`, no `git add -A`.
- No plan/phase vocabulary in code/comments/tests or conventional commit messages; scrub at commit consolidation. Process prose and status schemas use train/slice/milestone.
- Train cleanup happens only after land + confirmation: remove closed worktrees/temp, preserve durable records, verify clean status.
- Release-close history purge of scratch/report clutter is a user-authorized destructive operation with final user go-ahead at execution time.
- Rule text stays terse; new process rules require governance approval.

## Protocol Files

- `reference/PROTOCOL.md` — Verter overlay and full rule detail: governance, mandate, decision modes, dispatch, measured author selection, design adversary, review, gates, landing lease, invariants, cleanliness, anti-rogue, confirm/integration.
- `reference/LANDING-PROTOCOL.md` — pre-land sync, re-review triggers, design mirror, train commit preparation, true ff, cleanup, CTO confirm handoff.
- `reference/CHECKPOINT-PROTOCOL.md` — append-only progress ledger, artifact validity, idempotence, corruption recovery.
- `reference/WAIT-PROTOCOL.md` — OPT-IN `claude -p` path only: headless `-p` waiting via foreground chunked poll-loop; no background-then-yield. Default Agent-tool dispatch is harness-managed and needs none of it.
