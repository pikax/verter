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
| Implementer/fix | Agent sub-agent | full | writes all code/tests/fixes; new commit per finding; receives one decided solution + steps, makes no architecture/solution choice |
| Review leg — claims-aware codex x1 | codex | read-only | latest/high reasoning; ADVERSARIAL on the change's OWN stated claims (untrusted assertions to TRY to refute, default-skeptical; true claims may survive). A bounded, audited lens — claims supplied only as attack targets, never author narrative or a conclusion to confirm — so consistent with Never-Prime. Full bound: `reference/PROTOCOL.md` → Review Cadence |
| Review leg — unprimed codex x1 | codex | read-only | latest/high reasoning; BLIND + neutral-broad — artifact with claims/intent withheld, hunts HARSHLY for ANY defect; burden-of-proof, not an outcome prior (LANDs only if the artifact survives genuine scrutiny — "neutral-broad" = un-claim-narrowed lens, NOT lower rigor; never licensed to skip attacking the artifact), NEVER primed |
| Review leg — adversarial claude x1 | Agent sub-agent (fresh ctx) | read-only | ADVERSARIAL, refute-first (CLAUDE-REVIEWER MANDATE); separate fresh agent from author; LAND only when it tried to break it and could not |
| §1a verifier | Agent sub-agent (fresh ctx) | full/throwaway | proves discrimination + gate + rule integrity |
| Confirm manager | Agent sub-agent (fresh ctx) | full/throwaway | independent post-land best-implementation gate |
| Architect/decider | codex only | read-only | all architecture forks; never code |
| Diagnostic | Agent sub-agent | throwaway/full | empirical check when reports conflict |

**Dispatch mechanism (default): the Agent/Task tool — gated on harness support.** The CTO spawns each manager as an Agent sub-agent; managers spawn their own implementer/reviewer/fix/verify (§1a) agents as Agent sub-agents (agents may spawn child agents — the manager→children topology). Managers NEVER spawn the confirm manager — only the CTO/MoM dispatches the separate unprimed confirm (and integration-confirm) MANAGER after land, keeping the post-land gate independent of the author. Each starts fresh with a self-contained brief as its prompt; its final message is its report. Agent is the default ONLY WHEN the harness guarantees (a) no inherited transcript/hidden state beyond the passed prompt, (b) a distinct agent identity, (c) status/stop/continue control, and (d) child-agent spawning where the role needs it; if any property is absent or unknown, fall back to `claude -p` (a separate process is an explicit fresh boundary). `claude -p` is otherwise OPT-IN only — explicit user request, or a separate account instance for multi-instance parallelism / work that must outlive the parent session (default Agent mode is single-account harness-managed parallelism; `claude -p` restores multi-account instances). Codex is unchanged either way: a Bash-invoked CLI for the two review legs + architecture, never `claude -p`. The mechanism affects oversight-gate PROPERTIES — confirm independence, reviewer model quality, fresh-context isolation, durable auditability — not just transport; those are preserved by the safeguards here, not by the swap alone. Each gate rests on a recorded precondition, not an assertion: confirm/integration-confirm independence on CTO-only dispatch; Agent-default on the recorded `CAPABILITY … result=PASS` proof (absent/stale ⇒ `claude -p`); reviewer/manager quality on the recorded highest-model+max-effort binding (unknown/default ⇒ BLOCK); auditability on the persisted brief+report+input-id+model+effort. A gate whose precondition is missing or stale is unmet.

Role separation is required; account separation is optional. On one account, use separate fresh Agent sub-agents (or fresh `-p` invocations on the opt-in path), neutral prompts, codex second opinions, and serialized heavy gates. Stop/escalate if no implementation agent capacity is available or codex is unavailable. Read live account mapping from the brief/ledger; never hard-code it.

## CTO Rules

