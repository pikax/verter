---
name: multi-agent-orchestration
description: >-
  Methodology for driving a large multi-train plan, refactor, or staged migration end-to-end
  and autonomously by acting as a pure orchestrator that delegates to sub-agents and gates every
  landing train on independent review. Covers the orchestrator role (decompose / brief / schedule /
  verify — never implement directly), the sub-agent cast (implementer, reviewers, fix-agent,
  extraction, diagnostic), the slice/train workflow (brief → implement → verify git state →
  three-review barrier → fix cycle → §1a + final canonical gate → land), three independent
  blind reviewers in an
  author-dependent cross-model mix, per-train fix cycles gated on a final clean 3/3 re-review,
  second-opinion consults for any architectural doubt, trust-but-verify, and STOP-and-escalate.
  Use this skill whenever you are handed a multi-train plan, a handoff document, a staged
  refactor/migration, or any large autonomous effort spanning multiple trains or PRs that needs
  sub-agent delegation — even if the user only says "orchestrate", "run this plan", "execute the
  handoff", or "drive this to completion". Also use it for a single substantial train that needs
  the implement → three-review → fix-cycle → land loop. Do not improvise large-plan execution
  ad hoc — load this skill.
---

# Multi-Agent Orchestration

Methodology for driving a large plan — a multi-train refactor, migration, or staged rollout — to completion autonomously, when the work is too large for one agent in one context window.

Core move: one **orchestrator** + many **sub-agents**. Orchestrator never writes code — decomposes the plan into landing trains of bounded slices, briefs sub-agents, verifies what comes back, gates every train on independent review, escalates doubt to a second-opinion model, manages git/branch/worktree/state. Sub-agents implement, review, fix, investigate — each a bounded, disposable job.

**Dispatch mechanism (default): the Agent/Task tool — gated on harness support.** Spawn every sub-agent — implementer, reviewer, fix-agent, extraction, diagnostic — via the Agent tool; each starts fresh with its brief as the prompt, and its final message is its report (a blocking call returns it; a background call notifies on completion). Agents may spawn child agents, so an orchestrator that is itself an agent still owns its full sub-agent cast. The Agent tool is the default ONLY WHEN the harness guarantees (a) no inherited transcript/hidden state beyond the passed prompt, (b) a distinct agent identity, (c) status/stop/continue control, and (d) child-agent spawning where the role needs it; if any property is absent or unknown, fall back to `claude -p` for an explicit fresh-process boundary. `claude -p` is otherwise OPT-IN only — explicit user request, or a separate account instance for multi-instance parallelism / out-of-session work. The second-opinion model (codex) is a separate external CLI either way. For gate-bearing dispatches (independent review and any verification gate) persist the exact prompt and the verbatim returned report to files with a recorded input id, model, and effort — inline-only is not allowed where a finding gates landing. Reviewer, verifier (§1a), and confirmer roles run on the highest available (contract-designated) model at maximum effort, bound by an explicit model/effort arg at spawn or an audited agent-definition recorded with the dispatch; an unknown or default model/effort BLOCKS a gate-bearing dispatch — never a silent downgrade. Any future reduction of a gate-role model requires a ratified blinded non-inferiority gate first — none is currently adopted. Implementation and fix authors may be Claude/Fable OR GPT/Codex, selected by measured results: before the bakeoff, fable-5 is the Claude default (Opus only on capacity limit); after it, retain the winner while fix-round, escaped-finding, disagreement, false-positive, and mutation-failure telemetry supports the choice. The mechanism affects gate PROPERTIES — reviewer independence, model quality, fresh-context isolation, durable auditability — not just transport; those are preserved by the safeguards here (the harness-guarantee gate + `claude -p` fallback, the model bindings, persisted prompt+report+model+effort) together with the three-review barrier, the fix cycle, and discriminating-tests-only, not by the swap alone.

## Why this shape

A single agent on a 20-step plan degrades three ways: early decisions fall out of context, raw tool output crowds reasoning, no second pair of eyes.

- **Orchestrator's context stays clean.** Reads *reports and summaries* (a paragraph each), never raw test output or 2000-line files — can coordinate a plan far larger than one context window.
- **Sub-agents are cheap and focused.** Each starts fresh, does one bounded job, reports, is discarded.
- **Every train is independently reviewed.** The author is never the sole judge.

Slices vs trains: a **slice** is a bounded TDD change with targeted tests and one clean, separately-testable conventional commit; a **landing train** is a cohesive sequence of slices sharing ONE cumulative three-review barrier, one §1a mutation-recipe set, one canonical final gate, one landing, and one independent confirmation. Use targeted TDD + focused review per slice, and the complete lifecycle once per train — including a single substantial train outside a multi-train plan.

