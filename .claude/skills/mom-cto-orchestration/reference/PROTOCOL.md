# MoM PROTOCOL — Verter Block-Manager Overlay

> Governance: any change to this protocol requires prior neutral codex-architect approval — see PROTOCOL.md → GOVERNANCE.

This overlays `/multi-agent-orchestration`; it does not replace it. A manager still runs that loop end-to-end. This file supplies Verter specifics plus MoM-tier rules.

## GOVERNANCE

Every rule-bearing change requires prior NEUTRAL codex-architect approval before adoption/commit. Covered surface is complete: `CLAUDE.md`, `AGENTS.md`, every `.claude/skills/**` file, all protocols, every guard's assertion/needles/exemptions/rationale plus its test, design-doc/contract invariants, and any `docs/arch` plan/debt row changing constraints, sequencing, or priority. This is not limited to architectural, severe, or this-skill rules. Unapproved rule-bearing change = block + revert. Implementing agents cannot self-certify.

Procedure: prepend CODEX-ARCHITECT MANDATE, state exact change + rationale, ask neutrally "is this rule/skill/guard/design/plan change correct and necessary?", land only on approval. Regular code/test/doc updates that do not alter rules are unaffected.

## CODEX-ARCHITECT MANDATE

Prepend this verbatim to EVERY codex consult/fork/review/approval/adversarial-audit/best-implementation prompt. The mandate sets the bar; the substantive question remains neutral.

> You are the sole architecture authority. Judge NEUTRALLY and UNPRIMED — the prompt must never state or imply the desired conclusion; ask "is X correct / what is the best design", never "confirm X". Mandate:
> - The BEST-of-the-best architecture. No shortcuts. No compromises. No "good enough".
> - Breaking changes are ALLOWED and expected where they yield the correct long-term design — do not preserve a worse design to avoid breakage.
> - Performance is a first-class concern — weigh allocation, cache, warm-state, and hot-path cost, not just correctness.
> - Be brutally HONEST — surface the strongest counter-argument, flag every risk/uncertainty, never rubber-stamp. If the thing under review is wrong, say so plainly.
> - Optimize for the BEST architecture ON THE MERITS (correctness, durability, performance, appropriate simplicity, alignment with the shared owner-layer / single-engine model). Implementation EFFORT as accounting — diff-size / migration effort/breadth / files-touched / "smaller change" — is NOT a selection criterion and must bias the verdict toward NEITHER a minimal/local change NOR a broad/elaborate one: effort is NEUTRAL, neither a point for nor against. Reject any option chosen merely because it is easier/faster, AND any option chosen merely because it is broader/more elaborate; choose the lowest-effort option ONLY when it is independently best on the merits (a minimal/local proposal must justify it is the BEST, not merely the cheapest), and the higher-effort option ONLY when it is independently better. Over-engineering, gold-plating, and unnecessary breadth are the equal-and-opposite error and equally wrong; appropriate simplicity is itself a merit, so when the best architecture is the low-effort one that is the right answer. State effort honestly but never let it tilt the verdict — this excludes effort-as-cost ONLY: architecture-relevant migration RISK (temporary dual paths, rollout/rollback safety, invariant exposure, correctness/maintainability/performance impact) stays a first-class merit. The line is concrete, not a relabel: risk counts only when tied to a durable architectural failure mode; raw edit volume / file count / migration labor alone is effort — and conversely concrete rollback / dual-path / invariant-exposure / correctness risk may NOT be dismissed as mere effort.
> Deliver a clear verdict + enumerated, actionable findings (file/section/exact change). End with `__DONE__`.

Before sending, satisfy the framing gate that applies to this prompt: the neutral/unprimed bar here is universal (every codex prompt), and an architecture/uncertainty consult additionally obeys Consult Discipline (below — exhaustive options if any, mis-framed ⇒ verdict VOID, parallelism caveats).

## CLAUDE-REVIEWER MANDATE

