# MoM PROTOCOL — Verter Block-Manager Overlay

> Governance: any change to this protocol requires prior neutral codex-architect approval — see PROTOCOL.md → GOVERNANCE.

This overlays `/multi-agent-orchestration`; it does not replace it. A manager still runs that loop end-to-end. This file supplies Verter specifics plus MoM-tier rules.

## Reading These Files

**`bash`-fenced blocks are EXECUTABLE; inline `<angle-brackets>` in prose are DESCRIPTIONS.** A `bash` fence runs as written and uses shell variables (`"$WORKTREE"`, `"$CODEX_MODEL"`); it never contains an angle-bracket placeholder, because `<` and `>` are shell redirections — a pasted `<worktree>` is a syntax error, not a prompt to fill something in. Prose may still write `git show <base>:<path>` to describe a command's SHAPE; that is not a snippet to run. Fences of other languages (the `json` status schema below) are data shapes, not commands, and are exempt.

**Executable does not mean FOREGROUND.** A fence is valid shell, but its DISPATCH SHAPE is stated in the fence itself and is part of the rule: the Verification Gate fence is a gate BODY — the `$CMD` of a detached, chunk-polled dispatch (`WAIT-PROTOCOL.md`) — and running it as a blocking foreground command violates the wait rule and will hit an idle timeout. "Copy-pasteable" was the wrong promise and is withdrawn: a fence is runnable, in the shape it says.

**Where these files restate one another, this file is authoritative on conflict.** The restatement is deliberate — an agent may load only one skill — but a duplicated rule is a drift surface, so the moment two copies disagree, PROTOCOL.md wins and the other copy is the defect to fix.

## GOVERNANCE

Every rule-bearing change requires prior NEUTRAL approval from the DESIGNATED AUTHORITY before **ADOPTION** — that is, before it lands on the integration branch and becomes binding. The designated authority is the codex architect in every case but one: where codex is STRUCTURALLY UNABLE to act, because the change is the repair of the very pin every codex leg requires, the authority is the user (see Unavailable-authority substitution below). Stating it this way is deliberate — an unqualified "codex must approve everything" makes the pin repair unapprovable and the rule self-blocking, and a strict reader then either deadlocks or breaks the rule to proceed. The requirement of prior neutral approval is universal and never waived; who holds it is defined, not improvised. Covered surface is complete: `CLAUDE.md`, `AGENTS.md`, every `.claude/skills/**` file, all protocols, every guard's assertion/needles/exemptions/rationale plus its test, design-doc/contract invariants, and any `docs/arch` plan/debt row changing constraints, sequencing, or priority. This is not limited to architectural, severe, or this-skill rules. An unapproved rule-bearing change that has LANDED = block + revert. Implementing agents cannot self-certify.

The gate is on ADOPTION, not on authorship: a PROPOSAL — the edit, committed on an unmerged block branch — is how the change becomes reviewable at all, and forbidding it would forbid the review. A proposed rule change binds nobody until it lands. What is forbidden is landing it, or citing it, before approval.

Procedure: prepend CODEX-ARCHITECT MANDATE, state exact change + rationale, ask neutrally "is this rule/skill/guard/design/plan change correct and necessary?", land only on approval. Regular code/test/doc updates that do not alter rules are unaffected.

**Unavailable-authority substitution.** One case is structurally different from every other, and it is named here rather than left for an agent to improvise: when codex approval is UNOBTAINABLE for the very change that would restore codex — a retired or unavailable model pin — the ratifying authority is the USER. Every codex leg requires the pin; changing the pin requires a codex leg; so an unavailable pin makes its own repair unapprovable, and no amount of strictness resolves that, because the approver cannot act at all.

Do not describe this as an exception to the approval rule, and do not pretend nothing is given up. Both would be false, in opposite directions:

- It is NOT "X is required, except here" — the form Rule-File Integrity forbids. The requirement that a rule-bearing change be APPROVED BEFORE ADOPTION is untouched: the change is still reviewed, still ratified, still recorded. Only the IDENTITY of the approver changes, and only because the designated one is definitionally incapable of acting.
- It IS, honestly, a codex leg that does not happen. The cross-model check is genuinely absent for that one edit; the user is not a substitute for codex's independent lens, and claiming otherwise would be the overstatement this whole change exists to end. What makes it legitimate is not that nothing is lost — it is that the alternative is a system that can never repair itself, and that the loss is bounded, visible, and recorded.

Scope is strict and non-generalizable: `codex-model-policy.toml` only, for an unavailable pin only. It is never a "codex was slow / hard to reach / disagreed" escape, and it may not be extended by analogy to any other file or any other unreachable reviewer. It is a STOP boundary, escalated to the user, who ratifies the replacement and whose ratification is recorded with the reason the normal authority could not act.

## CODEX-ARCHITECT MANDATE

Prepend this verbatim to EVERY codex consult/fork/review/approval/adversarial-audit/best-implementation prompt. The mandate sets the bar; the substantive question remains neutral.

> You are the sole architecture authority. Judge NEUTRALLY and UNPRIMED — the prompt must never state or imply the desired conclusion; ask "is X correct / what is the best design", never "confirm X". Mandate:
> - The BEST-of-the-best architecture. No shortcuts. No compromises. No "good enough".
> - Breaking changes are ALLOWED and expected where they yield the correct long-term design — do not preserve a worse design to avoid breakage.
> - Performance is a first-class concern — weigh allocation, cache, warm-state, and hot-path cost, not just correctness.
> - Be brutally HONEST — surface the strongest counter-argument, flag every risk/uncertainty, never rubber-stamp. If the thing under review is wrong, say so plainly.
> - Prefer the correct, durable design over the easy/fast one, regardless of effort, size, or migration breadth.
> Deliver a clear verdict + enumerated, actionable findings (file/section/exact change). End with `__DONE__`.

## Never Prime

Every reviewer, consult, verifier, confirmer, and adjudicator prompt must present artifact + neutral question and let the agent decide. Do not say or imply the expected answer. Use "assess whether X is correct and complete / what is best"; never "confirm X is correct". Applies to post-land confirm too.

## Memory Is Not Authority

**Memory is non-authoritative context. Operational commands and volatile identifiers have exactly one current authority: the repository protocol/preflight. Memory may link to that authority but must not duplicate or override it.**

Briefs, prompts, ledgers and reports are also persisted — but each is scoped to one run and reviewed within it. Memory is the artifact that is written by one session, read as authoritative by every later one, scoped to no run, and reviewed by nobody: it has a protocol's reach with none of its gates. This rule is not "remove the bad remembered command"; it is that **memory can never again become an unreviewed executable policy surface**. A remembered command outlives the conditions that justified it, is copied into briefs, and is followed by agents that never saw it reviewed.

So: no memory carries an OPERATIONAL invocation, kill command, model slug, branch name, machine path, or other volatile identifier — the things a later agent will ACT on. It links to the authority instead.

Two corollaries, so this is not over-applied. Both turn on the same test: **would an agent RUN this, or does it merely record what was?**

- A **prohibition** is FAIL-SAFE where a prescription is FAIL-DANGEROUS. A stale prescription executes the wrong action; a stale prohibition merely refuses one — it can cost you work, but it cannot spend it wrongly. That asymmetry is why a prohibition may be recorded anywhere and a prescription may not; it is not a claim that a stale prohibition is harmless.
- A **historical record** is safe likewise. A memory or `docs/arch` row naming the branch, commit, or model that a past change landed on is PROVENANCE — an immutable fact about what already happened, not an instruction for today. Rewriting provenance to satisfy this rule would falsify the record it exists to keep.

**Briefs obey a different rule, because two unlike things get confused when they are lumped together.** The test is RE-DERIVABILITY, not literalness:

- **VOLATILE GLOBAL CONFIG** — the codex model slug, reasoning effort, timeout, retry ceiling. Never embedded in a brief, a memory, or any rule file EXCEPT the two places that must hold it: the **policy authority** (`codex-model-policy.toml`, which IS the pin) and the **run ledger** (`CODEX_PIN`, which records what actually bound, as evidence). Everywhere else it is resolved at preflight — precisely because a stray copy goes stale and then silently binds the wrong reviewer at full confidence.
- **RUN IDENTITY** — repo root, worktree path, block branch, INTEGRATION BRANCH, block id. Carried in the brief. Be precise about why, because the halves differ: repo root, worktree, and the block branch ARE re-derivable in place (`git rev-parse --show-toplevel`, `git branch --show-current`) and are carried only so the brief is self-contained — re-deriving them is equally correct. The **integration branch and the block id CANNOT be re-derived at all** — `git branch --show-current` in a block worktree returns the BLOCK branch, so an agent told to "re-derive the integration branch" derives the wrong one and rebases onto itself. Those two MUST be carried; nothing in the worktree knows them.

So the rule is not "never freeze a re-readable fact" — a self-contained brief may restate run identity, and an agent may re-derive what is re-derivable. The rule is narrower: **volatile global config is frozen in exactly two places — the authority that defines it and the ledger that records what bound — and nowhere else.** Its hazard is that a stray copy keeps working after the authority changes, binding the wrong model at full confidence. Run identity carries the opposite risk — it goes missing, not stale — so carrying it is safe and omitting it is not.

## Decision Admission

Any falsifiable repository claim that could change scope, a brief, a baseline, a regression, a verdict, a finding disposition, or a REOPEN is recorded at that boundary as `VERIFIED` — with fresh input-bound evidence: commit/worktree, relevant build or prerequisite identity, query/command, result, evidence locator — or as `HYPOTHESIS` with the deciding check named. A report or review NEVER verifies its own premise; a review finding's factual claim is a hypothesis until independently re-derived against the named tree. Only `VERIFIED`, `PRODUCT-DECISION`, or `ARCH-DECISION` may become an unconditional instruction; a `HYPOTHESIS` may dispatch diagnosis or a CONDITIONAL fix ("locate and reproduce; if verified, fix; else STOP") only. Use the cheapest authoritative check permitted to the role before escalating to an architecture or fix swarm.

Labels attach at decision boundaries, NOT to every material sentence — sentence-level labelling is token-heavy, self-certifying, and gets dropped under pressure. Ordinary explanatory prose stays unlabelled.