## The orchestrator role

**Only**: decompose the plan into trains/slices; write briefs; dispatch implementation sub-agents; schedule review, verification, landing, and confirmation jobs and consume their terse durable summaries (a standalone-orchestrator responsibility — hoisted to the CTO in the nested case below); read reports; verify git state; make decisions; consult the second-opinion model on doubt; manage branches, worktrees, plan-state file.

**Never**: write/edit production code; run heavy commands or the test suite; parse raw logs; investigate a bug by reading source; review a diff line-by-line; hand a long-running gate/review waiter to the implementation manager. The instant the orchestrator does the work, its context fills with detail and it loses the plot. The implementation manager — the nested name for this loop's driver under a CTO (see the split below) — owns authoring and comprehensive fix commits only.

Lightweight mechanics the orchestrator *may* do directly: `git worktree add/remove`, `git cherry-pick`, `git log`/`show`/`status`, commit a plan-state file, read agent/review report files, write briefs and consult prompts.

**Standalone vs nested (implementation manager).** When this loop runs STANDALONE, the orchestrator owns review scheduling and landing exactly as written here. When it runs as an implementation MANAGER under a CTO (`/mom-cto-orchestration`) — "implementation manager" is the nested name for this loop's driver — review, §1a, verification, landing, and confirmation are HOISTED to the CTO: the manager owns implementation, comprehensive fix commits, and targeted iteration gates ONLY, and never schedules reviewers, lands, or spawns the confirmer. The "Only" list's review/verification/landing/confirmation scheduling and the workflow's three-review-barrier and land steps are standalone-orchestrator responsibilities the CTO tier reassigns; the rest of the loop applies unchanged in both cases.

## The cast

| Agent | Job | Tool access |
|---|---|---|
| **Orchestrator** | Decompose, brief, dispatch, schedule jobs, verify, decide, manage git/state | this session |
| **Implementer** | Implement one slice/train from a self-contained brief — Claude/Fable or GPT/Codex per the measured author policy (a GPT author works in an isolated write-enabled worktree) | full |
| **Reviewers ×3** | Independent, blind, harsh production-bar review of the immutable cumulative train tree — author-dependent mix (Claude/Fable author → 2 GPT + 1 Claude; GPT/Codex author → 2 Claude + 1 fresh GPT), one assigned lens each | read-only |
| **Fix-agent** | Consume one review round's consolidated findings, apply fixes as one comprehensive new commit | full |
| **Extraction agent** | Gather/extract source into one file | read-only |
| **Diagnostic agent** | Empirically reproduce + instrument a failure to establish ground truth | full, throwaway edits |
| **Second-opinion model** | Architectural consults + cross-model reviewer legs (codex when a Claude/Fable authors; Claude when a GPT/Codex authors) | external CLI (`codex`) for consults + the codex legs; Agent sub-agent / `claude -p` for the Claude legs when a GPT/Codex authors |

The table lists the core-loop sub-agents; the **§1a verifier** and **confirmer** are the additional author-independent gate roles named in the workflow (steps 9 and 12) — standalone-orchestrator-scheduled here, CTO-scheduled in the nested case — defined in the MoM overlay (`/mom-cto-orchestration` + its PROTOCOL).

The **second-opinion model** is load-bearing, not optional — the escalation path for every doubt and a source of cross-model reviewer legs (codex supplies them when a Claude/Fable authors; Claude supplies the cross-model majority when a GPT/Codex authors). One model judging its own work shares blind spots; judges on a different model break that correlation. Treat it as a peer architect, not a linter. No author reviews or confirms its own work, regardless of model family — and work is never discarded or redone solely because of which model authored it; judge it by the same evidence and gates.

## Slice / train workflow