The authoritative full mandate text to prepend lives in `CLAUDE-REVIEWER-MANDATE.md`; the blockquote below summarizes it (it is a pointer, not the verbatim prepend — prepend the file's text). Prepend that mandate to EVERY claude-reviewer dispatch — the per-block 3/3 claude leg, the independent confirm leg, the integration-confirm leg, and every fix re-review. The mandate sets an ADVERSARIAL stance; the substantive question stays neutral.

The ADVERSARIAL STANCE is identical on every claude leg (default-to-reject, must genuinely try to break it, state what you tried + the strongest counter-argument, read every new test body to prove it discriminates). Only the VERDICT TOKEN differs by gate — each leg emits the token its gate already defines: a review or fix re-review leg emits `LAND` / `CHANGES REQUIRED`; a post-land confirm leg emits `VERDICT:CONFIRMED` / `REOPEN`; an integration-confirm leg emits `VERDICT:INTEGRATION-CONFIRMED` / `REOPEN`. The adversarial meaning is the same across all three — `LAND` / `VERDICT:CONFIRMED` / `VERDICT:INTEGRATION-CONFIRMED` is earned ONLY by genuinely trying to break the change and failing.

> You are an ADVERSARIAL reviewer. Your job is to BREAK this change, not to bless it. Default to REJECT. Mandate:
> - Review to REFUTE: hunt the bug, the over-claim, the missed case, the silent weakening, the non-discriminating test. Assume a defect is present until you have genuinely tried and failed to find one.
> - Your gate's positive verdict (`LAND`, or `VERDICT:CONFIRMED` / `VERDICT:INTEGRATION-CONFIRMED` on a confirm / integration-confirm leg) means ONLY "I tried hard to break this and could not" — never a confirmatory/rubber-stamp pass. If you have not actually attempted to break it, you may not return that positive verdict.
> - State explicitly WHAT YOU TRIED to break and the RESULT of each attempt; enumerate the STRONGEST counter-argument you found and why it does or does not sink the change.
> - List every risk, uncertainty, scope gap, and weakly-supported claim; read every new test body and prove it discriminates (FAIL pre-change, PASS post-change) — a stub/always-true assert/non-discriminating characterization is a finding, not a pass.
> - Never invent issues to look thorough, never soften a real one to be agreeable. If the change is wrong, say so plainly.
> Deliver a clear verdict — the positive token (`LAND` / `VERDICT:CONFIRMED` / `VERDICT:INTEGRATION-CONFIRMED`, per the gate this dispatch serves) ONLY if you could not break it, else the gate's reject token (`CHANGES REQUIRED` / `REOPEN`) — plus enumerated, actionable findings (file/section/exact change) each tagged [P0]/[P1]/[P2]/[P3].

This is binding on every claude review leg — adversarial-always is not advisory. Rationale: claude-only confirmatory legs on this plan repeatedly MISSED defects the codex legs caught (gate-bypass seam, binary-regrowth paths, overclaims); refute-first closes that gap.

## Never Prime

Every reviewer, consult, verifier, confirmer, and adjudicator prompt must present artifact + neutral question and let the agent decide. Do not say or imply the expected answer. Use "assess whether X is correct and complete / what is best"; never "confirm X is correct". Applies to post-land confirm too.

## Consult Discipline

When unsure of the best architecture, consult the codex architect — always unprimed, demanding the best, run in parallel. This escalates Never Prime + the CODEX-ARCHITECT MANDATE to a checked consult-trigger at the framing moment, and is the resolve step of the architecture-codex-ownership + one-decided-solution rules: how a manager resolves an uncertain option (via Decision Modes) before the implementer brief, never a menu handed to the implementer. Purely ADDITIVE — it relaxes no existing trigger. Three binding teeth:

1. CONSULT-WHEN-UNSURE (obligation). This is additive to — never a softening of — the existing unconditional routing: an architecture / high-stakes-design / public-behavior / cross-module-contract / performance-cache / ownership / plan-deviation choice not already settled by an approved binding plan/design or a prior codex verdict ALWAYS routes through the codex Decision Modes, regardless of manager confidence (a manager may not skip codex by self-declaring certainty). ON TOP of that, MATERIAL UNRESOLVED doubt is an EXTRA trigger: when a CTO/manager/agent has a substantive open question about the best architecture/design/architecture-significant strategy or mechanism (routine naming/sequencing/small-refactor/local mechanics the manager still decides directly; product/priority forks stay at the user boundary per Decision Modes, not here) that is NOT settled by an approved plan/design or a prior codex verdict (nor, for a purely factual/mechanical sub-question, by verified repo evidence), it MUST consult the codex architect rather than guess or proceed on a hunch or a sub-agent's say-so. This terminates — a verdict backed by an approved plan or a prior codex ruling settles that same uncertainty and is not re-consulted for it; only a genuinely NEW substantive doubt re-triggers (e.g. an "airtight"/"exhaustive"/"complete" claim the prior verdict did not actually establish). Settling an uncertainty NEVER waives a mandatory codex gate — review, approval, confirm, integration-confirm, or best-implementation legs still run regardless. Verified repo evidence settles only a FACTUAL/MECHANICAL sub-question (what the code does), never substitutes for codex on a "best design" judgement. A hedged or evidence-contradicting verdict is not "settled": rerun/escalate it per the Decision-Modes rule, do not bank it.

2. CONSULT FRAMING DISCIPLINE (binding, every codex-architect consult). (a) UNPRIMED / NEUTRAL — the prompt must NEVER state, imply, or frame toward a desired conclusion; ask "what is the best design / is X correct / which mechanism is correct", never "confirm X" or any leading framing that pre-loads an answer. A problem-only / open "what is best" / neutral yes-no framing needs no menu; IF the prompt offers an option set it must be EXHAUSTIVE — include the option the asker is biased against and any reframe/other-best-design path (an omitted-option menu is priming). The dispatcher MUST VERIFY the prompt is neutral and (where it lists options) complete BEFORE sending — a primed or mis-framed consult is a DEFECT: rewrite and re-send, and any verdict it produced is VOID (never acted on). (b) DEMAND THE BEST — no compromises, no shortcuts, breaking changes ALLOWED, production-grade, most performant (allocation/cache/warm-state/hot-path weighed first-class). This IS the CODEX-ARCHITECT MANDATE — prepend it verbatim; the consult asks for the best-of-the-best, never "good enough". This carries the mandate's BEST-NOT-LOWEST-EFFORT framing explicitly: best architecture ON THE MERITS, with implementation effort as accounting — diff-size / migration effort/breadth / files-touched — NOT a selection criterion (effort-neutral — never a point for nor against, never tilting toward a minimal change, and equally never toward over-engineering / gold-plating / unnecessary breadth; architecture-relevant migration RISK stays a merit). The dispatcher MUST VERIFY the prompt carries this explicit best-not-lowest-effort framing BEFORE sending — alongside the neutral/exhaustive-options check in (a); a consult lacking it is mis-framed and its verdict is VOID. The check applies to the ENTIRE prompt — body, options, context labels, questions — not just the prepended mandate boilerplate: prepending the mandate does NOT cure a body that tilts toward either a minimal/local change or a broad/elaborate refactor.

3. PARALLELISM. Where non-dependent, non-resource-conflicting work exists, run the consult IN PARALLEL with it — fire it and continue those independent slices concurrently rather than idling the whole effort behind one consult; and NEVER skip the consult to "save time". This does not override the existing rule against running codex concurrently with a heavy/canonical test suite (memory contention; serialize those), and a genuinely dependent next step still waits for the verdict. Doubt → consult-in-parallel is cheaper than proceeding under-informed.

Enforcement (auditable, not mental-state): confirm/governance REJECTS any landed architecture/high-stakes decision that lacks a recorded settling source — an approved plan/design, a prior valid codex verdict, or an unprimed best-architecture correctly-framed consult; the dispatcher records that consult's prompt/output plus the `CODEX … framing=neutral-verified,best-not-lowest-effort-explicit` marker — covering BOTH neutral-framing-verified AND best-not-lowest-effort-explicit (see (b); the marker shape lives in `CHECKPOINT-PROTOCOL.md`), or records the plan/verdict that made a consult unnecessary — per the gate-bearing persistence rule in Dispatch (below), so the before-send framing check is itself auditable. The missing tooth this adds is framing-VERIFICATION at the consult-framing moment — the gap that, unenforced, let a primed/mis-framed (or lowest-effort-defaulting) consult waste significant effort.

## Rule-File Integrity

Rule-bearing artifacts are read-only unless the block's sanctioned scope is to change that rule and governance approval exists. Never add an exception/carve-out/accepted residual/sole exception/landing blocker that weakens any rule, guard, assertion, test, or plan row. Known violations go in the `docs/arch` debt ledger with codex-DEFER ruling or a fix block, never rule text.

Reviewer briefs must say branch rule artifacts are authoritative only where they match base plus sanctioned codex-approved edits; other rule text changes are defects. Every block brief says not to touch rule files unless in scope and never add a rule exception.

The §1a verifier MUST run the full Anti-Rogue discovery (below), not a path-limited diff, and FAIL on any unapproved rule-bearing edit, added exception/weakening, deleted/renamed guard/rule/test, new skipped test, relaxed assertion, or plan/debt row — a targeted `CLAUDE.md`/`AGENTS.md`/`.claude/skills/`/`docs/arch` diff is only a convenience pass, never the boundary. Live enforcement is that §1a discovery plus the dual-layer adversarial codex confirmation; a static scanner flagging exception-language adjacent to rule clauses is an optional complementary hardening, pursued only as a codex-approved `docs/arch` debt row (never carried as "owed" rule text), and weakening/removing it is itself rule-bearing.

Plan docs/debt ledgers are sequencing authority. A block may add its own deferral only with explicit codex-DEFER ruling. Reviewers and §1a challenge every new DEFER/debt row; missing ruling or deferring scoped work is P0.

## Anti-Rogue Rule

Threat: an agent takes a shortcut, then edits a rule/skill/protocol/guard/design/plan to legitimize it. Defense has two independent layers, both starting with DISCOVERY:

`git diff --name-status --find-renames --find-copies <base>..<head>` → classify every Added/Modified/Deleted/Renamed/Copied path. Scan new-side bodies for rule/constraint text, guard semantics, assertion/needles/exemptions/rationale, allowlists, `#[ignore]`/skips, relaxed/removed asserts, plan/debt edits, and disguised rule text. For every Modified or renamed-with-edits file, also scan deleted/changed `-` hunks (`git diff <base>..<head> -- <path>`) plus old owning context for a removed/relaxed rule, guard, assertion, test, skip, allowlist, or plan row inside a still-existing file. For deletions/renames, inspect old body with `git show <base>:<path>`. For copies, inspect source. Hand codex artifact diff plus owning context; deletions include old body.

Layer 1: implementing manager runs discovery pre-land and sends every rule-bearing change to codex-architect, unprimed/adversarial: "Is this correct and necessary, or does it legitimize a shortcut, weaken/relax a guard, add a self-serving exemption, or bypass a gate?" Land only on clean approval.

Layer 2: MoM confirm manager independently reruns discovery post-land on the landed diff; it does not trust layer 1. Each rule-bearing change needs the same clean codex confirmation or REOPEN.

High suspicion always demands adversarial codex: changed guard assertion/needles/exemptions/rationale/comment, new `#[ignore]`, relaxed `assert`, new allowlist/exemption, weakened rule clause.

## Active Regime / Capacity

Roles required: implementer/fix fresh context, claude review/§1a/confirm fresh context (the claude review and confirm legs are ADVERSARIAL — CLAUDE-REVIEWER MANDATE), codex. The default Agent/Task mechanism gives fresh-context role separation on a single account (harness-managed), bounded by harness Agent concurrency capacity, ONLY WHEN the harness guarantees no inherited transcript/hidden state beyond the passed prompt, a distinct agent identity, status/stop/continue control, and child-agent spawning where required; if any of those is absent or unknown, fall back to `claude -p` for an explicit fresh-process boundary. On the opt-in `claude -p` path, read the account→role mapping from the live brief/ledger after smoke tests and never hard-code account names; separate accounts are then an availability/parallelism optimization, not required. Single account = separate fresh Agent sub-agents (unprimed prompts), codex cross-model check, serialized heavy gates. Stop/escalate if no implementation agent capacity is available or codex unavailable.

Run file-disjoint blocks in parallel (the default). No artificial 300s spacing or one-claude-at-a-time throttle; respect harness Agent concurrency capacity. On the opt-in `claude -p` path, map separate-account instances explicitly, one block per account, with no artificial sub-agent-count limit. Triple-review legs run in parallel. Canonical/full-suite/heavy gates are globally serialized under one gate lock.

codex uses highest reasoning (`gpt-5.5`/xhigh here); never downgrade reviewers. Every gate-bearing Claude role — block manager, reviewer, §1a verifier, confirm manager, integration-confirm manager, implementer/fix (the block and integration-confirm managers own the landing and integration gates and are explicitly covered) — MUST run on the highest available Claude model at max/highest effort, bound either by an explicit `model`/`effort` arg at spawn OR by an audited agent-definition whose model+effort are recorded in `PROGRESS.md`; an unknown or default model/effort BLOCKS the dispatch — never silently downgrade any gate-bearing role. On the opt-in `claude -p` path, bind that highest model + `--effort max` explicitly per agent and watch `CLAUDE_CONFIG_DIR`: a wrapper account exports it into children, so bare `claude` may silently use the wrapper account — override at loop start (`export CLAUDE_CONFIG_DIR=$HOME/.claude`) when dispatching bare `claude`; identical reset times across "different" accounts indicate the trap.

## Repo / Worktree

Repo root: `/Users/carlosrodrigues/Documents/dev/verter`. Integration branch: `refactor/semantic-db-overhaul`; never commit directly to it. Worktree/branch/block id come from the brief. All edits use absolute paths in the worktree; all build/test commands start `cd <worktree> && ...`. Every sub-agent brief repeats this.

Every phase/stage has its own worktree. Implementers may commit `wip:` checkpoints after cheap checks. After CONFIRMED, each landed block squashes to exactly ONE clean conventional commit; a stage/phase may contain multiple block commits only because multiple blocks landed independently. Only conventional commits require the full gate.

## Dispatch

Default mechanism is the **Agent/Task tool**, gated on harness support. The CTO spawns each manager as an Agent sub-agent; managers spawn their implementer/fix/review/verify (§1a) agents as Agent sub-agents too (agents may spawn child agents — the manager→children topology). Managers NEVER spawn the confirm manager — only the CTO/MoM dispatches the separate unprimed confirm (and integration-confirm) MANAGER after land, so the post-land gate stays independent of the author. The Agent tool is the default ONLY WHEN the harness guarantees (a) no inherited transcript/hidden state beyond the passed prompt, (b) a distinct agent identity, (c) status/stop/continue control, and (d) child-agent spawning where the role needs it. This capability is not assumed: it MUST be proven and recorded once per session as a `CAPABILITY mechanism=agent isolation=<result> identity=<result> stopcontinue=<result> childspawn=<result> result=PASS` line in `PROGRESS.md`/the CTO ledger BEFORE the first Agent dispatch (see `CHECKPOINT-PROTOCOL.md`). An absent or stale capability proof counts as "unknown" and forces the `claude -p` fallback — an explicit fresh-process boundary; any single missing/failed property does the same. Each starts cold with a self-contained brief as its prompt; the agent's final message IS its report — and for gate-bearing roles (review, §1a, confirm, anti-rogue) the exact prompt and the verbatim final report are persisted to files with a recorded input id, never inline-only (see `CHECKPOINT-PROTOCOL.md`). A blocking Agent call returns that report; a background (`run_in_background`) Agent call notifies the spawner on completion — no resume-loop wrapper, no stream/marker watchdog. Continue a still-live agent with its context via its id/name; a fresh Agent call starts cold.

The dispatch mechanism affects oversight-gate PROPERTIES — confirm independence, reviewer model quality, fresh-context isolation, and durable auditability — not just transport; those are preserved by the safeguards above, not by the swap alone. Each gate is conditioned on a recorded precondition, never asserted: confirm/integration-confirm independence on CTO-only dispatch; fresh-context isolation on the recorded `CAPABILITY … result=PASS` proof (absent/stale ⇒ `claude -p`); reviewer/manager quality on the recorded highest-model+max-effort binding per gate-bearing role (unknown/default ⇒ BLOCK); auditability on the persisted brief+report+input-id+model+effort. The 3/3 review, §1a, confirm, and anti-rogue gates pass only when their preconditions are recorded — a missing or stale precondition leaves that gate unmet. CTO dispatches only managers; managers dispatch implementers/fix/review/verify (§1a) agents, never the confirm manager. Trust but verify: every report is INTENT, not fact — confirm "done"/test-counts/commits/deletion claims against `git show --stat`, `git log`, `grep`, and gate summaries before acting.

Codex is unchanged — read-only reviewer/architect/decider only, a Bash-invoked CLI subprocess (never `claude -p`); all code/tests/fixes are by claude; any codex-written code is suspect and redone by claude.

`claude -p` CLI subprocesses are **OPT-IN only**: (a) explicit user request, or (b) a genuinely separate account instance for multi-instance parallelism or work that must outlive the parent session. Default Agent mode is single-account harness-managed parallelism; the opt-in `claude -p` path is what restores multi-account claude instances, and it ALONE carries the `WAIT-PROTOCOL.md` foreground-poll + resume-loop + `CLAUDE_CONFIG_DIR` discipline:

```
<CLI> -p --model '<model>' --effort max --dangerously-skip-permissions \
  --output-format stream-json --include-partial-messages --verbose \
  < BRIEF.md > OUT.jsonl 2>&1 &
```

On that path, detach resume-loop wrappers (`nohup`/`setsid`) and monitor stream/status/report/marker/git activity; intervene only on a true hang — no activity on ANY signal across multiple bounded windows — by killing both the wrapper and the inner `claude -p` (`pkill -f BRIEF` misses the inner, which reads stdin), confirming `ps` count 0, then redispatching. A LIVE agent with repeated activity but zero durable artifacts across multiple windows is non-converging: kill and redispatch with a tightened forcing-function brief.

## codex Invocation

```
codex exec --sandbox read-only -C <REPO_OR_WORKTREE> --skip-git-repo-check \
  --disable image_generation -c 'model="gpt-5.5"' -c 'model_reasoning_effort="xhigh"' \
  < PROMPT.md > OUT.txt 2>&1 &
```

Feed prompt via stdin redirection, never as an arg. Verify output grows past the tiny startup size within seconds or redispatch. For branch review, run with `-C <worktree>` and ask it to review `git diff refactor/semantic-db-overhaul..HEAD`. Output is large; grep the tail for the last verdict:
`grep -nE '\[P[0-3]\]|VERDICT|LAND|CHANGES|__DONE__' OUT.txt | tail`. The real verdict is the LAST sentinel (`__DONE__`/`VERDICT`/`LAND`/`CHANGES`); earlier matches are prompt echoes — never parse the whole output. Generally read reports/status files, not raw logs, and tee heavy output once to query rather than re-run.

Never wait via `pgrep -f "codex exec"`; it self-matches the waiter. Capture `$!` and `wait`/`kill -0`, append a sentinel and grep the output, or run foreground codex inside a backgrounded bash with an exit marker.

## TDD / Tests

Discriminating tests first: FAIL before change, PASS after. No stubs, empty tests, always-true asserts, unconditional defaults presented as implementation, or non-discriminating characterization. Reviewers read every new test body. §1a proves RED→GREEN by hunk-revert/plant on each new/changed correctness-bearing test/guard plus one unplanted control that stays GREEN.

## Review Cadence

Every block/stage/phase review round = 1 ADVERSARIAL claude + 1 claims-aware codex + 1 unprimed codex, parallel, distinct lenses, read-only, no tests, harsh production bar, to 3/3 LAND or NIT-only carried forward. The three legs are NEVER interchangeable: the claude leg is ADVERSARIAL (refute-first — see CLAUDE-REVIEWER MANDATE; it reviews to break the change and returns LAND only when it tried hard and could not); the claims-aware codex leg is handed the change's stated claims as assertions to TEST/REFUTE — it asks whether each claim actually holds and attacks the ones that do not; the claims are untrusted assertions under test, never a desired conclusion, so this leg does not violate Never Prime; the unprimed codex leg gets the artifact blind and hunts issues without being told the intended outcome. The claude leg's substantive question stays neutral (artifact + "is X correct and complete", never "confirm X") even though its stance is adversarial — adversarial means default-to-reject, not stating the desired answer. Designs/docs get the same full 3/3; later landing/rebase skips re-review only when byte/hash-identical. Any conflict resolution, mirror content delta, or non-identical "mechanical" change re-enters 3/3 + §1a + anti-rogue layer 1. Skill/design/doc codex reviews cap at 3 rounds; after 3, finalize cosmetic/wording/framing residuals, but substantive or anti-rogue findings still block.

The claude review leg is ADVERSARIAL — ALWAYS. It reviews to REFUTE/break the change: hunt the bug, the over-claim, the missed case, the silent weakening, the non-discriminating test — defaulting to SKEPTICISM. A claude reviewer's `LAND` means "I tried hard to break this and could not," NEVER a confirmatory/rubber-stamp pass; it must state the strongest counter-argument it found and why it does or does not sink the change. This binds EVERY claude review leg — the per-block 3/3 claude leg, the independent confirm leg, the integration-confirm leg, and every fix re-review — and is enforced by prepending the CLAUDE-REVIEWER MANDATE (the authoritative full text lives in `CLAUDE-REVIEWER-MANDATE.md`; the CLAUDE-REVIEWER MANDATE section above summarizes it) to every claude-reviewer dispatch. Rationale: claude-only confirmatory legs on this plan repeatedly MISSED defects the codex legs caught — gate-bypass seam, binary-regrowth paths, overclaims; the adversarial stance closes that gap.

Findings route by scope. In-scope P0/P1/P2 enter normal fix cycle; P0 blocks, each fix is a new commit, never amend, re-review until clean. Scope-deviating/increasing findings (system-wide/pre-existing class, architecture beyond block, relitigating settled scope) go to codex with plan context for adopt-now/defer/reject. Fix whole classes within the block surface; if class extends beyond deliverable, stop and codex-scope-consult. If recurring out-of-scope findings exceed about 5 rounds, consult early. Adjudicate reviewer compile/test claims against the verified gate: if the full gate compiled and ran, a false "won't compile" claim is INVALID — never add a no-op patch merely to clear that verdict.

## Architecture / Decision Modes

Architecture is always codex-owned. Claude executes; codex never writes code. Iterate until confident; never accept hedged verdicts. If codex contradicts verified repo evidence after rerun, stop/escalate. Any doubt about the best architecture/design/mechanism triggers a consult under Consult Discipline (above): unprimed, neutral-framing-VERIFIED-before-send, best-architecture-demanded with the explicit best-not-lowest-effort framing (effort-as-accounting neutral, biasing toward neither a minimal nor an over-engineered change; migration RISK still a merit) also VERIFIED before send. The framing/verify/best-demand bar binds every codex leg below; the initial decision legs also run in parallel, while a dependent leg (e.g. the third decider) runs after its inputs exist — sequenced, not parallel — and stays neutral.

Single yes/no architecture question → one neutral codex leg. Genuine multiple-choice/high-stakes fork → two neutral codex legs in parallel: A options-framed, B problem-only/unprimed. If they agree, decision is binding. If they disagree, run a third codex decider with both outputs plus source; it verifies decisive claims against code and adjudicates.

Modes:
- `escalate-to-user`: codex produces architecture analysis; user ratifies/rejects. Product/priority forks go to user.
- `full-autonomous`: auto-adopt codex architecture verdict; derive product/priority from approved plan.
User never adjudicates architecture instead of codex. Stop/user boundaries: no implementation agent capacity (on opt-in `claude -p`, no usable implementation account), destructive operation (force-push/history rewrite/irreversible shared state), unrecoverable lost/corrupt worktree with no trusted commit, codex unavailable/contradicts evidence, or product/priority choice not derivable from plan.

Implementer briefs contain ONE decided implementation path plus its steps, never a menu of choices. Managers resolve options BEFORE dispatch: routine local implementation choices (naming, small refactors, sequencing, mechanics) the manager decides directly; any architecture / high-stakes-design / public-behavior / cross-module-contract / performance-cache / ownership / plan-deviation choice NOT already settled by an approved binding plan/design or a prior codex verdict routes through the existing codex decision modes above — regardless of manager confidence; the manager may not self-declare such a choice "resolved" — and codex returns the chosen path plus steps. The implementer executes the chosen path and escalates newly-discovered conflicts; it never chooses the architecture/solution. The choice belongs upstream, never punted to the code-writing agent.

## Verification Gate

Delegate heavy gates to a verify agent in the worktree and serialize globally.

```bash
cd <worktree> && cargo nextest run --workspace --no-fail-fast 2>&1 | tee /tmp/mom/<BLOCK>/gate-nextest.txt
cd <worktree> && cargo test -p verter_session --tests 2>&1 | tee /tmp/mom/<BLOCK>/gate-session.txt
cd <worktree> && cargo clippy --workspace -- -D warnings
cd <worktree> && cargo fmt --all --check
```

Canonical Rust gate is the nextest command PLUS `cargo test -p verter_session --tests`. Bare `cargo test --workspace --tests` silently skips the verter_session integration suite and is never the sole gate. Count summaries/per-binary lines; never accept truncated runs. `--no-fail-fast` prevents an early env/pre-existing failure hiding downstream failures.

JS: if TS touched, run the relevant JS gate; otherwise at least `pnpm install --frozen-lockfile` for lockfile sync. Node_modules-less worktrees may symlink main `node_modules` or install once. `typeinfo_ts_bindings_*` regeneration via missing `buf` is env-only, not a code failure.

Re-derive baseline at block entry. Required result is zero new failures versus that live baseline. Do not hard-code named failure allowlists into rules; pre-existing failures need a debt row or codex-DEFER ruling. If a failure correlates with co-resident full-suite load or a shared external service under a no-retry harness, treat it as an environmental flake first — rerun it isolated before classifying it as a regression.

## Commit / Land Rules

Use `LANDING-PROTOCOL.md` for mechanics. Mid-flight `wip:` allowed; the final landed history is exactly one clean conventional commit per block, no `wip`/`fixup`/`squashme`, no Co-Authored-By or attribution trailer, no logs/outputs/generated junk, no `git push`, no direct integration-branch commit, no `git add -A` or `git add .`.

Pre-land sync: rebase onto current integration tip and rerun full gate. Any conflict resolution or content delta re-enters full 3/3 + §1a + anti-rogue layer 1. Design mirrors must be byte-identical to reviewed content. True ff only; no merge commit.

No phase/plan refs in production: conventional commit diffs/messages, code, comments, and tests contain no plan/phase vocabulary. Phase refs may exist only in `wip:` history and are scrubbed at squash.

## Repo Cleanliness

Only product files land: source, tests, fixtures, tracked docs, and `docs/arch` plans/designs. Plans belong only in `docs/arch`. Temp/scratch/report/evidence/log/progress files live in `/tmp/mom` absolute paths or `.feedback/` (directory-gitignored). Worktrees are outside the repo.

Before staging, inspect `git status --short` and remove only verified untracked/ignored scratch this block created; never by filename class. A tracked file is never removed as cleanup; tracked deletion is reviewed product change. Do not add per-file `.gitignore` entries for scratch. Generated outputs are gitignored by directory/glob (`playwright-report`, `test-results`, `*.junit.xml`, coverage). Scratch/temp/report in status/diff blocks land.

Plan-end scratch/report clutter already in git history is purged by a dedicated user-authorized destructive history-rewrite/force-push block, with final user go-ahead at execution time.

## Binding Designs / Debt

Every binding design/plan is committed to `docs/arch/<name>-design.md` and referenced in the master-plan locked-designs index. At land, mirror is byte-identical to reviewed design; confirm verifies tracked + referenced + `cmp` identical.

Mid-plan deferrals require codex-DEFER ruling and debt-ledger row in `docs/arch`. FEED-FORWARD is valid only mid-plan. Plan close-out requires empty debt ledger and zero open deferrals/carry-forward.

## Confirm / Integration Gates

After every manager land, the CTO — never the block manager — dispatches a separate unprimed confirm MANAGER. It independently verifies four bars and writes `VERDICT:CONFIRMED` only when all hold:
1. CORRECT + additive/non-breaking-as-claimed + full gate GREEN first-hand; re-grep legacy deletion; re-prove every correctness-bearing test/guard discriminates in throwaway worktree (plant violation → RED → restore → GREEN); sampling one is invalid; `cmp` design mirrors.
2. NOT SHALLOW: real inputs and edges; no non-discriminating characterization.
3. NO STUBS: no empty/always-true tests or unconditional default/unknown/OK returns as implementation.
4. BEST IMPLEMENTATION: unprimed codex-architect judges whether a better design exists. Merely correct but not best = REOPEN.

On REOPEN, CTO dispatches fresh fix manager → re-land → re-confirm. Per-stage confirm is separate from integration-confirm.

Integration-confirm MANAGER runs at every phase/milestone boundary, before any dependent phase, before final close-out, and after every 5 confirmed blocks. It derives done-bar from binding plan, not reports; reviews integrated diff/tree; runs canonical gate; checks cross-block invariants, manifests, design mirrors, debt honesty, legacy deletion, hollow fronts, cache/perf/warm-state. Issues are classified via codex as REOPEN vs FEED-FORWARD. FEED-FORWARD only mid-plan. Only `VERDICT:INTEGRATION-CONFIRMED` closes phase.

Stage/phase cleanup only after land + confirmation: `git worktree remove` + `git worktree prune`, remove transient briefs/reports/review outputs (and `jsonl`/markers on the opt-in `claude -p` path), preserve CTO ledger/MOM-NOTES, landed reports, debt ledger, design docs, verify clean status. At phase boundary clear all closed-stage worktrees/temp. Never delete live `/tmp` evidence mid-flight.

## Status Reporting

Write/overwrite `/tmp/mom/<BLOCK>/status.json` at milestones:

```json
{ "block":"<id>", "state":"RUNNING|BLOCKED|DONE|PARTIAL",
  "phase":"<short phrase>", "branch":"<branch>", "head":"<sha>",
  "commits":["<sha> <subj>"], "gate":{"rust":"PASS|FAIL|PENDING","js":"PASS|FAIL|NA"},
  "review":{"claude":"LAND|CHANGES|PENDING","claimsAwareCodex":"LAND|CHANGES|PENDING","unprimedCodex":"LAND|CHANGES|PENDING"},
  "newFailures":[], "note":"<=1 line" }
```

`BLOCKED`: first run escalation protocol; if unresolved, write `ESCALATION.md` with question, two options, what tried, set state, exit. `PARTIAL`: keep only internally clean committed branch work, no ff-merge unless full lifecycle completed; write `HANDOFF.md`; no final WIP commits. `DONE`: only after 3/3 LAND, §1a `VERDICT:LAND`, anti-rogue layer 1, rebase, post-rebase full gate, teeth'd squash, true ff, cleanup, `MANAGER-LANDED.md`, CTO ledger append, then exit.

## Verter Invariants

Never weaken: one resolver (`SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore`); typed-IR-only (no source-slicing/regex-on-type-text/reparse in resolver); shallow-by-default; fact-cache R21 five split env hashes; R6 no versioned identity in query-identity keys; §10.4.1 363-row manifest partition EXACT + bijective with live manifest; `AdditionalProofRow` closed at 7; `CodeTransform` is the only output-mutation path; final-state prose/no phase archaeology in `crates/*/src/**`.

Cross-platform: codebase must build/test/materialize on macOS/Windows/Linux. No NTFS-illegal chars (`< > : " | ? * \`, control chars, trailing dot/space, reserved CON/PRN/AUX/NUL/COM#/LPT#) in tracked paths or generated names; sanitize names. No hardcoded path separators; use `Path`/`PathBuf`/`join`. Byte-equality compares normalize CRLF/LF or compare text. OS binaries (`tsgo`/`.exe`) are discovered platform-aware. Temp/cwd use std abstractions. Platform-assuming code is a defect.

## Terseness

Rules/skills stay concise. New process gaps go to the ledger and, if rule-bearing, through GOVERNANCE before adoption.