Enforcement is JUDGMENT: exercised by the CTO immediately before re-briefing, relaying a correction, declaring a baseline or regression, or dispatching a reopen; interpretive evidence is owned by a diagnostic/verify manager; the confirm manager audits cited evidence post-land. Residual risk: weak evidence self-certified as `VERIFIED`. Input identity plus the independent confirm audit reduce but do not eliminate it.

## Intent Contract

No implementation brief is dispatched without a ratified intent contract (`CLAUDE.md` → Planning): necessity, required and forbidden observable outcomes, authority/fallback order, a planned test or gate per stable acceptance ID, and material performance bounds. A substrate block may reference its parent contract but states the invariant and performance contribution it owns. Mechanism design does not begin before the contract exists. Ratification comes from the approved plan or product authority; architecture is ratified through the configured codex decision mode.

## Rule-File Integrity

Rule-bearing artifacts are read-only unless the block's sanctioned scope is to change that rule. A PROPOSED change on an unmerged branch is not an edit-in-violation — it is the artifact under review, and forbidding it would forbid the review that approval depends on (GOVERNANCE above). Approval gates ADOPTION; a proposal binds nobody until it lands, and reviewers treat an unapproved rule edit OUTSIDE a sanctioned proposal as a defect. Never add an exception/carve-out/accepted residual/sole exception/landing blocker that WEAKENS any rule, guard, assertion, test, or plan row. Known violations go in the `docs/arch` debt ledger with codex-DEFER ruling or a fix block, never rule text.

The **governance recovery path** in GOVERNANCE above (an unavailable model pin) is NOT an exception to that rule, and calling it one would itself be the forbidden `X is required, except here` form. Nothing is waived there: a rule-bearing edit still requires approval before adoption. What changes is WHO ratifies — because the normal approver is *definitionally incapable of acting* (codex cannot review the repair of the very pin it needs in order to run), the ratifying authority is the user. That is authority SUBSTITUTION under an unreachable approver, not a relaxed gate: the bar is unchanged, the approval is still obtained, and it is obtained from the only authority that can give it. A carve-out that WEAKENS a rule, guard, assertion, test, or plan row remains forbidden with no exceptions, and none may be minted by analogy to this.

Distinguish two things that look alike and are opposites. A **carve-out** relaxes what the rule demands ("X is required, except here") — forbidden in rule text, always. **Naming the limit of the current mechanism** does not relax anything: it states the requirement in full, admits the mechanism does not yet reach it, and points at the debt row that owns the gap. That is REQUIRED, not tolerated (`CLAUDE.md` → Architecture guards: "the rule text names the planned guard/test and the gap is tracked"). The test is whether an agent could cite the sentence to skip a gate. "Terminate only your own recorded tree; descendant reaping is not yet guaranteed (GI-6)" forbids exactly what it forbade before. "Terminate your own tree, unless that is inconvenient" is a carve-out. The first is honesty; the second is the thing this rule exists to stop.

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

Roles required: implementer/fix fresh context, claude review/§1a/confirm fresh context, codex. The default Agent/Task mechanism gives fresh-context role separation on a single account (harness-managed), bounded by harness Agent concurrency capacity, ONLY WHEN the four capability properties are ESTABLISHED AND RECORDED per `Dispatch` below — no inherited transcript/hidden state beyond the passed prompt, a distinct agent identity, status/stop/continue control, and child-agent spawning where required. Read that standard from `Dispatch` and nowhere else: it is an ATTESTATION (a risk-accepted precondition, not a proof — GI-9/GI-14), and saying "the harness GUARANTEES" here while `Dispatch` says an unproven attestation authorizes dispatch would leave a session holding an attestation both required to fall back and permitted to proceed. Absent, failed, stale, or unknown ⇒ fall back to `claude -p` for an explicit fresh-process boundary. On the opt-in `claude -p` path, read the account→role mapping from the live brief/ledger after smoke tests and never hard-code account names; separate accounts are then an availability/parallelism optimization, not required. Single account = separate fresh Agent sub-agents (unprimed prompts), codex cross-model check, serialized heavy gates. Stop/escalate if no implementation agent capacity is available or codex unavailable.

Run file-disjoint blocks in parallel (the default). No artificial 300s spacing or one-claude-at-a-time throttle; respect harness Agent concurrency capacity. On the opt-in `claude -p` path, map separate-account instances explicitly, one block per account, with no artificial sub-agent-count limit. Triple-review legs run in parallel.

Canonical/full-suite/heavy gates are globally SERIALIZED — concurrent cargo runs in one worktree can corrupt the target dir, and two heavy gates on one machine produce timings and flakes that neither owns. "Serialized under a lock" is only a rule if the lock exists, so the lock is defined in the Verification Gate fence below and is acquired AROUND the gate, not beside it. A lock that is taken and released without wrapping anything serializes nothing.

**That fence is a gate BODY, not a foreground command.** It waits on a lock and then runs the full suite — minutes at best, and the lock wait is bounded only by a long ceiling — so pasting it into a blocking foreground call violates the wait rule in `WAIT-PROTOCOL.md` (a long task that nothing will wake you from) and will hit an idle timeout. It is the `$CMD` of a DETACHED, chunk-polled dispatch: launch it through the WAIT-PROTOCOL lifecycle, poll its marker, collect its EXIT STATUS. The inner `errexit`/`pipefail` shell is what makes that status honest; the marker is what makes the wait legal.

codex model and effort are POLICY per role, read from the ratified authority — never a silently downgraded reviewer. The slug is NEVER hardcoded in any rule file OTHER than the policy authority itself (which IS the pin): the protocol references `CODEX_MODEL_POLICY`, the ledger records the resolved mapping with the run identity, invocations interpolate it, and the startup banner verifies it (`codex Invocation`). Preflight discovers AVAILABILITY, never intent. Unavailable, substituted, unknown, or mismatched ⇒ BLOCK the leg; never substitute, upgrade, downgrade, or reuse another role's model. Every gate-bearing Claude role — block manager, reviewer, §1a verifier, confirm manager, integration-confirm manager, implementer/fix (the block and integration-confirm managers own the landing and integration gates and are explicitly covered) — MUST run on the highest available Claude model at max/highest effort, bound either by an explicit `model`/`effort` arg at spawn OR by an audited agent-definition whose model+effort are recorded in `PROGRESS.md`; an unknown or default model/effort BLOCKS the dispatch — never silently downgrade any gate-bearing role. That BLOCK is deliberate and is not a deadlock: a gate-bearing role whose model and effort cannot be audited must not run AT ALL, so the correct move is to stop and escalate, never to fall back to an unaudited dispatch. On the opt-in `claude -p` path, bind that highest model + `--effort max` explicitly per agent and watch `CLAUDE_CONFIG_DIR`: a wrapper account exports it into children, so bare `claude` may silently use the wrapper account — override at loop start (`export CLAUDE_CONFIG_DIR=$HOME/.claude`) when dispatching bare `claude`; identical reset times across "different" accounts indicate the trap.

## Repo / Worktree

Repo root, integration branch, worktree, branch, and block id are RUN IDENTITY: they ALL come from the brief and are never hardcoded here. A literal REPO ROOT, WORKTREE PATH, or BRANCH NAME in this protocol is an instruction that cannot be followed on another machine or after the integration branch moves — and it cannot be repaired by "re-derive it live", because the integration branch is exactly what a block worktree cannot tell you: `git branch --show-current` there returns the BLOCK branch. (This bans run-identity literals, not every path: the portable scratch convention below — `/tmp/mom/<block>/…` — is deliberate and machine-independent.) Carry run identity in the brief; resolve volatile global config (model, effort, timeout, retry ceiling) at preflight (see Memory Is Not Authority). Never commit directly to the integration branch. All edits use absolute paths in the worktree, and every build/test command RUNS FROM the worktree — either `cd "$WORKTREE" && <cmd>` per command, or a single `cd "$WORKTREE"` at the top of a block (as the Verification Gate fence does). What is forbidden is a command whose working directory is merely assumed: the orchestrator's shell cwd is the MAIN checkout, and a relative path there silently produces a plausible, wrong result. Every sub-agent brief repeats this.

Every phase/stage has its own worktree. Implementers may commit `wip:` checkpoints after cheap checks. A LANDED block IS exactly ONE clean conventional commit: the manager squashes and lands it (`LANDING-PROTOCOL.md`), and the CTO confirms it AFTER it lands — squash precedes land precedes confirm, and nothing is re-squashed afterwards. A REOPEN is a NEW commit on top, never a rewrite of landed history. A stage/phase may contain multiple block commits only because multiple blocks landed independently. Only conventional commits require the full gate.

## Dispatch

Default mechanism is the **Agent/Task tool**, gated on harness support. The CTO spawns each manager as an Agent sub-agent; managers spawn their implementer/fix/review/verify (§1a) agents as Agent sub-agents too (agents may spawn child agents — the manager→children topology). Managers NEVER spawn the confirm manager — only the CTO/MoM dispatches the separate unprimed confirm (and integration-confirm) MANAGER after land, so the post-land gate stays independent of the author. The Agent tool is the default ONLY WHEN four properties are ESTABLISHED AND RECORDED: (a) no inherited transcript/hidden state beyond the passed prompt, (b) a distinct agent identity, (c) status/stop/continue control, and (d) child-agent spawning where the role needs it. Recorded once per session as a `CAPABILITY mechanism=agent isolation=<result> identity=<result> stopcontinue=<result> childspawn=<result> result=PASS` line in `PROGRESS.md`/the CTO ledger BEFORE the first Agent dispatch (see `CHECKPOINT-PROTOCOL.md`).

"Established and recorded" is the operative bar, and it is deliberately not the words "the harness GUARANTEES" — that would promise something no line in a ledger can deliver, and would contradict the very next paragraph. Today the establishment is an attestation: a risk-accepted precondition, not a proof (GI-9), with the question of whether it should suffice escalated at GI-14. What is never permitted is ASSUMING a property from silence.

State the strength of that record accurately, because overstating it is the defect these rules exist to close: the line is an ATTESTATION, not a proof — a typed `PASS` cannot by itself demonstrate the absence of inherited hidden state. Two rules follow, and they must be read together or they contradict each other:

