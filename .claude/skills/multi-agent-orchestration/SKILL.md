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

Methodology for driving a large plan — multi-block refactor, migration, or staged cutover — to completion autonomously, when the work is too large for one agent in one context window.

Core move: one **orchestrator** + many **sub-agents**. Orchestrator never writes code — decomposes plan into blocks, briefs a sub-agent per block, verifies what comes back, gates every block on independent review, escalates doubt to a second-opinion model, manages git/branch/worktree/state. Sub-agents implement, review, fix, investigate — each a bounded, disposable job.

**Dispatch mechanism (default): the Agent/Task tool — gated on harness support.** Spawn every sub-agent — implementer, reviewer, fix-agent, extraction, diagnostic — via the Agent tool; each starts fresh with its brief as the prompt, and its final message is its report (a blocking call returns it; a background call notifies on completion). Agents may spawn child agents, so an orchestrator that is itself an agent still owns its full sub-agent cast. The Agent tool is the default ONLY WHEN the harness guarantees (a) no inherited transcript/hidden state beyond the passed prompt, (b) a distinct agent identity, (c) status/stop/continue control, and (d) child-agent spawning where the role needs it; if any property is absent or unknown, fall back to `claude -p` for an explicit fresh-process boundary. `claude -p` is otherwise OPT-IN only — explicit user request, or a separate account instance for multi-instance parallelism / out-of-session work. The second-opinion model (codex) is a separate external CLI either way. For gate-bearing dispatches (independent review and any verification gate) persist the exact prompt and the verbatim returned report to files with a recorded input id, model, and effort — inline-only is not allowed where a finding gates landing. Every gate-bearing Claude role (independent reviewer, verification/§1a, fix-agent, implementer) MUST run on the highest available Claude model at max effort, bound by an explicit model/effort arg at spawn or an audited agent-definition recorded with the dispatch; an unknown or default model/effort BLOCKS the dispatch — never a silent downgrade. The mechanism affects gate PROPERTIES — reviewer independence, model quality, fresh-context isolation, durable auditability — not just transport; those are preserved by the safeguards here (the harness-guarantee gate + `claude -p` fallback, the highest-model requirement, persisted prompt+report+model+effort) together with dual review, the fix cycle, and discriminating-tests-only, not by the swap alone.

## Why this shape

A single agent on a 20-step plan degrades three ways: early decisions fall out of context, raw tool output crowds reasoning, no second pair of eyes.

- **Orchestrator's context stays clean.** Reads *reports* (a paragraph each), never raw test output or 2000-line files — can coordinate a plan far larger than one context window.
- **Sub-agents are cheap and focused.** Each starts fresh, does one block, reports, is discarded.
- **Every block is independently reviewed.** Author is never the sole judge.

Use per-block loop (steps 3–8) for any single substantial block even outside a multi-block plan.

## The orchestrator role

**Only**: decompose plan; write briefs; dispatch sub-agents; read reports; verify git state; run reviews; make decisions; consult second-opinion model on doubt; manage branches, worktrees, plan-state file. Resolve every option before the implementer brief — decide routine implementation choices directly; route any architecture / high-stakes-design / public-behavior / cross-module-contract / performance-cache / ownership / plan-deviation choice NOT already settled by the approved plan or a prior second-opinion verdict to the second-opinion model (the architecture-decision path; under MoM/CTO this is the codex decision modes) regardless of confidence, never self-declaring it "resolved"; and brief the implementer ONE decided solution. Never hand a menu down; the implementer makes no architecture/solution choice.

**Never**: write/edit production code; run test suite; investigate a bug by reading source; review a diff line-by-line. The instant the orchestrator does the work, its context fills with detail and it loses the plot.

Lightweight mechanics the orchestrator *may* do directly: `git worktree add/remove`, `git cherry-pick`, `git log`/`show`/`status`, commit a plan-state file, read agent/review report files, write briefs and consult prompts.

## The cast

| Agent | Job | Tool access |
|---|---|---|
| **Orchestrator** | Decompose, brief, dispatch, verify, decide, manage git/state | this session |
| **Implementer** | Implement one block from a self-contained brief — one decided solution + steps, makes no architecture/solution choice | full |
| **Reviewer (claude — ADVERSARIAL)** | Independent, harsh, refute-first review of one block — reviews to BREAK the change, default-to-reject; LAND only when it tried hard to find a bug/over-claim/weakening/non-discriminating test and could not | read-only |
| **Fix-agent** | Consume review findings, apply fixes as a new commit | full |
| **Extraction agent** | Gather/extract source into one file | read-only |
| **Diagnostic agent** | Empirically reproduce + instrument a failure to establish ground truth | full, throwaway edits |
| **Second-opinion model** | Architectural consults + independent code review per block | external CLI (e.g. `codex`) |

The **second-opinion model** is load-bearing, not optional — escalation path for every doubt and independent half of every review. One model judging its own work shares blind spots; an outside judge on a different model breaks that correlation. Treat it as a peer architect, not a linter.