- Architecture is always codex-owned. Modes differ only by whether codex's verdict is auto-adopted or user-ratified and where product/priority forks go.
- Implementer briefs carry ONE decided path + steps, never a menu: managers decide routine implementation choices directly and route any architecture / high-stakes-design / public-behavior / cross-module-contract / performance-cache / ownership / plan-deviation choice NOT already settled by an approved binding plan/design or a prior codex verdict through the existing codex decision modes — regardless of manager confidence, never self-declaring it "resolved" — before briefing; implementers never choose the architecture/solution.
- Never prime any reviewer, consult, verifier, confirmer, or adjudicator. Ask neutral questions; do not state the desired conclusion.
- Consult Discipline (full rule: `reference/PROTOCOL.md` → Consult Discipline; purely additive resolve step — relaxes no existing trigger): on MATERIAL UNRESOLVED doubt about the best architecture/design/mechanism (not settled by an approved binding plan/design or a prior valid codex verdict; verified evidence settles only factual/mechanical sub-questions and never substitutes for codex on a best-design judgement) consult the codex architect, never guess — on top of, never a softening of, the existing unconditional routing of unsettled architecture choices; a settled verdict is not re-consulted. Every such consult is UNPRIMED/NEUTRAL (exhaustive options if any are offered), framing VERIFIED before send (mis-framed ⇒ verdict VOID), and DEMANDS THE BEST with the explicit best-not-lowest-effort framing (best ON THE MERITS; implementation effort as accounting — diff-size / migration effort/breadth / files-touched — is NOT a selection criterion: effort-neutral, never tilting toward a minimal change nor toward over-engineering, while architecture-relevant migration RISK stays a merit) also dispatcher-VERIFIED before send, and runs IN PARALLEL with non-dependent, non-resource-conflicting work.
- Structural-Confinement-First (full rule: `reference/PROTOCOL.md` → Structural-Confinement-First): identity/confinement guards for Rust items/types/modules default to compiler/structural enforcement (privacy/visibility/`E0603`, type-state, sealed traits, marker-trait derives), not name-spelling scanners (aliases/re-exports/shadowing/`cfg`/macros launder identity). A scanner is allowed only for an invariant/residue no compiler/structural mechanism can express (incl. a supplement to a structural primary) — that "no mechanism fits" judgement routes through the architecture rail (approved plan / codex ruling), never an implementer self-certification, and a new/modified scanner carries a durable guard-local record (`scanner_invariant`/`scanner_justification`/`mechanism_ruling`/`hardening_rounds=<n>`/`hardening_history`; missing/non-auditable count blocks further scanner changes). Scanner hardening is bounded (full mechanics in canonical): after two hardening rounds (add OR broaden, any trigger) or any laundering escape, no further add/broaden — replace structural, or a codex ruling permits only non-broadening maintenance (and on a laundering escape the guard's documented claim narrows to what it actually enforces); a later add/broaden need reopens the structural decision, never a third round; a `docs/arch` debt row is TEMPORARY scanner debt only. A purely syntactic call-shape ban and counts of compiler/structural facts stay allowed, but a ban/count keyed to a spelled item/type/module/path stays an identity guard under this rule. Subject to GOVERNANCE and Consult Discipline.
- Test-Tightening 80/20 Bound (full rule, rationale, examples, and the exact codex-gated mechanics: `reference/PROTOCOL.md` → Test-Tightening 80/20 Bound): after two auditable substantive TEST-ONLY tightening rounds for the same behavioral test-coverage claim, a third-round decision belongs to neutral/unprimed codex (codex assesses; it is never told the answer). NON-behavioral safeguard-test claims (structural/invariant/scanner-parity/identity/...) get the SAME two-round bound via the self-contained safeguard-test counterpart trigger in PROTOCOL — they do NOT escape the rail by being non-behavioral. TEST-side only — never production code, rule/plan guards, architecture-guard mechanisms, or the SC-first scanner MECHANISM (the scanner detection predicate stays SC-first even inside a `#[test]`; only its fixtures/assertions are lean-test). Keep SAFEGUARD-TESTS lean in all phases, classified by ROLE not directory; COMMON/contract/prior-defect/severity coverage is MANDATORY, and "lean"/"STOP" bound only the EDGE-residual chase, never a basis to skip required coverage. A contested role (or any relabel that would lower the coverage bar) defaults to full APPLICABLE rigor for the underlying claim (behavioral/contract/structural/invariant/security/compat — "behavioral" only when the claim is behavioral) and routes to codex; an edge residual needs a codex 80/20 classification FIRST, then codex's disposition (terminal-accept or DEFER-debt) — never a manager-unilateral acceptance bypassing the r3 codex classification. A ballooning guard-TEST is a SIGNAL that routes that same two-round hardening to the architecture rail, where codex decides production-design-simplify vs rich-coverage on the merits (not a predetermined "always simplify"); SC-first still holds. Production rigor, no-stub/discrimination, and the §1a/review/confirm/valid-debt gates all still bind.
- Every codex architecture/review/approval/adversarial/best-impl prompt prepends the CODEX-ARCHITECT MANDATE (`reference/PROTOCOL.md`); every claude-reviewer dispatch prepends the CLAUDE-REVIEWER MANDATE (`reference/CLAUDE-REVIEWER-MANDATE.md`).
- Multiple-choice/high-stakes architecture forks use two neutral codex legs; disagreement uses a third code-verifying codex decider. Claude implements; codex never writes code.
- Default Agent-tool dispatch is harness-managed: a blocking Agent call returns the manager's report; a background (`run_in_background`) Agent call notifies the CTO on completion — no watchdog, no liveness-by-mtime. The watchdog / foreground-poll discipline (`reference/WAIT-PROTOCOL.md`) applies ONLY to the opt-in `claude -p` path, where headless `-p` managers and sub-agents never background-then-yield.
- Every review round is 1 ADVERSARIAL claude + 1 claims-aware codex + 1 unprimed codex, parallel, distinct lenses, to 3/3 LAND or NIT-only carried forward (full composition, bounds, and rationale: `reference/PROTOCOL.md` → Review Cadence). All three are harsh; they differ by WHAT each attacks, for lens diversity — TWO adversarial legs (default-to-reject) + ONE blind leg (burden-of-proof): the claude leg is refute-first on the artifact/change (CLAUDE-REVIEWER MANDATE; LAND only when it tried to break it and could not), reviewing the whole artifact rather than being handed the claims as a checklist to confirm; the claims-aware codex leg attacks the change's OWN stated claims as untrusted assertions to refute (a bounded, audited lens — ALL the change's stated claims supplied as a source-spanned inventory, only as attack targets, never author narrative or a steer toward a conclusion, so it stays consistent with Never-Prime); the unprimed codex leg is BLIND + neutral-broad, hunting HARSHLY for ANY defect with claims/intent withheld (LANDs only if the artifact survives genuine scrutiny — "neutral-broad" = un-claim-narrowed lens, NOT lower rigor; "not refute-first" only = no supplied claims to refute, never a license to skip attacking the artifact) and NEVER primed. Designs/docs/skills get the same bar; skill/design/doc codex review rounds cap at 3 except substantive or anti-rogue findings.
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
- `reference/CLAUDE-REVIEWER-MANDATE.md` — the adversarial refute-first mandate prepended to every claude-reviewer dispatch (per-block 3/3 claude leg, independent confirm leg, integration-confirm leg, fix re-reviews).
- `reference/LANDING-PROTOCOL.md` — pre-land sync, re-review triggers, design mirror, teeth'd squash, true ff, cleanup, CTO confirm handoff.
- `reference/CHECKPOINT-PROTOCOL.md` — append-only progress ledger, artifact validity, idempotence, corruption recovery.
- `reference/WAIT-PROTOCOL.md` — OPT-IN `claude -p` path only: headless `-p` waiting via foreground chunked poll-loop; no background-then-yield. Default Agent-tool dispatch is harness-managed and needs none of it.