- **UNRECORDED means absent.** A missing, failed, or stale entry is treated exactly as a failure: it forces the `claude -p` fallback. No property is ever assumed from silence.
- **A recorded `PASS` is OPERATIVE, and is a recorded risk acceptance rather than a proof.** It authorizes Agent dispatch today. This is stated plainly rather than dressed up: if "unproven" also disqualified an attested property, the attestation would authorize nothing, every dispatch would fall to `claude -p`, and the rule would be a dead letter that reads strict. The executable probe that would let this record earn the word "proof" is owed and tracked (`docs/arch/gate-integrity-ledger.md`, GI-9), and **whether an attestation may authorize Agent dispatch at all is an open question escalated to the ratifying authority** — who may tighten this to "no dispatch without the GI-9 probe". Until they rule, the operative answer is the one written here, and its weakness is on the record instead of hidden behind the word "verified". An absent or stale capability attestation counts as "unknown" and forces the `claude -p` fallback — an explicit fresh-process boundary; any single missing/failed property does the same. Each starts cold with a self-contained brief as its prompt; the agent's final message IS its report — and for gate-bearing roles (review, §1a, confirm, anti-rogue) the exact prompt and the verbatim final report are persisted to files with a recorded input id, never inline-only (see `CHECKPOINT-PROTOCOL.md`). A blocking Agent call returns that report; a background (`run_in_background`) Agent call notifies the spawner on completion — no resume-loop wrapper, no stream/marker watchdog. Continue a still-live agent with its context via its id/name; a fresh Agent call starts cold.

The dispatch mechanism affects oversight-gate PROPERTIES — confirm independence, reviewer model quality, fresh-context isolation, and durable auditability — not just transport. Be exact about their standing, because "preserved" overstates every one of them to a different degree. Confirm independence (CTO-only dispatch), reviewer quality (recorded model+effort binding), and auditability (persisted brief+report+input-id) are PROCEDURAL REQUIREMENTS with recorded evidence — they are checkable after the fact, and a missing record fails the gate, but a followed procedure is not a mechanism that makes the property true. Fresh-context isolation is weaker still: its safeguard is a typed attestation, a RISK-ACCEPTED PRECONDITION and not a proof (GI-9) — and note that confirm INDEPENDENCE partly rests on it, since a confirmer that inherited the author's context is not independent no matter who dispatched it. So: three are enforced by recorded procedure, the fourth is accepted with its gap on the record, and none is "guaranteed" in the sense that word invites. Each gate is conditioned on a recorded precondition, never asserted: confirm/integration-confirm independence on CTO-only dispatch; fresh-context isolation on the recorded `CAPABILITY … result=PASS` attestation (an assertion pending the GI-9 probe, not a proof) (absent/stale ⇒ `claude -p`); reviewer/manager quality on the recorded highest-model+max-effort binding per gate-bearing role (unknown/default ⇒ BLOCK); auditability on the persisted brief+report+input-id+model+effort. The 3/3 review, §1a, confirm, and anti-rogue gates pass only when their preconditions are recorded — a missing or stale precondition leaves that gate unmet. CTO dispatches only managers; managers dispatch implementers/fix/review/verify (§1a) agents, never the confirm manager. Trust but verify: every report is INTENT, not fact, and every decision-bearing claim is unverified until ADMITTED. The CTO confirms git metadata directly (`git show --stat`, `git log`) and may run one bounded mechanical probe per pending decision; source/content, test-count, baseline, regression, and review-finding claims are re-derived by a fresh diagnostic/verify manager against the named tree before the CTO acts (see Decision Admission).

Codex is unchanged — read-only reviewer/architect/decider only, a Bash-invoked CLI subprocess (never `claude -p`); all code/tests/fixes are by claude; any codex-written code is suspect and redone by claude.

`claude -p` CLI subprocesses are **OPT-IN only**, on exactly three triggers: (a) explicit user request; (b) a genuinely separate account instance for multi-instance parallelism or work that must outlive the parent session; or (c) **the capability fallback above** — an absent, failed, or stale `CAPABILITY … result=PASS` attestation. (c) is not a loophole, it is what keeps the dispatch rules from deadlocking: without it, a session whose harness capability is merely UNKNOWN could neither use Agent (unproven) nor `claude -p` (not opted in), and would have no legal way to dispatch anything at all. Default Agent mode is single-account harness-managed parallelism; the opt-in `claude -p` path is what restores multi-account claude instances, and it carries the `claude -p`-SPECIFIC discipline below — the resume-loop and `CLAUDE_CONFIG_DIR` handling. Be careful with the word ALONE, which an earlier draft used here and which was wrong: the detach + marker + chunk-poll LIFECYCLE is not `claude -p`-specific at all. It applies to ANY long detached shell task, the heavy Verification Gate included (`WAIT-PROTOCOL.md`). What is specific to `claude -p` is the agent-level resume-loop and the config-dir trap. Agent calls need no polling of any kind — the harness resumes you. Detached shell gates always do:

```bash
# $CLAUDE_CLI / $CLAUDE_MODEL: resolved at preflight, never literals.
# EVERY path is absolute and PER-DISPATCH. Shared literal OUT/MARKER names let one task's
# completion satisfy another's watchdog — a collision that reads exactly like success — and a
# relative BRIEF.md resolves against whatever cwd the shell happens to hold.
BRIEF="$RUN_DIR/$DISPATCH_ID.brief.md"
OUT="$RUN_DIR/$DISPATCH_ID.jsonl"; MARKER="$RUN_DIR/$DISPATCH_ID.marker"
rm -f "$OUT" "$MARKER" "$MARKER.tmp" \
  || { echo "cannot clear stale artifacts — refusing to dispatch"; exit 1; }   # stale = false pass
# `setsid` makes the child a process-group LEADER, which is what terminate_recorded_tree signals on
# POSIX. It is NOT present in git-bash on Windows — hardcoding it there fails the launch outright
# ("setsid: command not found") and the child never starts, so it is applied only where it exists;
# the helper falls back to the enumerated descendant closure everywhere else. Both launch paths
# spawn IDENTICALLY: a launcher that differs from what the terminator assumes is an orphan generator.
SETSID=""; command -v setsid >/dev/null 2>&1 && SETSID="setsid"   # literal we set; unquoted on purpose

# Values are passed as POSITIONAL ARGS into a SINGLE-QUOTED body — nothing is interpolated into the
# `-c` string. Both launch paths spawn IDENTICALLY (a launcher that differs from what the terminator
# assumes is an orphan generator), and the shape is the one WAIT-PROTOCOL needs: there the command is
# a caller-supplied $CMD, and interpolating it binds the `> "$OUT"` redirect to the LAST command
# inside it — a compound $CMD then loses its output and, if it ends in `exit`, dies before publishing
# the marker. This fence's command is fixed, so it does not hit that failure itself; it uses the same
# form so there is ONE launch shape to reason about, and so no future edit reintroduces the hazard.
$SETSID nohup bash -c '
  "$1" -p --model "$2" --effort max --dangerously-skip-permissions \
       --output-format stream-json --include-partial-messages --verbose \
       < "$3" > "$4" 2>&1
  echo $? > "$5.tmp"; mv "$5.tmp" "$5"
' _ "$CLAUDE_CLI" "$CLAUDE_MODEL" "$BRIEF" "$OUT" "$MARKER" >/dev/null 2>&1 &
WRAPPER_PID=$!   # capture IMMEDIATELY, before any other async launch overwrites $!.
# The in-shell PID is not enough: a restart loses it. The LEDGER is the durable ownership record,
# so if the append fails we are holding a live child we could never prove we own — precisely the
# state this protocol forbids. We still have the PID right now, so kill it while we can.
if ! echo "DISPATCH role=$ROLE wrapper-pid=$WRAPPER_PID out=$OUT marker=$MARKER" \
       >> "$RUN_DIR/PROGRESS.md"; then
  echo "ledger append FAILED — terminating the child rather than orphaning it"
  if ! terminate_recorded_tree "$WRAPPER_PID"; then
    # BOTH failed: we can neither RECORD the child nor KILL it. Exiting with a warning here would
    # abandon a live tree that nothing owns — the exact orphan this protocol forbids, dressed as a
    # handled error. The transcript is now the record of last resort, so it must carry the PID, and
    # the run STOPS rather than stacking further dispatches on top of an untracked process.
    echo "BLOCK: unrecorded, unterminated child wrapper-pid=$WRAPPER_PID out=$OUT"
    echo "BLOCK: ledger unwritable AND termination unconfirmed — HUMAN CLEANUP REQUIRED"
    exit 2   # distinct from 1: not a failed dispatch — an UNOWNED LIVE TREE
  fi
  exit 1
fi
```

The marker carries the child's EXIT STATUS, never the word `DONE` — a present marker means the task ENDED, not that it succeeded (`WAIT-PROTOCOL.md`).

On that path, detach resume-loop wrappers (`nohup`/`setsid`) and monitor stream/status/report/marker/git activity; intervene only on a true hang — no activity on ANY signal across multiple bounded windows — by terminating the RECORDED wrapper PID and its process tree (which includes the inner `claude -p`, itself recorded as `wrapper-pid` in the ledger), confirming those PIDs are gone, then redispatching. Never terminate by pattern or image name (`pkill -f BRIEF`, `killall`): a pattern match reaches other dispatches and the user's own processes, and a sibling killed by someone else's cleanup is indistinguishable from a hang. A LIVE agent with repeated activity but zero durable artifacts across multiple windows is non-converging: terminate its recorded tree and redispatch with a tightened forcing-function brief.

## codex Invocation

**Model and effort are POLICY, not discovery.** Three things stay separate, and none may leak into another:

1. **POLICY** — `CODEX_MODEL_POLICY[role] = { model, reasoning_effort }`, for every role: review, §1a, anti-rogue, architecture, best-implementation adjudication. The sole ratified authority is **`reference/codex-model-policy.toml`** — the ONE source of the CURRENT OPERATIONAL PIN. It is the one file that MAY carry a slug, because it IS the pin; no OTHER rule file, template, brief, or memory may originate one. (A `docs/arch` or `docs/better-implementation` row naming the model that issued a past ruling is PROVENANCE — an immutable fact about what already happened, not the pin. It is exempt, and rewriting it would falsify the record.) Current shape: **ONE entry applied to EVERY role; there is no role split**, so a dispatcher cannot select a model by selecting a role. The TOML's `roles` array is authoritative; any list in prose is illustrative. A policy change is then one config value, never a doc edit.
2. **PREFLIGHT FACTS** — whether the POLICY-SELECTED model is available; which model/effort ACTUALLY bound; whether the startup banner matches the expected entry. Preflight discovers availability. It never chooses.
3. **FAILURE** — unavailable, substituted, unknown, or mismatched model/effort **BLOCKS THE LEG**. Preflight must NOT select a replacement, upgrade, or downgrade. A blocked leg is loud; a substituted reviewer is silent — and silence is the defect being closed.