## Per-block workflow

1. **Front-load the design.** For non-trivial blocks, run a *scope consult* with the second-opinion model **before** briefing the implementer. Plans go stale — consult re-grounds scope against the code as it is now. Consult's verdict becomes the brief's scope section.
2. **Isolate.** Create a dedicated worktree + branch. Never work on the integration branch directly — failed block stays trivially discardable.
3. **Brief.** Write a self-contained brief. Implementer starts cold with zero shared context; brief carries everything (see `references/templates.md`).
4. **Dispatch the implementer.**
5. **Verify git state.** Report states *intent*, not *fact*. Check directly: commit exists, is scoped to expected files/area, untouched what should be untouched. Report vs `git show --stat` disagreement is a real signal.
6. **Dual review.** In parallel: (a) independent ADVERSARIAL claude reviewer sub-agent — reviews to refute/break the change, default-to-reject, never a rubber-stamp; (b) second-opinion model code review. They catch different problem classes.
7. **Fix cycle.** Fix-agent consumes **both** reviews, fixes findings as a *new* commit (never amend). Then re-review with the SAME review set — the adversarial claude reviewer AND the second-opinion model; a genuinely clean re-review across both is the gate (the adversarial claude re-review is part of the gate, not only the second-opinion model). Repeat fix → re-review until clean.
8. **Land.** Re-verify integration branch hasn't moved; merge/cherry-pick block onto it; sub-agent runs full verification gate on integration branch; record landing (feedback-log entry); remove block's worktree.

Move to next block without pausing — plan is already approved. Re-planning only for genuine plan-invalidating discoveries, routed through the second-opinion model.

## Core principles

**Be autonomous; escalate doubt.** Drive without pausing for user. For any architectural doubt, plan deviation, non-obvious decision, or sub-agent escalation — consult the second-opinion model and follow its recommendation; never guess or proceed on a hunch. "When unsure, consult" bounds the blast radius of a wrong call. A genuinely new architecture/design/mechanism doubt re-triggers a consult even after a prior verdict, UNLESS that specific doubt is already settled by an approved plan, a prior verdict, or (for a factual/mechanical question only — evidence never settles a best-design call) verified evidence — a settled answer is not re-consulted; only a new substantive doubt is. Every such consult is UNPRIMED/NEUTRAL — never state or imply the desired conclusion; a problem-only, open "what is best", or neutral yes-no framing needs no menu, but if you offer options the set must be exhaustive (include the option you are biased against — a leading menu that omits the right option is priming). VERIFY the prompt is neutral and complete BEFORE sending — a primed or mis-framed consult is a defect to rewrite and re-send, and its verdict is void. Demand the best (production-ready, breaking changes acceptable; see Second-opinion model mechanics), with the explicit best-not-lowest-effort framing — best ON THE MERITS, with implementation effort as accounting (diff-size / migration effort/breadth / files-touched) NOT a selection criterion (effort-neutral: never tilting toward a minimal change nor toward over-engineering / gold-plating; architecture-relevant migration RISK still a merit) — and VERIFY the prompt carries that framing before sending. Fire the consult IN PARALLEL with non-dependent, non-resource-conflicting work; never serialize the whole effort behind one consult (do not run it beside a heavy test suite — memory contention). Under MoM/CTO this is the codex Consult Discipline (`/mom-cto-orchestration` → PROTOCOL.md).

**Trust but verify.** Every report states *intent*, not *fact*. After every implementer and fix-agent, verify git state. When test counts or "this is done" claims are surprising, re-verify. When two agents contradict each other, dispatch a diagnostic agent for empirical ground truth, then act on it. The second-opinion model is not infallible — a surprising finding is worth a cheap cross-check before spending a fix round on it.

**A STOP is success, not failure.** Brief every sub-agent: if it hits a gap, blocker, or ambiguity it cannot resolve *within its brief*, STOP and report — never paper over with a workaround, shim, or stub that happens to pass. When a sub-agent STOPs, orchestrator consults second-opinion model, spins a new sub-block or fix round. Clean STOP costs one consult; buried wrong fix costs a debugging expedition three blocks later.

**Comprehensive-audit pattern.** When a finding is one instance of a class — per-cache, per-field, per-call-site, per-scan gap — fix brief must instruct fix-agent to audit and fix the *whole class*, not just the named site. One symptom almost always has siblings.

**Dual review on every block.** Block is not landable until BOTH the adversarial claude reviewer re-review AND the second-opinion re-review are genuinely clean — not "clean enough", not "only nits remain unaddressed". A real but deferred finding is tracked explicitly as a carry-forward item, never silently dropped. The claude reviewer leg is ADVERSARIAL — always: it reviews to break the change, defaults to skepticism, and a `LAND` means "I tried hard to break this and could not," never a confirmatory pass; it states the strongest counter-argument it found and why it does or does not sink the change.