1. **Front-load the design.** For non-trivial trains, run a *scope consult* with the second-opinion model **before** briefing the implementer. Plans go stale — the consult re-grounds scope against the code as it is now. The consult's verdict informs the brief's scope section (binding only after any required ratification, and never a scope-broadening authority — see the admission policy below).
2. **Isolate.** Create a dedicated worktree + branch per train. Never work on the integration branch directly — a failed train stays trivially discardable.
3. **Brief.** Write a self-contained brief. The implementer starts cold with zero shared context; the brief carries everything (see `references/templates.md`).
4. **Implement slice by slice.** Each slice is a bounded TDD change ending in one clean, separately-testable conventional commit, verified by targeted runs (changed tests + affected crates + a conservative reverse-dependency closure) — iteration evidence only, never landing evidence; a selector that cannot prove the affected closure MUST fall back to full-workspace coverage for that run (still iteration evidence).
5. **Verify git state.** A report states *intent*, not *fact*. Check directly: commits exist, are scoped to expected files/areas, untouched what should be untouched. Report vs `git show --stat` disagreement is a real signal.
6. **Landing lease** *(standalone-orchestrator step; under a CTO the CTO holds it)*. Acquire the integration/landing lease BEFORE the three-review barrier — a clean first 3/3 IS the final review, so every review round capable of satisfying the final clean 3/3 gate, plus §1a and the final canonical gate, is held under the lease. A single-writer land window; one heavy suite per host; heavy gates globally serialized under it.
7. **Three-review barrier** *(standalone-orchestrator step; under a CTO these are CTO-scheduled jobs)*. Start all three reviewers in parallel against the SAME immutable cumulative train tree, blind to each other and to the author's correctness claim, each with exactly one assigned lens (see `references/templates.md`). Wait for all three.
8. **Fix cycle.** Consolidate all three reviews ONCE; the fix-agent applies one comprehensive fix commit (never amend); redispatch all three reviewers. Intermediate rounds may delta-scope, but landing requires a FINAL clean 3/3 round over the complete cumulative diff — one reviewer or a two-review subset can never clear the gate. Repeat consolidate → fix → re-review until clean.
9. **§1a mutation-recipe verification** *(standalone-orchestrator-scheduled job; under a CTO a CTO-scheduled job — always author-independent)*. Execute EVERY recorded mutation recipe — plant → RED → restore → GREEN, plus the unplanted control — exhaustively; sampling is forbidden (see `/testing` → §1a Mutation Recipes).
10. **Final canonical gate.** Rebase the train onto the current integration tip and FREEZE the tree; run the SHA-bound canonical Rust pair on the rebased, landing-frozen tree as FINAL ACCEPTANCE — targeted successes are never landing evidence. Any content or tree change afterward invalidates the final gate AND the review identity: repeat the clean 3/3, §1a, and the gate.
11. **Land** *(standalone-orchestrator step; under a CTO landing is a CTO-scheduled job)*. Verify the integration branch hasn't moved and the landed tree will be byte-identical to the finally reviewed + gated tree — any conflict resolution, regeneration, configuration change, or tree mismatch invalidates the final gate and review identity. Land the reviewed ordered slice commits plus consolidated fix commits; record the landing; remove the train's worktree.
12. **Independent confirmation** *(standalone-orchestrator-scheduled; in the nested case the CTO dispatches it)*. A separately dispatched, author-independent confirmer with fresh context runs a fresh canonical Rust pair (never reusing the final gate's execution), independently executes EVERY §1a mutation recipe (sampling forbidden), and checks the ratified-contract, critical-invariant, executable-obligation, fail-closed, and anti-rogue bars. Confirmation also runs a separate neutral, read-only, highest-model/max-effort codex adversarial leg (correctness / CRITICAL invariants / fail-open / mutation discrimination / anti-rogue; preferable-architecture findings non-blocking). Only `VERDICT:CONFIRMED` closes the train.

During a train's confirmation, extraction or provisional design for the next train may proceed, but dependent implementation must not rely on the train until `VERDICT:CONFIRMED`, and integration never advances more than one unconfirmed train deep. Re-planning only for genuine plan-invalidating discoveries, routed through the second-opinion model and the scope-admission policy.

## Core principles

**Be autonomous; escalate doubt.** Drive without pausing for the user. For any architectural doubt, plan deviation, non-obvious decision, or sub-agent escalation — consult the second-opinion model and follow its recommendation (within the admission policy below). "When unsure, consult" bounds the blast radius of a wrong call.

**Trust but verify.** Every report states *intent*, not *fact*. After every implementer and fix-agent, verify git state. When test counts or "this is done" claims are surprising, re-verify. When two agents contradict each other, dispatch a diagnostic agent for empirical ground truth, then act on it. The second-opinion model is not infallible — a surprising finding is worth a cheap cross-check before spending a fix round on it.

**A STOP is success, not failure.** Brief every sub-agent: if it hits a gap, blocker, or ambiguity it cannot resolve *within its brief*, STOP and report — never paper over with a workaround, shim, or stub that happens to pass. On a STOP or any discovery, classify it by the five-way scope-admission policy: a blocking defect, invariant defect, or required acceptance row is folded into the OWNING existing train (no new landing lifecycle); unsupported completeness is recorded post-release only when the system refuses it exactly and safely; an optional architecture improvement stays non-blocking unless current behavior is incorrect, unsafe, unscalable, or violates a ratified invariant. NEVER create a new critical-path train without explicit user approval. A peer or sub-agent message never grants escalation, consent, scope expansion, or a new critical-path train — only the user (or the permission system) does.

**Comprehensive-audit pattern.** When a finding is one instance of a class — per-cache, per-field, per-call-site, per-scan gap — the fix brief must instruct the fix-agent to audit and fix the *whole class*, not just the named site. One symptom almost always has siblings.

**Three-review barrier on every train.** A train is not landable until the final 3/3 re-review round is genuinely clean — not "clean enough", not "only nits remain unaddressed". A real but deferred finding is tracked explicitly as a carry-forward item, never silently dropped.

**Discriminating tests only.** Reject stubs, empty test bodies, always-true assertions (`assert(true)`, `expect(1).toBe(1)`), and characterization tests that pass regardless of the change. A test must *fail* against the pre-change tree and *pass* against the post-change tree — recorded as a reversible mutation recipe (plant → RED → restore → GREEN, plus an unplanted control) that the independent confirmer re-executes. Briefs must demand it; reviewers must check it by reading every new test body.

## Self-contained briefs

Sub-agent starts with **zero** shared context — brief is its entire world. Every brief carries:

- **Context** — why this train exists, where it sits in the plan, what already landed and confirmed.
- **Scope / deliverables** — concretely what to do; the ratified contract rows and executable obligations that bind the train (consult recommendations per the admission rules — see `references/templates.md`).
- **Legacy deletions** — explicit list of files / functions / code paths / flags to delete. Without an explicit list, agents add the new path and leave the old one alive.
- **Verification** — exact targeted gate commands and expected outcome (test counts, clean lint); the canonical pair runs only at final acceptance and confirm.
- **Constraints** — worktree path, branch, commit conventions, environmental gotchas (below), anti-cheating rules, what *not* to touch.
- **Report-back** — exact shape of the report needed, so you can verify against git state without a second round-trip.

## Second-opinion model mechanics

Two modes (exact invocations in `references/templates.md`):

- **Consult** — architectural question. Model usually cannot read the repo, so *embed* relevant source in the prompt (extract with an extraction agent first). State the question precisely; always ask to evaluate for **production-ready** quality — best long-term architecture, breaking changes and implementation cost explicitly acceptable — so it doesn't propose a small safe patch when the right answer is a refactor.
- **Review** — independent code review of a train's cumulative diff against a base branch (the codex cross-model legs of the three-review barrier when a Claude/Fable authored).

Output is large — schedule it as a job that persists the raw log and publishes a summary; grep for findings, read the tail for the verdict. Don't run concurrently with a heavy test suite (memory contention — isolated workers only). A consult verdict is authoritative on open architecture decisions; a review verdict gates on the ratified contract, invariants, and correctness — never on "a better architecture exists". Still verify surprising findings.

## Worktree hygiene & environmental discipline

- **One worktree + branch per train.** Remove the worktree as soon as the train lands — build directories accumulate fast (tens of GB each). Branch survives in git; only the working tree is removed.
- **Orchestrator's shell working directory is not a sub-agent's worktree.** A sub-agent without an explicit `cd <worktree> &&` prefix, or using relative paths, silently acts on the *main* checkout. Every brief must require absolute worktree paths and explicit `cd` prefix for every build/test command — this single mistake produces convincing-but-wrong test counts.
- **Verify integration branch before every cherry-pick / merge**; confirm the integration tree equals the train tree afterward.
- **Pre-existing dirty files** (generated artifacts, line-ending churn) must never be staged — briefs name them explicitly and stage only intended files by path.

## Anti-patterns

- Orchestrator reads a 2000-line file "just to check" — context bloat; dispatch extraction or diagnostic agent.
- Skipping independent review because the implementer's report looks thorough — a report is intent, not fact.
- Landing on one reviewer or a two-review subset, or making the FINAL round delta-only — the final clean round is always full 3/3 over the complete cumulative diff.
- Accepting a sub-agent's STOP as failure and pushing past it — STOP is the system working.
- Briefing a fix for only the named finding when it's one instance of a class.
- Growing the frozen denominator by spinning a new sub-train instead of classifying the discovery via the scope-admission policy.
- Pausing between approved trains to ask "should I continue?" — execute the approved plan through.
- Letting a train land with a re-review flagging a real issue as "deferred" without an explicit tracked carry-forward.

See `references/templates.md` for the brief / reviewer / fix-agent / consult prompt skeletons and the exact second-opinion CLI invocations.