**Pin retirement is a STOP, and the recovery authority is the USER — not codex.** If the pinned model is retired or unavailable, no codex leg can run; but changing the pin is a rule-bearing edit, which normally requires codex approval. That is a genuine deadlock, and it resolves in exactly one direction: an unavailable pin is a STOP boundary (`Architecture / Decision Modes` already lists "codex unavailable"), escalated to the user, who ratifies the replacement pin. Codex approval is not obtainable for that edit and is not required for it. Do not resolve the deadlock by substituting a model — that is the silent-downgrade defect wearing a governance hat.

**Preflight can tell you what is AVAILABLE. It cannot tell you what is INTENDED.** A preflight that infers policy from availability is the capability-vs-evidence conflation: the same defect wearing a different hat.

Resolve ONCE from the ratified authority → persist the mapping with the run identity (`CHECKPOINT-PROTOCOL.md` → `CODEX_PIN`) → interpolate into every invocation → verify the banner. Preflight is step zero: an absent, stale, or unverified policy record BLOCKS the dispatch.

Preflight reads the authority ONCE and resolves the entry into two SCALARS, `CODEX_MODEL` and `CODEX_EFFORT`. Bash associative arrays hold strings, not nested records, so a policy lookup cannot be written inline as a parameter expansion: `${CODEX_MODEL_POLICY[$ROLE].model}` is not a command — it is a `bad substitution` that exits 1 — and it must never be copied into a snippet as if it were executable. Resolve to scalars in preflight; interpolate the scalars. The block below is the ONE invocation form in the tree; every other file references it and never re-derives a second copy.

```bash
# Every scalar below is resolved at preflight from reference/codex-model-policy.toml
# ($CODEX_MODEL, $CODEX_EFFORT, $CODEX_TIMEOUT, $CODEX_MAX_ATTEMPTS). None is a literal here.
# Foreground, bounded timeout, prompt on stdin, transcript to a file, VERDICT to its own file.
#
# The bound is GNU coreutils `timeout`, and it is the ONLY thing standing between a wedged leg and
# an unbounded stall — so its absence must BLOCK, not silently degrade into an unbounded run. Stock
# macOS does not ship it (Homebrew coreutils installs it as `gtimeout`), and BSD variants reject
# --kill-after, so the candidate is PROBED with the exact flag rather than trusted by name.
TIMEOUT_BIN=""
for c in timeout gtimeout; do
  if command -v "$c" >/dev/null 2>&1 && "$c" --kill-after=1s 1s true >/dev/null 2>&1; then
    TIMEOUT_BIN="$c"; break
  fi
done
[ -n "$TIMEOUT_BIN" ] || {
  echo "BLOCK: no GNU 'timeout' supporting --kill-after (macOS: brew install coreutils)"; exit 1; }

# The RETRY CEILING is EXECUTED here, not described beside a fence that dispatches once. A prose-only
# "redispatch up to max_attempts" next to a single-shot fence is a lifecycle the code does not
# implement — and a declared-but-unused $CODEX_MAX_ATTEMPTS is the tell. The ledger lines are written
# by the same loop that dispatches, so a leg cannot run without leaving a record that it ran.
#
# VALIDATE BOTH BOUNDS BEFORE TRUSTING EITHER. A bound you did not check is not a bound, and each of
# these fails OPEN — the dangerous direction — in a way the surrounding prose would still call bounded:
#
#   CODEX_MAX_ATTEMPTS: `[ "$ATTEMPT" -gt "$CODEX_MAX_ATTEMPTS" ]` with an empty, non-numeric, or
#   out-of-integer-range value is a `test` ERROR, and an erroring test inside `if` is simply FALSE —
#   so the "bounded" loop never exits and redispatches forever. A 40-digit ceiling defeats the bound
#   exactly as a non-numeric one does, so the range is capped, not merely the syntax.
#   (Verified: `[ 5 -gt "" ]` and `[ 5 -gt abc ]` are both false.)
#
#   CODEX_TIMEOUT: GNU `timeout 0 …` means NO TIMEOUT. A zero (or empty, or unit-less garbage)
#   silently converts the one hard bound on a foreground leg into an unbounded hang — the exact stall
#   this protocol exists to prevent, arrived at through the mechanism meant to prevent it.
case "$CODEX_MAX_ATTEMPTS" in
  ''|*[!0-9]*) echo "BLOCK: CODEX_MAX_ATTEMPTS='$CODEX_MAX_ATTEMPTS' is not a positive integer"; exit 1 ;;
esac
# Range-check NUMERICALLY, both ends. A literal-`0` pattern misses `00`, which is also zero — the same
# leading-zero near-miss as the timeout below. `-ge 1` and `-le 10` catch every spelling of it.
{ [ "$CODEX_MAX_ATTEMPTS" -ge 1 ] && [ "$CODEX_MAX_ATTEMPTS" -le 10 ]; } 2>/dev/null \
  || { echo "BLOCK: CODEX_MAX_ATTEMPTS='$CODEX_MAX_ATTEMPTS' is out of range (1..10)"; exit 1; }

# Strip the unit and require the NUMBER to be positive. Matching the literals `0`/`0s`/`0m`/`0h` is
# NOT enough, and the near-miss is the dangerous one: `00s` is not `0s`, so a literal list accepts it,
# and `00s`, `000m`, `0d` are all ZERO. GNU `timeout 0 …` means NO TIMEOUT — so any of them silently
# converts the leg's only hard bound into an unbounded hang, then reports `state=OK`. Verified.
case "$CODEX_TIMEOUT" in
  ''|*[!0-9smhd]*) echo "BLOCK: CODEX_TIMEOUT='$CODEX_TIMEOUT' is not a duration"; exit 1 ;;
esac
CODEX_TIMEOUT_N="${CODEX_TIMEOUT%[smhd]}"          # 550s -> 550, 10m -> 10, 0d -> 0, 00s -> 00
case "$CODEX_TIMEOUT_N" in
  ''|*[!0-9]*) echo "BLOCK: CODEX_TIMEOUT='$CODEX_TIMEOUT' has no single numeric part"; exit 1 ;;
esac
[ "$CODEX_TIMEOUT_N" -gt 0 ] 2>/dev/null \
  || { echo "BLOCK: CODEX_TIMEOUT='$CODEX_TIMEOUT' is ZERO — GNU timeout reads 0 as 'no timeout'"; exit 1; }
ATTEMPT=1
while : ; do
  # A verdict/transcript left by a PREVIOUS attempt is a stale pass that satisfies today's check —
  # and a REMOVAL THAT FAILS leaves it in place, so the removal is checked like anything else.
  rm -f "$VERDICT" "$OUT" || { echo "cannot clear stale artifacts — refusing to dispatch"; exit 1; }
  echo "CODEX_DISPATCH id=$DISPATCH_ID role=$ROLE attempt=$ATTEMPT model=$CODEX_MODEL" \
       "effort=$CODEX_EFFORT prompt=$PROMPT out=$OUT timeout=$CODEX_TIMEOUT state=RUNNING" \
       >> "$LEDGER" || { echo "cannot record the dispatch — refusing to run an unrecorded leg"; exit 1; }

  # --kill-after: TERM alone does not guarantee termination — a child that ignores it keeps
  # running. --kill-after escalates to KILL, so "the timeout terminated it" is true, not hoped.
  # The subshell keeps the `cd` from leaking into the next iteration.
  ( cd "$WORKTREE" && "$TIMEOUT_BIN" --kill-after=30s "$CODEX_TIMEOUT" codex exec \
      --sandbox read-only --skip-git-repo-check \
      -c "model=\"$CODEX_MODEL\"" \
      -c "model_reasoning_effort=\"$CODEX_EFFORT\"" \
      -o "$VERDICT" \
      < "$PROMPT" > "$OUT" 2>&1 )
  CODEX_EXIT=$?   # capture FIRST — the next command overwrites $?

  # The BANNER CHECK is part of the gate, not a note beside it: left to prose it is a judgment an
  # agent may simply skip, and an unverified leg then passes at full confidence. It is read from the
  # BANNER REGION ONLY and matched with -x (WHOLE LINE). Both matter — an unanchored search of the
  # transcript would match the model name inside the ECHOED PROMPT (a check the prompt can satisfy by
  # itself is not a check), and a substring match would accept a "<pinned-model>-mini" that merely
  # starts the same way. (Written generically on purpose: the slug lives in the policy authority and
  # nowhere else — not even in an illustration, which is exactly the kind of copy that outlives a pin.) The region is taken STRUCTURALLY, between the CLI's two `----` rules, never by a
  # magic line count: a fixed window (`sed -n 1,20p`) swallows prompt text the moment the banner
  # changes size.
  #
  # The region must be CLOSED, and this is the subtle one — the check LOOKED structural but was not.
  # `awk '/^-+$/{n++; next} n==1'` emits everything after the FIRST rule when the second is MISSING, so
  # a reformatted or truncated banner silently promotes the WHOLE TRANSCRIPT — prompt echo included —
  # into "the banner region", and the echoed policy lines then satisfy both greps at full confidence.
  # Verified: it accepted a leg whose real banner read `cheap-substituted`. So BOTH rules must exist and
  # the scan STOPS at the second. An unclosed banner is a FAILED leg, never a pass.
  #
  # `[ -s "$VERDICT" ]` is an EXISTENCE gate — the leg produced its artifact — NOT a verdict validator:
  # a non-empty file may still hold a refusal, and judging its CONTENT is the reader's job. The
  # machine-checkable grammar that would change that is owed at GI-7.
  # Extract the FIRST `----`-delimited region and require it to LOOK LIKE the CLI's banner, then
  # require the fields to be unambiguous within it:
  #   (1) a CLOSED region — an unclosed one runs to EOF and swallows the prompt echo;
  #   (2) the region carries the banner's STRUCTURAL KEYS (`workdir:`, `provider:`, `sandbox:`,
  #       `session id:`), not merely the two lines we intend to test. A stray `----` block in the
  #       echoed prompt does not look like a banner; this is what stops the region from being the
  #       wrong region;
  #   (3) EXACTLY ONE `model:` and ONE `reasoning effort:` line inside it — an ambiguous region is a
  #       FAILED leg, never a leg that gets to pick whichever line matches.
  #
  # DO NOT anchor on "the first line is a rule": the CLI prints `Reading prompt from stdin...` and its
  # version line FIRST. An earlier draft required that anchor and rejected every real leg — a gate so
  # strict it fails closed on the truth is still a broken gate, and it was caught only by running it.
  #
  # HONEST LIMIT, and it is the reason GI-8 exists: this is a SCRAPE of a stream that also contains the
  # echoed prompt. It is hardened against the failure modes actually observed, but a text scrape of a
  # contaminated stream cannot be PROVEN spoof-proof — a sufficiently banner-shaped block inside the
  # prompt, combined with a malformed real banner, still defeats it. Do not describe this check as
  # spoof-proof. The airtight version is a PINNED banner contract read through one shared, tested
  # validator (ideally structured CLI output, not a transcript scrape) — owed at GI-8.
  BANNER=""
  if [ "$(grep -c '^-\+$' "$OUT")" -ge 2 ]; then
    BANNER="$(awk '/^-+$/{n++; next} n==1{print} n>=2{exit}' "$OUT")"
    for key in 'workdir: ' 'provider: ' 'sandbox: ' 'session id: '; do
      printf '%s\n' "$BANNER" | grep -q "^$key" || BANNER=""      # not banner-shaped => wrong region
    done
    if [ "$(printf '%s\n' "$BANNER" | grep -c '^model: ')" -ne 1 ] \
       || [ "$(printf '%s\n' "$BANNER" | grep -c '^reasoning effort: ')" -ne 1 ]; then
      BANNER=""   # ambiguous region: prompt text reached it, or this is not the banner
    fi
  fi
  if [ "$CODEX_EXIT" -eq 0 ] && [ -s "$VERDICT" ] && [ -n "$BANNER" ] \
     && printf '%s\n' "$BANNER" | grep -qxF "model: $CODEX_MODEL" \
     && printf '%s\n' "$BANNER" | grep -qxF "reasoning effort: $CODEX_EFFORT"; then
    # CHECK the append. A bare `echo … >> "$LEDGER"` followed by `break` discards the append's status:
    # the record fails to write, the loop breaks, and the leg reports SUCCESS with no evidence it ever
    # ran — a gate whose receipt is optional is not a gate. Verified: the `break` swallows it.
    echo "CODEX_RESULT id=$DISPATCH_ID attempt=$ATTEMPT exit=0 banner=verified verdict=$VERDICT state=OK" \
      >> "$LEDGER" || { echo "BLOCK: leg passed but its result could not be recorded"; exit 1; }
    break
  fi

  echo "LEG FAILED: exit=$CODEX_EXIT verdict=$( [ -s "$VERDICT" ] && echo present || echo MISSING )"
  printf '%s\n' "$BANNER" | grep -E '^(model|reasoning effort):' | sed 's/^/  banner: /'
  # The FAILED receipt is checked exactly like the OK one. Guarding only the success path is the
  # half-fix that keeps reappearing: an unchecked failure append is discarded by the arithmetic and
  # the retry below, so a later attempt can exit 0 with no record that the earlier one ever failed —
  # the attempt history silently rewrites itself to a clean run.
  echo "CODEX_RESULT id=$DISPATCH_ID attempt=$ATTEMPT exit=$CODEX_EXIT state=FAILED" >> "$LEDGER" \
    || { echo "BLOCK: leg failed AND its failure could not be recorded"; exit 1; }

  ATTEMPT=$((ATTEMPT + 1))
  if [ "$ATTEMPT" -gt "$CODEX_MAX_ATTEMPTS" ]; then
    echo "BLOCK: $CODEX_MAX_ATTEMPTS attempts exhausted — do not proceed without this leg"
    exit 1   # a LITERAL 1, never "$CODEX_EXIT": a leg can exit 0 having written NO verdict, and
  fi         # `exit "$CODEX_EXIT"` would then exit 0 — a failure detector that reports success
done
```