**Review-leg tiers.** This is the base two-leg dual review (one adversarial claude reviewer + one second-opinion/codex leg) — correct for standalone single-block use on one account. When MoM/CTO orchestration is in force, its three-leg composition (1 adversarial claude + 1 claims-aware codex + 1 unprimed codex — see `/mom-cto-orchestration`) SUPERSEDES this two-leg loop; both tiers stay valid. The adversarial-claude STANCE applies in BOTH tiers regardless of leg count, and the fix cycle consumes findings from ALL review legs that actually ran.

**Discriminating tests only.** Reject stubs, empty test bodies, always-true assertions (`assert(true)`, `expect(1).toBe(1)`), and characterization tests that pass regardless of the change. A test must *fail* against the pre-change tree and *pass* against the post-change tree. Briefs must demand it; reviewers must check it by reading every new test body.

## Self-contained briefs

Sub-agent starts with **zero** shared context — brief is its entire world. Every brief carries:

- **Context** — why this block exists, where it sits in the plan, what already landed.
- **Scope / deliverables** — concretely what to do; for a substrate block, the second-opinion consult's verdict verbatim.
- **Legacy deletions** — explicit list of files / functions / code paths / flags to delete. Without an explicit list, agents add the new path and leave the old one alive.
- **Verification** — exact gate commands and expected outcome (test counts, clean lint).
- **Constraints** — worktree path, branch, commit conventions, environmental gotchas (below), anti-cheating rules, what *not* to touch.
- **Report-back** — exact shape of the report needed, so you can verify against git state without a second round-trip.

## Second-opinion model mechanics

Two modes (exact invocations in `references/templates.md`):

- **Consult** — architectural question. Model usually cannot read the repo, so *embed* relevant source in the prompt (extract with an extraction agent first). State the question precisely; always ask to evaluate for **production-ready** quality — best long-term architecture ON THE MERITS, breaking changes and implementation cost explicitly acceptable — and make the best-not-lowest-effort framing explicit: implementation effort as accounting (diff-size / migration effort/breadth / files-touched) is NOT a selection criterion (effort-neutral; architecture-relevant migration RISK still a merit), so it neither proposes a small safe patch when the right answer is a refactor NOR inflates scope into over-engineering when the right answer is the minimal one. Verify the prompt carries this framing before sending.
- **Review** — independent code review of a block's diff against a base branch.

Output is large — grep for findings, read the tail for verdict. Don't run concurrently with a heavy test suite (memory contention). Verdict is authoritative on architecture; still verify surprising findings.

## Worktree hygiene & environmental discipline

- **One worktree + branch per block.** Remove worktree as soon as block lands — build directories accumulate fast (tens of GB each). Branch survives in git; only working tree is removed.
- **Orchestrator's shell working directory is not a sub-agent's worktree.** A sub-agent without an explicit `cd <worktree> &&` prefix, or using relative paths, silently acts on the *main* checkout. Every brief must require absolute worktree paths and explicit `cd` prefix for every build/test command — this single mistake produces convincing-but-wrong test counts.
- **Install workspace deps in every fresh worktree before any JS/TS work.** `git worktree add` does NOT copy `node_modules/` (gitignored, untracked), so a fresh worktree starts with no installed dependencies. Run `pnpm install --frozen-lockfile` in the worktree root *once*, right after creating it, before any `pnpm test` / vitest / Node script that imports a workspace package — otherwise tests fail spuriously (missing modules) and an agent misreads it as a regression. The orchestrator runs it at worktree-creation time; every brief whose sub-agent runs JS/TS tests or a workspace-importing Node script must also state "run `pnpm install --frozen-lockfile` in the worktree before the JS gate". Pure-Rust blocks and `node:`-stdlib-only scripts don't need it, but installing once up front is cheap insurance. (If a build script the tests depend on sits in pnpm's ignored-build-scripts list, e.g. `@bufbuild/buf`, run the documented `pnpm approve-builds` / build step too.)
- **Verify integration branch before every cherry-pick / merge**; confirm integration tree equals block tree afterward.
- **Pre-existing dirty files** (generated artifacts, line-ending churn) must never be staged — briefs name them explicitly and stage only intended files by path.

## Anti-patterns

- Orchestrator reads a 2000-line file "just to check" — context bloat; dispatch extraction or diagnostic agent.
- Skipping second-opinion review because implementer's report looks thorough — report is intent, not fact.
- Accepting a sub-agent's STOP as failure and pushing past it — STOP is the system working.
- Briefing a fix for only the named finding when it's one instance of a class.
- Pausing between approved blocks to ask "should I continue?" — execute the approved plan through.
- Letting a block land with re-review flagging a real issue as "deferred" without an explicit tracked carry-forward.

See `references/templates.md` for the brief / reviewer / fix-agent / consult prompt skeletons and the exact second-opinion CLI invocations. The §2 reviewer skeleton is the self-contained adversarial-reviewer mandate (default-to-reject, refute-first, "what I tried to break" + strongest-counter-argument, read every test body) the orchestrator dispatches — copy it verbatim so every claude review and re-review leg stays adversarial, never confirmatory.