Reading the banner is NOT the forbidden verdict-scan: it reads what the CLI itself printed about what bound — a fact about the process, not a claim the model authored. The verdict still comes from `$VERDICT`, and only from there.

`-o/--output-last-message` writes the leg's FINAL MESSAGE — and only that — to `$VERDICT`. This is what makes "read the verdict, never scan for it" mechanical rather than aspirational: `$OUT` is the full transcript and is CONTAMINATED BY CONSTRUCTION (it contains the prompt echoed back, so every token a verdict scan looks for appears in it), while `$VERDICT` contains nothing but what the leg finally said. Read `$VERDICT`. Use `$OUT` only to diagnose.

**Lifecycle — ONE policy, in every file.** Foreground, with an explicit bounded timeout (`timeout_seconds`). On expiry `timeout --kill-after` has terminated and joined the LEG PROCESS ITSELF — so there is no separate kill step and no PID to hunt, which is precisely why the leg runs in the foreground. Be exact about the limit: that guarantee covers the timeout-managed process, NOT any descendants it spawned, which are not guaranteed reaped (`docs/arch/gate-integrity-ledger.md`, GI-6). FAIL the leg and REDISPATCH, bounded to `max_attempts`, then BLOCK and escalate (an unbounded redispatch loop silently burns reasoning budget forever). Never detach a codex leg, never background-and-poll it, never end a turn waiting on it. Parallelism comes from separate managed review calls with distinct output paths — never from global shell process manipulation. A trailing `&` is the defect, not the pattern.

**Ownership recording is scoped to the dispatch shape — do not demand it where it is unexecutable.** A FOREGROUND leg has no `&`, therefore no `$!`: you cannot record its PID "before the wait", because there is no wait to scope. `timeout --kill-after` is the bound and the terminator, and the blocking call is the join — so in the ordinary case there is nothing to orphan you from. (A host, shell, or harness death can still orphan a foreground child; that is not a case ownership-recording would have helped with, and it is not claimed away here.) A foreground leg records `CODEX_DISPATCH` (id, role, prompt/output paths, timeout, attempt, model, effort) for auditability, and `CODEX_RESULT` (exit, banner, verdict) after the join. It does NOT record a PID.

**A DETACHED dispatch is the opposite, and there the rule is strict:** capture `$!` at spawn — before any other async launch overwrites it — and write it to the durable ledger before the wait (`WAIT-PROTOCOL.md`). A caller with no DURABLE record of what it spawned cannot prove across a restart which tree it owns, and "terminate only your own tree" degrades into a pattern kill, which is forbidden. That is why the detach and the record travel together.

A bare PID is a WEAK ownership token: PIDs are reused, and a plain PID kill does not reap descendants. Termination therefore goes through ONE shared helper, so no caller invents its own — and, crucially, so termination is CONFIRMED rather than assumed. A kill command that fails silently and is never checked is not cleanup; it is an orphan with a reassuring log line.

Three facts drive the shape below, and each was established by RUNNING it under git-bash, not by reading a manual:

1. **`taskkill` does not accept the PID that `$!` gives you.** `$!` is the MSYS pid; `taskkill` wants the native Windows pid (`/proc/<pid>/winpid`). Fed the MSYS pid it prints `ERROR: The process "…" not found`, exits **128**, and kills nothing — so a helper written the obvious way terminates *nothing at all* on the platform it was written for.
2. **`taskkill //F //T` does NOT reap the tree.** Fed the correct winpid it reports `SUCCESS: The process … has been terminated` and exits 0 — while **every descendant keeps running** (observed: leader gone, 3 of 3 children alive). `//T` is not the containment the name suggests.
3. Therefore **confirming the leader is gone is a FALSE GREEN.** A helper that kills by winpid and then checks only `kill -0 "$leader"` returns *success* with a live orphan tree — the precise failure this protocol exists to end.

So the descendant set is enumerated from the recorded root BEFORE the kill (a dead parent hides its children), the whole closure is terminated, and the confirmation covers the closure — not the leader.

```bash
# Emit "pid ppid" per line, or FAIL. The process table is read in the ONE portable way, because the
# obvious way is wrong on half the platforms: bare `ps` prints `PID PPID …` under MSYS but
# `PID TTY TIME CMD` under POSIX, so reading "the second column" as the parent silently yields an
# EMPTY descendant set on Linux and macOS — and an empty set is indistinguishable from "no children",
# so the terminator would report a CONFIRMED clean tree while the descendants keep running.
# `ps -eo` is the POSIX form (MSYS's ps rejects it outright, exit 1, no output — hence the fallback).
process_table() {
  local out src rc
  for src in posix msys; do
    case "$src" in
      posix) out="$(ps -eo pid=,ppid= 2>/dev/null)"; rc=$? ;;                       # Linux/macOS
      msys)  if ps 2>/dev/null | head -1 | grep -qE '^[[:space:]]*PID[[:space:]]+PPID'; then
               out="$(set -o pipefail; ps 2>/dev/null | tail -n +2 | awk '{print $1, $2}')"
               rc=$?                                                                # pipefail: ps's
             else out=""; rc=1; fi ;;                                               # status, not awk's
    esac
    # CHECK THE PRODUCER'S STATUS. A `ps` that exits NONZERO but emits syntactically valid partial
    # output is a DETECTABLE failure — and accepting it because the rows happen to parse turns a
    # broken read into a short table, which the terminator then "confirms". This is not the disclosed
    # undetectable-truncation residual below: that one is invisible, this one announces itself and was
    # being thrown away. Without `pipefail` the MSYS pipeline returns AWK's status, never ps's.
    [ "$rc" -eq 0 ] || continue
    # EVERY row must be exactly `int int`, and there must be at least one — validating only the FIRST
    # row, or trusting a pipeline's exit status, lets a GARBLED read through as a short table, and a
    # short table is not a small tree: it is a tree with descendants you cannot see, which the
    # terminator would then "confirm". (The `bad` flag is not decoration: in awk a body `exit 1` still
    # runs END, and an END that also exits OVERRIDES the status — `{exit 1} END{exit (NR==0)}` returns
    # 0. Verified.)
    #
    # BE EXACT ABOUT THE LIMIT: this rejects EMPTY and MALFORMED tables. It CANNOT detect a table that
    # was silently TRUNCATED yet remains well-formed — every row parses, the rows are simply missing —
    # and no amount of row validation can, because nothing in the output says how long it should have
    # been. That residual is real and is not asserted away: it is why enumerate-and-confirm is a
    # stopgap and a CONTAINMENT OBJECT (which never enumerates at all) is the fix owed at GI-6.
    if [ -n "$out" ] && printf '%s\n' "$out" | awk '
         { if (NF != 2 || $1 !~ /^[0-9]+$/ || $2 !~ /^[0-9]+$/) { bad = 1; exit } }
         END { exit (bad || NR == 0) }'; then
      printf '%s\n' "$out"; return 0
    fi
  done
  return 1     # CANNOT ENUMERATE — the caller must fail closed, never assume "nothing to kill"
}

# Transitive descendants of $1, by PARENTAGE from the RECORDED root — never by image name or pattern.
# Be precise about what this buys, because it is half of "terminate only your own tree", not all of it.
# It removes PATTERN matching: the set is derived from a recorded root by parentage, never from an
# image name, so cleanup does not sweep up every `codex` on the machine the way `pkill -f` does. It
# does NOT establish OWNERSHIP IDENTITY — a PID is not a start identity, so a recorded PID recycled
# before enumeration puts an UNRELATED process in the closure, and that process is then killed. Which
# is to say: this narrows the blast radius, it does not prove the tree is yours. Saying it "can never
# reach a sibling leg" would be exactly the overclaim this page is about. The gap is the GI-6
# start-identity residual; no arrangement of `ps` closes it, only a containment object does.
descendants_of() {
  local root="$1" table pid ppid found=1 known=" $1 " out=""
  table="$(process_table)" || return 1
  while [ "$found" -eq 1 ]; do
    found=0
    while read -r pid ppid; do
      case "$pid"  in ''|*[!0-9]*) continue ;; esac   # BOTH columns are validated. Matching a
      case "$ppid" in ''|*[!0-9]*) continue ;; esac   # non-numeric ppid against the known-set is
      case "$known" in *" $ppid "*)                   # how a malformed row joins an unrelated tree.
        case "$known" in *" $pid "*) ;; *) known="$known$pid "; out="$out $pid"; found=1 ;; esac ;;
      esac
    done <<< "$table"
  done
  echo "$out"
}

# Is $1 alive? The ONE liveness oracle — every page uses this, none uses `kill -0`.
#   0 = alive   1 = gone   2 = CANNOT TELL (never "gone")
# `kill -0` fails for "no such process" AND for "permission denied", so it cannot distinguish a dead
# process from a live one we may not signal. A caller that treats its failure as death certifies an
# exit that never happened. The table knows the difference; the signal does not.
pid_alive() {
  local t
  t="$(process_table)" || return 2
  printf '%s\n' "$t" | awk -v p="$1" '$1 == p { f = 1 } END { exit !f }'
}

terminate_recorded_tree() {              # $1 = recorded PID (captured from $! at spawn)
  local root="$1" kids winpid
  # BEFORE the kill: a reaped parent orphans its children, and an orphan you never enumerated is one
  # you can never confirm. If the table is unreadable we STOP — a cleanup that cannot see the tree
  # cannot honestly confirm it is gone, and reporting success there is the whole failure.
  if ! kids="$(descendants_of "$root")"; then
    echo "CANNOT ENUMERATE the process table — refusing to report a confirmed termination for $root"
    return 1
  fi
  if command -v taskkill >/dev/null 2>&1 && [ -r "/proc/$root/winpid" ]; then
    winpid="$(cat "/proc/$root/winpid")" # NOT $root — taskkill needs the Windows pid (fact 1)
    taskkill //F //T //PID "$winpid" >/dev/null 2>&1
  elif [ -n "${SETSID:-}" ]; then
    kill -TERM -- "-$root" 2>/dev/null   # POSIX: signal the GROUP the setsid launch created
    sleep 1
    kill -KILL -- "-$root" 2>/dev/null   # ESCALATE — TERM is catchable; a hung child ignores it
  fi
  kill -9 $kids "$root" 2>/dev/null      # the enumerated closure: what //T and the group both miss
  sleep 1

  # CONFIRM THE CLOSURE, not the leader (fact 3) — and confirm it against the PROCESS TABLE, not with
  # `kill -0`. `kill -0` returns failure for "no such process" AND for "permission denied", so a
  # process we merely lack the right to signal reads as GONE: the helper would report a confirmed
  # termination over a live orphan tree, which is the exact false-green it exists to prevent. Presence
  # in the table is presence, whatever our permission to signal it.
  TBL="$(process_table)" \
    || { echo "cannot confirm termination of $root: process table unreadable"; return 1; }
  for p in $root $kids; do
    if printf '%s\n' "$TBL" | awk -v p="$p" '$1 == p { found = 1 } END { exit !found }'; then
      echo "TERMINATION UNCONFIRMED: pid $p in the tree of $root is still alive"
      return 1
    fi
  done
  return 0
}
```

**Known residual, NOT covered above:** a descendant that re-parents or spawns between the enumeration and the kill is outside the closure this helper computed, and a PID is not a start identity (PIDs are reused). Closing that needs a containment object created atomically with the child — a Windows Job Object or a POSIX cgroup — which this protocol does not have and does not pretend to: it is owned by the gate-integrity block (`docs/arch/gate-integrity-ledger.md` → GI-6). Until it lands, a CONFIRMED return means *the closure I enumerated is gone*, not *nothing escaped*.

Two residuals stay OPEN and are owned by the gate-integrity block (`docs/arch/gate-integrity-ledger.md`, GI-6): a descendant that escaped its group is not guaranteed reaped, and a PID is not a start identity, so a recycled PID could in principle be confirmed "gone" by the wrong process. Neither is asserted away here.

**Cleanup is scoped to what you recorded, never global.** Terminate only a PID/process tree recorded as owned by THIS dispatch. NEVER terminate by image name or pattern (`taskkill /F /IM`, `pkill -f codex`, `killall`, `Stop-Process -Name`): those match EVERY codex process on the machine — the user's own sessions and desktop app, and the sibling review leg. A leg killed by another dispatch's cleanup is INDISTINGUISHABLE from a stalled leg. (HYPOTHESIS, unproven: global cleanup may itself contribute to the observed stall class. The rule stands on ownership regardless; do not mine historical stalls to settle it.)

**Banner check — the policy is a gate, not a wish.** The CLI echoes model and effort in its startup banner. Verify BOTH against the expected `CODEX_MODEL_POLICY` entry; a mismatch BLOCKS the leg. Do not trust the policy; read what actually bound. This is what turns a pin into a gate.

**A leg whose verdict artifact you cannot produce DOES NOT COUNT AS HAVING RUN.** (It may in fact have run — you cannot know, and "it probably ran" has never been evidence. Treat it as failed.) The leg counts only if `CODEX_EXIT` is 0, the banner matched, and `$VERDICT` exists, is non-empty, and you READ it. Any non-zero exit ⇒ FAILED (124 is `timeout`'s convention, but the operative test is simply "not 0"): redispatch up to `$CODEX_MAX_ATTEMPTS`, then BLOCK and escalate. A timed-out process whose partial transcript happens to contain the right words is not a verdict.

**A grep is DIAGNOSTIC ONLY — it may never establish a verdict.** Grep `$OUT` to ORIENT (`grep -nE '\[P[0-3]\]' "$OUT" | tail`) or to diagnose a failure; never to conclude. `LAND`, `CHANGES`, and `__DONE__` all occur in the prompt echo — this file's own mandate is prepended VERBATIM to every prompt and comes straight back in the transcript — so a scan can hand back a verdict the leg never rendered. This is not hypothetical: a TIMED-OUT leg, killed mid-tool-call, was observed with **35** occurrences of `__DONE__` in its transcript and no final message at all. `__DONE__` is a TERMINATOR, not a verdict. Read `$VERDICT`; do not pattern-match `$OUT`.

The prompt is fed on stdin, never as an arg. For branch review, run against the worktree and review `git diff <integration-branch>..HEAD`, taking the branch from the brief.

Never wait via `pgrep -f "codex exec"`; it self-matches the waiter, and pattern matching is not ownership.

## TDD / Tests

Discriminating tests first: FAIL before change, PASS after. No stubs, empty tests, always-true asserts, unconditional defaults presented as implementation, or non-discriminating characterization. Reviewers read every new test body. §1a proves RED→GREEN through the affected declared canonical/CI entry point — not only direct test invocation — by hunk-revert/plant on each new/changed correctness-bearing test/guard plus one unplanted control that stays GREEN, and REJECTS absent eligibility, discovery, build, selection, execution, skip, or completion evidence (`CLAUDE.md` → Verification Must Prove Execution). A gate that reports success without proving it ran its intended surface is FAIL, not PASS.

### Plant Verification

**A plant that fails to apply reports a pass.** Every §1a and confirm discrimination check rests on planting a defect and observing a failure — so if the mutation silently no-ops, the check certifies a worthless test as discriminating, with a green run and no error. This is the most dangerous face of the hollow-gate class because it sits INSIDE the verification of the verification.

Before any planted run is trusted:

- **PROVE THE PLANT IS IN THE SOURCE.** `git diff` the mutated file and confirm the mutation is actually present. **Never trust the mutation command's exit code** — `perl`, `sed`, `grep`, and friends all exit 0 on a non-match. A zero exit means "the tool ran", never "the edit landed".
- **The verification search must prove the plant is UNIQUE and NEW, not merely present.** A pre-existing occurrence of the planted string is a false positive — grepping for a string that was already in the file certifies nothing. Assert the count CHANGED (`n` → `n+1`), or plant a token that cannot already exist.
- **A green planted run means the plant failed, until proven otherwise.** If the mutated tree passes, the FIRST hypothesis is "the mutation did not apply", not "the test is weak" and never "the code is correct".

Invariant: a discrimination check that cannot distinguish "the plant did not apply" from "the code is correct" is not a discrimination check. Restore, re-verify the restore, and re-run before reporting any RED→GREEN result.

## Review Cadence

Every block/stage/phase review round = 2 codex + 1 claude, parallel, neutral, distinct lenses, read-only, no tests, harsh production bar, to 3/3 LAND or NIT-only carried forward. Designs/docs get the same full 3/3; later landing/rebase skips re-review only when byte/hash-identical. Any conflict resolution, mirror content delta, or non-identical "mechanical" change re-enters 3/3 + §1a + anti-rogue layer 1. Skill/design/doc codex reviews cap at 3 rounds; after 3, finalize cosmetic/wording/framing residuals, but substantive or anti-rogue findings still block.

Findings route by scope. In-scope P0/P1/P2 enter normal fix cycle; P0 blocks, each fix is a new commit, never amend, re-review until clean. Every scope-deviating correctness finding (system-wide/pre-existing class, architecture beyond block, relitigating settled scope) is DISPOSITIONED before related work continues as `ADOPT-NOW`, `DEFER`, or `REJECT`, ruled by codex with plan context. `ADOPT-NOW` records the scope + acceptance-contract change. `DEFER` requires a codex-DEFER ruling and a debt row naming the durable OWNER BLOCK (never an ephemeral agent identity), the RESOLUTION GATE no later than plan close, the acceptance ID/test, and the ruling reference. `REJECT` records evidence and rationale. A TODO or feedback entry is not a disposition; a finding is never silently absorbed into the block nor silently deferred.

A reviewer's factual premise is a `HYPOTHESIS` until re-derived (see Decision Admission) — never relay a review correction into a fix brief as established fact. Fix whole classes within the block surface; if class extends beyond deliverable, stop and codex-scope-consult. If recurring out-of-scope findings exceed about 5 rounds, consult early. Adjudicate reviewer compile/test claims against the verified gate: if the full gate compiled and ran, a false "won't compile" claim is INVALID — never add a no-op patch merely to clear that verdict.

## Architecture / Decision Modes

Architecture is always codex-owned. Claude executes; codex never writes code. Iterate until confident; never accept hedged verdicts. If codex contradicts verified repo evidence after rerun, stop/escalate.

Single yes/no architecture question → one neutral codex leg. Genuine multiple-choice/high-stakes fork → two neutral codex legs in parallel: A options-framed, B problem-only/unprimed. If they agree, decision is binding. If they disagree, run a third codex decider with both outputs plus source; it verifies decisive claims against code and adjudicates.

Modes:
- `escalate-to-user`: codex produces architecture analysis; user ratifies/rejects. Product/priority forks go to user.
- `full-autonomous`: auto-adopt codex architecture verdict; derive product/priority from approved plan.
User never adjudicates architecture instead of codex. Stop/user boundaries: no implementation agent capacity (on opt-in `claude -p`, no usable implementation account), destructive operation (force-push/history rewrite/irreversible shared state), unrecoverable lost/corrupt worktree with no trusted commit, codex unavailable/contradicts evidence, or product/priority choice not derivable from plan.

## Verification Gate

Delegate heavy gates to a verify agent in the worktree and serialize globally.

```bash
# BOTH options are load-bearing, and each closes a DIFFERENT false-green:
#   pipefail — without it a pipeline returns its LAST command's status, so `cargo … | tee`
#              returns tee's status and a failing gate exits 0.
#   errexit  — without it these are four independent commands and the BLOCK's status is the
#              LAST one's, so a failing `nextest` followed by a passing `cargo fmt` exits 0.
# Fixing only one still yields a gate that reports success without having passed.
set -o errexit -o pipefail

# Acquire the global heavy-gate lock AROUND the gate. `mkdir` is the atomic test-and-set.
# Only "already exists" is contention — any OTHER mkdir failure (bad path, permissions) is a
# hard error, and a loop that cannot tell them apart spins forever on a typo.
#
# The lock path is a CONSTANT, not a caller-supplied variable. An overridable lock dir is not a
# global lock: two agents resolving it differently take two independent locks, serialize nothing,
# and both report success. So the path is fixed here, and a pre-set override is REJECTED rather
# than honoured — an env var that silently disables mutual exclusion is worse than no lock.
GATE_LOCK_DIR="/tmp/mom/locks"
mkdir -p "$GATE_LOCK_DIR" || { echo "cannot create $GATE_LOCK_DIR"; exit 1; }
LOCK="$GATE_LOCK_DIR/heavy-gate.lock"

# A failed `mkdir` has THREE causes and they need three different answers — inferring instead of
# probing gets one of them wrong in a way that hangs:
#   lock exists                      -> genuine contention: WAIT
#   lock absent, parent writable     -> the holder released in the gap between our mkdir and our
#                                       test: RETRY (hard-failing here fails the gate at the exact
#                                       moment it became acquirable)
#   lock absent, parent NOT writable -> permissions, read-only fs: HARD ERROR. Retrying a condition
#                                       that will never clear waits forever, and an infinite wait is
#                                       indistinguishable from a slow gate.
# The wait is also BOUNDED: a lock leaked by a hard-killed holder (GI-12) must surface as a BLOCK a
# human can act on, never as a run that hangs until someone notices.
lock_parent_writable() { local p="$GATE_LOCK_DIR/.probe.$$"; mkdir "$p" 2>/dev/null || return 1; rmdir "$p"; }
WAITED=0; LOCK_MAX_WAIT=3600
until mkdir "$LOCK" 2>/dev/null; do
  if [ ! -d "$LOCK" ] && ! lock_parent_writable; then
    echo "BLOCK: cannot create $LOCK and $GATE_LOCK_DIR is not writable (permissions? read-only fs?)"
    exit 1
  fi
  if [ "$WAITED" -ge "$LOCK_MAX_WAIT" ]; then
    echo "BLOCK: heavy-gate lock held >${LOCK_MAX_WAIT}s. Do NOT steal it — confirm the recorded"
    echo "       owner in $LOCK/owner is dead, then remove it by hand (see GI-12)."
    exit 1
  fi
  sleep 10; WAITED=$((WAITED + 10))
done
# Release on EVERY exit path — success, failure, errexit abort. And CHECK the release: a leaked
# lock blocks every later gate forever, so a cleanup that fails must fail the run rather than
# quietly hand back the gate's green. `$?` is preserved first so a passing gate still reports
# its own status, and only a genuine release failure overrides it.
trap 'rc=$?; rm -rf "$LOCK" || { echo "FAILED TO RELEASE $LOCK — later gates will block"; exit 1; }; exit $rc' EXIT

# The owner record is written IMMEDIATELY after acquisition — but `mkdir` is the atomic test-and-set,
# so there is an unavoidable window between "lock exists" and "lock has an owner". A hard kill inside
# that window leaves an OWNERLESS stale lock, and a recovery procedure that keys on the owner record
# would have nothing to key on. That case is therefore called out rather than left to be discovered:
# an ownerless lock is a holder that died at acquisition, and it is reclaimable once no gate workload
# from that run remains (GI-12 owns making this automatic). Closing the window entirely needs an
# atomic publish of dir+owner together, which `mkdir` alone cannot give.
# The timestamp is captured and CHECKED separately. `echo "$$ $(date …)" > owner` returns ECHO's
# status, so a failed `date` writes a malformed owner record and passes — the same class of swallowed
# failure this whole page is about, one line from the end of it.
LOCK_STAMP="$(date -u +%FT%TZ)" \
  || { echo "cannot read the clock — refusing to write an unidentifiable owner record"; exit 1; }
echo "$$ $LOCK_STAMP" > "$LOCK/owner" \
  || { echo "acquired the lock but could not record ownership — releasing rather than holding it anonymously"; exit 1; }

cd "$WORKTREE"
cargo nextest run --workspace --no-fail-fast 2>&1 | tee "$GATE_DIR/gate-nextest.txt"
cargo test -p verter_session --tests 2>&1 | tee "$GATE_DIR/gate-session.txt"
cargo clippy --workspace -- -D warnings
cargo fmt --all --check
```

The release is `rm -rf`, never `rmdir`: the lock directory holds the `owner` file, and `rmdir` only removes EMPTY directories — so a `rmdir … 2>/dev/null` trap fails SILENTLY and leaks the lock forever, blocking every later gate. That is a cleanup that does not happen and reports nothing, i.e. the same class as everything else on this page; the first draft of this very snippet had it. A lock left by a HARD-killed holder (no trap fires) is a STALE LOCK. Clearing one automatically without racing a live holder needs the start-identity machinery GI-6 owes, so today it is cleared by a human (`docs/arch/gate-integrity-ledger.md`, GI-12). And be exact about what that human must establish, because the obvious check is not sufficient: **a dead owner PID does not mean the gate workload is gone.** The holder's `cargo`/`tee`/test descendants can outlive the shell that spawned them — and once that shell is dead they are re-parented, so they cannot even be enumerated from the recorded owner any more. Confirming the owner PID (or its start identity) therefore proves the LOCK is abandoned, not that the WORK is finished; releasing on that basis alone can start a second heavy gate on top of a first one still running, which is the exact corruption the lock exists to prevent. The human must confirm no gate workload from that run remains — then remove the lock. Never unblock a gate by deleting a lock you have not verified is dead.

Canonical Rust gate is the nextest command PLUS `cargo test -p verter_session --tests`. Bare `cargo test --workspace --tests` silently skips the verter_session integration suite and is never the sole gate. Count summaries/per-binary lines; never accept truncated runs. `--no-fail-fast` prevents an early env/pre-existing failure hiding downstream failures. A gate's exit status is evidence ONLY if a failure can actually reach it: verify the pipeline propagates (`pipefail`) AND that a failing step aborts the block (`errexit`, `&&`, or explicit status aggregation) before you trust any green.

JS: if TS touched, run the relevant JS gate; otherwise at least `pnpm install --frozen-lockfile` for lockfile sync. Node_modules-less worktrees may symlink main `node_modules` or install once. `typeinfo_ts_bindings_*` regeneration via missing `buf` is env-only, not a code failure.

**One pass bar, and it is GREEN.** Re-derive the baseline at block entry — but the baseline is a DIAGNOSTIC for classifying a failure, never a second, weaker pass bar. "Zero new failures against a red baseline" and confirm's "full gate GREEN" cannot both be operative: when the baseline is red an agent could satisfy whichever it prefers and truthfully claim "the gate", which is not a gate at all. So the operative rule is the strict one:

**A nonzero gate BLOCKS. There is no exclusion, no allowlist, and no override — today, none.** The fence aborts on the first nonzero result (`errexit`), and that is the whole rule: nothing an agent writes in a document can make a red run exit 0, and no wording makes a red gate "GREEN".

This is deliberately stricter than the earlier draft, which said an enumerated, ruled pre-existing failure could be "excluded" so that confirm could still observe a pass. That could not be honestly implemented and should not be: the fence exits nonzero for an enumerated failure exactly as it does for a new one, no mechanism distinguishes them, and a human declaring the difference is not a gate — it is the thing being gated marking its own homework. **A red gate is red.** An agent facing one BLOCKS and escalates; it never lands, and it never reports "GREEN except…".

- A pre-existing failure is a **classification, not a permission.** It does not lower the bar, and it does not become acceptable by being older than the block — an unbounded "it was already broken" escape is precisely how a red suite becomes permanent.
- What a debt row or codex-DEFER ruling records is that the failure is KNOWN and OWNED. It does not authorize a landing. Changing the bar itself is a rule-bearing decision for the ratifying authority, recorded — never an agent's classification of its own gate output.
- The machine-checkable exclusion that would let a genuinely ruled failure be skipped BY THE MECHANISM (rather than waved through by the actor being gated) is owed at `docs/arch/gate-integrity-ledger.md` → GI-11. **Until it exists, there is no exclusion at all** — which is the fail-closed reading, and the only one that does not require the rule text to call a failing run green.

If a failure correlates with co-resident full-suite load or a shared external service under a no-retry harness, treat it as an environmental flake first — rerun it isolated before classifying it as a regression. That is a claim about what the failure IS, and it still has to end in a green run.

## Commit / Land Rules

Use `LANDING-PROTOCOL.md` for mechanics. Mid-flight `wip:` allowed; the final landed history is exactly one clean conventional commit per block, no `wip`/`fixup`/`squashme`, no Co-Authored-By or attribution trailer, no logs/outputs/generated junk, no `git push`, no direct integration-branch commit, no `git add -A` or `git add .`.

Pre-land sync: rebase onto current integration tip and rerun full gate. Any conflict resolution or content delta re-enters full 3/3 + §1a + anti-rogue layer 1. Design mirrors must be byte-identical to reviewed content. True ff only; no merge commit.

No phase/plan refs in production: production source (`crates/*/src/**`), code, comments, tests, and conventional commit MESSAGES contain no plan/phase vocabulary. `docs/arch` is the DESIGNATED home for plans, designs, and debt ledgers, and is exempt by construction — the same protocol that bans the vocabulary elsewhere REQUIRES those documents ("Plans belong only in `docs/arch`"; a deferral needs a `docs/arch` debt row), so a ban reaching their file contents would forbid what it mandates. Phase refs may exist in `wip:` history and are scrubbed at squash.

## Repo Cleanliness

Only product files land: source, tests, fixtures, tracked docs, and `docs/arch` plans/designs. Plans belong only in `docs/arch`. Temp/scratch/report/evidence/log/progress files live in `/tmp/mom` absolute paths or `.feedback/` (directory-gitignored). Worktrees are outside the repo.

Before staging, inspect `git status --short` and remove only verified untracked/ignored scratch this block created; never by filename class. A tracked file is never removed as cleanup; tracked deletion is reviewed product change. Do not add per-file `.gitignore` entries for scratch. Generated outputs are gitignored by directory/glob (`playwright-report`, `test-results`, `*.junit.xml`, coverage). Scratch/temp/report in status/diff blocks land.

Plan-end scratch/report clutter already in git history is purged by a dedicated user-authorized destructive history-rewrite/force-push operation, with final user go-ahead at execution time.

**This does NOT loosen the no-push rule, and the two must not be read as contradicting each other.** "Never push" and "no `git push`" are absolute for the LANDING path: a block manager, an implementer, a reviewer, and the CTO never push, ever, under any circumstance — landing is squash + true fast-forward on a local integration branch, full stop. The history purge is not part of that path. It is a separate, explicitly user-authorized DESTRUCTIVE operation, performed at a STOP boundary (`Architecture / Decision Modes` lists "destructive operation (force-push/history rewrite/irreversible shared state)"), with the user giving the go-ahead at execution time. An agent may never reach for it to satisfy a landing rule, and its existence never authorizes a push anywhere else. If you find yourself citing this paragraph to justify a push during a block, you are wrong. Owner and gate: `docs/arch/gate-integrity-ledger.md` → GI-13.

## Binding Designs / Debt

Every binding design/plan is committed to `docs/arch/<name>-design.md` and referenced in the master-plan locked-designs index. At land, mirror is byte-identical to reviewed design; confirm verifies tracked + referenced + `cmp` identical.

Mid-plan deferrals require codex-DEFER ruling and debt-ledger row in `docs/arch`. FEED-FORWARD is valid only mid-plan. Plan close-out requires empty debt ledger and zero open deferrals/carry-forward.

## Confirm / Integration Gates

After every manager land, the CTO — never the block manager — dispatches a separate unprimed confirm MANAGER. It independently verifies four bars and writes `VERDICT:CONFIRMED` only when all hold:
1. CORRECT + additive/non-breaking-as-claimed + full gate GREEN first-hand; re-grep legacy deletion; re-prove every correctness-bearing test/guard discriminates in throwaway worktree (plant violation → PROVE THE PLANT IS IN THE SOURCE → RED → restore → GREEN); a green planted run means the plant failed until proven otherwise (see Plant Verification); sampling one is invalid; `cmp` design mirrors.
2. NOT SHALLOW: real inputs and edges; no non-discriminating characterization.
3. NO STUBS: no empty/always-true tests or unconditional default/unknown/OK returns as implementation.
4. BEST IMPLEMENTATION: unprimed codex-architect judges whether a better design exists. Merely correct but not best = REOPEN.

Record every REOPEN against a PRE-EXISTING acceptance ID or architecture-seam ID, with symptom, root-cause class, and new evidence. On REOPEN the CTO first runs Decision Admission; only an admitted FIX verdict dispatches a fresh fix manager → re-land → re-confirm. Per-stage confirm is separate from integration-confirm.

**Second-reopen circuit breaker.** The second REOPEN on the same ID LAPSES that design's approval. Before any further fix dispatch: pause implementation, obtain an unprimed codex ruling of `RETAIN`, `REDESIGN`, or `REMOVE` stating the evidence that distinguishes the next attempt, and route any product-intent change to product authority. This does not conflict with one-pass execution (`CLAUDE.md` → Execution): one-pass governs executing an APPROVED design; the breaker fires when APPROVAL HAS LAPSED — a different event, and the reason execution must stop rather than grind on. A first reopen is usually an ordinary implementation defect; adjudicating every first reopen would turn routine confirmation into an architecture round. Reset the counter ONLY on a newly ratified design revision — never because the symptom was renamed or the review axis changed. Enforcement is JUDGMENT: the CTO assigns the stable ID on the first reopen and stops dispatch on the second; seam misclassification is the residual gameable surface.

Integration-confirm MANAGER runs at every phase/milestone boundary, before any dependent phase, before final close-out, and after every 5 confirmed blocks. It derives done-bar from binding plan, not reports; reviews integrated diff/tree; runs canonical gate; checks cross-block invariants, manifests, design mirrors, debt honesty, legacy deletion, hollow fronts, cache/perf/warm-state. Issues are classified via codex as REOPEN vs FEED-FORWARD. FEED-FORWARD only mid-plan. Only `VERDICT:INTEGRATION-CONFIRMED` closes phase.

Stage/phase cleanup only after land + confirmation: `git worktree remove` + `git worktree prune`, remove transient briefs/reports/review outputs (and `jsonl`/markers on the opt-in `claude -p` path), preserve CTO ledger/MOM-NOTES, landed reports, debt ledger, design docs, verify clean status. At phase boundary clear all closed-stage worktrees/temp. Never delete live `/tmp` evidence mid-flight.

## Status Reporting

Write/overwrite `/tmp/mom/<BLOCK>/status.json` at milestones:

```json
{ "block":"<id>", "state":"RUNNING|BLOCKED|DONE|PARTIAL",
  "phase":"<short phrase>", "branch":"<branch>", "head":"<sha>",
  "commits":["<sha> <subj>"], "gate":{"rust":"PASS|FAIL|PENDING","js":"PASS|FAIL|NA"},
  "review":{"claude":"LAND|CHANGES|PENDING","codexA":"...","codexB":"..."},
  "newFailures":[], "note":"<=1 line" }
```

`BLOCKED`: first run escalation protocol; if unresolved, write `ESCALATION.md` with question, two options, what tried, set state, exit. `PARTIAL`: keep only internally clean committed branch work, no ff-merge unless full lifecycle completed; write `HANDOFF.md`; no final WIP commits. `DONE`: only after 3/3 LAND, §1a `VERDICT:LAND`, anti-rogue layer 1, rebase, post-rebase full gate, teeth'd squash, true ff, `MANAGER-LANDED.md`, CTO ledger append, then exit — leaving the block in `LANDED / AWAITING-CONFIRM`. **`DONE` explicitly does NOT include cleanup:** the manager removes neither its scratch nor its worktree, because the CTO dispatches confirm only after the manager has exited, and a manager that cleaned up at land would destroy the evidence of a gate that has not run yet. Worktree/scratch removal is stage/phase cleanup, owned by the CTO, after land + CONFIRMED (`LANDING-PROTOCOL.md` §5).

## Verter Invariants

Never weaken: one resolver (`SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore`); typed-IR-only (no source-slicing/regex-on-type-text/reparse in resolver); shallow-by-default; fact-cache R21 five split env hashes; R6 no versioned identity in query-identity keys; §10.4.1 363-row manifest partition EXACT + bijective with live manifest; `AdditionalProofRow` closed at 7; `CodeTransform` is the only output-mutation path; final-state prose/no phase archaeology in `crates/*/src/**`.

Cross-platform: codebase must build/test/materialize on macOS/Windows/Linux. No NTFS-illegal chars (`< > : " | ? * \`, control chars, trailing dot/space, reserved CON/PRN/AUX/NUL/COM#/LPT#) in tracked paths or generated names; sanitize names. No hardcoded path separators; use `Path`/`PathBuf`/`join`. Byte-equality compares over CHECKED-OUT TEXT — text whose line endings a checkout may rewrite — normalize CRLF/LF or compare as text; a same-checkout identity check between two files that were both written by the same run (the design-mirror `cmp`) is a raw byte compare by design and is not affected. OS binaries (`tsgo`/`.exe`) are discovered platform-aware. Temp/cwd use std abstractions. Platform-assuming code is a defect.

## Terseness

Rules/skills stay concise. New process gaps go to the ledger and, if rule-bearing, through GOVERNANCE before adoption.
